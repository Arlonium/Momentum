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
use mementum_store::{Store, VersionReason};
use mementum_sync::SyncConfig;
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
    release_only();
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
             ({versions} versions coupées) : p50 {p50:?}, p95 {p95:?}, max {:?} \
             ({KEYSTROKES} frappes)",
            times.iter().max().unwrap()
        );
        if every.is_zero() {
            assert_eq!(versions, KEYSTROKES, "une version avant chaque frappe");
        }
        if p95 >= TYPING_BUDGET {
            over.push(format!(
                "frappe, {what} : p95 {p95:?} (budget {TYPING_BUDGET:?}), mesures {times:?}"
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
    release_only();
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

    // Une restauration : la page a changé depuis (réécrite), une version d'avant restauration
    // est coupée, puis les blocs de la version sont écrits par-dessus.
    let version = f.store.list_versions(f.page).await.unwrap()[0].id;
    f.write_mmd(&mmd).await;
    let mut restores = Vec::new();
    for _ in 0..3 {
        f.write_mmd(&mmd).await;
        let start = Instant::now();
        f.hub.restore_version(f.page, version, None).await.unwrap();
        restores.push(start.elapsed());
    }
    println!("MESURE restauration d'une page de {BLOCKS} blocs (acteur occupé) : {restores:?}");
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
