//! L'écriture d'un agent dans l'acteur de sa page (Task S4 du lot 5) : la lecture et sa révision
//! (R1226), l'écriture atomique (R1229) en une commande, la version `before_agent` relue et écrite
//! hors de l'acteur entre deux commandes (R1234, R1257, R1572), la tranche qui sait quel jeton l'a
//! écrite, et `PUT /mmd` et la restauration par différence (R40, R188 levés en partie).
//!
//! Les tests qui disent « dix minutes plus tard » arrêtent l'horloge de tokio ([`StoppedClock`],
//! `common/clock.rs`) ; leurs attentes se bornent en temps réel ([`until`]).

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use common::clock::StoppedClock;
use common::*;
use mementum_crdt::PageDoc;
use mementum_crdt::ops::{Operation, Place, Reason, read_markdown};
use mementum_md::parse;
use mementum_store::{Version, VersionReason, Writer};
use mementum_sync::{ConnInfo, Edit, EditRefusal, Edited, Hub, OnConflict, PageRead, SyncConfig};
use sqlx::Row;
use uuid::Uuid;
use yrs::{Doc, Text, Transact, XmlElementRef, XmlFragment, XmlOut};

/// L'intervalle des versions de ces tests : celui du réglage par défaut.
const INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Attente maximale d'une condition, en temps réel.
const REAL_WAIT: Duration = Duration::from_secs(30);

fn config() -> SyncConfig {
    SyncConfig {
        version_every: INTERVAL,
        ..SyncConfig::default()
    }
}

/// Attend que `ready` soit vraie, [`REAL_WAIT`] de temps réel au plus.
async fn until(what: &str, mut ready: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + REAL_WAIT;
    while !ready().await {
        assert!(std::time::Instant::now() < deadline, "{what}");
        tokio::task::yield_now().await;
    }
}

/// Une connexion du compte `account`, qui peut écrire, synchronisée.
async fn connect(f: &Fixture, account: Uuid) -> Client {
    let conn = ConnInfo {
        actor: Some(account),
        ..writing_conn()
    };
    let client = Client::connect(&f.url_for(conn))
        .await
        .expect("connexion du client");
    until("le client n'a pas reçu l'état du serveur", async || {
        client.is_synced()
    })
    .await;
    client
}

/// Fait `send`, puis attend que le serveur ait traité ce que le client a envoyé : il traite les
/// messages d'une connexion dans l'ordre, et répond ensuite à la resynchronisation.
async fn heard(client: &Client, send: impl FnOnce()) {
    let before = client.step2_count();
    send();
    client.resync();
    until("le serveur n'a pas traité le message", async || {
        client.step2_count() > before
    })
    .await;
}

/// Tape `typed` à la fin du bloc de rang `rank`, puis attend que le serveur l'ait traité.
async fn types_in(client: &Client, rank: usize, typed: &str) {
    let block = client.blocks()[rank].id.clone();
    heard(client, || client.type_text(&block, typed)).await;
}

/// Attend que `client` voie le texte `expected` (Ruling R343 : avant de faire taper un client à la
/// suite d'une écriture du serveur, il doit l'avoir reçue).
async fn sees(client: &Client, expected: &str) {
    until(
        &format!("le client n'a pas reçu « {expected} »"),
        async || text(&client.blocks()) == expected,
    )
    .await;
}

/// L'acteur de la page s'arrête faute de connexion et de demande pendant `idle_close` (le modèle
/// de `tests/versions.rs`).
async fn actor_stops(f: &Fixture) {
    let deadline = std::time::Instant::now() + REAL_WAIT;
    let settle = std::time::Instant::now() + Duration::from_millis(100);
    while std::time::Instant::now() < settle {
        tokio::task::yield_now().await;
    }
    loop {
        tokio::time::advance(f.hub.config().idle_close + Duration::from_millis(1)).await;
        let round = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < round {
            if f.hub.live_pages().await.is_empty() {
                return;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "l'acteur de la page ne s'arrête pas"
        );
    }
}

async fn versions(f: &Fixture) -> Vec<Version> {
    f.store.list_versions(f.page).await.unwrap()
}

async fn version_text(f: &Fixture, id: Uuid) -> String {
    let state = f
        .store
        .get_version(f.page, id)
        .await
        .unwrap()
        .expect("la version");
    text(
        &PageDoc::from_updates([state.as_slice()])
            .expect("un état lisible")
            .read_blocks(),
    )
}

async fn live_text(f: &Fixture) -> String {
    text(&f.hub.read_blocks(f.page).await.unwrap())
}

/// Un agent : le compte `account`, par le jeton `token`.
fn agent(account: Uuid, token: Uuid) -> Writer {
    Writer {
        account: Some(account),
        token: Some(token),
    }
}

/// Une écriture de `writer`, sur la lecture `base` (sa révision et ses empreintes).
fn edit(writer: Writer, base: Option<&PageRead>, ops: Vec<Operation>) -> Edit {
    Edit {
        ops,
        base: base.map(|read| (read.revision.clone(), Some(read.hashes.clone()))),
        on_conflict: OnConflict::Rebase,
        writer,
        commented: None,
        alone: false,
    }
}

fn str_replace(old: &str, new: &str) -> Operation {
    Operation::StrReplace {
        old: old.into(),
        new: new.into(),
        anchor: None,
        replace_all: false,
    }
}

fn insert(at: Place, mmd: &str) -> Operation {
    Operation::Insert {
        at,
        blocks: read_markdown(0, mmd).unwrap(),
    }
}

fn replace_block(anchor: &str, mmd: &str) -> Operation {
    Operation::ReplaceBlock {
        anchor: anchor.into(),
        blocks: read_markdown(0, mmd).unwrap(),
        allow_deleting_content: false,
    }
}

fn replace_page(mmd: &str) -> Operation {
    Operation::ReplacePage {
        blocks: read_markdown(0, mmd).unwrap(),
        allow_deleting_content: false,
    }
}

async fn edits(f: &Fixture, edit: Edit) -> Edited {
    match f.hub.edit_page(f.page, edit).await {
        Ok(edited) => edited,
        Err(refusal) => panic!("écriture refusée : {refusal:?}"),
    }
}

/// Le nœud de contenu du bloc de premier niveau de rang `rank`.
fn content_of(doc: &Doc, rank: usize) -> XmlElementRef {
    let fragment = doc.get_or_insert_xml_fragment(mementum_crdt::FRAGMENT);
    let txn = doc.transact();
    let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
        panic!("pas de racine blockGroup");
    };
    let Some(XmlOut::Element(container)) = group.get(&txn, rank as u32) else {
        panic!("pas de bloc de rang {rank}");
    };
    let Some(XmlOut::Element(content)) = container.get(&txn, 0) else {
        panic!("pas de nœud de contenu");
    };
    content
}

/// Une frappe faite par l'onglet `tab` à la fin du bloc de rang `rank`, **sans l'envoyer** : sur
/// la page telle que l'onglet la voit, encore en route quand le serveur écrit.
fn keystroke(tab: &Client, rank: usize, typed: &str) -> Vec<u8> {
    let offline = detached(tab);
    let content = content_of(&offline, rank);
    let mut txn = offline.transact_mut();
    let Some(XmlOut::Text(text)) = content.get(&txn, 0) else {
        panic!("pas de texte dans le bloc de rang {rank}");
    };
    let end = text.len(&txn);
    text.insert(&mut txn, end, typed);
    txn.encode_update_v1()
}

/// Un témoin : un compte de plus sur la page, dont l'état de présence sert de barrière (R342).
/// La diffusion d'une page est ordonnée : quand un client voit la marque du témoin, tout ce que
/// le serveur a diffusé avant elle lui est déjà parvenu.
struct Witness {
    client: Client,
    marks: u32,
}

impl Witness {
    async fn join(f: &Fixture) -> Witness {
        Witness {
            client: connect(f, new_user(&f.db).await).await,
            marks: 0,
        }
    }

    async fn passed(&mut self, observers: &[&Client]) {
        self.marks += 1;
        let mark = format!(r#"{{"user":{{"name":"Témoin {}"}}}}"#, self.marks);
        self.client.set_awareness(&mark);
        let id = self.client.client_id();
        for observer in observers {
            until("la marque du témoin n'est pas arrivée", async || {
                observer.awareness_states().get(&id) == Some(&mark)
            })
            .await;
        }
    }
}

/// Le jeton par lequel chaque entrée du journal de la page a été écrite, dans l'ordre des `seq`.
async fn tokens_of_journal(f: &Fixture) -> Vec<Option<Uuid>> {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", f.db.display()))
        .await
        .unwrap();
    let tokens = sqlx::query("SELECT token_id FROM doc_updates WHERE node_id = ? ORDER BY seq")
        .bind(f.page.to_string())
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|row| {
            row.get::<Option<String>, _>(0)
                .map(|id| Uuid::parse_str(&id).unwrap())
        })
        .collect();
    pool.close().await;
    tokens
}

/// R1226 : la révision d'une lecture est celle de ses blocs, même quand une personne tape pendant
/// les lectures. Chaque révision lue est retrouvée parmi celles par lesquelles l'onglet est passé,
/// avec le texte qu'il avait alors.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_revision_read_is_the_revision_of_the_blocks_read() {
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd("Départ\n").await;
    let tab = f.client().await;
    sees(&tab, "Départ").await;
    let block = tab.blocks()[0].id.clone();
    let mut seen: HashMap<String, String> = HashMap::new();
    seen.insert(tab.page().unwrap().revision(), text(&tab.blocks()));
    let typed: String = "abcdefghijklmnopqrstuvwxyz".repeat(2);
    let end = format!("Départ{typed}");
    let (hub, page) = (f.hub.clone(), f.page);
    let goal = end.clone();
    let reader = tokio::spawn(async move {
        let mut reads = Vec::new();
        loop {
            let read = hub.read_page(page).await.unwrap();
            let done = text(&read.blocks) == goal;
            reads.push(read);
            if done {
                return reads;
            }
        }
    });
    // Une frappe à la fois, chacune traitée avant la suivante : elles tombent au fil des
    // lectures, qui s'enchaînent sans arrêt, et non en une rafale traitée entre deux lectures.
    for letter in typed.chars() {
        tab.type_text(&block, &letter.to_string());
        seen.insert(tab.page().unwrap().revision(), text(&tab.blocks()));
        processed(&tab).await;
    }
    let reads = reader.await.unwrap();
    assert!(reads.len() > 1);
    for read in &reads {
        assert_eq!(
            seen.get(&read.revision),
            Some(&text(&read.blocks)),
            "une révision lue n'est pas celle des blocs lus"
        );
    }
}

/// R1227, R1229 : une écriture d'agent et une frappe faite en même temps dans un autre bloc
/// arrivent toutes deux, chez le serveur et chez l'onglet.
#[tokio::test]
async fn an_agent_edit_and_a_keystroke_both_land() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    f.write_mmd("Un\n\nDeux\n").await;
    let tab = connect(&f, alice).await;
    sees(&tab, "Un\nDeux").await;
    // La frappe est faite avant l'écriture de l'agent, et arrive après elle.
    let typed = keystroke(&tab, 1, " trois");
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    edits(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_block(&first, "Uno\n")],
        ),
    )
    .await;
    heard(&tab, || tab.send_update(&typed)).await;
    assert_eq!(live_text(&f).await, "Uno\nDeux trois");
    tab.apply_local(&typed);
    sees(&tab, "Uno\nDeux trois").await;
    assert_eq!(
        text(&stored_page(&f.store, f.page).await.read_blocks()),
        "Uno\nDeux trois"
    );
}

/// R1229 : un appel dont une opération est refusée n'écrit rien : ni le journal, ni la page, ni
/// la diffusion (prouvé par un témoin, R342).
#[tokio::test]
async fn a_failing_operation_writes_nothing() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bob = new_user(&f.db).await;
    f.write_mmd("Un\n\nDeux\n").await;
    let observer = f.client().await;
    sees(&observer, "Un\nDeux").await;
    let mut witness = Witness::join(&f).await;
    let journal = f.store.load_doc(f.page).await.unwrap();
    let received = observer.updates_received();
    let read = f.hub.read_page(f.page).await.unwrap();
    let refused = f
        .hub
        .edit_page(
            f.page,
            edit(
                agent(bob, new_token(&f.db, bob).await),
                Some(&read),
                vec![str_replace("Un", "Uno"), str_replace("Quatre", "Cinq")],
            ),
        )
        .await;
    match refused {
        Err(EditRefusal::Operation {
            refusal, revision, ..
        }) => {
            assert_eq!(refusal.index, 1);
            assert_eq!(refusal.reason, Reason::NoMatch);
            assert_eq!(revision, read.revision);
        }
        other => panic!("{other:?}"),
    }
    witness.passed(&[&observer]).await;
    assert_eq!(
        observer.updates_received(),
        received,
        "quelque chose a été diffusé"
    );
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap(),
        journal,
        "le journal a changé"
    );
    assert_eq!(live_text(&f).await, "Un\nDeux");
    assert_eq!(
        f.hub.read_page(f.page).await.unwrap().revision,
        read.revision
    );
}

/// R1229, R454 : une écriture de plusieurs opérations part aux autres en un seul message.
#[tokio::test]
async fn an_agent_edit_is_one_message_for_the_others() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bob = new_user(&f.db).await;
    f.write_mmd("Un\n\nDeux\n").await;
    let observer = f.client().await;
    sees(&observer, "Un\nDeux").await;
    let mut witness = Witness::join(&f).await;
    let received = observer.updates_received();
    let read = f.hub.read_page(f.page).await.unwrap();
    edits(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![
                str_replace("Un", "Uno"),
                insert(Place::End, "Trois\n"),
                str_replace("Deux", "Dos"),
            ],
        ),
    )
    .await;
    witness.passed(&[&observer]).await;
    assert_eq!(observer.updates_received(), received + 1);
    assert_eq!(text(&observer.blocks()), "Uno\nDos\nTrois");
}

/// R1226, leçon 8 : la révision rendue est celle que lit ensuite qui lit la page, celle de la
/// page relue de son journal ; `before` est celle de la page sur laquelle l'écriture a été faite.
#[tokio::test]
async fn the_revision_returned_is_the_one_a_reader_gets_next() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bob = new_user(&f.db).await;
    f.write_mmd("Un\n").await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let edited = edits(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![insert(Place::End, "Deux\n")],
        ),
    )
    .await;
    assert_eq!(edited.before, read.revision);
    assert_ne!(edited.revision, read.revision);
    let next = f.hub.read_page(f.page).await.unwrap();
    assert_eq!(edited.revision, next.revision);
    assert_eq!(edited.blocks, next.blocks);
    assert_eq!(edited.hashes, next.hashes);
    assert_eq!(
        stored_page(&f.store, f.page).await.revision(),
        edited.revision
    );
}

/// R1234, règle 1 : une opération destructrice d'un jeton qui remplace ce qu'une personne a écrit
/// coupe d'abord une version `before_agent`, qui garde ce que l'opération remplace.
#[tokio::test]
async fn a_version_is_cut_before_an_agent_replaces_what_a_human_wrote() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    f.write_mmd("Un\n").await;
    let tab = connect(&f, alice).await;
    sees(&tab, "Un").await;
    types_in(&tab, 0, " humain").await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    let edited = edits(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_block(&first, "Remplacé\n")],
        ),
    )
    .await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::BeforeAgent);
    assert_eq!(edited.version, Some(kept[0].id));
    assert_eq!(version_text(&f, kept[0].id).await, "Un humain");
    assert!(kept[0].authors.iter().any(|a| a.id == alice));
    assert_eq!(live_text(&f).await, "Remplacé");
}

/// R1234, règle 1 : `str_replace`, `insert` et `move_block` ne coupent pas de version.
#[tokio::test]
async fn str_replace_insert_and_move_cut_no_version() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    f.write_mmd("Un\n\nDeux\n").await;
    let tab = connect(&f, alice).await;
    sees(&tab, "Un\nDeux").await;
    types_in(&tab, 0, " humain").await;
    let token = new_token(&f.db, bob).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let second = read.blocks[1].id.clone();
    edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![
                str_replace("humain", "agent"),
                insert(Place::End, "Trois\n"),
                Operation::MoveBlock {
                    anchor: second,
                    to: Place::Start,
                },
            ],
        ),
    )
    .await;
    assert!(versions(&f).await.is_empty());
    assert_eq!(live_text(&f).await, "Deux\nUn agent\nTrois");
}

/// R1234, règle 1 (R1278) : un jeton qui remplace ce qu'il a écrit seul depuis la dernière version
/// ne coupe rien ; un autre jeton qui le remplace en coupe une.
#[tokio::test]
async fn an_agent_replacing_what_it_wrote_alone_cuts_no_version() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let bob = new_user(&f.db).await;
    f.write_mmd("Un\n").await;
    actor_stops(&f).await;
    assert_eq!(
        versions(&f).await.len(),
        1,
        "la version de fin de l'écriture humaine"
    );
    let (a, b) = (new_token(&f.db, bob).await, new_token(&f.db, bob).await);
    let read = f.hub.read_page(f.page).await.unwrap();
    let edited = edits(
        &f,
        edit(
            agent(bob, a),
            Some(&read),
            vec![insert(Place::End, "Deux\n")],
        ),
    )
    .await;
    let second = edited.blocks[1].id.clone();
    let read = f.hub.read_page(f.page).await.unwrap();
    edits(
        &f,
        edit(
            agent(bob, a),
            Some(&read),
            vec![replace_block(&second, "Deux bis\n")],
        ),
    )
    .await;
    assert_eq!(
        versions(&f).await.len(),
        1,
        "une version pour un agent seul"
    );
    let read = f.hub.read_page(f.page).await.unwrap();
    edits(
        &f,
        edit(
            agent(bob, b),
            Some(&read),
            vec![replace_block(&second, "Deux ter\n")],
        ),
    )
    .await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::BeforeAgent);
    assert_eq!(version_text(&f, kept[0].id).await, "Un\nDeux bis");
}

/// R1234, règle 3 : un agent qui écrit toutes les trente et une secondes, l'acteur arrêté par
/// `idle_close` entre deux écritures, ne coupe aucune version `closing` une fois la page
/// versionnée, et une `periodic` au plus par intervalle. La page naît vide, sans version : au doute
/// (R1601), sa première tranche coupe sa `closing`, et c'est la seule. Le journal est daté par
/// l'horloge du système, que l'horloge arrêtée de tokio ne fait pas avancer : chaque entrée est
/// vieillie de trente et une secondes à chaque tour, l'acteur arrêté (la base n'est jamais récrite
/// sous un acteur vivant).
#[tokio::test]
async fn an_agent_writing_every_31_seconds_cuts_at_most_one_version_per_interval() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let bob = new_user(&f.db).await;
    let token = new_token(&f.db, bob).await;
    let rounds = 25u32;
    for round in 0..rounds {
        let read = f.hub.read_page(f.page).await.unwrap();
        edits(
            &f,
            edit(
                agent(bob, token),
                Some(&read),
                vec![insert(Place::End, &format!("Ligne {round}\n"))],
            ),
        )
        .await;
        actor_stops(&f).await;
        exec_sql(
            &f.db,
            "UPDATE doc_updates SET created_at = created_at - 31000",
        )
        .await;
        tokio::time::advance(Duration::from_secs(1)).await;
    }
    let mut kept = versions(&f).await;
    // La plus ancienne : la `closing` de la première tranche d'une page jamais versionnée.
    let first = kept.pop().expect("une version");
    assert_eq!(first.reason, VersionReason::Closing, "{first:?}");
    assert!(
        kept.iter().all(|v| v.reason == VersionReason::Periodic),
        "{kept:?}"
    );
    let elapsed = Duration::from_secs(31 * u64::from(rounds));
    assert!(
        kept.len() as u64 <= elapsed.as_secs() / INTERVAL.as_secs(),
        "{} versions en {elapsed:?}",
        kept.len()
    );
    assert_eq!(kept.len(), 1, "{kept:?}");
}

/// R1234, règle 3 : une tranche où une personne a écrit coupe toujours sa version de fin, même si
/// un agent y a écrit aussi.
#[tokio::test]
async fn a_slice_written_by_a_human_still_cuts_its_closing() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    let read = f.hub.read_page(f.page).await.unwrap();
    edits(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![insert(Place::End, "Agent\n")],
        ),
    )
    .await;
    f.hub
        .write_blocks(f.page, parse("Agent\n\nHumain\n").unwrap(), Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Closing);
    let mut ids: Vec<Uuid> = kept[0].authors.iter().map(|a| a.id).collect();
    ids.sort();
    let mut expected = vec![alice, bob];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(kept[0].agent_authors, [bob]);
}

/// R1234, règle 2 : après une compaction, la part de la tranche qu'elle a purgée ne dit plus qui
/// l'a écrite ; elle compte comme écrite par d'autres, et la première opération destructrice d'un
/// jeton coupe une version, même si ce jeton avait tout écrit.
#[tokio::test]
async fn after_a_compaction_the_slice_counts_as_written_by_others() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(SyncConfig {
        snapshot_every: 1,
        ..config()
    })
    .await;
    let bob = new_user(&f.db).await;
    let token = new_token(&f.db, bob).await;
    // Une page versionnée d'abord : une page jamais versionnée compte déjà comme écrite par
    // d'autres (R1601).
    f.write_mmd("Humain\n").await;
    actor_stops(&f).await;
    assert_eq!(versions(&f).await.len(), 1);
    let read = f.hub.read_page(f.page).await.unwrap();
    let edited = edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![insert(Place::End, "Agent\n")],
        ),
    )
    .await;
    f.hub.compacted(f.page).await.unwrap();
    actor_stops(&f).await;
    assert_eq!(
        versions(&f).await.len(),
        1,
        "une version de fin pour un agent seul"
    );
    let unversioned = f.store.unversioned(f.page).await.unwrap();
    assert!(unversioned.unknown_writers, "{unversioned:?}");
    let last = edited.blocks.last().unwrap().id.clone();
    let read = f.hub.read_page(f.page).await.unwrap();
    edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![replace_block(&last, "Remplacé\n")],
        ),
    )
    .await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::BeforeAgent);
}

/// R1601 (R1234, règle 2) : une page née avec son contenu (une duplication, une page créée en lot),
/// qu'aucune version ne garde et où personne n'a écrit depuis, ne dit pas qui a écrit ce contenu :
/// la première opération destructrice d'un jeton coupe une version qui le garde.
#[tokio::test]
async fn a_page_born_with_content_is_versioned_before_a_token_replaces_it() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bob = new_user(&f.db).await;
    let (born, _) = PageDoc::new_page();
    born.write_blocks(&parse("Contenu d'origine\n").unwrap())
        .unwrap();
    let space = f.store.create_space("Copies", None).await.unwrap();
    let page = f
        .store
        .create_node(mementum_store::NewNode {
            space_id: space.id,
            parent_id: None,
            after: None,
            kind: mementum_store::NodeKind::Page,
            title: "Copie".into(),
            created_by: None,
            grants: Vec::new(),
            doc: Some(born.encode_state()),
        })
        .await
        .unwrap()
        .id;
    let read = f.hub.read_page(page).await.unwrap();
    assert_eq!(text(&read.blocks), "Contenu d'origine");
    let first = read.blocks[0].id.clone();
    let edited = f
        .hub
        .edit_page(
            page,
            edit(
                agent(bob, new_token(&f.db, bob).await),
                Some(&read),
                vec![replace_block(&first, "Remplacé\n")],
            ),
        )
        .await
        .expect("l'écriture de l'agent");
    let kept = f.store.list_versions(page).await.unwrap();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::BeforeAgent);
    assert_eq!(edited.version, Some(kept[0].id));
    let state = f
        .store
        .get_version(page, kept[0].id)
        .await
        .unwrap()
        .unwrap();
    let blocks = PageDoc::from_updates([state.as_slice()])
        .unwrap()
        .read_blocks();
    assert_eq!(text(&blocks), "Contenu d'origine");
}

/// Une page dont un humain vient d'écrire le premier bloc (`humain`), et la lecture qu'en a faite
/// un agent : de quoi lancer une opération destructrice qui coupe une version.
async fn page_written_by_a_human(f: &Fixture, mmd: &str) -> (Client, Uuid) {
    let alice = new_user(&f.db).await;
    f.write_mmd(mmd).await;
    let tab = connect(f, alice).await;
    sees(&tab, &text(&parse(mmd).unwrap())).await;
    types_in(&tab, 0, " humain").await;
    (tab, alice)
}

/// Lance une écriture d'agent dans une tâche à elle, et attend que sa version `before_agent` soit
/// retenue par la garde du hub.
async fn edit_held_at_its_version(
    f: &Fixture,
    edit: Edit,
) -> tokio::task::JoinHandle<Result<Edited, EditRefusal>> {
    let (hub, page): (Arc<Hub>, Uuid) = (f.hub.clone(), f.page);
    let task = tokio::spawn(async move { hub.edit_page(page, edit).await });
    until("la version d'avant l'agent n'est pas retenue", async || {
        f.hub.agent_versions_waiting() == 1
    })
    .await;
    task
}

/// R1257, R1572 : une frappe envoyée pendant la version `before_agent` est servie avant la fin de
/// la version : un autre la voit pendant que la version est retenue (une barrière, pas une durée).
#[tokio::test]
async fn a_keystroke_during_the_before_agent_version_is_served() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (tab, _) = page_written_by_a_human(&f, "Un\n\nDeux\n").await;
    let observer = f.client().await;
    sees(&observer, "Un humain\nDeux").await;
    let bob = new_user(&f.db).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_block(&first, "Agent\n")],
        ),
    )
    .await;
    types_in(&tab, 1, " servi").await;
    sees(&observer, "Un humain\nDeux servi").await;
    assert_eq!(
        f.hub.agent_versions_waiting(),
        1,
        "la version n'est plus retenue"
    );
    drop(held);
    task.await.unwrap().expect("l'écriture de l'agent");
    sees(&observer, "Agent\nDeux servi").await;
}

/// R1572 c : la version porte le `seq` de l'état encodé : une frappe servie entre les deux
/// commandes n'y est pas, et reste dans la tranche en cours.
#[tokio::test]
async fn the_before_agent_version_covers_the_encoded_seq() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (tab, alice) = page_written_by_a_human(&f, "Un\n\nDeux\n").await;
    let bob = new_user(&f.db).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    let encoded_seq = f.store.unversioned(f.page).await.unwrap();
    assert!(encoded_seq.since.is_some());
    let seq_before = f
        .store
        .load_doc(f.page)
        .await
        .unwrap()
        .updates
        .last()
        .unwrap()
        .0;
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_block(&first, "Agent\n")],
        ),
    )
    .await;
    types_in(&tab, 1, " après").await;
    drop(held);
    task.await.unwrap().expect("l'écriture de l'agent");
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(
        kept[0].seq, seq_before,
        "la version ne couvre pas l'état encodé"
    );
    assert_eq!(version_text(&f, kept[0].id).await, "Un humain\nDeux");
    let after = f.store.unversioned(f.page).await.unwrap();
    assert!(
        after.since.is_some(),
        "la frappe d'entre les commandes est tenue pour versionnée"
    );
    assert!(after.authors.contains(&alice), "{after:?}");
    assert!(after.others);
}

/// R1572 d : une frappe qui continue, entre les deux commandes, dans le bloc que l'opération
/// remplace fait refuser l'écriture ; ce que la personne a tapé reste, rien de l'agent n'est écrit.
#[tokio::test]
async fn typing_on_in_a_touched_block_between_the_two_commands_refuses_the_write() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (tab, _) = page_written_by_a_human(&f, "Un\n\nDeux\n").await;
    let bob = new_user(&f.db).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_block(&first, "Agent\n")],
        ),
    )
    .await;
    types_in(&tab, 0, " encore").await;
    let journal = f.store.load_doc(f.page).await.unwrap();
    drop(held);
    let refused = task.await.unwrap();
    assert!(
        matches!(
            &refused,
            Err(EditRefusal::Operation { refusal, .. }) if refusal.reason == Reason::RevisionConflict
        ) || matches!(&refused, Err(EditRefusal::VersionRefused)),
        "{refused:?}"
    );
    assert_eq!(live_text(&f).await, "Un humain encore\nDeux");
    assert_eq!(
        f.store.load_doc(f.page).await.unwrap(),
        journal,
        "l'agent a écrit"
    );
}

/// R1234, règle 4 : un bloc que l'opération touche, changé pendant que la version s'écrit, fait
/// refuser l'écriture, ou recouper une version : jamais l'agent n'écrit par-dessus ce que la
/// version ne garde pas. Ici par `replace_page`, que la fusion à trois juge (R1231).
#[tokio::test]
async fn a_block_changed_while_the_version_is_written_refuses_or_cuts_again() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (tab, _) = page_written_by_a_human(&f, "Un\n\nDeux\n").await;
    let bob = new_user(&f.db).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_page("Un agent\n\nDeux\n")],
        ),
    )
    .await;
    types_in(&tab, 0, " encore").await;
    drop(held);
    let result = task.await.unwrap();
    let kept = versions(&f).await;
    match result {
        Err(EditRefusal::Operation { refusal, .. }) => {
            assert_eq!(refusal.reason, Reason::RevisionConflict);
            assert_eq!(live_text(&f).await, "Un humain encore\nDeux");
        }
        Err(EditRefusal::VersionRefused) => {
            assert_eq!(live_text(&f).await, "Un humain encore\nDeux");
        }
        Ok(_) => {
            assert_eq!(kept.len(), 2, "écrite sans recouper : {kept:?}");
            assert!(version_text(&f, kept[0].id).await.contains("encore"));
        }
        Err(other) => panic!("{other:?}"),
    }
}

/// R1562 : la seconde commande refait le plan avec la base de l'agent, jamais avec les
/// empreintes de l'état encodé. L'agent a lu la page, une personne a changé le second bloc, puis
/// l'agent récrit la page entière d'après sa lecture en ne changeant que le premier : la fusion à
/// trois n'écrit que le premier bloc, aux deux commandes, et ce que la personne a écrit reste.
#[tokio::test]
async fn the_second_command_plans_on_the_base_of_the_agent() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = new_user(&f.db).await;
    f.write_mmd("Un\n\nDeux\n\nTrois\n").await;
    let tab = connect(&f, alice).await;
    sees(&tab, "Un\nDeux\nTrois").await;
    let bob = new_user(&f.db).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    types_in(&tab, 1, " humain").await;
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_page("Un agent\n\nDeux\n\nTrois\n")],
        ),
    )
    .await;
    types_in(&tab, 2, " aussi").await;
    drop(held);
    task.await.unwrap().expect("l'écriture de l'agent");
    assert_eq!(live_text(&f).await, "Un agent\nDeux humain\nTrois aussi");
}

/// R1572 : une écriture sans version est une commande de l'acteur ; avec une version, deux.
#[tokio::test]
async fn a_non_destructive_edit_is_one_command() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (_tab, _) = page_written_by_a_human(&f, "Un\n").await;
    let bob = new_user(&f.db).await;
    let token = new_token(&f.db, bob).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let calls = f.hub.calls();
    edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![insert(Place::End, "Deux\n")],
        ),
    )
    .await;
    assert_eq!(f.hub.calls(), calls + 1);
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    let calls = f.hub.calls();
    let edited = edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![replace_block(&first, "Uno\n")],
        ),
    )
    .await;
    assert!(edited.version.is_some());
    assert_eq!(f.hub.calls(), calls + 2);
}

/// R1234 : les `before_agent` passent les premières à la purge, vingt au plus : sur une page qui
/// garde déjà cent versions ordinaires, vingt-cinq opérations destructrices d'un agent, chacune
/// après une écriture d'une personne, n'en font partir que vingt.
#[tokio::test]
async fn before_agent_versions_push_out_at_most_twenty_ordinary_ones() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    let state = stored_page(&f.store, f.page).await.encode_state();
    for _ in 0..mementum_store::MAX_VERSIONS {
        f.store
            .put_version(f.page, 0, &state, &[], VersionReason::Periodic)
            .await
            .unwrap();
    }
    let token = new_token(&f.db, bob).await;
    for round in 0..25 {
        f.hub
            .write_blocks(
                f.page,
                parse(&format!("Humain {round}\n")).unwrap(),
                Some(alice),
            )
            .await
            .unwrap();
        let read = f.hub.read_page(f.page).await.unwrap();
        let first = read.blocks[0].id.clone();
        let edited = edits(
            &f,
            edit(
                agent(bob, token),
                Some(&read),
                vec![replace_block(&first, &format!("Agent {round}\n"))],
            ),
        )
        .await;
        assert!(edited.version.is_some(), "tour {round}");
    }
    let kept = versions(&f).await;
    let agents = kept
        .iter()
        .filter(|v| v.reason == VersionReason::BeforeAgent)
        .count();
    let ordinary = kept
        .iter()
        .filter(|v| v.reason == VersionReason::Periodic)
        .count();
    assert_eq!(agents, mementum_store::MAX_BEFORE_AGENT);
    assert_eq!(
        ordinary,
        mementum_store::MAX_VERSIONS - mementum_store::MAX_BEFORE_AGENT
    );
}

/// R1234 : une écriture par jeton garde son jeton au journal, et la version qui la garde dit son
/// compte parmi ceux qui ont écrit par un agent.
#[tokio::test]
async fn a_token_edit_is_attributed_in_the_journal_and_in_the_version() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (tab, alice) = page_written_by_a_human(&f, "Un\n").await;
    drop(tab);
    let bob = new_user(&f.db).await;
    let token = new_token(&f.db, bob).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![insert(Place::End, "Deux\n")],
        ),
    )
    .await;
    assert_eq!(tokens_of_journal(&f).await.last(), Some(&Some(token)));
    assert_eq!(actors(&f.db, f.page).await.last(), Some(&Some(bob)));
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![replace_block(&first, "Uno\n")],
        ),
    )
    .await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    let mut authors: Vec<Uuid> = kept[0].authors.iter().map(|a| a.id).collect();
    authors.sort();
    let mut expected = vec![alice, bob];
    expected.sort();
    assert_eq!(authors, expected);
    assert_eq!(kept[0].agent_authors, [bob]);
    let journal = tokens_of_journal(&f).await;
    assert!(
        journal.iter().filter(|t| **t == Some(token)).count() >= 2,
        "{journal:?}"
    );
    assert!(
        journal.contains(&None),
        "l'écriture humaine a pris un jeton"
    );
}

/// R188, levé en partie : une restauration écrit par différence ; une frappe en route dans un
/// bloc qu'elle ne touche pas est gardée.
#[tokio::test]
async fn a_restore_keeps_a_keystroke_in_an_untouched_block() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    f.hub
        .write_blocks(f.page, parse("Un\n\nDeux\n").unwrap(), Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    let first = versions(&f).await[0].id;
    let tab = connect(&f, alice).await;
    sees(&tab, "Un\nDeux").await;
    types_in(&tab, 0, " plus").await;
    let typed = keystroke(&tab, 1, " trois");
    f.hub
        .restore_version(f.page, first, Some(alice))
        .await
        .unwrap();
    heard(&tab, || tab.send_update(&typed)).await;
    assert_eq!(live_text(&f).await, "Un\nDeux trois");
}

/// R40 : `PUT /mmd` écrit par différence ; une frappe en route dans un bloc qu'il ne touche pas
/// est gardée.
#[tokio::test]
async fn put_mmd_keeps_a_keystroke_in_an_untouched_block() {
    let f = Fixture::new(SyncConfig::default()).await;
    let alice = new_user(&f.db).await;
    f.write_mmd("Un\n\nDeux\n").await;
    let tab = connect(&f, alice).await;
    sees(&tab, "Un\nDeux").await;
    let typed = keystroke(&tab, 1, " trois");
    f.hub
        .write_blocks(f.page, parse("Uno\n\nDeux\n").unwrap(), Some(alice))
        .await
        .unwrap();
    heard(&tab, || tab.send_update(&typed)).await;
    assert_eq!(live_text(&f).await, "Uno\nDeux trois");
}

/// R1233 : `writers` compte les connexions qui peuvent écrire, sans charger la page : sans acteur
/// vivant, il rend 0 sans en lancer un.
#[tokio::test]
async fn writers_counts_without_loading_the_page() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    actor_stops(&f).await;
    let calls = f.hub.calls();
    assert_eq!(f.hub.writers(f.page).await, 0);
    assert!(f.hub.live_pages().await.is_empty(), "la page a été chargée");
    assert_eq!(f.hub.calls(), calls, "une demande est partie à un acteur");
    let writer = connect(&f, new_user(&f.db).await).await;
    let reader = Client::connect_synced(&f.url_for(ConnInfo {
        can_write: false,
        ..writing_conn()
    }))
    .await;
    assert_eq!(f.hub.writers(f.page).await, 1);
    drop(writer);
    until("la connexion qui écrit n'est pas partie", async || {
        f.hub.writers(f.page).await == 0
    })
    .await;
    drop(reader);
}

/// Une écriture d'agent qui ne change rien n'écrit rien : ni journal, ni diffusion, ni tranche.
#[tokio::test]
async fn an_agent_edit_that_changes_nothing_writes_nothing() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bob = new_user(&f.db).await;
    f.write_mmd("Un\n").await;
    let journal = f.store.load_doc(f.page).await.unwrap();
    let read = f.hub.read_page(f.page).await.unwrap();
    let edited = edits(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![str_replace("Un", "Un")],
        ),
    )
    .await;
    assert_eq!(edited.revision, read.revision);
    assert!(edited.touched.is_empty());
    assert_eq!(f.store.load_doc(f.page).await.unwrap(), journal);
}

/// R1231 : une opération destructrice sans révision est refusée ; une révision dépassée l'est en
/// `fail`, et en `rebase` quand ses empreintes ne sont plus connues.
#[tokio::test]
async fn a_destructive_edit_needs_a_revision_and_a_known_base() {
    let f = Fixture::new(SyncConfig::default()).await;
    let bob = new_user(&f.db).await;
    f.write_mmd("Un\n\nDeux\n").await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    let token = new_token(&f.db, bob).await;
    let refused = f
        .hub
        .edit_page(
            f.page,
            edit(agent(bob, token), None, vec![replace_block(&first, "X\n")]),
        )
        .await;
    assert!(
        matches!(refused, Err(EditRefusal::RevisionRequired)),
        "{refused:?}"
    );
    f.write_mmd("Un\n\nDeux bis\n").await;
    let mut fail = edit(
        agent(bob, token),
        Some(&read),
        vec![replace_block(&first, "X\n")],
    );
    fail.on_conflict = OnConflict::Fail;
    let refused = f.hub.edit_page(f.page, fail).await;
    assert!(
        matches!(refused, Err(EditRefusal::Conflict { .. })),
        "{refused:?}"
    );
    let mut unknown = edit(
        agent(bob, token),
        Some(&read),
        vec![replace_block(&first, "X\n")],
    );
    unknown.base = Some((read.revision.clone(), None));
    let refused = f.hub.edit_page(f.page, unknown).await;
    assert!(
        matches!(refused, Err(EditRefusal::Conflict { .. })),
        "{refused:?}"
    );
    // Rejouée sur la page d'aujourd'hui : le bloc remplacé n'a pas changé depuis la lecture.
    edits(
        &f,
        edit(
            agent(bob, token),
            Some(&read),
            vec![replace_block(&first, "X\n")],
        ),
    )
    .await;
    assert_eq!(live_text(&f).await, "X\nDeux bis");
}

/// Task S2 (R1294) : une écriture estimée au-delà du budget est refusée, sans rien écrire.
#[tokio::test]
async fn an_edit_beyond_the_budget_is_refused() {
    let f = Fixture::new(SyncConfig {
        agent_write_budget: Duration::from_nanos(1),
        ..SyncConfig::default()
    })
    .await;
    let bob = new_user(&f.db).await;
    f.write_mmd("Un\n").await;
    let journal = f.store.load_doc(f.page).await.unwrap();
    let read = f.hub.read_page(f.page).await.unwrap();
    let refused = f
        .hub
        .edit_page(
            f.page,
            edit(
                agent(bob, new_token(&f.db, bob).await),
                Some(&read),
                vec![insert(Place::End, "Deux\n")],
            ),
        )
        .await;
    assert!(
        matches!(refused, Err(EditRefusal::TooLong(_))),
        "{refused:?}"
    );
    assert_eq!(f.store.load_doc(f.page).await.unwrap(), journal);
}

/// R1336 : une frappe dans l'ancre d'un `insert` (un bloc que l'appel ne touche que par sa place),
/// entre les deux commandes d'une écriture qui coupe une version, ne fait pas recouper la version :
/// une seule `before_agent`, et l'écriture passe, la frappe gardée.
#[tokio::test]
async fn a_keystroke_in_an_insert_anchor_between_the_two_commands_cuts_one_version() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (tab, _) = page_written_by_a_human(&f, "Un\n\nDeux\n").await;
    let bob = new_user(&f.db).await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let (first, second) = (read.blocks[0].id.clone(), read.blocks[1].id.clone());
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![
                replace_block(&first, "Agent\n"),
                insert(Place::After(second), "Trois\n"),
            ],
        ),
    )
    .await;
    types_in(&tab, 1, " servi").await;
    drop(held);
    task.await.unwrap().expect("l'écriture de l'agent");
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::BeforeAgent);
    assert_eq!(live_text(&f).await, "Agent\nDeux servi\nTrois");
}

/// R1338 : `on_conflict: "fail"` sur la révision que l'agent a lue, quand le serveur coupe
/// l'écriture en deux commandes pour une version : une frappe pendant la version dans un bloc que
/// l'écriture ne touche pas n'est pas un conflit ; dans un bloc qu'elle remplace, si.
#[tokio::test]
async fn fail_is_judged_on_the_touched_blocks_across_the_two_commands() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (tab, _) = page_written_by_a_human(&f, "Un\n\nDeux\n").await;
    let bob = new_user(&f.db).await;
    let token = new_token(&f.db, bob).await;
    let failing = |read: &PageRead, mmd: &str| {
        let mut edit = edit(
            agent(bob, token),
            Some(read),
            vec![replace_block(&read.blocks[0].id, mmd)],
        );
        edit.on_conflict = OnConflict::Fail;
        edit
    };
    // Une frappe dans le second bloc : l'écriture passe.
    let read = f.hub.read_page(f.page).await.unwrap();
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(&f, failing(&read, "Agent\n")).await;
    types_in(&tab, 1, " servi").await;
    drop(held);
    task.await.unwrap().expect("l'écriture de l'agent");
    assert_eq!(live_text(&f).await, "Agent\nDeux servi");
    // Une frappe dans le bloc remplacé, par une personne : c'est un conflit, rien n'est écrit.
    types_in(&tab, 0, " humain").await;
    let read = f.hub.read_page(f.page).await.unwrap();
    let held = f.hub.hold_agent_versions();
    let task = edit_held_at_its_version(&f, failing(&read, "Encore l'agent\n")).await;
    types_in(&tab, 0, " encore").await;
    drop(held);
    let refused = task.await.unwrap();
    assert!(
        matches!(&refused, Err(EditRefusal::Operation { refusal, .. }) if refusal.reason == Reason::RevisionConflict),
        "{refused:?}"
    );
    assert_eq!(live_text(&f).await, "Agent humain encore\nDeux servi");
}

/// R1339 : une opération destructrice d'un jeton qui ne change rien (un bloc remplacé par lui-même)
/// ne coupe pas de version, et n'écrit rien.
#[tokio::test]
async fn a_destructive_edit_that_changes_nothing_cuts_no_version() {
    let f = Fixture::new(SyncConfig::default()).await;
    let (_tab, _) = page_written_by_a_human(&f, "Un\n").await;
    let bob = new_user(&f.db).await;
    let journal = f.store.load_doc(f.page).await.unwrap();
    let read = f.hub.read_page(f.page).await.unwrap();
    let first = read.blocks[0].id.clone();
    let edited = edits(
        &f,
        edit(
            agent(bob, new_token(&f.db, bob).await),
            Some(&read),
            vec![replace_block(&first, "Un humain\n")],
        ),
    )
    .await;
    assert!(edited.version.is_none());
    assert!(
        versions(&f).await.is_empty(),
        "une version pour une écriture sans effet"
    );
    assert_eq!(edited.revision, read.revision);
    assert_eq!(f.store.load_doc(f.page).await.unwrap(), journal);
}
