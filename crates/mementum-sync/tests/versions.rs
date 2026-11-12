//! L'historique d'une page (Ruling R135), côté acteur : quand une version est coupée, ce qu'elle
//! porte (l'état et les comptes qui ont écrit depuis la version d'avant), la restauration, et ce
//! que la compaction et un redémarrage n'en perdent pas.
//!
//! **L'horloge de tokio est arrêtée** ([`StoppedClock`]) : elle n'avance que quand le test
//! l'avance (`tokio::time::advance`), si bien que « dix minutes plus tard » ne dépend pas de la
//! vitesse du poste. Une horloge simplement en pause (`tokio::time::pause`) ne suffit pas : dès
//! que plus aucune tâche n'a de travail, tokio l'avance de lui-même jusqu'au prochain délai, et
//! c'est le cas à chaque lecture ou écriture dans SQLite, faite par un fil à part : les dix
//! minutes de la maintenance du pool de sqlx, les 30 s de son délai d'acquisition ou de
//! `idle_close` passeraient pendant une écriture. Une tâche qui cède la main sans fin garde le
//! moteur occupé, et l'horloge ne bouge plus d'elle-même. En contrepartie, aucun délai de tokio
//! ne finit tout seul : les attentes de ces tests se bornent en temps réel ([`until`]).

mod common;

use std::time::Duration;

use common::*;
use mementum_crdt::PageDoc;
use mementum_md::{Block, parse};
use mementum_store::{MAX_BEFORE_RESTORE, MAX_VERSIONS, Version, VersionReason};
use mementum_sync::{ConnInfo, SyncConfig, SyncError};
use tokio::task::JoinHandle;
use uuid::Uuid;
use yrs::{Text, Transact, XmlFragment, XmlOut};

/// L'intervalle des versions de ces tests : celui du réglage par défaut.
const INTERVAL: Duration = Duration::from_secs(10 * 60);

/// Attente maximale d'une condition, en temps réel.
const REAL_WAIT: Duration = Duration::from_secs(30);

/// L'horloge de tokio arrêtée, tant qu'elle vit (voir l'en-tête du fichier).
struct StoppedClock(JoinHandle<()>);

impl StoppedClock {
    fn start() -> StoppedClock {
        tokio::time::pause();
        StoppedClock(tokio::spawn(async {
            loop {
                tokio::task::yield_now().await;
            }
        }))
    }
}

impl Drop for StoppedClock {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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

/// Tape `typed` à la fin du premier bloc, puis attend que le serveur l'ait traité : il traite
/// les messages d'une connexion dans l'ordre, et répond ensuite à la resynchronisation.
async fn types(client: &Client, typed: &str) {
    let block = client.blocks()[0].id.clone();
    heard(client, || client.type_text(&block, typed)).await;
}

/// Fait `send`, puis attend que le serveur ait traité ce que le client a envoyé.
async fn heard(client: &Client, send: impl FnOnce()) {
    let before = client.step2_count();
    send();
    client.resync();
    until("le serveur n'a pas traité le message", async || {
        client.step2_count() > before
    })
    .await;
}

async fn advance(by: Duration) {
    tokio::time::advance(by).await;
}

/// L'acteur de la page s'arrête faute de connexion et de demande pendant `idle_close`, et
/// quitte le registre. Une connexion lâchée part un peu après (la socket se ferme, puis
/// l'acteur l'apprend) : tant que l'acteur n'est pas arrêté, l'horloge est poussée de nouveau,
/// une fois laissé à ce qu'il faisait le temps de finir (5 s réelles). La borne peut faire
/// échouer le test à tort sur un poste très chargé, jamais le faire passer.
async fn actor_stops(f: &Fixture) {
    let deadline = std::time::Instant::now() + REAL_WAIT;
    // Le temps qu'une connexion lâchée soit partie.
    let settle = std::time::Instant::now() + Duration::from_millis(100);
    while std::time::Instant::now() < settle {
        tokio::task::yield_now().await;
    }
    loop {
        advance(f.hub.config().idle_close + Duration::from_millis(1)).await;
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

/// Les blocs de l'état gardé par la version `id`.
async fn version_blocks(f: &Fixture, id: Uuid) -> Vec<Block> {
    let state = f
        .store
        .get_version(f.page, id)
        .await
        .unwrap()
        .expect("la version");
    PageDoc::from_updates([state.as_slice()])
        .expect("un état lisible")
        .read_blocks()
}

async fn version_text(f: &Fixture, id: Uuid) -> String {
    text(&version_blocks(f, id).await)
}

fn authors(version: &Version) -> Vec<Uuid> {
    version.authors.iter().map(|a| a.id).collect()
}

fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids
}

async fn live_text(f: &Fixture) -> String {
    text(&f.hub.read_blocks(f.page).await.unwrap())
}

/// Les trames `versions` qu'a reçues le client.
fn versions_frames(client: &Client) -> usize {
    client
        .page_events()
        .iter()
        .filter(|frame| frame.as_str() == r#"{"type":"versions"}"#)
        .count()
}

/// Des frappes pendant deux minutes, moins que l'intervalle : aucune version pendant la
/// session ; à l'arrêt de l'acteur, une seule, `closing`, qui porte l'état de la fin et les deux
/// comptes qui ont tapé.
#[tokio::test]
async fn burst_shorter_than_the_interval_is_one_version_at_stop() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    let (a, b) = (connect(&f, alice).await, connect(&f, bob).await);
    for i in 0..12 {
        let (client, typed) = if i % 2 == 0 { (&a, "a") } else { (&b, "b") };
        types(client, typed).await;
        advance(Duration::from_secs(10)).await;
    }
    assert!(
        versions(&f).await.is_empty(),
        "aucune version en cours de session"
    );
    let end = live_text(&f).await;
    assert_eq!(end, "abababababab");

    drop((a, b));
    actor_stops(&f).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Closing);
    assert_eq!(authors(&kept[0]), sorted(vec![alice, bob]));
    assert_eq!(version_text(&f, kept[0].id).await, end);
}

/// Une session plus longue que l'intervalle : la version est coupée juste avant l'écriture qui
/// suit l'intervalle (compté depuis la première écriture non versionnée), et porte l'état
/// d'avant elle, avec les comptes de la tranche. La connexion reçoit la trame `versions`. La
/// tranche suivante commence avec cette écriture.
#[tokio::test]
async fn long_session_cuts_a_version_before_the_write_that_follows_the_interval() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    let (a, b) = (connect(&f, alice).await, connect(&f, bob).await);

    types(&a, "A").await;
    advance(INTERVAL - Duration::from_secs(60)).await;
    types(&a, "B").await;
    assert!(versions(&f).await.is_empty(), "neuf minutes : pas encore");
    advance(Duration::from_secs(120)).await;
    // Onze minutes après la première écriture non versionnée.
    types(&b, "C").await;

    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Periodic);
    assert_eq!(version_text(&f, kept[0].id).await, "AB", "l'état d'avant C");
    assert_eq!(authors(&kept[0]), [alice]);
    assert_eq!(live_text(&f).await, "ABC");
    until("la trame `versions` n'est pas arrivée", async || {
        versions_frames(&a) == 1
    })
    .await;

    // La tranche suivante ne compte que C, et Bob.
    advance(Duration::from_secs(60)).await;
    types(&b, "D").await;
    assert_eq!(versions(&f).await.len(), 1, "une minute après C : rien");
    drop((a, b));
    actor_stops(&f).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Closing);
    assert_eq!(version_text(&f, kept[0].id).await, "ABCD");
    assert_eq!(authors(&kept[0]), [bob]);
}

/// Une page qui n'a pas changé ne coupe rien, ni en cours de session, ni à l'arrêt, ni après
/// un rechargement de son acteur, même quand des mises à jour lui arrivent. Yjs met tout son
/// jeu de suppressions dans l'étape 2 : un onglet qui revient (coupure du réseau, veille,
/// connexions fermées pour un droit) envoie donc, sur une page qui a des suppressions, une mise
/// à jour qui ne change rien. Elle n'est ni écrite au journal ni relayée (Ruling R181) : écrite,
/// l'acteur suivant, chargé par une simple lecture, la relisait comme une écriture de l'onglet,
/// et coupait à son arrêt une version identique à la précédente, avec un auteur qui n'avait rien
/// changé. Deux cycles, comme le relecteur l'a montré.
#[tokio::test]
async fn page_that_did_not_change_cuts_nothing() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    f.write_mmd("Un\n").await;
    // Elle remplace la première : la page a des suppressions.
    f.write_mmd("Deux\n").await;
    actor_stops(&f).await;
    let kept: Vec<Uuid> = versions(&f).await.iter().map(|v| v.id).collect();
    assert_eq!(kept.len(), 1);
    let journal = actors(&f.db, f.page).await.len();

    for cycle in 0..2 {
        let tab = connect(&f, alice).await;
        // L'étape 2 d'un onglet qui revient, sans rien de neuf : tout son jeu de suppressions.
        let page = tab.page().unwrap();
        let step2 = page.encode_diff(&page.state_vector()).unwrap();
        assert!(
            step2.len() > 2,
            "cycle {cycle} : des suppressions : {step2:?}"
        );
        heard(&tab, || tab.send_update(&step2)).await;
        advance(INTERVAL + Duration::from_secs(60)).await;
        heard(&tab, || tab.send_update(&step2)).await;
        assert!(
            versions(&f).await.len() == 1,
            "cycle {cycle} : rien en session"
        );
        assert_eq!(
            actors(&f.db, f.page).await.len(),
            journal,
            "cycle {cycle} : rien au journal"
        );
        drop(tab);
        actor_stops(&f).await;
        // Une simple lecture : un acteur neuf charge la page, relit la tranche, puis s'arrête.
        assert_eq!(live_text(&f).await, "Deux");
        actor_stops(&f).await;
        let now: Vec<Uuid> = versions(&f).await.iter().map(|v| v.id).collect();
        assert_eq!(now, kept, "cycle {cycle} : aucune version de plus");
    }
}

/// Une écriture du serveur (`PUT /mmd`) compte comme celle d'un client : elle ouvre une
/// tranche, son compte en est l'auteur, et la version suivante est coupée avant l'écriture qui
/// suit l'intervalle. Une écriture sans compte n'ajoute personne.
#[tokio::test]
async fn server_write_counts_like_a_client_write() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    // Quelqu'un a la page ouverte : sans connexion, l'acteur s'arrêterait pendant les dix
    // minutes, et couperait sa version `closing`.
    let open = f.client().await;
    let write = async |mmd: &str, actor: Option<Uuid>| {
        f.hub
            .write_blocks(f.page, parse(mmd).unwrap(), actor)
            .await
            .unwrap();
    };

    write("Un\n", Some(alice)).await;
    advance(INTERVAL + Duration::from_secs(1)).await;
    write("Deux\n", Some(bob)).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Periodic);
    assert_eq!(version_text(&f, kept[0].id).await, "Un");
    assert_eq!(authors(&kept[0]), [alice]);

    write("Trois\n", None).await;
    drop(open);
    actor_stops(&f).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Closing);
    assert_eq!(version_text(&f, kept[0].id).await, "Trois");
    assert_eq!(
        authors(&kept[0]),
        [bob],
        "l'écriture sans compte n'ajoute personne"
    );
}

/// Restaurer écrit les blocs de la version par-dessus le contenu, avec leurs identifiants, par
/// une écriture ordinaire : les connexions la reçoivent par la synchro, avec la trame
/// `versions`. Quand la page a changé depuis sa dernière version, une version `before_restore`
/// garde juste avant ce qui est remplacé, avec les auteurs de la tranche ; elle se restaure à son
/// tour. Une restauration qui suit une restauration, sans écriture entre elles, ne coupe rien : ce
/// qu'elle remplace est le contenu d'une version gardée (Ruling R190). L'écriture d'une
/// restauration n'ouvre pas de tranche : l'acteur qui s'arrête ensuite ne coupe rien.
#[tokio::test]
async fn restore_cuts_a_version_of_what_it_replaces() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob, carol) = (
        new_user(&f.db).await,
        new_user(&f.db).await,
        new_user(&f.db).await,
    );
    f.hub
        .write_blocks(f.page, parse("Un\n\nDeux\n").unwrap(), Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    let first = versions(&f).await[0].id;
    let first_blocks = version_blocks(&f, first).await;
    assert_eq!(text(&first_blocks), "Un\nDeux");

    let observer = connect(&f, carol).await;
    types(&observer, "Trois").await;
    let replaced = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(text(&replaced), "UnTrois\nDeux");

    f.hub
        .restore_version(f.page, first, Some(bob))
        .await
        .unwrap();
    assert_eq!(
        f.hub.read_blocks(f.page).await.unwrap(),
        first_blocks,
        "les blocs de la version, identifiants compris"
    );
    until(
        "le contenu restauré n'est pas arrivé par la synchro",
        async || observer.blocks() == first_blocks,
    )
    .await;
    until("la trame `versions` n'est pas arrivée", async || {
        versions_frames(&observer) == 1
    })
    .await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::BeforeRestore);
    assert_eq!(version_blocks(&f, kept[0].id).await, replaced);
    assert_eq!(authors(&kept[0]), [carol]);

    // Ce qui a été remplacé se restaure à son tour, sans rien couper : ce que cette
    // restauration remplace est le contenu de la première version, toujours gardée.
    let before_restore = kept[0].id;
    f.hub
        .restore_version(f.page, before_restore, Some(alice))
        .await
        .unwrap();
    assert_eq!(f.hub.read_blocks(f.page).await.unwrap(), replaced);
    assert_eq!(versions(&f).await, kept, "rien de coupé");
    assert_eq!(version_blocks(&f, first).await, first_blocks);

    // Une écriture, puis une restauration : la page a changé, ce qu'elle remplace est gardé.
    types(&observer, "Quatre").await;
    let changed = f.hub.read_blocks(f.page).await.unwrap();
    f.hub
        .restore_version(f.page, first, Some(bob))
        .await
        .unwrap();
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::BeforeRestore);
    assert_eq!(version_blocks(&f, kept[0].id).await, changed);
    assert_eq!(authors(&kept[0]), [carol]);

    drop(observer);
    actor_stops(&f).await;
    assert_eq!(
        versions(&f).await,
        kept,
        "l'écriture de la restauration n'ouvre pas de tranche"
    );
}

/// Restaurer tour à tour deux versions, sans écriture entre elles, ne coupe rien : ce que chaque
/// restauration remplace est le contenu d'une version gardée (Ruling R190). Avant, chacune
/// coupait une version `before_restore`, et cent requêtes vidaient l'historique.
#[tokio::test]
async fn alternating_restores_cut_nothing() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    f.write_mmd("A\n").await;
    actor_stops(&f).await;
    f.write_mmd("B\n").await;
    actor_stops(&f).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 2);
    let (b, a) = (kept[0].id, kept[1].id);
    let _open = f.client().await;
    for round in 0..50 {
        let (version, expected) = if round % 2 == 0 { (a, "A") } else { (b, "B") };
        f.hub
            .restore_version(f.page, version, Some(alice))
            .await
            .unwrap();
        assert_eq!(live_text(&f).await, expected, "tour {round}");
    }
    assert_eq!(versions(&f).await, kept, "rien de coupé");
}

/// Une boucle « écriture puis restauration », cent fois, après cent versions ordinaires : chaque
/// restauration coupe sa version `before_restore` (la page a changé), mais une page n'en garde
/// que vingt (Ruling R189) : il reste au moins quatre-vingts versions `periodic` et `closing`
/// d'avant, dont celle qu'on restaure.
#[tokio::test]
async fn write_and_restore_loop_keeps_the_versions_from_before() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, mallory) = (new_user(&f.db).await, new_user(&f.db).await);
    let open = f.client().await;
    // Une version avant chaque écriture qui suit la première ; aucune attente après la
    // dernière : les écritures de la boucle ne coupent pas de version périodique.
    for i in 0..=MAX_VERSIONS {
        if i > 0 {
            advance(INTERVAL + Duration::from_secs(1)).await;
        }
        f.hub
            .write_blocks(f.page, parse(&format!("Texte {i}\n")).unwrap(), Some(alice))
            .await
            .unwrap();
    }
    let before: Vec<Uuid> = versions(&f).await.iter().map(|v| v.id).collect();
    assert_eq!(before.len(), MAX_VERSIONS);
    let target = before[0];
    for i in 0..100 {
        f.hub
            .write_blocks(
                f.page,
                parse(&format!("Vandale {i}\n")).unwrap(),
                Some(mallory),
            )
            .await
            .unwrap();
        f.hub
            .restore_version(f.page, target, Some(mallory))
            .await
            .unwrap();
    }
    let kept = versions(&f).await;
    let ordinary: Vec<Uuid> = kept
        .iter()
        .filter(|v| v.reason != VersionReason::BeforeRestore)
        .map(|v| v.id)
        .collect();
    let restores = kept.len() - ordinary.len();
    assert!(
        restores <= MAX_BEFORE_RESTORE,
        "{restores} versions d'avant restauration"
    );
    assert!(
        ordinary.len() >= MAX_VERSIONS - MAX_BEFORE_RESTORE,
        "{} versions ordinaires gardées",
        ordinary.len()
    );
    assert!(ordinary.iter().all(|id| before.contains(id)));
    assert!(
        ordinary.contains(&target),
        "la version restaurée est restée"
    );
    drop(open);
}

/// Le cas limite de la règle : la version restaurée est la prochaine que la purge retirerait.
/// Sans écriture, aucune version n'est coupée, donc aucune n'est purgée ; mais la première coupe
/// suivante l'emporterait, et l'état que la page a gardé depuis la restauration ne serait plus
/// atteignable seul. L'écriture de la restauration ouvre donc une tranche dans ce cas : la version
/// `closing` en garde une copie neuve, qui survit à la purge de l'ancienne (Ruling R190).
#[tokio::test]
async fn restored_version_next_in_line_for_purge_is_kept_afresh() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let open = f.client().await;
    for i in 0..=MAX_VERSIONS {
        f.write_mmd(&format!("Texte {i}\n")).await;
        advance(INTERVAL + Duration::from_secs(1)).await;
    }
    drop(open);
    actor_stops(&f).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), MAX_VERSIONS);
    let oldest = kept[MAX_VERSIONS - 1].id;
    let oldest_blocks = version_blocks(&f, oldest).await;

    f.hub
        .restore_version(f.page, oldest, Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    let now = versions(&f).await;
    assert_eq!(now.len(), MAX_VERSIONS);
    assert!(
        now.iter().all(|v| v.id != oldest),
        "la version restaurée est partie à la coupe suivante"
    );
    assert_eq!(now[0].reason, VersionReason::Closing);
    assert_eq!(version_blocks(&f, now[0].id).await, oldest_blocks);
    assert_eq!(authors(&now[0]), [alice]);
}

/// Restaurer ce que la page a déjà comme contenu ne change rien : ni écriture au journal, ni
/// version `before_restore`, ni trame.
#[tokio::test]
async fn restoring_what_the_page_already_has_changes_nothing() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    f.write_mmd("Un\n").await;
    actor_stops(&f).await;
    let first = versions(&f).await[0].id;
    let observer = connect(&f, alice).await;
    let rows = row_counts(&f.db).await;

    f.hub
        .restore_version(f.page, first, Some(alice))
        .await
        .unwrap();
    assert!(still_open(&observer).await);
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert_eq!(versions_frames(&observer), 0);
    assert_eq!(live_text(&f).await, "Un");
}

/// La connexion est encore servie : une resynchronisation reçoit sa réponse. Elle passe
/// derrière tout ce que l'acteur a diffusé avant.
async fn still_open(client: &Client) -> bool {
    heard(client, || {}).await;
    !client.is_closed()
}

/// Une version d'une autre page, ou inconnue, ne se restaure pas : `NotFound`, et rien
/// n'est écrit.
#[tokio::test]
async fn version_of_another_page_is_not_restored() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let other = new_page(&f.store).await;
    f.hub
        .write_blocks(other, parse("Ailleurs\n").unwrap(), None)
        .await
        .unwrap();
    let theirs = f
        .store
        .put_version(
            other,
            1,
            &stored_page(&f.store, other).await.encode_state(),
            &[],
            VersionReason::Closing,
        )
        .await
        .unwrap();
    f.write_mmd("Ici\n").await;
    let rows = row_counts(&f.db).await;
    for version in [theirs, Uuid::now_v7()] {
        let refused = f.hub.restore_version(f.page, version, None).await;
        assert!(matches!(refused, Err(SyncError::NotFound)), "{refused:?}");
    }
    assert_eq!(row_counts(&f.db).await, rows);
    assert_eq!(live_text(&f).await, "Ici");
}

/// Un état gardé que la page ne relit pas (base abîmée) : la restauration est refusée par une
/// erreur, sans toucher au contenu ni arrêter l'acteur, et sans version `before_restore`.
#[tokio::test]
async fn unreadable_version_is_refused_without_touching_the_page() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    f.write_mmd("Un\n").await;
    actor_stops(&f).await;
    let broken = versions(&f).await[0].id;
    exec_sql(
        &f.db,
        &format!("UPDATE doc_versions SET state_blob = x'01ff07' WHERE id = '{broken}'"),
    )
    .await;
    let observer = connect(&f, alice).await;
    types(&observer, " et deux").await;
    let rows = row_counts(&f.db).await;

    let refused = f.hub.restore_version(f.page, broken, Some(alice)).await;
    assert!(matches!(refused, Err(SyncError::Crdt(_))), "{refused:?}");
    assert_eq!(row_counts(&f.db).await, rows, "rien d'écrit");
    assert!(still_open(&observer).await, "l'acteur sert toujours");
    assert_eq!(live_text(&f).await, "Un et deux");
}

/// Une page garde ses cent dernières versions : les plus anciennes partent.
#[tokio::test]
async fn versions_are_capped_at_100() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let _open = f.client().await;
    // Chaque écriture après la première suit l'intervalle : une version avant chacune.
    for i in 0..=MAX_VERSIONS + 1 {
        f.write_mmd(&format!("Texte {i}\n")).await;
        advance(INTERVAL + Duration::from_secs(1)).await;
    }
    let kept = versions(&f).await;
    assert_eq!(kept.len(), MAX_VERSIONS);
    assert_eq!(
        version_text(&f, kept[0].id).await,
        format!("Texte {MAX_VERSIONS}")
    );
    assert_eq!(
        version_text(&f, kept[MAX_VERSIONS - 1].id).await,
        "Texte 1",
        "la plus ancienne, « Texte 0 », est partie"
    );
}

/// La compaction purge le journal, pas les versions : une version coupée avant elle se relit
/// et se restaure après. Et les auteurs d'une tranche dont le journal a été purgé restent
/// connus de l'acteur, qui les a notés en écrivant.
#[tokio::test]
async fn version_survives_compaction() {
    let _clock = StoppedClock::start();
    let cfg = SyncConfig {
        snapshot_every: 2,
        ..config()
    };
    let f = Fixture::new(cfg).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    f.hub
        .write_blocks(f.page, parse("Avant\n").unwrap(), Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    let first = versions(&f).await[0].id;

    let b = connect(&f, bob).await;
    for typed in [" un", " deux", " trois"] {
        types(&b, typed).await;
    }
    let (journal, snapshot) = (
        actors(&f.db, f.page).await.len(),
        f.store.load_doc(f.page).await.unwrap().snapshot,
    );
    assert!(journal < 3, "le journal a été compacté : {journal}");
    let versioned = versions(&f).await[0].seq;
    assert!(snapshot.is_some_and(|(seq, _)| seq > versioned));
    assert_eq!(version_text(&f, first).await, "Avant");

    f.hub
        .restore_version(f.page, first, Some(alice))
        .await
        .unwrap();
    assert_eq!(live_text(&f).await, "Avant");
    let kept = versions(&f).await;
    assert_eq!(kept[0].reason, VersionReason::BeforeRestore);
    assert_eq!(version_text(&f, kept[0].id).await, "Avant un deux trois");
    assert_eq!(
        authors(&kept[0]),
        [bob],
        "Bob, que le journal ne nomme plus"
    );
}

/// Après un arrêt brutal (le processus tué : pas de version `closing`), l'acteur qui démarre
/// relit au journal la tranche en cours, ses auteurs et sa date : la version suivante les
/// porte.
#[tokio::test]
async fn slice_is_read_again_from_the_journal_after_a_restart() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    // L'acteur qu'a lancé la création du document, arrêté sans rien avoir écrit.
    actor_stops(&f).await;
    // Ce qu'un processus précédent a écrit avant d'être tué : une écriture d'Alice, au journal.
    let page = stored_page(&f.store, f.page).await;
    let update = page
        .write_blocks(&parse("Avant la panne\n").unwrap())
        .unwrap();
    f.store
        .append_updates(f.page, &[&update], Some(alice))
        .await
        .unwrap();

    // Quelqu'un ouvre la page : un acteur neuf la charge.
    let _open = f.client().await;
    assert_eq!(live_text(&f).await, "Avant la panne");
    advance(INTERVAL + Duration::from_secs(1)).await;
    f.hub
        .write_blocks(f.page, parse("Après\n").unwrap(), Some(bob))
        .await
        .unwrap();
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Periodic);
    assert_eq!(version_text(&f, kept[0].id).await, "Avant la panne");
    assert_eq!(authors(&kept[0]), [alice]);
}

/// À l'arrêt du serveur (`Hub::shutdown`), l'acteur d'une page ouverte coupe sa version
/// `closing`, puis ferme ses connexions ; aucun acteur ne démarre plus ensuite.
#[tokio::test]
async fn server_stop_cuts_a_closing_version() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let a = connect(&f, alice).await;
    types(&a, "Tapé").await;

    f.hub.shutdown().await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Closing);
    assert_eq!(version_text(&f, kept[0].id).await, "Tapé");
    assert_eq!(authors(&kept[0]), [alice]);
    until("la connexion n'est pas fermée", async || a.is_closed()).await;
    assert!(f.hub.live_pages().await.is_empty());
    assert!(matches!(
        f.hub.read_blocks(f.page).await,
        Err(SyncError::Closed)
    ));
}

/// Une version `closing` que la base refuse n'empêche pas l'acteur de s'arrêter ; le contenu
/// est au journal, et l'acteur suivant y relit la tranche : elle sera dans la version d'après.
#[tokio::test]
async fn closing_version_that_cannot_be_written_does_not_keep_the_actor() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    exec_sql(
        &f.db,
        "CREATE TRIGGER panne BEFORE INSERT ON doc_versions \
         BEGIN SELECT RAISE(ABORT, 'panne simulée'); END",
    )
    .await;
    f.hub
        .write_blocks(f.page, parse("Gardé\n").unwrap(), Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    assert!(versions(&f).await.is_empty());

    exec_sql(&f.db, "DROP TRIGGER panne").await;
    assert_eq!(live_text(&f).await, "Gardé", "le contenu est au journal");
    actor_stops(&f).await;
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(version_text(&f, kept[0].id).await, "Gardé");
    assert_eq!(authors(&kept[0]), [alice]);
}

/// Une tranche relue au journal plus vieille que l'horloge de la machine (une écriture d'avant
/// son démarrage, ou du temps où l'horloge système était fausse) est due tout de suite : la
/// première écriture qui suit coupe sa version. L'instant de tokio ne remonte pas avant le
/// démarrage de la machine ; la tranche repartait de « maintenant », un intervalle de plus.
#[tokio::test]
async fn slice_older_than_the_machine_is_due_at_once() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    actor_stops(&f).await;
    let page = stored_page(&f.store, f.page).await;
    let update = page.write_blocks(&parse("Très ancien\n").unwrap()).unwrap();
    f.store
        .append_updates(f.page, &[&update], Some(alice))
        .await
        .unwrap();
    // Écrite en 1970.
    exec_sql(&f.db, "UPDATE doc_updates SET created_at = 1").await;

    let _open = f.client().await;
    f.hub
        .write_blocks(f.page, parse("Neuf\n").unwrap(), Some(bob))
        .await
        .unwrap();
    let kept = versions(&f).await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].reason, VersionReason::Periodic);
    assert_eq!(version_text(&f, kept[0].id).await, "Très ancien");
    assert_eq!(authors(&kept[0]), [alice]);
}

/// La limite de la restauration, telle qu'elle est (Ruling R188) : restaurer remplace tout le
/// contenu, comme `PUT /mmd`. Une frappe partie avant la restauration, sur le contenu remplacé,
/// arrive dans des nœuds supprimés : elle n'est ni dans la page, ni dans aucune version.
/// L'écriture par différence du lot 5 (R40) servira aux deux. Ce test le dit, et changera de
/// sens ce jour-là.
#[tokio::test]
async fn typing_concurrent_with_a_restore_is_lost() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let (alice, bob) = (new_user(&f.db).await, new_user(&f.db).await);
    f.hub
        .write_blocks(f.page, parse("Un\n").unwrap(), Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    let first = versions(&f).await[0].id;
    let tab = connect(&f, bob).await;
    types(&tab, " deux").await;

    // Une frappe faite sur le contenu d'avant, encore en route quand la restauration passe.
    let offline = detached(&tab);
    let typed = {
        let content = first_content(&offline);
        let mut txn = offline.transact_mut();
        let Some(XmlOut::Text(text)) = content.get(&txn, 0) else {
            panic!("pas de texte dans le premier bloc");
        };
        let end = text.len(&txn);
        text.insert(&mut txn, end, " trois");
        txn.encode_update_v1()
    };
    f.hub
        .restore_version(f.page, first, Some(alice))
        .await
        .unwrap();
    heard(&tab, || tab.send_update(&typed)).await;

    assert_eq!(live_text(&f).await, "Un", "la frappe en route est perdue");
    for version in versions(&f).await {
        assert!(
            !version_text(&f, version.id).await.contains("trois"),
            "elle n'est dans aucune version"
        );
    }
}

/// Un bloc supprimé après la version revient avec son identifiant quand on la restaure : c'est
/// ce qui fait retrouver leur bloc aux fils de commentaires.
#[tokio::test]
async fn deleted_block_comes_back_with_its_id() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    f.hub
        .write_blocks(f.page, parse("Un\n\nDeux\n").unwrap(), Some(alice))
        .await
        .unwrap();
    actor_stops(&f).await;
    let first = versions(&f).await[0].id;
    let before = f.hub.read_blocks(f.page).await.unwrap();
    let deux = before[1].id.clone();

    // « Deux » supprimé : seul le premier bloc reste, avec son identifiant.
    f.hub
        .write_blocks(f.page, before[..1].to_vec(), Some(alice))
        .await
        .unwrap();
    let ids = f.ids().await;
    assert_eq!(ids, [before[0].id.clone()]);

    f.hub
        .restore_version(f.page, first, Some(alice))
        .await
        .unwrap();
    let after = f.hub.read_blocks(f.page).await.unwrap();
    assert_eq!(after, before);
    assert_eq!(
        after[1].id, deux,
        "le bloc supprimé revient avec son identifiant"
    );
}

/// Deux versions, `A` puis `B`, coupées à l'arrêt de l'acteur, sans connexion : rend leurs
/// identifiants, `B` (la plus récente) d'abord.
async fn two_versions(f: &Fixture) -> (Uuid, Uuid) {
    f.write_mmd("A\n").await;
    actor_stops(f).await;
    f.write_mmd("B\n").await;
    actor_stops(f).await;
    let kept = versions(f).await;
    assert_eq!(kept.len(), 2);
    (kept[0].id, kept[1].id)
}

/// P3 (Ruling R197) : une simple lecture après une restauration ne coupe rien, et ne nomme
/// personne. L'écriture de la restauration est au journal, après la dernière version ; sans la
/// borne de la version restaurée (`covers_seq`), l'acteur rechargé par la lecture la relisait
/// comme une tranche ouverte, au nom de qui avait restauré, et coupait à son arrêt une version
/// identique à la restaurée.
#[tokio::test]
async fn a_read_after_a_restore_cuts_nothing() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let (_, a) = two_versions(&f).await;
    let before = versions(&f).await;

    f.hub.restore_version(f.page, a, Some(alice)).await.unwrap();
    actor_stops(&f).await;
    assert_eq!(live_text(&f).await, "A", "une simple lecture");
    actor_stops(&f).await;
    assert_eq!(versions(&f).await, before, "rien de coupé");
}

/// P2 (Ruling R197) : des restaurations successives sans écriture entre elles ne coupent rien,
/// y compris quand l'acteur s'arrête et se recharge entre elles (un agent qui passe par l'API
/// à plus de trente secondes d'intervalle) : hormis la première version `before_restore`, si la
/// page avait changé depuis sa dernière version.
#[tokio::test]
async fn restores_across_reloads_cut_nothing() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let (b, a) = two_versions(&f).await;
    // La page change : la première restauration garde ce qu'elle remplace.
    f.write_mmd("C\n").await;
    let start = versions(&f).await.len();
    for round in 0..6 {
        let (version, expected) = if round % 2 == 0 { (a, "A") } else { (b, "B") };
        f.hub
            .restore_version(f.page, version, Some(alice))
            .await
            .unwrap();
        actor_stops(&f).await;
        assert_eq!(live_text(&f).await, expected, "tour {round}");
        actor_stops(&f).await;
    }
    let kept = versions(&f).await;
    assert_eq!(kept.len(), start + 1, "{kept:?}");
    let restores = kept
        .iter()
        .filter(|v| v.reason == VersionReason::BeforeRestore)
        .count();
    assert_eq!(restores, 1, "seule la première restauration coupe");
}

/// P1 (Ruling R197) : aucun contenu qu'une restauration remplace n'est perdu, rechargements de
/// l'acteur compris : chacun est dans une version.
#[tokio::test]
async fn content_replaced_by_a_restore_is_kept_across_reloads() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let (b, a) = two_versions(&f).await;
    let mut replaced = Vec::new();
    let restore = async |version: Uuid, replaced: &mut Vec<String>| {
        replaced.push(live_text(&f).await);
        f.hub
            .restore_version(f.page, version, Some(alice))
            .await
            .unwrap();
        actor_stops(&f).await;
    };

    f.write_mmd("C\n").await;
    restore(a, &mut replaced).await;
    // Une simple lecture, l'acteur rechargé puis arrêté.
    assert_eq!(live_text(&f).await, "A");
    actor_stops(&f).await;
    restore(b, &mut replaced).await;
    f.write_mmd("D\n").await;
    actor_stops(&f).await;
    restore(a, &mut replaced).await;
    f.write_mmd("E\n").await;
    restore(b, &mut replaced).await;

    assert_eq!(replaced, ["C", "A", "D", "E"]);
    let mut kept = Vec::new();
    for version in versions(&f).await {
        kept.push(version_text(&f, version.id).await);
    }
    for content in &replaced {
        assert!(kept.contains(content), "« {content} » perdu : {kept:?}");
    }
}

/// P4 (Ruling R197) : ce que porte la version restaurée reste atteignable tant que c'est le
/// contenu de la page, y compris quand c'est la version `before_restore` de la même
/// restauration qui en fait la prochaine que la purge retirerait. Historique plein, page
/// changée depuis sa dernière version : on restaure la deuxième plus ancienne ; sa coupe
/// `before_restore` purge la plus ancienne. L'état restauré dure plus d'un intervalle, puis une
/// écriture arrive : une version `periodic` le garde avant elle. Sans cela, la version
/// `closing` d'après purgeait la version restaurée, et ce contenu n'était plus nulle part.
#[tokio::test]
async fn restored_content_survives_the_purge_its_own_cut_makes() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let open = f.client().await;
    for i in 0..=MAX_VERSIONS {
        if i > 0 {
            advance(INTERVAL + Duration::from_secs(1)).await;
        }
        f.write_mmd(&format!("Texte {i}\n")).await;
    }
    let kept = versions(&f).await;
    assert_eq!(kept.len(), MAX_VERSIONS);
    let second_oldest = kept[MAX_VERSIONS - 2].id;
    let restored = version_blocks(&f, second_oldest).await;
    assert_eq!(text(&restored), "Texte 1");

    f.hub
        .restore_version(f.page, second_oldest, Some(alice))
        .await
        .unwrap();
    advance(INTERVAL + Duration::from_secs(1)).await;
    f.write_mmd("Après\n").await;
    drop(open);
    actor_stops(&f).await;

    let mut found = false;
    for version in versions(&f).await {
        found |= version_blocks(&f, version.id).await == restored;
    }
    assert!(found, "le contenu restauré n'est plus dans aucune version");
}

/// Ruling R197, P2 au plafond des versions d'avant restauration : restaurer la plus ancienne
/// `before_restore` d'une page qui en garde vingt ne coupe aucune copie, rechargements compris.
/// Seule une autre coupe `before_restore` la retirerait, et il lui faut une écriture d'abord,
/// donc un contenu qui n'est plus le sien : la version ordinaire qu'une tranche produirait ne la
/// touche pas. L'exception de purge la prenait pour « la prochaine à partir », ouvrait une
/// tranche, et chaque arrêt coupait une `closing` identique.
#[tokio::test]
async fn restoring_at_the_cap_of_versions_before_a_restore_cuts_no_copy() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let (_, a) = two_versions(&f).await;
    for i in 0..MAX_BEFORE_RESTORE {
        f.write_mmd(&format!("X{i}\n")).await;
        f.hub.restore_version(f.page, a, Some(alice)).await.unwrap();
    }
    let before = versions(&f).await;
    assert_eq!(before.len(), 2 + MAX_BEFORE_RESTORE);
    let oldest_before_restore = before
        .iter()
        .rev()
        .find(|v| v.reason == VersionReason::BeforeRestore)
        .unwrap()
        .id;
    assert_eq!(version_text(&f, oldest_before_restore).await, "X0");
    actor_stops(&f).await;

    for round in 0..4 {
        f.hub
            .restore_version(f.page, oldest_before_restore, Some(alice))
            .await
            .unwrap();
        actor_stops(&f).await;
        assert_eq!(
            live_text(&f).await,
            "X0",
            "tour {round} : une simple lecture"
        );
        actor_stops(&f).await;
        f.hub.restore_version(f.page, a, Some(alice)).await.unwrap();
        actor_stops(&f).await;
    }
    let after = versions(&f).await;
    assert_eq!(
        after, before,
        "aucune version de plus, aucune copie de « X0 »"
    );
}

/// La promesse de R189, sur un historique plein (quatre-vingts versions ordinaires et vingt
/// `before_restore`) : des restaurations seules, l'acteur rechargé entre elles, ne font partir
/// aucune version ordinaire.
#[tokio::test]
async fn restores_alone_push_out_no_ordinary_version() {
    let _clock = StoppedClock::start();
    let f = Fixture::new(config()).await;
    let alice = new_user(&f.db).await;
    let open = f.client().await;
    let ordinary = MAX_VERSIONS - MAX_BEFORE_RESTORE;
    for i in 0..=ordinary {
        if i > 0 {
            advance(INTERVAL + Duration::from_secs(1)).await;
        }
        f.write_mmd(&format!("Texte {i}\n")).await;
    }
    let target = versions(&f).await[0].id;
    for i in 0..MAX_BEFORE_RESTORE {
        f.write_mmd(&format!("X{i}\n")).await;
        f.hub
            .restore_version(f.page, target, Some(alice))
            .await
            .unwrap();
    }
    drop(open);
    actor_stops(&f).await;
    let before = versions(&f).await;
    assert_eq!(before.len(), MAX_VERSIONS);
    let ordinary_ids = |kept: &[Version]| -> Vec<Uuid> {
        kept.iter()
            .filter(|v| v.reason != VersionReason::BeforeRestore)
            .map(|v| v.id)
            .collect()
    };
    let oldest_before_restore = before
        .iter()
        .rev()
        .find(|v| v.reason == VersionReason::BeforeRestore)
        .unwrap()
        .id;

    for round in 0..3 {
        f.hub
            .restore_version(f.page, oldest_before_restore, Some(alice))
            .await
            .unwrap();
        actor_stops(&f).await;
        assert_eq!(live_text(&f).await, "X0", "tour {round}");
        actor_stops(&f).await;
        f.hub
            .restore_version(f.page, target, Some(alice))
            .await
            .unwrap();
        actor_stops(&f).await;
    }
    let after = versions(&f).await;
    assert_eq!(ordinary_ids(&after), ordinary_ids(&before));
    assert_eq!(after.len(), MAX_VERSIONS);
}
