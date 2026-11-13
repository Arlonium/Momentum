//! Les fils de commentaires d'une page (Ruling R134) : un fil, ses messages et leurs réactions,
//! relus dans l'ordre ; ce que le store vérifie dans la transaction qui écrit (le fil est de
//! cette page, le message de ce fil, et de ce compte quand on le demande) ; ce qui part avec un
//! message, avec un fil, avec une page.

mod common;

use common::{Fixture, account, fresh, int, page};
use mementum_store::{
    Authorship, Comment, CommentLimit, MAX_MESSAGES_PER_ACCOUNT, MAX_MESSAGES_PER_PAGE,
    MAX_MESSAGES_PER_THREAD, MAX_PAGE_REACTIONS_PER_ACCOUNT, MAX_REACTIONS_PER_ACCOUNT,
    MAX_REACTIONS_PER_MESSAGE, MAX_REACTIONS_PER_PAGE, MAX_TEXT_BYTES_PER_ACCOUNT,
    MAX_TEXT_BYTES_PER_PAGE, MAX_THREADS_PER_ACCOUNT, MAX_THREADS_PER_PAGE, StoreError, Thread,
};
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
    let (edited, changed) = store
        .edit_comment(p, first.id, reply.id, bob.id, "Réponse corrigée")
        .await
        .unwrap();
    assert!(changed);
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
    let (edited, _) = store
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

/// Modifier un message avec le texte qu'il a déjà ne change rien : ni le texte, ni la date de
/// modification, et le store le dit (`false`). Un autre texte, si.
#[tokio::test]
async fn editing_without_a_change_writes_nothing() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let p = new_page(&f, "Page").await;
    let thread = store.create_thread(p, None, alice.id, "Un").await.unwrap();
    let message = thread.comments[0].id;
    let edit = async |body: &str| {
        store
            .edit_comment(p, thread.id, message, alice.id, body)
            .await
            .unwrap()
    };

    let (same, changed) = edit("Un").await;
    assert!(!changed);
    assert_eq!(same, thread.comments[0]);
    let (edited, changed) = edit("Deux").await;
    assert!(changed && edited.edited_at.is_some());
    let (again, changed) = edit("Deux").await;
    assert!(!changed);
    assert_eq!(again, edited);
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

/// Écrit `count` fils sur la page, résolus, par du SQL direct : préparer une page pleine sans
/// passer par 2 000 transactions. Le fil `i` (de 1 à `count`) est ouvert par
/// `authors[i % authors.len()]`. Leurs identifiants ne sont pas des UUID : on ne les nomme pas.
async fn fill_threads(f: &Fixture, page: Uuid, authors: &[Uuid], count: u32) {
    let names: Vec<String> = authors.iter().map(|a| format!("\"{a}\"")).collect();
    let (names, n) = (names.join(","), authors.len());
    common::exec_sql(
        &f.db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {count}) \
             INSERT INTO comment_threads (id, node_id, created_by, created_at, resolved_at, \
               resolved_by) \
             SELECT printf('fil-{page}-%05d', i), '{page}', a, i, i, a \
             FROM (SELECT i, json_extract('[{names}]', printf('$[%d]', i % {n})) AS a FROM n)"
        ),
    )
    .await;
}

/// Écrit `count` messages de `author` sur la page, par du SQL direct, répartis dans `threads`
/// fils neufs qu'il ouvre ; chaque message porte `body`, une expression SQL. Les identifiants
/// ne sont pas des UUID : on ne les nomme pas.
async fn fill_messages(
    f: &Fixture,
    page: Uuid,
    author: Uuid,
    threads: u32,
    count: u32,
    body: &str,
) {
    common::exec_sql(
        &f.db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {threads}) \
             INSERT INTO comment_threads (id, node_id, created_by, created_at) \
             SELECT printf('plein-{page}-%05d', i), '{page}', '{author}', i FROM n"
        ),
    )
    .await;
    common::exec_sql(
        &f.db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < {count} - 1) \
             INSERT INTO comments (id, thread_id, author_id, body, created_at) \
             SELECT printf('message-{page}-%06d', i), \
                    printf('plein-{page}-%05d', i % {threads} + 1), '{author}', {body}, i FROM n"
        ),
    )
    .await;
}

/// Le refus attendu d'un plafond, et rien d'autre.
fn limited<T: std::fmt::Debug>(result: Result<T, StoreError>, limit: CommentLimit) -> bool {
    matches!(result, Err(StoreError::CommentLimit(l)) if l == limit)
}

/// Ruling R204 : une page porte au plus `MAX_THREADS_PER_PAGE` fils, résolus compris. Le
/// suivant est refusé dans la transaction qui l'écrirait (`CommentLimit::Threads`), et rien
/// n'est écrit ; une autre page n'en est pas gênée. Les fils de départ sont ouverts par dix
/// comptes, dont aucun n'a atteint sa part (`MAX_THREADS_PER_ACCOUNT`) : c'est bien le
/// plafond de la page qui refuse.
#[tokio::test]
async fn threads_of_a_page_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let mut authors = Vec::new();
    for i in 0..10 {
        authors.push(account(store, &format!("compte-{i}")).await.id);
    }
    let zed = account(store, "zed").await;
    let full = new_page(&f, "Pleine").await;
    let other = new_page(&f, "Autre").await;
    assert_eq!(MAX_THREADS_PER_PAGE, 2_000);
    // Le premier compte en ouvre 199, les neuf autres 200.
    fill_threads(&f, full, &authors, MAX_THREADS_PER_PAGE - 1).await;

    store
        .create_thread(full, None, authors[0], "Le dernier permis")
        .await
        .expect("la borne est permise");
    let refused = store
        .create_thread(full, Some("bloc"), zed.id, "Un de trop")
        .await;
    assert!(
        matches!(
            refused,
            Err(StoreError::CommentLimit(
                mementum_store::CommentLimit::Threads
            ))
        ),
        "{refused:?}"
    );
    let count = format!("SELECT COUNT(*) FROM comment_threads WHERE node_id = '{full}'");
    assert_eq!(int(&f.db, &count).await, 2_000);
    assert_eq!(
        int(
            &f.db,
            "SELECT COUNT(*) FROM comments WHERE body = 'Un de trop'"
        )
        .await,
        0
    );
    store
        .create_thread(other, None, zed.id, "Ailleurs")
        .await
        .expect("une autre page");
}

/// Ruling R204 : un fil porte au plus `MAX_MESSAGES_PER_THREAD` messages, le premier compris ;
/// la réponse suivante est refusée (`CommentLimit::Messages`), et passe de nouveau quand un
/// message est supprimé.
#[tokio::test]
async fn messages_of_a_thread_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let p = new_page(&f, "Page").await;
    let thread = store
        .create_thread(p, None, alice.id, "Premier")
        .await
        .unwrap();
    assert_eq!(MAX_MESSAGES_PER_THREAD, 500);
    let others = MAX_MESSAGES_PER_THREAD - 2;
    common::exec_sql(
        &f.db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {others}) \
             INSERT INTO comments (id, thread_id, author_id, body, created_at) \
             SELECT printf('message-%05d', i), '{}', '{}', 'Réponse', i FROM n",
            thread.id, alice.id
        ),
    )
    .await;

    let last = store
        .reply_to_thread(p, thread.id, alice.id, "La dernière permise")
        .await
        .expect("la borne est permise");
    let refused = store
        .reply_to_thread(p, thread.id, alice.id, "Une de trop")
        .await;
    assert!(
        matches!(
            refused,
            Err(StoreError::CommentLimit(
                mementum_store::CommentLimit::Messages
            ))
        ),
        "{refused:?}"
    );
    let count = format!(
        "SELECT COUNT(*) FROM comments WHERE thread_id = '{}'",
        thread.id
    );
    assert_eq!(int(&f.db, &count).await, 500);
    store
        .delete_comment(p, thread.id, last.id, Authorship::Any)
        .await
        .unwrap();
    store
        .reply_to_thread(p, thread.id, alice.id, "De nouveau")
        .await
        .expect("une place libérée");
}

/// Ruling R204 : un message porte au plus `MAX_REACTIONS_PER_MESSAGE` emojis différents. Un
/// emoji de plus est refusé (`CommentLimit::Reactions`) ; un emoji déjà là se pose encore, par
/// un autre compte ; une réaction déjà posée ne change rien, sans erreur, à la borne comme
/// ailleurs. Les 32 emojis sont posés par quatre comptes, huit chacun (leur part,
/// `MAX_REACTIONS_PER_ACCOUNT`) ; Bob, qui n'en a posé aucun, est refusé par le plafond du
/// message.
#[tokio::test]
async fn reactions_of_a_message_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let mut posers = vec![alice.id];
    for i in 1..4 {
        posers.push(account(store, &format!("compte-{i}")).await.id);
    }
    let p = new_page(&f, "Page").await;
    let thread = store.create_thread(p, None, alice.id, "Un").await.unwrap();
    let message = thread.comments[0].id;
    assert_eq!(MAX_REACTIONS_PER_MESSAGE, 32);
    let emojis: Vec<String> = (0..=MAX_REACTIONS_PER_MESSAGE)
        .map(|i| char::from_u32(0x1F600 + i).unwrap().to_string())
        .collect();
    for (i, emoji) in emojis[..32].iter().enumerate() {
        let (_, changed) = store
            .add_reaction(p, thread.id, message, posers[i / 8], emoji)
            .await
            .expect("sous la borne");
        assert!(changed);
    }
    let refused = store
        .add_reaction(p, thread.id, message, bob.id, &emojis[32])
        .await;
    assert!(
        matches!(
            refused,
            Err(StoreError::CommentLimit(
                mementum_store::CommentLimit::Reactions
            ))
        ),
        "{refused:?}"
    );
    let (_, changed) = store
        .add_reaction(p, thread.id, message, bob.id, &emojis[0])
        .await
        .expect("un emoji déjà là");
    assert!(changed);
    let (comment, changed) = store
        .add_reaction(p, thread.id, message, alice.id, &emojis[0])
        .await
        .expect("déjà posée");
    assert!(!changed);
    assert_eq!(comment.reactions.len(), 32);
    assert_eq!(
        int(&f.db, "SELECT COUNT(DISTINCT emoji) FROM comment_reactions").await,
        32
    );
}

/// Ruling R350 : une page porte au plus `MAX_MESSAGES_PER_PAGE` messages, tous fils compris.
/// Le suivant est refusé dans la transaction qui l'écrirait (`CommentLimit::PageMessages`), en
/// réponse comme en fil neuf, même quand le fil a encore de la place ; rien n'est écrit ;
/// modifier un message passe encore ; une autre page n'en est pas gênée ; un message supprimé
/// rend sa place.
#[tokio::test]
async fn messages_of_a_page_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let full = new_page(&f, "Pleine").await;
    let other = new_page(&f, "Autre").await;
    assert_eq!(MAX_MESSAGES_PER_PAGE, 10_000);
    let thread = store
        .create_thread(full, None, alice.id, "Premier")
        .await
        .unwrap();
    // 9 998 messages de plus, dans 21 fils : aucun fil n'atteint ses 500 messages.
    fill_messages(&f, full, bob.id, 21, MAX_MESSAGES_PER_PAGE - 2, "'Message'").await;
    let count = format!(
        "SELECT COUNT(*) FROM comments c JOIN comment_threads t ON t.id = c.thread_id \
         WHERE t.node_id = '{full}'"
    );
    assert_eq!(int(&f.db, &count).await, 9_999);

    let last = store
        .reply_to_thread(full, thread.id, alice.id, "La dernière permise")
        .await
        .expect("la borne est permise");
    let refused = store
        .reply_to_thread(full, thread.id, alice.id, "Une de trop")
        .await;
    assert!(limited(refused, CommentLimit::PageMessages));
    let refused = store
        .create_thread(full, Some("bloc"), alice.id, "Un fil de trop")
        .await;
    assert!(limited(refused, CommentLimit::PageMessages));
    assert_eq!(int(&f.db, &count).await, 10_000);
    let threads = format!("SELECT COUNT(*) FROM comment_threads WHERE node_id = '{full}'");
    assert_eq!(int(&f.db, &threads).await, 22);
    let (edited, changed) = store
        .edit_comment(full, thread.id, last.id, alice.id, "Modifiée")
        .await
        .expect("modifier ne compte pas un message de plus");
    assert!(changed);
    assert_eq!(edited.body, "Modifiée");
    store
        .create_thread(other, None, alice.id, "Ailleurs")
        .await
        .expect("une autre page");

    store
        .delete_comment(full, thread.id, last.id, Authorship::Any)
        .await
        .unwrap();
    store
        .reply_to_thread(full, thread.id, alice.id, "De nouveau")
        .await
        .expect("une place libérée");
}

/// Ruling R350 : les messages d'une page portent au plus `MAX_TEXT_BYTES_PER_PAGE` octets de
/// texte, comptés en UTF-8 (un « é » en vaut deux). L'écriture qui les dépasserait d'un octet
/// est refusée (`CommentLimit::PageText`), réponse, fil neuf ou modification qui allonge un
/// message ; une modification qui garde ou réduit la longueur passe ; rien n'est écrit.
#[tokio::test]
async fn text_of_a_page_is_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let full = new_page(&f, "Pleine").await;
    let other = new_page(&f, "Autre").await;
    assert_eq!(MAX_TEXT_BYTES_PER_PAGE, 4 * 1024 * 1024);
    let thread = store
        .create_thread(full, None, alice.id, "Premier")
        .await
        .unwrap();
    // 419 messages de 10 000 « x » : avec « Premier », 4 190 007 octets.
    fill_messages(
        &f,
        full,
        bob.id,
        1,
        419,
        "replace(hex(zeroblob(10000)), '00', 'x')",
    )
    .await;
    let used = 7 + 419 * 10_000;
    let left = MAX_TEXT_BYTES_PER_PAGE as usize - used;
    // Exactement ce qui reste, en octets : des « é » et un « x ». En caractères, c'est moitié
    // moins : un compte en caractères laisserait passer la suite.
    let exact = format!("{}x", "é".repeat((left - 1) / 2));
    assert_eq!(exact.len(), left);
    let bytes = format!(
        "SELECT SUM(length(CAST(c.body AS BLOB))) FROM comments c \
         JOIN comment_threads t ON t.id = c.thread_id WHERE t.node_id = '{full}'"
    );

    let last = store
        .reply_to_thread(full, thread.id, alice.id, &exact)
        .await
        .expect("la borne est permise");
    assert_eq!(int(&f.db, &bytes).await, i64::from(MAX_TEXT_BYTES_PER_PAGE));
    let refused = store.reply_to_thread(full, thread.id, alice.id, "a").await;
    assert!(limited(refused, CommentLimit::PageText));
    let refused = store.create_thread(full, None, alice.id, "a").await;
    assert!(limited(refused, CommentLimit::PageText));
    let longer = format!("{exact}a");
    let refused = store
        .edit_comment(full, thread.id, last.id, alice.id, &longer)
        .await;
    assert!(limited(refused, CommentLimit::PageText));
    assert_eq!(int(&f.db, &bytes).await, i64::from(MAX_TEXT_BYTES_PER_PAGE));
    let stored = format!("SELECT body FROM comments WHERE id = '{}'", last.id);
    assert_eq!(
        common::texts(&f.db, &stored).await,
        [Some(exact.clone())],
        "le message refusé n'a pas changé"
    );

    let same_length = format!("{}y", "é".repeat((left - 1) / 2));
    store
        .edit_comment(full, thread.id, last.id, alice.id, &same_length)
        .await
        .expect("même longueur");
    store
        .edit_comment(full, thread.id, last.id, alice.id, "court")
        .await
        .expect("plus court");
    store
        .reply_to_thread(full, thread.id, alice.id, "a")
        .await
        .expect("de la place libérée");
    store
        .create_thread(other, None, alice.id, &exact)
        .await
        .expect("une autre page");
}

/// Ruling R351 : un compte ouvre au plus `MAX_THREADS_PER_ACCOUNT` fils sur une page, résolus
/// compris (`CommentLimit::OwnThreads`) : répondre n'en ouvre pas ; sa part ne gêne ni les
/// autres comptes sur la page, ni lui sur une autre page ; un de ses fils supprimé lui rend une
/// place.
#[tokio::test]
async fn threads_of_an_account_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let other = new_page(&f, "Autre").await;
    assert_eq!(MAX_THREADS_PER_ACCOUNT, 200);
    fill_threads(&f, p, &[alice.id], MAX_THREADS_PER_ACCOUNT - 1).await;

    let last = store
        .create_thread(p, None, alice.id, "Le dernier permis")
        .await
        .expect("la borne est permise");
    let refused = store
        .create_thread(p, Some("bloc"), alice.id, "Un de trop")
        .await;
    assert!(limited(refused, CommentLimit::OwnThreads));
    let hers = format!(
        "SELECT COUNT(*) FROM comment_threads WHERE node_id = '{p}' AND created_by = '{}'",
        alice.id
    );
    assert_eq!(int(&f.db, &hers).await, 200);
    assert_eq!(
        int(
            &f.db,
            "SELECT COUNT(*) FROM comments WHERE body = 'Un de trop'"
        )
        .await,
        0
    );
    // Répondre n'ouvre pas de fil : Alice répond encore, chez elle comme chez Bob.
    let answer = store
        .reply_to_thread(p, last.id, alice.id, "Une réponse")
        .await
        .expect("une réponse");
    let bobs = store
        .create_thread(p, None, bob.id, "La part de Bob")
        .await
        .expect("un autre compte");
    store
        .reply_to_thread(p, bobs.id, alice.id, "Chez Bob")
        .await
        .expect("une réponse");
    store
        .create_thread(other, None, alice.id, "Ailleurs")
        .await
        .expect("une autre page");

    // Un fil part avec son dernier message : la place revient.
    for comment in [last.comments[0].id, answer.id] {
        store
            .delete_comment(p, last.id, comment, Authorship::Any)
            .await
            .unwrap();
    }
    store
        .create_thread(p, None, alice.id, "De nouveau")
        .await
        .expect("une place libérée");
}

/// Ruling R351 : un compte pose au plus `MAX_REACTIONS_PER_ACCOUNT` emojis différents sur un
/// message (`CommentLimit::OwnReactions`), y compris un emoji qu'un autre y a déjà posé ; une
/// réaction qu'il a déjà posée ne change rien, sans erreur ; sa part ne gêne pas les autres ;
/// une réaction retirée lui rend une place.
#[tokio::test]
async fn reactions_of_an_account_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let thread = store.create_thread(p, None, alice.id, "Un").await.unwrap();
    let message = thread.comments[0].id;
    assert_eq!(MAX_REACTIONS_PER_ACCOUNT, 8);
    let emojis: Vec<String> = (0..=MAX_REACTIONS_PER_ACCOUNT)
        .map(|i| char::from_u32(0x1F600 + i).unwrap().to_string())
        .collect();
    let react =
        async |who: Uuid, emoji: &str| store.add_reaction(p, thread.id, message, who, emoji).await;
    react(bob.id, &emojis[8]).await.expect("Bob");
    for emoji in &emojis[..8] {
        let (_, changed) = react(alice.id, emoji).await.expect("sous la borne");
        assert!(changed);
    }

    assert!(limited(
        react(alice.id, &emojis[8]).await,
        CommentLimit::OwnReactions
    ));
    let (_, changed) = react(alice.id, &emojis[0]).await.expect("déjà posée");
    assert!(!changed);
    let hers = format!(
        "SELECT COUNT(*) FROM comment_reactions WHERE user_id = '{}'",
        alice.id
    );
    assert_eq!(int(&f.db, &hers).await, 8);
    for emoji in &emojis[..7] {
        let (_, changed) = react(bob.id, emoji).await.expect("la part de Bob");
        assert!(changed);
    }

    store
        .remove_reaction(p, thread.id, message, alice.id, &emojis[0])
        .await
        .unwrap();
    let (comment, changed) = react(alice.id, &emojis[8])
        .await
        .expect("une place libérée");
    assert!(changed);
    assert_eq!(comment.reactions.len(), 9);
}

/// Ruling R350 et R351 : des écritures de front à la borne, chacune dans sa transaction ; une
/// seule passe. Le dernier message d'une page (des réponses et des fils neufs mêlés), puis le
/// dernier fil d'un compte.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_side_by_side_at_the_bound_let_one_through() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let full = new_page(&f, "Pleine").await;
    let thread = store
        .create_thread(full, None, alice.id, "Premier")
        .await
        .unwrap();
    fill_messages(&f, full, bob.id, 21, MAX_MESSAGES_PER_PAGE - 2, "'Message'").await;

    let writes: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            let (alice, thread) = (alice.id, thread.id);
            tokio::spawn(async move {
                if i % 2 == 0 {
                    store
                        .reply_to_thread(full, thread, alice, "De front")
                        .await
                        .map(|_| ())
                } else {
                    store
                        .create_thread(full, None, alice, "De front")
                        .await
                        .map(|_| ())
                }
            })
        })
        .collect();
    let mut passed = 0;
    for write in writes {
        match write.await.unwrap() {
            Ok(()) => passed += 1,
            refused => assert!(limited(refused, CommentLimit::PageMessages)),
        }
    }
    assert_eq!(passed, 1);
    let count = format!(
        "SELECT COUNT(*) FROM comments c JOIN comment_threads t ON t.id = c.thread_id \
         WHERE t.node_id = '{full}'"
    );
    assert_eq!(int(&f.db, &count).await, 10_000);

    let p = new_page(&f, "Page").await;
    fill_threads(&f, p, &[bob.id], MAX_THREADS_PER_ACCOUNT - 1).await;
    let writes: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let bob = bob.id;
            tokio::spawn(async move {
                store
                    .create_thread(p, None, bob, "De front")
                    .await
                    .map(|_| ())
            })
        })
        .collect();
    let mut passed = 0;
    for write in writes {
        match write.await.unwrap() {
            Ok(()) => passed += 1,
            refused => assert!(limited(refused, CommentLimit::OwnThreads)),
        }
    }
    assert_eq!(passed, 1);
}

/// Mesure, pas un test (Ruling R350) : ce que coûtent les plafonds de la page à une écriture
/// sur une page pleine, en release. 10 000 messages de 419 octets (4 Mio de texte, le plus de
/// lignes à compter pour la somme des octets) ; cent réponses acceptées juste sous la borne
/// (chacune supprimée ensuite), puis cent refusées à la borne.
///
/// `cargo test --release -p mementum-store --test comments -- --ignored --nocapture measure_writes_on_a_full_page`
#[tokio::test]
#[ignore = "mesure, à lancer en --release avec --nocapture"]
async fn measure_writes_on_a_full_page() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let full = new_page(&f, "Pleine").await;
    let thread = store
        .create_thread(full, None, alice.id, "Premier")
        .await
        .unwrap();
    fill_messages(
        &f,
        full,
        bob.id,
        21,
        MAX_MESSAGES_PER_PAGE - 2,
        "replace(hex(zeroblob(419)), '00', 'x')",
    )
    .await;
    let centile = |times: &mut Vec<std::time::Duration>, percent: usize| {
        times.sort();
        let rank = (times.len() * percent).div_ceil(100).max(1);
        times[rank - 1].as_secs_f64() * 1000.0
    };
    let mut accepted = Vec::new();
    for _ in 0..100 {
        let start = std::time::Instant::now();
        let reply = store
            .reply_to_thread(full, thread.id, alice.id, "Sous la borne")
            .await
            .unwrap();
        accepted.push(start.elapsed());
        store
            .delete_comment(full, thread.id, reply.id, Authorship::Any)
            .await
            .unwrap();
    }
    store
        .reply_to_thread(full, thread.id, alice.id, "La dernière")
        .await
        .unwrap();
    let mut refused = Vec::new();
    for _ in 0..100 {
        let start = std::time::Instant::now();
        let result = store
            .reply_to_thread(full, thread.id, alice.id, "De trop")
            .await;
        refused.push(start.elapsed());
        assert!(limited(result, CommentLimit::PageMessages));
    }
    // Les réactions (Ruling R357) : 19 999 sur la page, de dix comptes ; une acceptée (puis
    // retirée), cent fois, puis cent refusées à la borne.
    for i in 0..10 {
        let filler = account(store, &format!("compte-{i}")).await;
        fill_reactions(&f, full, filler.id, 250, 8).await;
    }
    common::exec_sql(
        &f.db,
        &format!(
            "DELETE FROM comment_reactions WHERE comment_id = 'message-{full}-000000' \
             AND emoji = char(128512) AND user_id IN (SELECT id FROM users WHERE name = 'compte-0')"
        ),
    )
    .await;
    let message = thread.comments[0].id;
    let mut reacted = Vec::new();
    for _ in 0..100 {
        let start = std::time::Instant::now();
        store
            .add_reaction(full, thread.id, message, alice.id, "👍")
            .await
            .unwrap();
        reacted.push(start.elapsed());
        store
            .remove_reaction(full, thread.id, message, alice.id, "👍")
            .await
            .unwrap();
    }
    store
        .add_reaction(full, thread.id, message, alice.id, "👍")
        .await
        .unwrap();
    let mut react_refused = Vec::new();
    for _ in 0..100 {
        let start = std::time::Instant::now();
        let result = store
            .add_reaction(full, thread.id, message, bob.id, "🎉")
            .await;
        react_refused.push(start.elapsed());
        assert!(limited(result, CommentLimit::PageReactions));
    }
    for (what, times) in [
        ("réponse acceptée, 9 999 messages", &mut accepted),
        ("réponse refusée, 10 000 messages", &mut refused),
        ("réaction acceptée, 19 999 réactions", &mut reacted),
        ("réaction refusée, 20 000 réactions", &mut react_refused),
    ] {
        println!(
            "MESURE {what} (4 Mio de texte) : p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            centile(times, 50),
            centile(times, 95),
            centile(times, 100)
        );
    }
}

/// Ruling R352 : supprimer un fil emporte ses messages et leurs réactions, en une écriture ;
/// rien d'une autre page ni d'un autre fil. Un fil qui n'est pas de cette page, ou qui n'est
/// plus, rend `NotFound` sans rien supprimer.
#[tokio::test]
async fn deleting_a_thread_takes_its_messages_and_reactions() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let other = new_page(&f, "Autre").await;
    let doomed = store.create_thread(p, None, alice.id, "Un").await.unwrap();
    let reply = store
        .reply_to_thread(p, doomed.id, bob.id, "Deux")
        .await
        .unwrap();
    store
        .add_reaction(p, doomed.id, reply.id, alice.id, "👍")
        .await
        .unwrap();
    let kept = store
        .create_thread(p, Some("bloc"), bob.id, "Gardé")
        .await
        .unwrap();
    let elsewhere = store
        .create_thread(other, None, alice.id, "Ailleurs")
        .await
        .unwrap();

    assert!(matches!(
        store.delete_thread(other, doomed.id).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.comment_threads(p).await.unwrap().len(), 2);
    store.delete_thread(p, doomed.id).await.unwrap();
    assert_eq!(store.comment_threads(p).await.unwrap(), [kept]);
    assert_eq!(store.comment_threads(other).await.unwrap(), [elsewhere]);
    for (table, left) in [
        ("comment_threads", 2),
        ("comments", 2),
        ("comment_reactions", 0),
    ] {
        let count = int(&f.db, &format!("SELECT COUNT(*) FROM {table}")).await;
        assert_eq!(count, left, "{table}");
    }
    assert!(matches!(
        store.delete_thread(p, doomed.id).await,
        Err(StoreError::NotFound)
    ));
}

/// Pose, par du SQL direct, les réactions de `user` sur les `messages` premiers messages écrits
/// par [`fill_messages`] sur la page : `per` emojis chacun, toujours les mêmes.
async fn fill_reactions(f: &Fixture, page: Uuid, user: Uuid, messages: u32, per: u32) {
    common::exec_sql(
        &f.db,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < {messages} - 1), \
               e(k) AS (SELECT 0 UNION ALL SELECT k + 1 FROM e WHERE k < {per} - 1) \
             INSERT INTO comment_reactions (comment_id, user_id, emoji, created_at) \
             SELECT printf('message-{page}-%06d', i), '{user}', char(128512 + k), i FROM n, e"
        ),
    )
    .await;
}

/// Ruling R357 : une page porte au plus `MAX_REACTIONS_PER_PAGE` réactions, tous messages et
/// tous comptes compris (`CommentLimit::PageReactions`), même pour un emoji déjà posé sur le
/// message par un autre ; une réaction déjà posée ne change rien, sans erreur ; une réaction
/// retirée rend sa place.
#[tokio::test]
async fn reactions_of_a_page_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let other = new_page(&f, "Autre").await;
    assert_eq!(MAX_REACTIONS_PER_PAGE, 20_000);
    let thread = store.create_thread(p, None, alice.id, "Un").await.unwrap();
    let message = thread.comments[0].id;
    // 20 000 réactions de dix comptes (2 000 chacun, leur part) sur 250 messages, moins une.
    fill_messages(&f, p, bob.id, 1, 250, "'Message'").await;
    for i in 0..10 {
        let filler = account(store, &format!("compte-{i}")).await;
        fill_reactions(&f, p, filler.id, 250, 8).await;
    }
    common::exec_sql(
        &f.db,
        &format!("DELETE FROM comment_reactions WHERE comment_id = 'message-{p}-000000' AND emoji = char(128512) AND user_id IN (SELECT id FROM users WHERE name = 'compte-0')"),
    )
    .await;
    let reactions = "SELECT COUNT(*) FROM comment_reactions";
    assert_eq!(int(&f.db, reactions).await, 19_999);

    let react =
        async |who: Uuid, emoji: &str| store.add_reaction(p, thread.id, message, who, emoji).await;
    react(alice.id, "👍").await.expect("la borne est permise");
    assert!(limited(
        react(bob.id, "👍").await,
        CommentLimit::PageReactions
    ));
    assert!(limited(
        react(bob.id, "🎉").await,
        CommentLimit::PageReactions
    ));
    let (_, changed) = react(alice.id, "👍").await.expect("déjà posée");
    assert!(!changed);
    assert_eq!(int(&f.db, reactions).await, 20_000);
    let elsewhere = store
        .create_thread(other, None, bob.id, "Ailleurs")
        .await
        .unwrap();
    store
        .add_reaction(other, elsewhere.id, elsewhere.comments[0].id, bob.id, "👍")
        .await
        .expect("une autre page");

    store
        .remove_reaction(p, thread.id, message, alice.id, "👍")
        .await
        .unwrap();
    react(bob.id, "🎉").await.expect("une place libérée");
}

/// Ruling R358 : un compte écrit au plus `MAX_MESSAGES_PER_ACCOUNT` messages sur une page, fils
/// ouverts et réponses compris (`CommentLimit::OwnMessages`) ; sa part ne gêne ni les autres sur
/// la page, ni lui ailleurs ; modifier un des siens passe encore.
#[tokio::test]
async fn messages_of_an_account_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let other = new_page(&f, "Autre").await;
    assert_eq!(MAX_MESSAGES_PER_ACCOUNT, 2_000);
    let bobs = store.create_thread(p, None, bob.id, "Bob").await.unwrap();
    fill_messages(&f, p, alice.id, 5, MAX_MESSAGES_PER_ACCOUNT - 1, "'x'").await;

    let last = store
        .reply_to_thread(p, bobs.id, alice.id, "La dernière permise")
        .await
        .expect("la borne est permise");
    assert!(limited(
        store.reply_to_thread(p, bobs.id, alice.id, "De trop").await,
        CommentLimit::OwnMessages
    ));
    assert!(limited(
        store.create_thread(p, None, alice.id, "De trop").await,
        CommentLimit::OwnMessages
    ));
    let hers = format!(
        "SELECT COUNT(*) FROM comments WHERE author_id = '{}'",
        alice.id
    );
    assert_eq!(int(&f.db, &hers).await, 2_000);
    store
        .edit_comment(p, bobs.id, last.id, alice.id, "Modifiée")
        .await
        .expect("modifier n'écrit pas de message de plus");
    store
        .reply_to_thread(p, bobs.id, bob.id, "Bob répond encore")
        .await
        .expect("un autre compte");
    store
        .create_thread(other, None, alice.id, "Ailleurs")
        .await
        .expect("une autre page");
    store
        .delete_comment(p, bobs.id, last.id, Authorship::Any)
        .await
        .unwrap();
    store
        .reply_to_thread(p, bobs.id, alice.id, "De nouveau")
        .await
        .expect("une place libérée");
}

/// Ruling R358 : les messages d'un compte sur une page portent au plus
/// `MAX_TEXT_BYTES_PER_ACCOUNT` octets de texte (`CommentLimit::OwnText`), en réponse, en fil
/// neuf ou en modification qui allonge un des siens ; de même longueur ou plus court, il passe ;
/// les autres écrivent encore.
#[tokio::test]
async fn text_of_an_account_is_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    assert_eq!(MAX_TEXT_BYTES_PER_ACCOUNT, 1024 * 1024);
    let bobs = store.create_thread(p, None, bob.id, "Bob").await.unwrap();
    // 104 messages de 10 000 « x » : 1 040 000 octets. Il en reste 8 576.
    fill_messages(
        &f,
        p,
        alice.id,
        1,
        104,
        "replace(hex(zeroblob(10000)), '00', 'x')",
    )
    .await;
    let left = MAX_TEXT_BYTES_PER_ACCOUNT as usize - 1_040_000;
    let exact = "é".repeat(left / 2);
    assert_eq!(exact.len(), left);

    let last = store
        .reply_to_thread(p, bobs.id, alice.id, &exact)
        .await
        .expect("la borne est permise");
    assert!(limited(
        store.reply_to_thread(p, bobs.id, alice.id, "a").await,
        CommentLimit::OwnText
    ));
    assert!(limited(
        store.create_thread(p, None, alice.id, "a").await,
        CommentLimit::OwnText
    ));
    let longer = format!("{exact}a");
    assert!(limited(
        store
            .edit_comment(p, bobs.id, last.id, alice.id, &longer)
            .await,
        CommentLimit::OwnText
    ));
    let same_length = format!("{}yy", "é".repeat(left / 2 - 1));
    store
        .edit_comment(p, bobs.id, last.id, alice.id, &same_length)
        .await
        .expect("même longueur");
    store
        .reply_to_thread(p, bobs.id, bob.id, &exact)
        .await
        .expect("un autre compte");
    store
        .edit_comment(p, bobs.id, last.id, alice.id, "court")
        .await
        .expect("plus court");
    store
        .reply_to_thread(p, bobs.id, alice.id, "a")
        .await
        .expect("de la place libérée");
}

/// Ruling R358 : un compte pose au plus `MAX_PAGE_REACTIONS_PER_ACCOUNT` réactions sur une
/// page (`CommentLimit::OwnPageReactions`) ; une réaction déjà posée ne change rien ; les autres
/// réagissent encore ; une réaction retirée rend sa place.
#[tokio::test]
async fn page_reactions_of_an_account_are_capped() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    assert_eq!(MAX_PAGE_REACTIONS_PER_ACCOUNT, 2_000);
    let thread = store.create_thread(p, None, bob.id, "Bob").await.unwrap();
    let message = thread.comments[0].id;
    fill_messages(&f, p, bob.id, 1, 250, "'Message'").await;
    fill_reactions(&f, p, alice.id, 250, 8).await;
    common::exec_sql(
        &f.db,
        &format!(
            "DELETE FROM comment_reactions WHERE comment_id = 'message-{p}-000000' \
             AND emoji = char(128512) AND user_id = '{}'",
            alice.id
        ),
    )
    .await;
    let react =
        async |who: Uuid, emoji: &str| store.add_reaction(p, thread.id, message, who, emoji).await;

    react(alice.id, "👍").await.expect("la borne est permise");
    assert!(limited(
        react(alice.id, "🎉").await,
        CommentLimit::OwnPageReactions
    ));
    let (_, changed) = react(alice.id, "👍").await.expect("déjà posée");
    assert!(!changed);
    react(bob.id, "🎉").await.expect("un autre compte");
    let hers = format!(
        "SELECT COUNT(*) FROM comment_reactions WHERE user_id = '{}'",
        alice.id
    );
    assert_eq!(int(&f.db, &hers).await, 2_000);
    store
        .remove_reaction(p, thread.id, message, alice.id, "👍")
        .await
        .unwrap();
    react(alice.id, "🎉").await.expect("une place libérée");
}

/// Rulings R357 et R358 : des écritures de front à la part d'un compte ; une seule passe. Huit
/// réponses quand il lui reste un message, puis huit réactions quand il lui en reste une.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_side_by_side_at_an_account_part_let_one_through() {
    let f = fresh().await;
    let store = &f.store;
    let alice = account(store, "alice").await;
    let bob = account(store, "bob").await;
    let p = new_page(&f, "Page").await;
    let thread = store.create_thread(p, None, bob.id, "Bob").await.unwrap();
    fill_messages(&f, p, alice.id, 5, MAX_MESSAGES_PER_ACCOUNT - 1, "'x'").await;
    let writes: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let (alice, thread) = (alice.id, thread.id);
            tokio::spawn(async move {
                store
                    .reply_to_thread(p, thread, alice, "De front")
                    .await
                    .map(|_| ())
            })
        })
        .collect();
    let mut passed = 0;
    for write in writes {
        match write.await.unwrap() {
            Ok(()) => passed += 1,
            refused => assert!(limited(refused, CommentLimit::OwnMessages)),
        }
    }
    assert_eq!(passed, 1);

    let q = new_page(&f, "Réactions").await;
    let thread = store.create_thread(q, None, bob.id, "Bob").await.unwrap();
    let message = thread.comments[0].id;
    fill_messages(&f, q, bob.id, 1, 250, "'Message'").await;
    fill_reactions(&f, q, alice.id, 250, 8).await;
    common::exec_sql(
        &f.db,
        &format!(
            "DELETE FROM comment_reactions WHERE comment_id = 'message-{q}-000000' \
             AND emoji = char(128512) AND user_id = '{}'",
            alice.id
        ),
    )
    .await;
    let writes: Vec<_> = (0..8u32)
        .map(|i| {
            let store = store.clone();
            let (alice, thread) = (alice.id, thread.id);
            let emoji = char::from_u32(0x1F400 + i).unwrap().to_string();
            tokio::spawn(async move {
                store
                    .add_reaction(q, thread, message, alice, &emoji)
                    .await
                    .map(|_| ())
            })
        })
        .collect();
    let mut passed = 0;
    for write in writes {
        match write.await.unwrap() {
            Ok(()) => passed += 1,
            refused => assert!(limited(refused, CommentLimit::OwnPageReactions)),
        }
    }
    assert_eq!(passed, 1);
}
