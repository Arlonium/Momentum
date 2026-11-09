//! Budget du §5.10 côté serveur : la part du serveur dans « Ouvrir une page
//! jamais vue (réseau local) < 300 ms ». Charger depuis SQLite le `PageDoc`
//! d'une page de 10 000 blocs, comme le fait l'acteur d'une page qui démarre,
//! prend moins de 300 ms au p95 sur 20 chargements :
//! - avec le pire journal de frappes (`snapshot_every - 1` mises à jour) ;
//! - après une grosse suppression, réécriture complète par le serveur ou
//!   contenu remplacé par un client (Ruling R40 : le ramasse-miettes de yrs
//!   se paie à l'écriture, jamais à chaque chargement).
//!
//! Ignorés par défaut, car la mesure n'a de sens qu'en `--release` :
//! `cargo test --release -p mementum-sync -- --ignored` (job `budgets` de la
//! CI).
//!
//! S'y ajoute un garde-fou, lancé avec les autres tests : l'attente de l'e2e
//! `budgets.spec.ts`, qui laisse le serveur fermer l'acteur des pages avant
//! de les ouvrir, suit `idle_close`.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use mementum_md::parse;
use mementum_store::Store;
use mementum_sync::SyncConfig;

/// Blocs de la page.
const BLOCKS: usize = 10_000;

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
    release_only();
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
    release_only();
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

/// L'e2e `apps/web/e2e/budgets.spec.ts` attend `SERVER_IDLE_CLOSE_MS` (plus une
/// marge) pour que le serveur ferme l'acteur des pages qu'il vient d'écrire, et
/// les relise dans SQLite à leur ouverture. Ce test échoue, au lieu que l'e2e
/// mesure sans bruit des pages encore en mémoire :
/// - si `mementum serve` (crates `mementum-server` et `mementum-cli`) crée son
///   hub avec autre chose que `SyncConfig::default()` ;
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
                            .map(|line| (path.clone(), line.trim().to_string())),
                    );
                }
            }
        }
    }
    assert!(
        !hubs.is_empty(),
        "aucun `Hub::new(` dans mementum-server ni mementum-cli"
    );
    for (path, line) in &hubs {
        // Le réglage par défaut tel quel : ni `SyncConfig { …, ..SyncConfig::default() }`,
        // ni un appel réparti sur plusieurs lignes (refusé ici, à revoir à la main).
        assert!(
            line.ends_with(", SyncConfig::default());") && !line.contains(".."),
            "`mementum serve` doit garder `SyncConfig::default()`, dont l'e2e suit `idle_close` : \
             {} : {line}",
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
