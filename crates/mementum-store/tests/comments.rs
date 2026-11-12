//! Les fils de commentaires d'une page (Ruling R134) : un fil, ses messages et leurs réactions,
//! relus dans l'ordre ; ce que le store vérifie dans la transaction qui écrit (le fil est de
//! cette page, le message de ce fil, et de ce compte quand on le demande) ; ce qui part avec un
//! message, avec un fil, avec une page.

mod common;

use common::{Fixture, account, fresh, int, page};
use mementum_store::{Authorship, Comment, StoreError, Thread};
use tokio::sync::broadcast::error::TryRecvError;
use uuid::Uuid;

/// Une page neuve, à la racine de l'espace de départ.
async fn new_page(f: &Fixture, title: &str) -> Uuid {
    f.store
        .create_node(page(&f.space, None, None, title))
        .await
        .unwrap()
        .id
}

/// Une réaction telle que le test la lit : l'emoji, puis les noms de ceux qui l'ont posée.
type ReactionView = (String, Vec<String>);

/// Un message tel que le test le lit : son auteur, son texte, s'il a été modifié, ses réactions.
type CommentView = (String, String, bool, Vec<ReactionView>);

fn comment_view(comment: &Comment) -> CommentView {
    (
        comment.author.name.clone(),
        comment.body.clone(),
        comment.edited_at.is_some(),
        comment
            .reactions
            .iter()
            .map(|r| {
                let names = r.users.iter().map(|u| u.name.clone()).collect();
                (r.emoji.clone(), names)
            })
            .collect(),
    )
}

/// Un fil tel que le test le lit : son bloc, qui l'a résolu, ses messages.
fn thread_view(thread: &Thread) -> (Option<String>, Option<String>, Vec<CommentView>) {
    (
        thread.block_id.clone(),
        thread.resolved_by.as_ref().map(|a| a.name.clone()),
        thread.comments.iter().map(comment_view).collect(),
    )
}

fn view(author: &str, body: &str, edited: bool, reactions: &[(&str, &[&str])]) -> CommentView {
    (
        author.to_string(),
        body.to_string(),
        edited,
        reactions
            .iter()
            .map(|(emoji, names)| {
                let names = names.iter().map(|n| n.to_string()).collect();
                (emoji.to_string(), names)
            })
            .collect(),
    )
}

/// Deux fils sur une page, l'un sur la page, l'autre sur un bloc, avec réponses, réactions,
/// une modification et une résolution : relus du plus ancien au plus récent, les messages dans
/// leur ordre, les réactions dans l'ordre où chaque emoji est apparu et les comptes dans
/// l'ordre où ils l'ont posée. Le fil d'une autre page n'y est pas. Une écriture de
/// commentaire n'avance ni la révision ni la version de l'espace, et n'annonce rien.
#[tokio::test]
async fn thread_with_replies_and_reactions_reads_back_in_order() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let carol = account(store, "carol").await;
    let p = new_page(&f, "Page").await;
    let other = new_page(&f, "Autre").await;
    let rev = store.space_rev(f.space.id).await.unwrap();
    let lsn = store.space_lsn(f.space.id).await.unwrap();
    let mut changes = store.changes();

    let first = store
        .create_thread(p, None, alice.id, "Premier")
        .await
        .unwrap();
    assert_eq!(
        thread_view(&first),
        (None, None, vec![view("alice", "Premier", false, &[])])
    );
    assert_eq!(
        (first.opened_by.id, first.opened_by.name.as_str()),
        (alice.id, "alice")
    );
    assert_eq!(first.resolved_at, None);
    assert_eq!(first.comments[0].created_at, first.created_at);
    let on_block = store
        .create_thread(p, Some("bloc-1"), bob.id, "Sur un bloc")
        .await
        .unwrap();
    store
        .create_thread(other, None, carol.id, "Ailleurs")
        .await
        .unwrap();
    let reply = store
        .reply_to_thread(p, first.id, bob.id, "Réponse")
        .await
        .unwrap();
    assert_eq!(comment_view(&reply), view("bob", "Réponse", false, &[]));
    store
        .reply_to_thread(p, first.id, carol.id, "Encore")
        .await
        .unwrap();
    let opening = first.comments[0].id;
    for (who, emoji) in [(&bob, "👍"), (&carol, "🎉"), (&carol, "👍"), (&alice, "🎉")] {
        let (_, changed) = store
            .add_reaction(p, first.id, opening, who.id, emoji)
            .await
            .unwrap();
        assert!(changed);
    }
    let (on_reply, _) = store
        .add_reaction(p, first.id, reply.id, alice.id, "👀")
        .await
        .unwrap();
    assert_eq!(
        comment_view(&on_reply),
        view("bob", "Réponse", false, &[("👀", &["alice"])])
    );
    // Un message modifié garde sa place, et dit qu'il l'a été.
    let edited = store
        .edit_comment(p, first.id, reply.id, bob.id, "Réponse corrigée")
        .await
        .unwrap();
    assert_eq!(
        comment_view(&edited),
        view("bob", "Réponse corrigée", true, &[("👀", &["alice"])])
    );
    assert!(edited.edited_at >= Some(edited.created_at));
    let (resolved, changed) = store
        .resolve_thread(p, on_block.id, alice.id, true, Authorship::Any)
        .await
        .unwrap();
    assert!(changed && resolved.resolved_at.is_some());

    let threads = store.comment_threads(p).await.unwrap();
    assert_eq!(
        threads.iter().map(thread_view).collect::<Vec<_>>(),
        vec![
            (
                None,
                None,
                vec![
                    view(
                        "alice",
                        "Premier",
                        false,
                        &[("👍", &["bob", "carol"]), ("🎉", &["carol", "alice"])]
                    ),
                    view("bob", "Réponse corrigée", true, &[("👀", &["alice"])]),
                    view("carol", "Encore", false, &[]),
                ]
            ),
            (
                Some("bloc-1".to_string()),
                Some("alice".to_string()),
                vec![view("bob", "Sur un bloc", false, &[])],
            ),
        ]
    );
    assert_eq!(threads[0].id, first.id);
    assert_eq!(threads[1], resolved);
    assert_eq!(
        store
            .comment_threads(other)
            .await
            .unwrap()
            .iter()
            .map(thread_view)
            .collect::<Vec<_>>(),
        vec![(None, None, vec![view("carol", "Ailleurs", false, &[])])]
    );

    // Rouvert : plus de date ni d'auteur de résolution.
    let (reopened, changed) = store
        .resolve_thread(p, on_block.id, bob.id, false, Authorship::Of(bob.id))
        .await
        .unwrap();
    assert!(changed);
    assert_eq!((reopened.resolved_at, reopened.resolved_by), (None, None));

    assert_eq!(store.space_rev(f.space.id).await.unwrap(), rev);
    assert_eq!(store.space_lsn(f.space.id).await.unwrap(), lsn);
    assert_eq!(changes.try_recv(), Err(TryRecvError::Empty));
}

/// Un fil qui n'est pas de cette page, un message qui n'est pas de ce fil, ou qui n'existe
/// pas : `NotFound`, sans rien écrire. Le store ne dit pas si l'identifiant existe ailleurs.
#[tokio::test]
async fn thread_or_message_of_another_page_is_not_found() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let p = new_page(&f, "Page").await;
    let other = new_page(&f, "Autre").await;
    let here = store.create_thread(p, None, alice.id, "Ici").await.unwrap();
    let there = store
        .create_thread(other, None, alice.id, "Là")
        .await
        .unwrap();
    let (here_msg, there_msg) = (here.comments[0].id, there.comments[0].id);
    let unknown = Uuid::now_v7();
    let not_found = |result: Result<(), StoreError>| matches!(result, Err(StoreError::NotFound));

    // Un fil qui n'est pas de cette page.
    for thread in [there.id, unknown] {
        assert!(not_found(
            store
                .reply_to_thread(p, thread, alice.id, "x")
                .await
                .map(|_| ())
        ));
        assert!(not_found(
            store
                .resolve_thread(p, thread, alice.id, true, Authorship::Any)
                .await
                .map(|_| ())
        ));
        assert!(
            store
                .thread_authors(p, thread, None)
                .await
                .unwrap()
                .is_none()
        );
    }
    // Un message qui n'est pas de ce fil, ou dont le fil n'est pas de cette page.
    for (thread, comment) in [
        (there.id, there_msg),
        (unknown, here_msg),
        (here.id, there_msg),
        (here.id, unknown),
    ] {
        let case = format!("fil {thread}, message {comment}");
        assert!(
            not_found(
                store
                    .edit_comment(p, thread, comment, alice.id, "x")
                    .await
                    .map(|_| ())
            ),
            "{case}"
        );
        assert!(
            not_found(
                store
                    .delete_comment(p, thread, comment, Authorship::Any)
                    .await
                    .map(|_| ())
            ),
            "{case}"
        );
        assert!(
            not_found(
                store
                    .add_reaction(p, thread, comment, alice.id, "👍")
                    .await
                    .map(|_| ())
            ),
            "{case}"
        );
        assert!(
            not_found(
                store
                    .remove_reaction(p, thread, comment, alice.id, "👍")
                    .await
                    .map(|_| ())
            ),
            "{case}"
        );
        assert_eq!(
            store
                .thread_authors(p, thread, Some(comment))
                .await
                .unwrap()
                .and_then(|found| found.comment_author),
            None,
            "{case}"
        );
    }
    // Une page qui n'existe pas, un compte qui n'existe pas.
    assert!(not_found(
        store
            .create_thread(unknown, None, alice.id, "x")
            .await
            .map(|_| ())
    ));
    assert!(not_found(
        store.create_thread(p, None, unknown, "x").await.map(|_| ())
    ));

    // Rien n'a été écrit : chaque fil garde son seul message, sans réaction.
    for (node, thread) in [(p, &here), (other, &there)] {
        assert_eq!(
            store.comment_threads(node).await.unwrap(),
            std::slice::from_ref(thread)
        );
    }
    assert_eq!(int(&f.db, "SELECT COUNT(*) FROM comments").await, 2);
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM comment_reactions").await,
        0
    );
    // Ce que lit la route avant d'écrire : qui a ouvert le fil, qui a écrit le message.
    let found = store
        .thread_authors(p, here.id, Some(here_msg))
        .await
        .unwrap()
        .expect("le fil de la page");
    assert_eq!(
        (found.opened_by, found.comment_author),
        (alice.id, Some(alice.id))
    );
}

/// Quand la route le demande (`Authorship::Of`), le store vérifie dans sa transaction que le
/// message est de ce compte, ou que le fil a été ouvert par lui : sinon `NotAuthor`, et rien
/// n'est écrit. `Authorship::Any` ne demande rien.
#[tokio::test]
async fn author_is_checked_in_the_writing_transaction() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let thread = store
        .create_thread(p, None, alice.id, "D'Alice")
        .await
        .unwrap();
    let message = thread.comments[0].id;
    let not_author = |result: Result<(), StoreError>| matches!(result, Err(StoreError::NotAuthor));

    assert!(not_author(
        store
            .edit_comment(p, thread.id, message, bob.id, "Pris par Bob")
            .await
            .map(|_| ())
    ));
    assert!(not_author(
        store
            .delete_comment(p, thread.id, message, Authorship::Of(bob.id))
            .await
            .map(|_| ())
    ));
    assert!(not_author(
        store
            .resolve_thread(p, thread.id, bob.id, true, Authorship::Of(bob.id))
            .await
            .map(|_| ())
    ));
    assert_eq!(
        store.comment_threads(p).await.unwrap(),
        std::slice::from_ref(&thread)
    );

    let (resolved, changed) = store
        .resolve_thread(p, thread.id, alice.id, true, Authorship::Of(alice.id))
        .await
        .unwrap();
    assert!(changed);
    assert_eq!(resolved.resolved_by.map(|a| a.id), Some(alice.id));
    // Déjà résolu : rien ne change, pas même qui l'a résolu.
    let (again, changed) = store
        .resolve_thread(p, thread.id, bob.id, true, Authorship::Any)
        .await
        .unwrap();
    assert!(!changed);
    assert_eq!(again.resolved_by.map(|a| a.id), Some(alice.id));
    // Son auteur, lui, supprime son message (le dernier : le fil part avec).
    let thread_gone = store
        .delete_comment(p, thread.id, message, Authorship::Of(alice.id))
        .await
        .unwrap();
    assert!(thread_gone);
    assert!(store.comment_threads(p).await.unwrap().is_empty());
}

/// L'auteur d'un message et celui qui a ouvert son fil sont jugés à part. Alice ouvre un fil,
/// Bob y répond : elle ne modifie ni ne supprime la réponse de Bob (`Authorship::Of`), il ne
/// résout pas le fil d'Alice, et il modifie sa réponse. Alice supprime son premier message : le
/// fil reste le sien. Bob, seul auteur qui reste, ne le résout toujours pas ; elle le résout, et
/// il ne le rouvre pas.
#[tokio::test]
async fn thread_opener_and_message_author_are_judged_apart() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let thread = store
        .create_thread(p, None, alice.id, "D'Alice")
        .await
        .unwrap();
    let first = thread.comments[0].id;
    let reply = store
        .reply_to_thread(p, thread.id, bob.id, "De Bob")
        .await
        .unwrap()
        .id;
    let not_author = |result: Result<(), StoreError>| matches!(result, Err(StoreError::NotAuthor));
    let resolve = async |by: Uuid, resolved: bool| {
        store
            .resolve_thread(p, thread.id, by, resolved, Authorship::Of(by))
            .await
            .map(|(thread, changed)| (thread.resolved_by.map(|a| a.id), changed))
    };

    assert!(not_author(
        store
            .edit_comment(p, thread.id, reply, alice.id, "Pris par Alice")
            .await
            .map(|_| ())
    ));
    assert!(not_author(
        store
            .delete_comment(p, thread.id, reply, Authorship::Of(alice.id))
            .await
            .map(|_| ())
    ));
    assert!(not_author(resolve(bob.id, true).await.map(|_| ())));
    let edited = store
        .edit_comment(p, thread.id, reply, bob.id, "De Bob, corrigé")
        .await
        .unwrap();
    assert_eq!(edited.body, "De Bob, corrigé");

    let thread_gone = store
        .delete_comment(p, thread.id, first, Authorship::Of(alice.id))
        .await
        .unwrap();
    assert!(!thread_gone);
    let left = store.comment_threads(p).await.unwrap();
    assert_eq!(left[0].opened_by.id, alice.id);
    assert_eq!(
        left[0]
            .comments
            .iter()
            .map(|c| c.author.id)
            .collect::<Vec<_>>(),
        [bob.id]
    );
    assert!(not_author(resolve(bob.id, true).await.map(|_| ())));
    assert_eq!(
        resolve(alice.id, true).await.unwrap(),
        (Some(alice.id), true)
    );
    assert!(not_author(resolve(bob.id, false).await.map(|_| ())));
    let after = store.comment_threads(p).await.unwrap();
    assert_eq!(after[0].resolved_by.as_ref().map(|a| a.id), Some(alice.id));
    assert_eq!(after[0].comments[0].body, "De Bob, corrigé");
}

/// Supprimer un message emporte ses réactions ; supprimer le dernier emporte son fil, dans la
/// même transaction.
#[tokio::test]
async fn deleting_the_last_message_deletes_the_thread() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let thread = store.create_thread(p, None, alice.id, "Un").await.unwrap();
    let first = thread.comments[0].id;
    let second = store
        .reply_to_thread(p, thread.id, bob.id, "Deux")
        .await
        .unwrap()
        .id;
    for message in [first, second] {
        store
            .add_reaction(p, thread.id, message, bob.id, "👍")
            .await
            .unwrap();
    }

    let thread_gone = store
        .delete_comment(p, thread.id, first, Authorship::Of(alice.id))
        .await
        .unwrap();
    assert!(!thread_gone);
    let left = store.comment_threads(p).await.unwrap();
    assert_eq!(
        left.iter().map(thread_view).collect::<Vec<_>>(),
        vec![(
            None,
            None,
            vec![view("bob", "Deux", false, &[("👍", &["bob"])])]
        )]
    );
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM comment_reactions").await,
        1
    );

    let thread_gone = store
        .delete_comment(p, thread.id, second, Authorship::Any)
        .await
        .unwrap();
    assert!(thread_gone);
    assert!(store.comment_threads(p).await.unwrap().is_empty());
    for table in ["comment_threads", "comments", "comment_reactions"] {
        let count = int(&f.db, &format!("SELECT COUNT(*) FROM {table}")).await;
        assert_eq!(count, 0, "{table}");
    }
}

/// Une réaction se pose une fois par compte et par emoji : la poser deux fois n'en fait
/// qu'une, la retirer deux fois ne retire rien la seconde. Chacun ne retire que la sienne.
#[tokio::test]
async fn same_reaction_twice_is_one_reaction() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let thread = store.create_thread(p, None, alice.id, "Un").await.unwrap();
    let message = thread.comments[0].id;
    let react = async |who: Uuid, on: bool| {
        let (comment, changed) = if on {
            store.add_reaction(p, thread.id, message, who, "👍").await
        } else {
            store
                .remove_reaction(p, thread.id, message, who, "👍")
                .await
        }
        .unwrap();
        (comment_view(&comment).3, changed)
    };
    let thumbs = |names: &[&str]| -> Vec<ReactionView> {
        if names.is_empty() {
            return Vec::new();
        }
        vec![(
            "👍".to_string(),
            names.iter().map(|n| n.to_string()).collect(),
        )]
    };

    assert_eq!(react(alice.id, true).await, (thumbs(&["alice"]), true));
    assert_eq!(react(alice.id, true).await, (thumbs(&["alice"]), false));
    assert_eq!(react(bob.id, true).await, (thumbs(&["alice", "bob"]), true));
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM comment_reactions").await,
        2
    );
    // Bob retire la sienne, pas celle d'Alice ; une seconde fois, rien.
    assert_eq!(react(bob.id, false).await, (thumbs(&["alice"]), true));
    assert_eq!(react(bob.id, false).await, (thumbs(&["alice"]), false));
    assert_eq!(react(alice.id, false).await, (thumbs(&[]), true));
    assert_eq!(
        int(&f.db, "SELECT COUNT(*) FROM comment_reactions").await,
        0
    );
}

/// Une page supprimée pour de bon emporte ses fils, leurs messages et leurs réactions ; ceux
/// d'une autre page restent.
#[tokio::test]
async fn threads_of_a_deleted_page_are_gone() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let p = new_page(&f, "Jetée").await;
    let kept = new_page(&f, "Gardée").await;
    for node in [p, kept] {
        let thread = store
            .create_thread(node, Some("bloc"), alice.id, "Un")
            .await
            .unwrap();
        let reply = store
            .reply_to_thread(node, thread.id, alice.id, "Deux")
            .await
            .unwrap();
        store
            .add_reaction(node, thread.id, reply.id, alice.id, "👍")
            .await
            .unwrap();
    }
    let kept_threads = store.comment_threads(kept).await.unwrap();

    store.trash_node(p, alice.id).await.unwrap();
    store.delete_node(p).await.unwrap();

    assert!(store.comment_threads(p).await.unwrap().is_empty());
    assert_eq!(store.comment_threads(kept).await.unwrap(), kept_threads);
    for (table, left) in [
        ("comment_threads", 1),
        ("comments", 2),
        ("comment_reactions", 1),
    ] {
        let count = int(&f.db, &format!("SELECT COUNT(*) FROM {table}")).await;
        assert_eq!(count, left, "{table}");
    }
}
