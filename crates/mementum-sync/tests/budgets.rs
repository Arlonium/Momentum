//! Budget du §5.10 côté serveur : la part du serveur dans « Ouvrir une page
//! jamais vue (réseau local) < 300 ms ». Charger depuis SQLite le `PageDoc`
//! d'une page de 10 000 blocs, comme le fait l'acteur d'une page qui démarre,
//! prend moins de 300 ms au p95 sur 20 chargements :
//! - avec le pire journal de frappes (`snapshot_every - 1` mises à jour) ;
//! - après une grosse suppression, réécriture complète par le serveur ou
//!   contenu remplacé par un client (Ruling R40 : le ramasse-miettes de yrs
//!   se paie à l'écriture, jamais à chaque chargement).
//!
//! Et la part du serveur dans « frappe visible chez un autre < 150 ms » quand
//! la frappe coupe une version (Ruling R135) : la version, l'état complet
//! d'une page de 10 000 blocs encodé et écrit, passe avant la frappe qui la
//! déclenche ; la frappe arrive chez l'autre client en moins de 150 ms au p95.
//!
//! Ignorés par défaut, car la mesure n'a de sens qu'en `--release` :
//! `cargo test --release -p mementum-sync -- --ignored` (job `budgets` de la
//! CI). Ils se mesurent **un à la fois** ([`alone`], Ruling R341) : libtest
//! lance en parallèle les tests d'un même binaire, et quatre mesures sur des
//! pages de 10 000 blocs se disputaient le disque et les cœurs (sur deux
//! cœurs, la frappe précédée d'une version coûtait le double ; chiffres au
//! registre du lot 3, « La CI remise au vert »).
//!
//! S'y ajoute un garde-fou, lancé avec les autres tests : l'attente de l'e2e
//! `budgets.spec.ts`, qui laisse le serveur fermer l'acteur des pages avant
//! de les ouvrir, suit `idle_close`.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use mementum_md::parse;
use mementum_store::{Store, VersionReason};
use mementum_sync::{ConnInfo, SyncConfig};
use tokio::sync::{Mutex, MutexGuard};
use yrs::ClientID;

/// Blocs de la page.
const BLOCKS: usize = 10_000;

/// « Frappe visible chez un autre (réseau local) < 150 ms » (§5.10).
const TYPING_BUDGET: Duration = Duration::from_millis(150);

/// Frappes mesurées.
const KEYSTROKES: usize = 20;

/// Chargements mesurés.
const LOADS: usize = 20;

/// Part serveur de « Ouvrir une page jamais vue (réseau local) < 300 ms ».
const BUDGET: Duration = Duration::from_millis(300);

/// Centile `q` (entre 0 et 1), au plus proche rang : 19ᵉ valeur sur 20 pour le p95.
fn percentile(values: &[Duration], q: f64) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort();
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

/// Une mesure de performance faite en debug ne dit rien.
fn release_only() {
    // Un `assert!` sur `cfg!` est refusé par clippy (`assertions_on_constants`).
    if cfg!(debug_assertions) {
        panic!(
            "mesure sans valeur en debug : lancer `cargo test --release -p mementum-sync -- --ignored`"
        );
    }
}

/// Le tour des mesures : une seule à la fois dans ce binaire (Ruling R341).
static ONE_AT_A_TIME: Mutex<()> = Mutex::const_new(());

/// À tenir pendant toute une mesure, depuis sa première ligne : elle attend que les autres
/// mesures de ce binaire aient fini, quel que soit le nombre de fils de libtest. Ce qu'une
/// mesure chronomètre est son propre coût, pas celui d'une autre qui écrit en même temps cent
/// versions de 3 Mo sur le même disque. Le verrou est celui de tokio : il se tient d'un `await`
/// à l'autre, et une mesure qui échoue (elle panique) ne le laisse pas empoisonné aux suivantes.
async fn alone() -> MutexGuard<'static, ()> {
    release_only();
    ONE_AT_A_TIME.lock().await
}

/// La page de mesure : `BLOCKS` paragraphes, en MMD sans ancres (chaque
/// écriture crée donc des blocs neufs).
fn page_mmd() -> String {
    (1..=BLOCKS)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

/// Attend que le serveur ait traité tout ce que `client` a envoyé : il traite
/// les messages d'une connexion dans l'ordre, et son étape 2 arrive après.
async fn processed(client: &Client) {
    let before = client.step2_count();
    client.resync();
    assert!(
        client
            .wait_until(Duration::from_secs(300), |c| c.step2_count() > before)
            .await,
        "le serveur n'a pas traité les mises à jour du client"
    );
}

/// `LOADS` chargements de la page depuis SQLite, chacun sur une connexion
/// neuve (cache de pages SQLite vide, comme pour l'acteur d'une page que
/// personne n'a ouverte), chronomètre autour de `stored_page` : l'instantané
/// et le journal relus, puis appliqués dans l'ordre des `seq`. C'est le gros
/// du démarrage d'un acteur, pas `actor::load` lui-même, qui n'est pas
/// public : l'observateur des suppressions que l'acteur pose entre
/// l'instantané et le journal (Ruling R40) n'est pas mesuré. Le nombre de
/// blocs est contrôlé hors chronomètre. Renvoie le dépassement du budget au
/// p95, s'il y en a un.
async fn loads_over_budget(f: &Fixture, blocks: usize, what: &str) -> Option<String> {
    let state = f.store.load_doc(f.page).await.unwrap();
    let journal = state.updates.len();
    let mut times = Vec::with_capacity(LOADS);
    for _ in 0..LOADS {
        let store = Store::open(&f.db).await.unwrap();
        let start = Instant::now();
        let page = stored_page(&store, f.page).await;
        let elapsed = start.elapsed();
        assert_eq!(page.read_blocks().len(), blocks);
        drop(page);
        store.close().await;
        times.push(elapsed);
    }
    let p50 = percentile(&times, 0.5);
    let p95 = percentile(&times, 0.95);
    println!(
        "MESURE chargement serveur, {what} (journal : {journal} mises à jour) : \
         p50 {p50:?}, p95 {p95:?}, max {:?} ({LOADS} chargements)",
        times.iter().max().unwrap()
    );
    (p95 >= BUDGET)
        .then(|| format!("chargement, {what} : p95 {p95:?} (budget {BUDGET:?}), mesures {times:?}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn load_10000_block_page_from_store_under_300ms() {
    let _alone = alone().await;
    let cfg = SyncConfig::default();
    let f = Fixture::new(cfg).await;
    f.write_mmd(&page_mmd()).await;
    let ids = f.ids().await;
    assert_eq!(ids.len(), BLOCKS);

    // Pire cas du journal : `snapshot_every - 1` mises à jour depuis
    // l'instantané (la suivante déclenche une compaction). L'écriture MMD en
    // est une ; un client tape les autres, une frappe par mise à jour.
    let edits = cfg.snapshot_every - 2;
    let client = f.client().await;
    for _ in 0..edits {
        client.type_text(&ids[0], "x");
    }
    processed(&client).await;
    drop(client);
    let journal = f.store.load_doc(f.page).await.unwrap().updates.len() as u64;
    assert_eq!(journal, cfg.snapshot_every - 1, "journal du pire cas");

    let over = loads_over_budget(
        &f,
        BLOCKS,
        &format!("page de {BLOCKS} blocs + {journal} frappes"),
    )
    .await;
    assert!(over.is_none(), "{}", over.unwrap_or_default());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn load_10000_block_page_after_full_rewrite_under_300ms() {
    let _alone = alone().await;
    let mmd = page_mmd();

    // (a) Réécriture complète par le serveur, comme `PUT /mmd` : tous les
    // blocs sont supprimés et recréés.
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&mmd).await;
    let start = Instant::now();
    f.write_mmd(&mmd).await;
    println!(
        "MESURE réécriture complète de {BLOCKS} blocs dans l'acteur : {:?}",
        start.elapsed()
    );
    let rewritten = loads_over_budget(
        &f,
        BLOCKS,
        &format!("page de {BLOCKS} blocs réécrite par le serveur"),
    )
    .await;
    drop(f);

    // (b) Un client remplace tout le contenu qu'il avait écrit, comme un
    // navigateur qui colle une page, puis « tout sélectionner » et colle de
    // nouveau : la seconde mise à jour supprime les 10 000 blocs et en insère
    // 10 000 autres. C'est le cas coûteux pour le ramasse-miettes de yrs : les
    // blocs supprimés et les neufs viennent du même client.
    let f = Fixture::new(SyncConfig::default()).await;
    let client = f.client().await;
    let blocks = parse(&mmd).unwrap();
    // Un seul document d'édition, donc un seul identifiant de client Yjs pour
    // les deux collages, comme un onglet.
    let editor = client.page().unwrap();
    for _ in 0..2 {
        let update = editor.write_blocks(&blocks).unwrap();
        client.apply_local(&update);
        processed(&client).await;
    }
    drop(client);
    let replaced = loads_over_budget(
        &f,
        BLOCKS,
        &format!("page de {BLOCKS} blocs remplacée par un client"),
    )
    .await;

    // Les deux cas sont mesurés avant de conclure.
    let over: Vec<String> = [rewritten, replaced].into_iter().flatten().collect();
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// `KEYSTROKES` frappes d'un client dans une page de `BLOCKS` blocs, chronométrées jusqu'à ce
/// qu'un autre client les ait reçues (son vecteur d'état atteint celui de l'auteur : un contrôle
/// qui ne relit pas la page). Renvoie les durées.
async fn keystrokes_seen_by_another(f: &Fixture) -> Vec<Duration> {
    let id = f.ids().await[0].clone();
    let (author, other) = (f.client().await, f.client().await);
    let mut times = Vec::with_capacity(KEYSTROKES);
    for _ in 0..KEYSTROKES {
        let start = Instant::now();
        author.type_text(&id, "x");
        let client = ClientID::new(author.client_id());
        let target = author.state_vector().get(&client);
        assert!(
            other
                .wait_until(Duration::from_secs(60), |c| c.state_vector().get(&client)
                    >= target)
                .await,
            "la frappe n'est pas arrivée"
        );
        times.push(start.elapsed());
    }
    times
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn keystroke_that_cuts_a_version_of_10000_block_page_under_150ms() {
    let _alone = alone().await;
    let mmd = page_mmd();

    // Ce que coûte une version, à part : l'état encodé depuis le document de l'acteur, puis
    // écrit, comme le fait `Actor::cut_version`.
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&mmd).await;
    let page = stored_page(&f.store, f.page).await;
    let start = Instant::now();
    let state = page.encode_state();
    let encoded = start.elapsed();
    let start = Instant::now();
    f.store
        .put_version(f.page, 1, &state, &[], VersionReason::Periodic)
        .await
        .unwrap();
    let written = start.elapsed();
    println!(
        "MESURE version d'une page de {BLOCKS} blocs : {} octets, encodée en {encoded:?}, \
         écrite en {written:?}",
        state.len()
    );
    drop(f);

    // Les frappes sans version (dix minutes), puis chacune précédée d'une version (intervalle
    // nul : l'écriture d'avant est toujours assez ancienne).
    let mut over = Vec::new();
    for (what, every) in [
        ("sans version", SyncConfig::default().version_every),
        ("chacune précédée d'une version", Duration::ZERO),
    ] {
        let f = Fixture::new(SyncConfig::default().with_version_every(every)).await;
        f.write_mmd(&mmd).await;
        let times = keystrokes_seen_by_another(&f).await;
        let versions = f.store.list_versions(f.page).await.unwrap().len();
        let (p50, p95) = (percentile(&times, 0.5), percentile(&times, 0.95));
        println!(
            "MESURE frappe vue par un autre client, page de {BLOCKS} blocs, {what} \
             ({versions} versions coupées) : p50 {p50:?}, p95 {p95:?}, max {:?}, \
             première {:?} ({KEYSTROKES} frappes)",
            times.iter().max().unwrap(),
            times[0]
        );
        if every.is_zero() {
            assert_eq!(versions, KEYSTROKES, "une version avant chaque frappe");
        }
        if p95 >= TYPING_BUDGET {
            over.push(format!(
                "frappe, {what} : p95 {p95:?} (budget {TYPING_BUDGET:?}), mesures {times:?}"
            ));
        }
        // La première frappe est la seule qui suive la connexion, et le p95 au plus proche rang
        // écarte toujours la plus lente des vingt : elle est jugée à part (Ruling R488). Sans
        // version seulement : précédée d'une version, elle paie aussi le disque du coureur (R487).
        if !every.is_zero() && times[0] >= TYPING_BUDGET {
            over.push(format!(
                "première frappe après la connexion, {what} : {:?} (budget {TYPING_BUDGET:?})",
                times[0]
            ));
        }
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// Une frappe de `author` dans le bloc `id`, chronométrée jusqu'à ce que `other` l'ait reçue (son
/// vecteur d'état atteint celui de l'auteur).
async fn keystroke_seen(author: &Client, other: &Client, id: &str) -> Duration {
    let start = Instant::now();
    author.type_text(id, "x");
    let client = ClientID::new(author.client_id());
    let target = author.state_vector().get(&client);
    assert!(
        other
            .wait_until(Duration::from_secs(60), |c| c.state_vector().get(&client)
                >= target)
            .await,
        "la frappe n'est pas arrivée"
    );
    start.elapsed()
}

/// Écritures entre deux compactions, dans [`keystroke_that_triggers_a_compaction_of_10000_block_page_under_150ms`].
const COMPACT_EVERY: u64 = 5;

/// Compactions mesurées une à une.
const COMPACTIONS: usize = 12;

/// Frappes à la suite du second temps de la mesure, au moins : on tape encore tant que
/// [`SNAPSHOTS_DURING_THE_STREAM`] instantanés ne se sont pas écrits (Ruling R887).
const STREAM: usize = 1_500;

/// Instantanés qui doivent s'être écrits pendant ces frappes. C'est la condition d'arrêt du
/// second temps, pas un compte fait après un nombre fixe de frappes : le nombre d'instantanés
/// écrits en 1 500 frappes est la durée des frappes divisée par celle d'une compaction (plus de
/// 100 ms), et un coureur dont le disque valide une frappe en 0,6 ms n'en voyait que six.
const SNAPSHOTS_DURING_THE_STREAM: usize = 10;

/// Ce que le second temps peut durer avant d'être tenu pour un échec : les compactions ne se font
/// plus pendant qu'on tape. La borne peut faire échouer la mesure à tort sur un poste très
/// chargé, jamais la faire passer.
const STREAM_BOUND: Duration = Duration::from_secs(120);

/// La part du serveur dans « frappe visible chez un autre < 150 ms » (§5.10) quand la frappe
/// déclenche une compaction d'une page de 10 000 blocs, et pour la frappe qui la suit (Ruling
/// R826 ; constat I-7.2 de la re-relecture 7). La compaction relit l'état (81 ms) avant de
/// l'écrire (Ruling R816) : faite dans l'acteur, elle retardait la frappe qui la déclenche (92 à
/// 103 ms sur ce poste, la tâche qui écrit sa connexion attendant que l'acteur rende la main) ou,
/// l'acteur cédant la main d'abord, la suivante (106 à 141 ms). Hors de l'acteur, ni l'une ni
/// l'autre ne l'attend.
///
/// Deux temps. (a) `COMPACTIONS` fois : des frappes jusqu'à celle qui déclenche la compaction,
/// celle-là et la suivante chronométrées, puis la compaction attendue (pour que la suivante en
/// déclenche une vraiment). (b) Au moins `STREAM` frappes à la suite, une compaction toutes les
/// `COMPACT_EVERY` écritures (une seule à la fois : celle qui tombe pendant une autre attend
/// l'écriture d'après), toutes chronométrées, jusqu'à ce qu'il s'y soit écrit
/// `SNAPSHOTS_DURING_THE_STREAM` instantanés, comptés en relisant celui que garde la base : les
/// frappes qui tombent pendant leur écriture l'attendent dans la voie de la page (Rulings R859,
/// R887).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn keystroke_that_triggers_a_compaction_of_10000_block_page_under_150ms() {
    let _alone = alone().await;
    let f = Fixture::new(SyncConfig {
        snapshot_every: COMPACT_EVERY,
        ..SyncConfig::default()
    })
    .await;
    f.write_mmd(&page_mmd()).await;
    f.hub.compacted(f.page).await.unwrap();
    let id = f.ids().await[0].clone();
    let (author, other) = (f.client().await, f.client().await);
    // Les écritures depuis la dernière compaction : l'écriture MMD en est une.
    let mut since = 1;
    let (mut triggering, mut following) = (Vec::new(), Vec::new());
    for _ in 0..COMPACTIONS {
        while since + 1 < COMPACT_EVERY {
            keystroke_seen(&author, &other, &id).await;
            since += 1;
        }
        let before = f
            .store
            .load_doc(f.page)
            .await
            .unwrap()
            .snapshot
            .map(|(seq, _)| seq);
        triggering.push(keystroke_seen(&author, &other, &id).await);
        following.push(keystroke_seen(&author, &other, &id).await);
        f.hub.compacted(f.page).await.unwrap();
        let after = f
            .store
            .load_doc(f.page)
            .await
            .unwrap()
            .snapshot
            .map(|(seq, _)| seq);
        assert!(after > before, "la frappe n'a pas déclenché de compaction");
        since = 1;
    }
    let mut stream = Vec::new();
    let mut snapshots = std::collections::HashSet::new();
    let started = Instant::now();
    while stream.len() < STREAM || snapshots.len() < SNAPSHOTS_DURING_THE_STREAM {
        assert!(
            started.elapsed() < STREAM_BOUND,
            "{} instantanés seulement pendant {} frappes à la suite ({STREAM_BOUND:?})",
            snapshots.len(),
            stream.len()
        );
        stream.push(keystroke_seen(&author, &other, &id).await);
        if stream.len() % 25 == 1 {
            let kept = f.store.load_doc(f.page).await.unwrap().snapshot;
            snapshots.extend(kept.map(|(seq, _)| seq));
        }
    }
    println!(
        "MESURE {} instantanés écrits pendant {} frappes à la suite, en {:?}",
        snapshots.len(),
        stream.len(),
        started.elapsed()
    );
    let mut over = Vec::new();
    for (what, times) in [
        ("qui déclenche une compaction", &triggering),
        ("qui suit celle qui déclenche une compaction", &following),
        (
            "à la suite, une compaction toutes les cinq écritures",
            &stream,
        ),
    ] {
        let (p50, p95) = (percentile(times, 0.5), percentile(times, 0.95));
        println!(
            "MESURE frappe vue par un autre client, page de {BLOCKS} blocs, {what} : p50 {p50:?}, \
             p95 {p95:?}, max {:?} ({} frappes)",
            times.iter().max().unwrap(),
            times.len()
        );
        if p95 >= TYPING_BUDGET {
            over.push(format!(
                "frappe {what} : p95 {p95:?} (budget {TYPING_BUDGET:?}), mesures {times:?}"
            ));
        }
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// Boucles d'écritures d'un simple lecteur, pendant les frappes de
/// [`keystroke_during_write_loops_of_a_reader_under_150ms`].
const READER_LOOPS: usize = 8;

/// La part du serveur dans « frappe visible chez un autre < 150 ms » (§5.10) pendant que
/// [`READER_LOOPS`] boucles d'un simple lecteur posent et retirent des favoris sans relâche,
/// chaque boucle sur sa page (la boucle de la relecture de la Task S5 : la frappe passait de
/// 4 ms à 876 ms au p95). Les frappes sont mesurées sans boucle, puis avec : la file
/// d'écriture du store (Rulings R510 à R516) donne son tour à l'acteur de la page après une
/// écriture du lecteur, pas après toutes celles qu'il a en file, et la frappe sans boucle ne
/// doit pas en payer le prix.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn keystroke_during_write_loops_of_a_reader_under_150ms() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let _alone = alone().await;
    let mmd = page_mmd();
    let mut over = Vec::new();
    for loops in [0, READER_LOOPS] {
        let f = Fixture::new(SyncConfig::default()).await;
        f.write_mmd(&mmd).await;
        let reader = new_user(&f.db).await;
        let stop = Arc::new(AtomicBool::new(false));
        let written = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..loops {
            let page = new_page(&f.store).await;
            let (store, stop, written) = (f.store.clone(), stop.clone(), written.clone());
            tasks.push(tokio::spawn(mementum_store::writes_by(
                Some(reader),
                async move {
                    let mut on = true;
                    while !stop.load(Ordering::Relaxed) {
                        let changed = store.set_favorite(reader, page, on, 1).await.unwrap();
                        assert!(changed, "chaque tour écrit");
                        on = !on;
                        written.fetch_add(1, Ordering::Relaxed);
                    }
                },
            )));
        }
        // Chaque boucle a écrit avant la première frappe.
        while written.load(Ordering::Relaxed) < 2 * loops {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let start = Instant::now();
        let before = written.load(Ordering::Relaxed);
        let times = keystrokes_seen_by_another(&f).await;
        let favorites = written.load(Ordering::Relaxed) - before;
        let elapsed = start.elapsed();
        stop.store(true, Ordering::Relaxed);
        for task in tasks {
            task.await.unwrap();
        }
        let (p50, p95) = (percentile(&times, 0.5), percentile(&times, 0.95));
        println!(
            "MESURE frappe vue par un autre client, page de {BLOCKS} blocs, pendant {loops} \
             boucles d'écritures d'un lecteur ({favorites} favoris écrits en {elapsed:?}) : \
             p50 {p50:?}, p95 {p95:?}, max {:?} ({KEYSTROKES} frappes)",
            times.iter().max().unwrap()
        );
        if p95 >= TYPING_BUDGET {
            over.push(format!(
                "frappe, {loops} boucles : p95 {p95:?} (budget {TYPING_BUDGET:?}), mesures {times:?}"
            ));
        }
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// Une mise à jour que la page refuse **après** l'avoir appliquée (elle écrit dans les
/// métadonnées de la page) : le document est alors rechargé du store (Ruling R538).
fn refused_after_it_is_applied(client: u64) -> Vec<u8> {
    let mut update = vec![1, 1];
    let mut id = client;
    while id > 0x7F {
        update.push(0x80 | (id & 0x7F) as u8);
        id >>= 7;
    }
    update.push(id as u8);
    // Horloge 0 ; une valeur `Any` (8), entrée de map (0x20) de la racine nommée « mementum ».
    update.extend_from_slice(&[0, 0x28, 1, 8]);
    update.extend_from_slice(b"mementum");
    update.extend_from_slice(&[1, b'x', 1, 119, 1, b'y', 0]);
    update
}

/// Le nombre de boucles d'un client refusé pendant la mesure de la frappe.
const REFUSED_LOOPS: usize = 4;

/// Budget du §5.10, « frappe visible chez un autre < 150 ms », pendant qu'un client envoie en
/// boucle une mise à jour refusée après son application : chaque refus recharge la page de
/// `BLOCKS` blocs (Ruling R538), et sans frein la boucle tient l'acteur occupé à recharger
/// (constat de la relecture de la garde des attributs : 296 ms au p95 pour quatre boucles, sept
/// rechargements par seconde et par boucle). Le frein (Ruling R725, module `brake`) : après
/// quelques refus de ce genre, les mises à jour du compte ferment leur connexion sans être
/// appliquées, pendant un temps qui double à chaque récidive. Mesuré une fois le frein mis ; ce
/// qui reste est le coût des connexions que les boucles ouvrent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn keystroke_during_loops_of_refused_updates_under_150ms() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let _alone = alone().await;
    let mmd = page_mmd();
    let mut over = Vec::new();
    for loops in [0, 1, REFUSED_LOOPS] {
        let f = Fixture::new(SyncConfig::default()).await;
        f.write_mmd(&mmd).await;
        let hostile = new_user(&f.db).await;
        let stop = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for n in 0..loops {
            let conn = ConnInfo {
                actor: Some(hostile),
                ..writing_conn()
            };
            let url = f.url_for(conn);
            let (stop, sent) = (stop.clone(), sent.clone());
            tasks.push(tokio::spawn(async move {
                let mut turn = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    turn += 1;
                    let Ok(client) = Client::connect(&url).await else {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        continue;
                    };
                    client.send_update(&refused_after_it_is_applied(
                        1_000_000 * (n as u64 + 1) + turn,
                    ));
                    client
                        .wait_until(Duration::from_secs(60), |c| c.is_closed())
                        .await;
                    sent.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }
        // Le frein est mis avant la première frappe : les refus gratuits du compte sont passés
        // (chacun a rechargé la page, et retardé d'autant la frappe tombée pendant lui : c'est
        // le coût borné du frein, dit au registre), et le suivant l'a freiné.
        let engaged = mementum_sync::FREE_REFUSALS as u64 + 1;
        while loops > 0 && f.hub.reloads() < engaged {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let start = Instant::now();
        let (before, reloads) = (sent.load(Ordering::Relaxed), f.hub.reloads());
        let times = keystrokes_seen_by_another(&f).await;
        let (turns, reloads) = (
            sent.load(Ordering::Relaxed) - before,
            f.hub.reloads() - reloads,
        );
        let elapsed = start.elapsed();
        stop.store(true, Ordering::Relaxed);
        for task in tasks {
            task.await.unwrap();
        }
        let (p50, p95) = (percentile(&times, 0.5), percentile(&times, 0.95));
        println!(
            "MESURE frappe vue par un autre client, page de {BLOCKS} blocs, pendant {loops} \
             boucles de mises à jour refusées après application ({turns} tours, {reloads} \
             rechargements en {elapsed:?}) : p50 {p50:?}, p95 {p95:?}, max {:?} ({KEYSTROKES} \
             frappes)",
            times.iter().max().unwrap()
        );
        if p95 >= TYPING_BUDGET {
            over.push(format!(
                "frappe, {loops} boucles refusées : p95 {p95:?} (budget {TYPING_BUDGET:?}), mesures {times:?}"
            ));
        }
    }
    assert!(over.is_empty(), "{}", over.join("\n"));
}

/// Ce que coûtent, sur une page de `BLOCKS` blocs, une restauration (le temps pendant lequel
/// l'acteur de la page est occupé : version `before_restore` coupée, puis tout le contenu
/// réécrit) et une version coupée quand la page en garde déjà cent (la purge en efface une dans
/// la même transaction). Mesuré et consigné au registre ; aucun budget du §5.10 ne porte sur une
/// restauration, qui ne bloque pas une frappe ordinaire hors de cet instant.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn restore_and_cut_with_a_full_history_of_10000_block_page() {
    let _alone = alone().await;
    let mmd = page_mmd();
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&mmd).await;
    let state = stored_page(&f.store, f.page).await.encode_state();

    // Cent versions déjà gardées, puis une de plus : la purge en retire une.
    let mut cuts = Vec::new();
    for seq in 0..=mementum_store::MAX_VERSIONS as i64 {
        let start = Instant::now();
        f.store
            .put_version(f.page, seq, &state, &[], VersionReason::Periodic)
            .await
            .unwrap();
        cuts.push(start.elapsed());
    }
    let full = *cuts.last().unwrap();
    let first = cuts[0];
    println!(
        "MESURE version d'une page de {BLOCKS} blocs ({} octets) : la première écrite en \
         {first:?}, la cent-unième (purge d'une version comprise) en {full:?}",
        state.len()
    );

    // Une restauration : la page a changé depuis (réécrite, un paragraphe de plus), une version
    // d'avant restauration est coupée, puis les blocs de la version sont écrits par-dessus.
    // Depuis le Ruling R201, `PUT /mmd` garde l'identifiant des blocs inchangés : récrite à
    // l'identique, la page vaudrait la version, et la restauration ne ferait rien (R180).
    let version = f.store.list_versions(f.page).await.unwrap()[0].id;
    let changed = format!("{mmd}\nChangée\n");
    f.write_mmd(&changed).await;
    let mut restores = Vec::new();
    for _ in 0..3 {
        f.write_mmd(&changed).await;
        let start = Instant::now();
        f.hub.restore_version(f.page, version, None).await.unwrap();
        restores.push(start.elapsed());
    }
    println!("MESURE restauration d'une page de {BLOCKS} blocs (acteur occupé) : {restores:?}");
}

/// Ce que coûte, dans le document d'une page de `BLOCKS` blocs, la reconnaissance d'un bloc
/// recréé et son report (Task 13b) : `apply_client_update` puis `dedupe_blocks`, comme l'acteur.
/// Quatre mises à jour faites depuis le même état, chacune reçue `LOADS` fois par une page
/// fraîche : une frappe (aucun élément inséré : rien ne s'exécute de neuf) ; une Entrée au milieu
/// d'un bloc (un bloc neuf, sans original : la recherche des originaux tourne et ne trouve rien) ;
/// le premier bloc déplacé tout en bas, seul (rien à reporter) ; le même déplacement, arrivé après
/// une frappe dans ce bloc (reportée). Mesuré et consigné au registre ; le budget du §5.10 qui
/// porte sur une frappe est mesuré par `keystroke_that_cuts_a_version_of_10000_block_page_under_150ms`.
#[tokio::test]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn move_of_a_block_in_10000_block_page() {
    let _alone = alone().await;
    let (base, _) = mementum_crdt::PageDoc::new_page();
    base.write_blocks(&parse(&page_mmd()).unwrap()).unwrap();
    let state = base.encode_state();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
    let (first, middle, last) = (&ids[0], &ids[BLOCKS / 2], &ids[BLOCKS - 1]);
    let from_state = || mementum_crdt::PageDoc::from_updates([state.as_slice()]).unwrap();
    let typed = from_state().replace_text(first, 0, 0, "de Tom ").unwrap();
    let entered = {
        let doc = from_state();
        doc.replace_text(middle, 10, 50, "").unwrap();
        let tail = parse("de la page de mesure, avec un peu de texte ordinaire.\n").unwrap();
        doc.insert_blocks_after(Some(middle), &tail).unwrap();
        doc.encode_diff(&from_state().state_vector()).unwrap()
    };
    let moved = from_state().move_block(first, Some(last)).unwrap();

    let mut lines = Vec::new();
    for (what, before, update) in [
        ("frappe", None, &typed),
        ("Entrée au milieu d'un bloc", None, &entered),
        ("déplacement, rien à reporter", None, &moved),
        (
            "déplacement arrivé après une frappe, reportée",
            Some(&typed),
            &moved,
        ),
    ] {
        let mut times = Vec::with_capacity(LOADS);
        let mut carried = 0;
        for _ in 0..LOADS {
            let page = from_state();
            if let Some(before) = before {
                page.apply_client_update(before).unwrap();
                page.dedupe_blocks();
            }
            let start = Instant::now();
            let applied = page.apply_client_update(update).unwrap();
            page.dedupe_blocks();
            times.push(start.elapsed());
            carried += usize::from(applied.carried.is_some());
        }
        lines.push(format!(
            "MESURE {what}, page de {BLOCKS} blocs, appliquée et dédoublonnée : p50 {:?}, \
             p95 {:?} ({carried} reports sur {LOADS})",
            percentile(&times, 0.5),
            percentile(&times, 0.95)
        ));
        if before.is_some() {
            assert_eq!(carried, LOADS, "{what} : la frappe est reportée");
        } else {
            assert_eq!(carried, 0, "{what} : rien à reporter");
        }
    }
    for line in lines {
        println!("{line}");
    }
}

/// Un grand groupe de blocs recréés d'un coup (R234 : un groupe glissé est recréé en entier) :
/// les 2 000 premiers blocs d'une page de `BLOCKS` blocs, dont chaque texte est découpé par une
/// frappe, déplacés tout en bas, seuls puis après une frappe dans chacun d'eux (toutes
/// reportées). Le relevé du texte découpe les blocs de yrs une fois pour tous (R371) : le coût
/// suit le groupe, pas le groupe multiplié par ce qu'il supprime (4 s avant ce découpage unique,
/// contre 0,36 s pour la même mise à jour appliquée sans report, sonde du registre).
#[tokio::test]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn group_of_2000_blocks_moved_in_10000_block_page() {
    const MOVED: usize = 2_000;
    let _alone = alone().await;
    let (base, _) = mementum_crdt::PageDoc::new_page();
    base.write_blocks(&parse(&page_mmd()).unwrap()).unwrap();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
    let fragmented =
        mementum_crdt::PageDoc::from_updates([base.encode_state().as_slice()]).unwrap();
    for id in &ids {
        fragmented.replace_text(id, 5, 0, "x").unwrap();
    }
    let state = fragmented.encode_state();
    let fresh = || mementum_crdt::PageDoc::from_updates([state.as_slice()]).unwrap();
    let typist = fresh();
    for id in &ids[..MOVED] {
        typist.replace_text(id, 0, 0, "T").unwrap();
    }
    let typed = typist.encode_diff(&fresh().state_vector()).unwrap();
    let mover = fresh();
    let mut after = ids[BLOCKS - 1].clone();
    for id in &ids[..MOVED] {
        mover.move_block(id, Some(&after)).unwrap();
        after = id.clone();
    }
    let moved = mover.encode_diff(&fresh().state_vector()).unwrap();
    for (what, before) in [("rien à reporter", None), ("tout reporté", Some(&typed))] {
        let page = fresh();
        if let Some(before) = before {
            page.apply_client_update(before).unwrap();
        }
        let start = Instant::now();
        let applied = page.apply_client_update(&moved).unwrap();
        page.dedupe_blocks();
        let took = start.elapsed();
        assert_eq!(applied.carried.is_some(), before.is_some(), "{what}");
        println!(
            "MESURE groupe de {MOVED} blocs recréés, page de {BLOCKS} blocs, {what} : {took:?} \
             (mise à jour {} octets, report {} octets)",
            moved.len(),
            applied.carried.map_or(0, |c| c.len())
        );
    }
}

/// L'e2e `apps/web/e2e/budgets.spec.ts` attend `SERVER_IDLE_CLOSE_MS` (plus une
/// marge) pour que le serveur ferme l'acteur des pages qu'il vient d'écrire, et
/// les relise dans SQLite à leur ouverture. Ce test échoue, au lieu que l'e2e
/// mesure sans bruit des pages encore en mémoire :
/// - si `mementum serve` (crates `mementum-server` et `mementum-cli`) crée son
///   hub avec autre chose que `SyncConfig::default()`, ou que ce réglage dont
///   seul l'intervalle des versions change (`with_version_every`, qui ne
///   touche à rien d'autre, Ruling R183) ;
/// - si la constante de l'e2e ne vaut plus `idle_close` de ce réglage.
#[test]
fn e2e_wait_follows_idle_close() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut hubs = Vec::new();
    for dir in ["mementum-server/src", "mementum-cli/src"] {
        let mut pending = vec![crates.join(dir)];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    hubs.extend(
                        source
                            .lines()
                            .filter(|line| line.contains("Hub::new("))
                            .map(|line| (path.clone(), source.clone(), line.trim().to_string())),
                    );
                }
            }
        }
    }
    assert!(
        !hubs.is_empty(),
        "aucun `Hub::new(` dans mementum-server ni mementum-cli"
    );
    for (path, source, line) in &hubs {
        // Le réglage par défaut tel quel, ou une variable du même fichier qui le prend en ne
        // changeant que l'intervalle des versions : ni `SyncConfig { …, ..SyncConfig::default() }`,
        // ni un appel réparti sur plusieurs lignes (refusé ici, à revoir à la main).
        let config = line
            .strip_suffix(");")
            .and_then(|call| call.rsplit_once(", "))
            .map(|(_, config)| config.trim());
        let only_versions_change = |name: &str| {
            source.lines().any(|other| {
                other.trim()
                    == format!(
                        "let {name} = SyncConfig::default().with_version_every(cfg.version_every);"
                    )
            })
        };
        let kept = match config {
            Some("SyncConfig::default()") => true,
            Some(name) => {
                name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && only_versions_change(name)
            }
            None => false,
        };
        assert!(
            kept && !line.contains(".."),
            "`mementum serve` doit garder `SyncConfig::default()` (seul l'intervalle des versions \
             peut changer), dont l'e2e suit `idle_close` : {} : {line}",
            path.display()
        );
    }

    let path = crates.join("../apps/web/e2e/budgets.spec.ts");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} illisible : {e}", path.display()));
    let value = source
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("const SERVER_IDLE_CLOSE_MS = ")?
                .strip_suffix(';')
        })
        .expect("constante `SERVER_IDLE_CLOSE_MS` absente de budgets.spec.ts");
    let wait: u128 = value
        .replace('_', "")
        .parse()
        .expect("`SERVER_IDLE_CLOSE_MS` n'est pas un entier");
    assert_eq!(
        wait,
        SyncConfig::default().idle_close.as_millis(),
        "`SERVER_IDLE_CLOSE_MS` (budgets.spec.ts) doit valoir `idle_close` de `SyncConfig`"
    );
}

/// Une écriture d'agent (Task S4 du lot 5) sur la lecture `read`.
fn agent_edit(
    writer: mementum_store::Writer,
    read: &mementum_sync::PageRead,
    ops: Vec<mementum_crdt::ops::Operation>,
) -> mementum_sync::Edit {
    mementum_sync::Edit {
        ops,
        base: Some((read.revision.clone(), Some(read.hashes.clone()))),
        writer,
        ..mementum_sync::Edit::default()
    }
}

/// Le remplacement d'un bloc par un paragraphe.
fn replace_block_op(anchor: &str, mmd: &str) -> mementum_crdt::ops::Operation {
    mementum_crdt::ops::Operation::ReplaceBlock {
        anchor: anchor.into(),
        blocks: parse(mmd).unwrap(),
        allow_deleting_content: false,
    }
}

/// Task S4 du lot 5 : la durée d'une écriture d'agent d'une opération sur une page de 10 000
/// blocs, appel du hub compris (une commande de l'acteur : lecture des blocs et de la révision,
/// plan, estimation, écriture par différence, journal, diffusion, relecture des blocs écrits), et
/// celle de la lecture d'une page (`Hub::read_page`, empreintes comprises). Une mesure, sans
/// borne : X2 en fait un budget s'il le faut (décision L14).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn measure_agent_edit_on_a_10000_block_page() {
    let _alone = alone().await;
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&page_mmd()).await;
    let writer = mementum_store::Writer::default();
    let mut reads = Vec::with_capacity(KEYSTROKES);
    let mut edits: Vec<(&str, Vec<Duration>)> = vec![
        ("str_replace dans un bloc", Vec::new()),
        ("insert d'un bloc", Vec::new()),
        ("replace_block", Vec::new()),
    ];
    for round in 0..KEYSTROKES {
        let start = Instant::now();
        let read = f.hub.read_page(f.page).await.unwrap();
        reads.push(start.elapsed());
        // Un paragraphe du milieu qu'aucun tour n'a encore touché.
        let mark = format!("Paragraphe {} ", BLOCKS / 2 + round);
        let middle = read
            .blocks
            .iter()
            .find(|b| mementum_md::block_text(b).starts_with(&mark))
            .expect("le paragraphe du tour")
            .id
            .clone();
        let ops = [
            vec![mementum_crdt::ops::Operation::StrReplace {
                old: "ordinaire".into(),
                new: format!("ordinaire {round}"),
                anchor: Some(middle.clone()),
                replace_all: false,
            }],
            vec![mementum_crdt::ops::Operation::Insert {
                at: mementum_crdt::ops::Place::After(middle.clone()),
                blocks: parse(&format!("Inséré {round}\n")).unwrap(),
            }],
            vec![replace_block_op(&middle, &format!("Remplacé {round}\n"))],
        ];
        for (ops, (_, times)) in ops.into_iter().zip(edits.iter_mut()) {
            let read = f.hub.read_page(f.page).await.unwrap();
            let start = Instant::now();
            f.hub
                .edit_page(f.page, agent_edit(writer, &read, ops))
                .await
                .unwrap();
            times.push(start.elapsed());
        }
    }
    println!(
        "MESURE lecture d'une page de {BLOCKS} blocs par un agent (blocs, révision, empreintes) : \
         p50 {:?}, p95 {:?}, max {:?}",
        percentile(&reads, 0.5),
        percentile(&reads, 0.95),
        reads.iter().max().unwrap()
    );
    for (what, times) in &edits {
        println!(
            "MESURE écriture d'agent, {what}, page de {BLOCKS} blocs : p50 {:?}, p95 {:?}, max {:?} \
             ({KEYSTROKES} écritures)",
            percentile(times, 0.5),
            percentile(times, 0.95),
            times.iter().max().unwrap()
        );
    }
}

/// Task S4 du lot 5 : de l'appel d'un agent à l'écriture reçue par un navigateur (un client de
/// test), sur une page de 10 000 blocs. La cible reste celle de la frappe, 150 ms (« Budgets de
/// performance » du plan du lot 5) ; une mesure, sans borne.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn measure_agent_edit_seen_by_a_client() {
    let _alone = alone().await;
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&page_mmd()).await;
    let other = f.client().await;
    let mut times = Vec::with_capacity(KEYSTROKES);
    for round in 0..KEYSTROKES {
        let read = f.hub.read_page(f.page).await.unwrap();
        let middle = read.blocks[BLOCKS / 2].id.clone();
        let ops = vec![mementum_crdt::ops::Operation::StrReplace {
            old: "ordinaire".into(),
            new: format!("ordinaire {round}"),
            anchor: Some(middle),
            replace_all: false,
        }];
        let before = other.updates_received();
        let start = Instant::now();
        f.hub
            .edit_page(
                f.page,
                agent_edit(mementum_store::Writer::default(), &read, ops),
            )
            .await
            .unwrap();
        assert!(
            other
                .wait_until(Duration::from_secs(60), |c| c.updates_received() > before)
                .await,
            "l'écriture de l'agent n'est pas arrivée"
        );
        times.push(start.elapsed());
    }
    println!(
        "MESURE écriture d'agent vue par un client, page de {BLOCKS} blocs : p50 {:?}, p95 {:?}, \
         max {:?} ({KEYSTROKES} écritures)",
        percentile(&times, 0.5),
        percentile(&times, 0.95),
        times.iter().max().unwrap()
    );
}

/// Task S4 du lot 5 (R1234, R1257, R1572) : une opération destructrice d'un jeton qui coupe une
/// version `before_agent` sur une page de 10 000 blocs : la durée de l'appel (deux commandes et la
/// version entre elles), et les frappes d'un autre pendant ce temps, vues par un troisième. La
/// version est retenue par la garde du hub jusqu'à ce que la première commande soit finie, puis
/// lâchée ; les frappes partent aussitôt, une à une, jusqu'à la fin de l'appel. Une mesure, sans
/// borne.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure de performance, à lancer en --release (job `budgets` de la CI)"]
async fn measure_before_agent_version_of_a_10000_block_page() {
    let _alone = alone().await;
    let f = Fixture::new(SyncConfig::default()).await;
    f.write_mmd(&page_mmd()).await;
    let id = f.ids().await[0].clone();
    let (author, other) = (f.client().await, f.client().await);
    let mut calls = Vec::new();
    let mut keystrokes = Vec::new();
    // Un jeton vivant : une écriture par jeton relit sa ligne au journal (R1607).
    let token = common::new_token(&f.db, common::new_user(&f.db).await).await;
    for round in 0..5 {
        // Une frappe d'une personne : la tranche est écrite par un autre que le jeton.
        keystroke_seen(&author, &other, &id).await;
        let read = f.hub.read_page(f.page).await.unwrap();
        let target = read.blocks[BLOCKS / 2].id.clone();
        let edit = agent_edit(
            mementum_store::Writer {
                account: None,
                token: Some(token),
            },
            &read,
            vec![replace_block_op(&target, &format!("Remplacé {round}\n"))],
        );
        let held = f.hub.hold_agent_versions();
        let (hub, page) = (f.hub.clone(), f.page);
        let start = Instant::now();
        let task = tokio::spawn(async move { hub.edit_page(page, edit).await });
        let deadline = Instant::now() + Duration::from_secs(60);
        while f.hub.agent_versions_waiting() == 0 {
            assert!(Instant::now() < deadline, "aucune version retenue");
            tokio::task::yield_now().await;
        }
        let first = start.elapsed();
        drop(held);
        while !task.is_finished() {
            keystrokes.push(keystroke_seen(&author, &other, &id).await);
        }
        let edited = task.await.unwrap().unwrap();
        assert!(edited.version.is_some(), "aucune version coupée");
        calls.push((first, start.elapsed()));
    }
    let versions = f.store.list_versions(f.page).await.unwrap();
    println!(
        "MESURE opération destructrice d'un agent avec version, page de {BLOCKS} blocs \
         (première commande, appel entier) : {calls:?} ; {} versions",
        versions.len()
    );
    if !keystrokes.is_empty() {
        println!(
            "MESURE frappes d'un autre pendant la version et la seconde commande : p50 {:?}, \
             p95 {:?}, max {:?} ({} frappes)",
            percentile(&keystrokes, 0.5),
            percentile(&keystrokes, 0.95),
            keystrokes.iter().max().unwrap(),
            keystrokes.len()
        );
    }
}
