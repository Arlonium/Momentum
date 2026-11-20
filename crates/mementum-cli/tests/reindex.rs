//! `mementum reindex` (lot 4, Task S7 ; conception §5.7) : la commande refait la projection de
//! chaque page (`blocks_index`, `page_links`) depuis son document, puis l'index plein texte depuis
//! la projection ; `--text-only` ne refait que l'index plein texte. Elle ne tourne que serveur
//! arrêté (Ruling R913), ouvre la base par la règle des copies d'avant migration (`backup.rs`), et
//! un serveur lancé ensuite ne refait rien : les jetons de la base et de l'index concordent.
//!
//! Les pages sont écrites par un vrai serveur, tué ensuite ; la base est relue par une connexion
//! à côté, l'index plein texte par `mementum_search`, une fois la commande finie.

mod common;
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use common::Server;
use mementum_search::{Found, PROJECTION_VERSION, SearchIndex, read_query};
use serde_json::{Value, json};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, Row, SqliteConnection};
use uuid::Uuid;

/// Mot de passe de test : une constante manifestement fausse.
const PASSWORD: &str = "mot-de-passe-de-test";

/// L'adresse publique des tests : un lien qui la porte et mène à une page est un rétrolien quand
/// la commande la connaît, comme le serveur (Ruling R998).
const ORIGIN: &str = "https://notes.exemple.fr";

/// Ce que dit la commande refusée pendant qu'un autre processus tient l'index.
const BUSY: &str = "l'index de recherche est déjà ouvert par un autre processus";

/// Ce que le serveur écrit au journal quand il reconstruit l'index plein texte, ou reprojette
/// toutes les pages, au démarrage.
const REBUILT: &str = "n'est pas celui de cette base";
const REPROJECTED: &str = "sous une autre version de la projection";

/// Attente maximale d'une condition (le serveur qui valide, ou qui le dit au journal) : bien plus
/// qu'il n'en faut, bien moins qu'un test qui pendrait. Elle peut faire échouer le test à tort
/// sur un poste très chargé, jamais le faire passer.
const WAIT: Duration = Duration::from_secs(30);

/// Les mots cherchés dans l'index plein texte.
const WORDS: [&str; 6] = [
    "pommes", "courge", "tomates", "recettes", "potager", "ailleurs",
];

/// Lance `mementum reindex --data-dir <data> <args>` avec ces variables, et rend sa sortie. Les
/// variables qu'elle pourrait lire sont retirées d'abord.
fn reindex(data: &Path, args: &[&str], vars: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mementum"))
        .arg("reindex")
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .env_remove("MEMENTUM_DATA_DIR")
        .env_remove("MEMENTUM_PUBLIC_URL")
        .env("RUST_LOG", "info")
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .output()
        .expect("lancement de mementum")
}

fn text(output: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// La commande a réussi : sa sortie standard.
fn succeeded(output: &Output) -> String {
    let (stdout, stderr) = text(output);
    assert!(output.status.success(), "échec : {stdout}\n{stderr}");
    stdout
}

/// La commande a échoué : sa sortie d'erreur.
fn failed(output: &Output) -> String {
    let (stdout, stderr) = text(output);
    assert!(
        !output.status.success(),
        "succès inattendu : {stdout}\n{stderr}"
    );
    stderr
}

/// Les noms des entrées de `dir`, triés.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Une instance remplie par un serveur, puis tué : deux pages d'Alice, dont la seconde mène à la
/// première par un lien relatif et par un lien sous [`ORIGIN`].
struct Filled {
    recipes: Uuid,
    garden: Uuid,
    cookie: String,
}

impl Filled {
    fn pages(&self) -> [Uuid; 2] {
        [self.recipes, self.garden]
    }
}

/// Récrit la page par `PUT /mmd`.
fn write_page(server: &Server, cookie: &str, page: Uuid, mmd: &str) {
    let path = format!("/api/v1/pages/{page}/mmd");
    let reply = server.request("PUT", &path, Some(mmd), &[("Cookie", cookie)]);
    assert_eq!(reply.status, 204, "{}", reply.body);
}

fn filled(data: &Path) -> Filled {
    let server = Server::start(data);
    let account = json!({ "email": "alice@exemple.fr", "name": "Alice", "password": PASSWORD });
    let signup = server.request(
        "POST",
        "/api/v1/auth/signup",
        Some(&account.to_string()),
        &[],
    );
    assert_eq!(signup.status, 201, "{}", signup.body);
    let cookie = signup.session_cookie().expect("cookie de session");
    let space = serde_json::from_str::<Value>(&signup.body).unwrap()["spaces"][0]["id"].clone();
    let page = |title: &str, mmd: &str| -> Uuid {
        let body = json!({ "spaceId": space, "title": title }).to_string();
        let reply = server.request("POST", "/api/v1/pages", Some(&body), &[("Cookie", &cookie)]);
        assert_eq!(reply.status, 201, "{}", reply.body);
        let page: Uuid = serde_json::from_str::<Value>(&reply.body).unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        write_page(&server, &cookie, page, mmd);
        page
    };
    let recipes = page(
        "Recettes d'automne",
        "Une tarte aux pommes caramélisées.\n\nUn gratin de courge.\n",
    );
    let garden = page(
        "Potager",
        &format!(
            "Des tomates anciennes, voir [les recettes](/p/{recipes}).\n\nEt \
             [ailleurs]({ORIGIN}/p/{recipes}) aussi.\n"
        ),
    );
    server.kill();
    Filled {
        recipes,
        garden,
        cookie,
    }
}

/// Mène `work` sur une connexion à la base, ouverte sans rien régler qui écrive (en lecture seule
/// si `read_only`), et la ferme.
fn with_db<T>(db: &Path, read_only: bool, work: impl AsyncFnOnce(&mut SqliteConnection) -> T) -> T {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let options = SqliteConnectOptions::new()
            .filename(db)
            .read_only(read_only)
            .busy_timeout(Duration::from_secs(5));
        let mut conn = SqliteConnection::connect_with(&options).await.unwrap();
        let out = work(&mut conn).await;
        conn.close().await.unwrap();
        out
    })
}

/// Une ligne de `blocks_index` : page, clé du bloc, type, texte, empreinte.
type BlockRow = (String, String, String, String, Vec<u8>);

/// Les lignes de `blocks_index`, rangées.
fn blocks(db: &Path) -> Vec<BlockRow> {
    with_db(db, true, async |conn| {
        sqlx::query(
            "SELECT node_id, block_id, type, text, hash FROM blocks_index \
             ORDER BY node_id, block_id",
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
        .collect()
    })
}

/// Les lignes de `page_links` : page, clé du bloc, page visée, rangées.
fn links(db: &Path) -> Vec<(String, String, String)> {
    with_db(db, true, async |conn| {
        sqlx::query(
            "SELECT node_id, block_id, target_id FROM page_links \
             ORDER BY node_id, block_id, target_id",
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect()
    })
}

/// Les lignes de `search_state` : page, `indexed_seq`, `failed_seq`, `truncated`, rangées.
fn states(db: &Path) -> Vec<(String, i64, Option<i64>, i64)> {
    with_db(db, true, async |conn| {
        sqlx::query(
            "SELECT node_id, indexed_seq, failed_seq, truncated FROM search_state \
             ORDER BY node_id",
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect()
    })
}

/// Les lignes de `search_outbox`.
fn outbox(db: &Path) -> i64 {
    with_db(db, true, async |conn| {
        sqlx::query_scalar("SELECT COUNT(*) FROM search_outbox")
            .fetch_one(&mut *conn)
            .await
            .unwrap()
    })
}

/// Un réglage de l'instance : `search.committed`, `search.intent`, `search.projection`.
fn setting(db: &Path, key: &str) -> Option<String> {
    with_db(db, true, async |conn| {
        sqlx::query_scalar("SELECT value FROM instance_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&mut *conn)
            .await
            .unwrap()
    })
}

/// Ce que l'index plein texte rend pour chacun de ces mots, parmi `pages`. L'index est fermé au
/// retour, ses fusions attendues : la commande suivante le vide sans trouver de fichier tenu.
fn searches(data: &Path, words: &[&str], pages: &[Uuid]) -> Vec<Found> {
    let opened = SearchIndex::open(&data.join("search")).unwrap();
    let allowed = pages.iter().copied().collect();
    let found = words
        .iter()
        .map(|words| {
            let query = read_query(words).unwrap();
            opened.index.search(&query, &allowed, 0, 20).unwrap()
        })
        .collect();
    opened.index.close().unwrap();
    found
}

/// Les pages que l'index plein texte rend pour ces mots, parmi `pages`.
fn found_pages(data: &Path, words: &str, pages: &[Uuid]) -> Vec<Uuid> {
    searches(data, &[words], pages)[0]
        .pages
        .iter()
        .map(|hit| hit.node_id)
        .collect()
}

/// Le jeton de la dernière validation de l'index plein texte.
fn index_token(data: &Path) -> Option<String> {
    let opened = SearchIndex::open(&data.join("search")).unwrap();
    let token = opened.token.clone();
    opened.index.close().unwrap();
    token
}

/// Attend `ready` ([`WAIT`] au plus), en le relisant toutes les 50 ms.
fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "jamais vu : {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

/// Plan, Task S7 : les tables de l'index vidées à la main, une page notée illisible, le dossier
/// de l'index supprimé : après la commande, la base et l'index portent ce qu'ils portaient. La
/// commande relit chaque page depuis son document (une page notée illisible est essayée de
/// nouveau), et prend l'adresse publique de la même configuration que le serveur (R998).
#[test]
fn reindex_rebuilds_everything_from_the_documents() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    let filled = filled(&data);
    let (recipes, garden) = (filled.recipes.to_string(), filled.garden.to_string());
    let db = data.join("mementum.db");
    let pages = filled.pages();
    let with_origin = ["--public-url", ORIGIN];

    let stdout = succeeded(&reindex(&data, &with_origin, &[]));
    assert!(stdout.contains("réindexation finie"), "{stdout}");
    let (rows, kept_links, kept_states) = (blocks(&db), links(&db), states(&db));
    let found = searches(&data, &WORDS, &pages);
    // Ce que la commande a fait, lu en clair : les blocs des deux pages, leurs rétroliens (le lien
    // relatif et celui qui porte l'adresse publique), et l'index plein texte qui les trouve.
    let texts: Vec<(&str, &str)> = rows
        .iter()
        .map(|(page, _, _, text, _)| (page.as_str(), text.as_str()))
        .collect();
    for expected in [
        (recipes.as_str(), "Une tarte aux pommes caramélisées."),
        (recipes.as_str(), "Un gratin de courge."),
        (garden.as_str(), "Des tomates anciennes, voir les recettes."),
        (garden.as_str(), "Et ailleurs aussi."),
    ] {
        assert!(texts.contains(&expected), "{expected:?} manque : {texts:?}");
    }
    assert_eq!(rows.len(), 4, "{texts:?}");
    assert_eq!(kept_links.len(), 2, "{kept_links:?}");
    assert!(
        kept_links
            .iter()
            .all(|(page, _, target)| *page == garden && *target == recipes),
        "{kept_links:?}"
    );
    assert!(
        kept_states
            .iter()
            .all(|(_, indexed, failed, _)| *indexed > 0 && failed.is_none()),
        "{kept_states:?}"
    );
    let hits = |found: &Found| -> Vec<Uuid> { found.pages.iter().map(|h| h.node_id).collect() };
    let expected: [&[Uuid]; 6] = [
        &[filled.recipes],
        &[filled.recipes],
        &[filled.garden],
        // « recettes » : le titre de la première, un mot de la seconde.
        &[filled.recipes, filled.garden],
        &[filled.garden],
        &[filled.garden],
    ];
    for ((word, found), expected) in WORDS.iter().zip(&found).zip(expected) {
        let mut got = hits(found);
        got.sort();
        let mut expected = expected.to_vec();
        expected.sort();
        assert_eq!(got, expected, "« {word} »");
    }
    assert_eq!(
        index_token(&data),
        setting(&db, "search.committed"),
        "les jetons concordent"
    );

    // Les tables vidées à la main (serveur arrêté), la première page notée illisible comme le
    // fait `Store::index_failed`, le dossier supprimé.
    with_db(&db, false, async |conn| {
        for statement in [
            "DELETE FROM blocks_index",
            "DELETE FROM page_links",
            "DELETE FROM search_outbox",
        ] {
            sqlx::query(statement).execute(&mut *conn).await.unwrap();
        }
        sqlx::query("UPDATE search_state SET failed_seq = indexed_seq WHERE node_id = ?")
            .bind(&recipes)
            .execute(&mut *conn)
            .await
            .unwrap();
    });
    std::fs::remove_dir_all(data.join("search")).unwrap();
    succeeded(&reindex(&data, &with_origin, &[]));
    assert_eq!(blocks(&db), rows);
    assert_eq!(links(&db), kept_links);
    assert_eq!(
        states(&db),
        kept_states,
        "la page notée illisible est relue"
    );
    assert_eq!(searches(&data, &WORDS, &pages), found);
    assert_eq!(index_token(&data), setting(&db, "search.committed"));
    assert_eq!(setting(&db, "search.intent"), None);
    assert_eq!(outbox(&db), 0);

    // Sans adresse publique, le lien qui la porte n'est plus un rétrolien ; la variable du
    // serveur la donne, comme à lui.
    succeeded(&reindex(&data, &[], &[]));
    assert_eq!(links(&db).len(), 1, "{:?}", links(&db));
    succeeded(&reindex(&data, &[], &[("MEMENTUM_PUBLIC_URL", ORIGIN)]));
    assert_eq!(links(&db), kept_links);
}

/// Plan, Task S7 : `--text-only` refait l'index plein texte depuis les tables, sans relire un
/// document : un bloc retiré à la main de `blocks_index` n'y est plus, et les tables restent
/// telles quelles. La commande entière, elle, le relit de son document.
#[test]
fn reindex_text_only_rebuilds_the_text_index_from_the_tables() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    let filled = filled(&data);
    let db = data.join("mementum.db");
    let pages = filled.pages();
    succeeded(&reindex(&data, &[], &[]));
    let (rows, kept_states) = (blocks(&db), states(&db));
    assert_eq!(found_pages(&data, "pommes", &pages), [filled.recipes]);

    // Le bloc des pommes retiré à la main des tables, le dossier de l'index supprimé.
    let apples = rows
        .iter()
        .find(|(_, _, _, text, _)| text.contains("pommes"))
        .map(|(_, block, _, _, _)| block.clone())
        .expect("le bloc des pommes");
    with_db(&db, false, async |conn| {
        sqlx::query("DELETE FROM blocks_index WHERE block_id = ?")
            .bind(&apples)
            .execute(&mut *conn)
            .await
            .unwrap();
    });
    std::fs::remove_dir_all(data.join("search")).unwrap();

    let stdout = succeeded(&reindex(&data, &["--text-only"], &[]));
    assert!(stdout.contains("index de recherche refait"), "{stdout}");
    assert_eq!(found_pages(&data, "pommes", &pages), Vec::<Uuid>::new());
    assert_eq!(found_pages(&data, "courge", &pages), [filled.recipes]);
    assert_eq!(found_pages(&data, "tomates", &pages), [filled.garden]);
    assert_eq!(found_pages(&data, "potager", &pages), [filled.garden]);
    // Les tables n'ont pas bougé : aucun document n'a été relu.
    let without: Vec<BlockRow> = rows
        .iter()
        .filter(|(_, block, _, _, _)| *block != apples)
        .cloned()
        .collect();
    assert_eq!(blocks(&db), without);
    assert_eq!(states(&db), kept_states);
    assert_eq!(index_token(&data), setting(&db, "search.committed"));
    assert_eq!(outbox(&db), 0);

    // Témoin : la commande entière relit le document, et le bloc revient.
    succeeded(&reindex(&data, &[], &[]));
    assert_eq!(blocks(&db), rows);
    assert_eq!(found_pages(&data, "pommes", &pages), [filled.recipes]);
}

/// Ruling R913 : un serveur qui tourne tient le verrou de l'écrivain de l'index dès son
/// démarrage ; la commande le prend avant tout et refuse, en français, sans avoir touché à la
/// base ni à l'index. Le verrou tenu sans serveur, sur une base à migrer, le montre : ouverte, la
/// base aurait été copiée puis migrée.
#[test]
fn reindex_refuses_while_a_server_holds_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    let server = Server::start(&data);
    for args in [&[][..], &["--text-only"]] {
        let stderr = failed(&reindex(&data, args, &[]));
        assert!(stderr.contains(BUSY), "{stderr}");
        assert!(stderr.contains("Rien n'a été modifié"), "{stderr}");
        assert!(stderr.contains("arrête"), "{stderr}");
    }
    assert_eq!(server.http("GET", "/health", None), (200, "ok".to_string()));
    server.kill();
    // Serveur arrêté, elle passe : c'est bien lui qui la refusait.
    succeeded(&reindex(&data, &[], &[]));

    let data = dir.path().join("verrou");
    std::fs::create_dir_all(&data).unwrap();
    let db = data.join("mementum.db");
    common::lot1_database(&db);
    let held = SearchIndex::open(&data.join("search")).unwrap();
    let bytes = std::fs::read(&db).unwrap();
    let before = entries(&data);
    let stderr = failed(&reindex(&data, &[], &[]));
    assert!(stderr.contains(BUSY), "{stderr}");
    assert_eq!(std::fs::read(&db).unwrap(), bytes, "la base a changé");
    assert_eq!(
        entries(&data),
        before,
        "une copie, ou un fichier, posé à côté"
    );
    held.index.close().unwrap();
}

/// La commande ouvre la base par `Store::open`, donc par `backup.rs` : une base à migrer est
/// copiée d'abord, et la copie est nommée ; une base refusée reste intacte, sans copie ; pas de
/// base, rien n'est créé ; une adresse publique invalide est refusée avant tout.
#[test]
fn reindex_opens_the_database_by_the_backup_rule() {
    let dir = tempfile::tempdir().unwrap();

    let absent = dir.path().join("absent");
    let stderr = failed(&reindex(&absent, &[], &[]));
    assert!(stderr.contains("aucune base"), "{stderr}");
    let stderr = failed(&reindex(
        &absent,
        &["--public-url", "notes.exemple.fr"],
        &[],
    ));
    assert!(stderr.contains("adresse publique invalide"), "{stderr}");
    failed(&reindex(
        &absent,
        &["--text-only", "--public-url", ORIGIN],
        &[],
    ));
    assert!(!absent.exists(), "rien n'est créé");

    // Une base du lot 1, à migrer : copiée, la copie nommée sur la sortie d'erreur, puis migrée
    // et indexée. Sa page, sans document, est trouvée par son titre.
    let data = dir.path().join("lot1");
    std::fs::create_dir_all(&data).unwrap();
    let db = data.join("mementum.db");
    common::lot1_database(&db);
    let backup = data.join("mementum.db.avant-0002.bak");
    let output = reindex(&data, &[], &[("RUST_LOG", "off")]);
    let (_, stderr) = text(&output);
    succeeded(&output);
    assert!(backup.is_file(), "copie attendue : {}", backup.display());
    let bytes = std::fs::metadata(&backup).unwrap().len();
    let line = stderr
        .lines()
        .find(|line| line.contains("mementum.db.avant-0002.bak"))
        .unwrap_or_else(|| panic!("la copie n'est pas nommée : {stderr}"));
    assert!(line.contains("migrée"), "{line}");
    assert!(line.contains(&format!("{bytes} octets")), "{line}");
    let home: Uuid = "01890000-0000-7000-8000-000000000003".parse().unwrap();
    assert_eq!(states(&db), [(home.to_string(), 0, None, 0)]);
    assert_eq!(found_pages(&data, "accueil", &[home]), [home]);
    assert_eq!(index_token(&data), setting(&db, "search.committed"));
    assert_eq!(
        setting(&db, "search.projection"),
        Some(PROJECTION_VERSION.to_string())
    );
    let (_, stderr) = text(&reindex(&data, &[], &[]));
    assert!(!stderr.contains(".bak"), "{stderr}");

    // Une base qu'un binaire intermédiaire a migrée : refusée, sans copie, sans un octet changé.
    let data = dir.path().join("intermediaire");
    std::fs::create_dir_all(&data).unwrap();
    let db = data.join("mementum.db");
    common::intermediate_database(&db, 3);
    std::fs::write(
        data.join("mementum.db.avant-0003.bak"),
        b"copie d'avant la 0003",
    )
    .unwrap();
    let bytes = std::fs::read(&db).unwrap();
    let stderr = failed(&reindex(&data, &[], &[]));
    assert!(
        stderr.contains("la migration 0003 de cette base n'est pas celle de ce binaire"),
        "{stderr}"
    );
    assert_eq!(std::fs::read(&db).unwrap(), bytes, "la base a changé");
    // Le verrou de l'index est pris avant la base : son dossier peut naître, rien d'autre.
    assert_eq!(
        entries(&data),
        ["mementum.db", "mementum.db.avant-0003.bak", "search"]
    );
}

/// Plan, Task S7 (R960, R909) : après la commande, les jetons de la base et de l'index concordent,
/// et la version de la projection est la sienne ; un serveur relancé ne reprojette ni ne
/// reconstruit rien, et cherche sur l'index que la commande a laissé. Témoin : le même démarrage
/// sans cet index le dit reconstruit.
#[test]
fn a_server_started_after_reindex_rebuilds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("donnees");
    let filled = filled(&data);
    let db = data.join("mementum.db");
    let pages = filled.pages();
    succeeded(&reindex(&data, &[], &[]));
    let committed = setting(&db, "search.committed");
    assert!(committed.is_some());
    assert_eq!(setting(&db, "search.intent"), None);
    assert_eq!(index_token(&data), committed, "les jetons concordent");
    assert_eq!(
        setting(&db, "search.projection"),
        Some(PROJECTION_VERSION.to_string())
    );
    assert_eq!(outbox(&db), 0);
    assert!(
        states(&db)
            .iter()
            .all(|(_, indexed, failed, _)| *indexed >= 0 && failed.is_none()),
        "{:?}",
        states(&db)
    );

    // Une écriture fait valider le serveur : il a comparé les jetons avant tout passage.
    let server = Server::start_with_env(
        &data,
        &[("RUST_LOG", "info"), ("MEMENTUM_INDEX_AFTER_MS", "0")],
    );
    write_page(
        &server,
        &filled.cookie,
        filled.garden,
        "Des tomates cerises.\n",
    );
    wait_until("une validation du serveur", || {
        setting(&db, "search.committed") != committed
    });
    let log = server.stderr();
    assert!(!log.contains(REBUILT), "reconstruit : {log}");
    assert!(!log.contains(REPROJECTED), "reprojeté : {log}");
    server.kill();
    // L'index que la commande a laissé, tenu à jour par le serveur.
    assert_eq!(found_pages(&data, "cerises", &pages), [filled.garden]);
    assert_eq!(found_pages(&data, "pommes", &pages), [filled.recipes]);

    // Témoin : sans l'index de la commande, le même démarrage le dit reconstruit.
    std::fs::remove_dir_all(data.join("search")).unwrap();
    let server = Server::start_with_env(&data, &[("RUST_LOG", "info")]);
    wait_until("la reconstruction dite au journal", || {
        server.stderr().contains(REBUILT)
    });
}

mod known_limits {
    use super::limit::known_limit;
    use super::*;

    /// Voulu (plan, « Reconstruction, sauvegardes ») : `mementum reindex` refait `blocks_index`
    /// depuis les documents, quoi que la table porte. Aujourd'hui (Ruling R1062) : chaque page est
    /// relue et projetée, mais écrite par différence avec les empreintes gardées (R919) ; une
    /// ligne dont le texte a changé dans la base sans que son empreinte change n'est pas récrite,
    /// et l'index plein texte, refait depuis la table, porte ce texte. Une ligne perdue, une
    /// empreinte changée ou une projection d'une autre version (R960) sont refaites.
    #[test]
    fn a_row_changed_by_hand_with_its_fingerprint_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("donnees");
        let filled = filled(&data);
        let db = data.join("mementum.db");
        let pages = filled.pages();
        succeeded(&reindex(&data, &[], &[]));
        assert_eq!(found_pages(&data, "courge", &pages), [filled.recipes]);

        // Le texte d'une ligne changé à la main, serveur arrêté, son empreinte gardée.
        with_db(&db, false, async |conn| {
            let changed = sqlx::query(
                "UPDATE blocks_index SET text = 'Un gratin de potiron.' \
                 WHERE text = 'Un gratin de courge.'",
            )
            .execute(&mut *conn)
            .await
            .unwrap();
            assert_eq!(changed.rows_affected(), 1);
        });
        succeeded(&reindex(&data, &[], &[]));

        let mut unmet = Vec::new();
        let texts: Vec<String> = blocks(&db).into_iter().map(|row| row.3).collect();
        if !texts.iter().any(|text| text == "Un gratin de courge.") {
            unmet.push("blocks_index ne porte pas le texte du document".to_string());
        }
        if found_pages(&data, "courge", &pages).is_empty() {
            unmet.push("le mot du document ne trouve plus la page".to_string());
        }
        if !found_pages(&data, "potiron", &pages).is_empty() {
            unmet.push("le mot changé à la main trouve la page".to_string());
        }
        known_limit(
            "R1062 : la projection s'écrit par différence avec les empreintes gardées",
            &unmet,
            &[
                "blocks_index ne porte pas le texte du document".to_string(),
                "le mot du document ne trouve plus la page".to_string(),
                "le mot changé à la main trouve la page".to_string(),
            ],
        );
    }
}
