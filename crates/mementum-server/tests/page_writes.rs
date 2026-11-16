//! Une écriture du serveur à la fois par page (Ruling R200).
//!
//! `PUT /mmd` et la restauration d'une version contrôlent les droits deux fois, puis envoient
//! une commande à l'acteur de la page (Ruling R89). Envoyées d'un coup, des dizaines de ces
//! requêtes passaient toutes leurs contrôles dans les premières millisecondes, puis attendaient
//! dans la boîte de l'acteur : un droit retiré ensuite ne les arrêtait plus, et elles
//! remplaçaient le contenu de la page l'une après l'autre (constat S1 de la relecture finale).
//! Chaque page a donc un verrou, pris après le premier contrôle et la lecture du corps, et tenu
//! pendant « second contrôle des droits, commande à l'acteur, réponse » : chaque requête relit
//! ses droits après la précédente.
//!
//! Ce que ces tests comptent se lit dans la base, pas à l'horloge : un déclencheur témoin note,
//! dans la transaction qui écrit chaque mise à jour de Tom au journal, s'il avait encore sa
//! ligne d'accès sur la page.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::sync::{Loops, SLOW_DISK, members, open_app, open_app_with, user};
use common::{Account, Team, TestApp, WAIT, exec_sql, json_request, query_i64};
use mementum_acl::Level;
use tokio::task::JoinSet;
use tower::ServiceExt;
use uuid::Uuid;

/// Requêtes envoyées d'un coup.
const BURST: usize = 24;

/// Paragraphes de chaque contenu : assez pour qu'une écriture prenne du temps dans l'acteur.
const PARAGRAPHS: usize = 200;

/// Un MMD de `n` paragraphes marqués de `tag`.
fn many_paragraphs(tag: &str, n: usize) -> String {
    let lines: Vec<String> = (0..n).map(|i| format!("{tag} {i}")).collect();
    lines.join("\n\n") + "\n"
}

/// Une requête de `by`, telle qu'un navigateur l'envoie.
fn request(by: &Account<'_>, builder: axum::http::request::Builder, body: Body) -> Request<Body> {
    builder
        .header(header::HOST, "localhost")
        .header(header::COOKIE, by.browser.cookie().expect("une session"))
        .body(body)
        .unwrap()
}

/// Pose le témoin : chaque mise à jour que `who` écrit au journal de `page` y note, dans la
/// transaction qui l'écrit, si `who` avait encore sa ligne d'accès sur la page.
async fn witness(t: &TestApp, page: Uuid, who: Uuid) {
    exec_sql(
        &t.db_path(),
        &format!(
            "CREATE TABLE witness (seq INTEGER NOT NULL, granted INTEGER NOT NULL); \
             CREATE TRIGGER witness_writes AFTER INSERT ON doc_updates \
             WHEN NEW.node_id = '{page}' AND NEW.actor_id = '{who}' \
             BEGIN INSERT INTO witness VALUES (NEW.seq, \
               (SELECT COUNT(*) FROM acl WHERE node_id = NEW.node_id AND subject = 'user:{who}')); \
             END;"
        ),
    )
    .await;
}

/// Les écritures notées par le témoin, avec l'accès (`true`) ou sans.
async fn witnessed(t: &TestApp, granted: bool) -> i64 {
    let sql = format!(
        "SELECT COUNT(*) FROM witness WHERE granted = {}",
        i64::from(granted)
    );
    query_i64(&t.db_path(), &sql).await
}

/// Envoie `requests` d'un coup ; dès qu'une a répondu `204`, Olga retire l'accès de Tom à la
/// page ; puis attend toutes les réponses. Rend leurs statuts.
async fn burst_then_revoke(
    t: &TestApp,
    team: &Team<'_>,
    page: Uuid,
    requests: Vec<Request<Body>>,
) -> Vec<StatusCode> {
    burst_then(t, requests, async {
        team.olga
            .browser
            .delete(&format!(
                "/api/v1/pages/{page}/access/users/{}",
                team.tom.id
            ))
            .await
            .assert_status(StatusCode::OK);
    })
    .await
}

/// Envoie `requests` d'un coup ; dès qu'une a répondu `204`, fait `then` ; puis attend toutes
/// les réponses. Rend leurs statuts.
async fn burst_then(
    t: &TestApp,
    requests: Vec<Request<Body>>,
    then: impl std::future::Future<Output = ()>,
) -> Vec<StatusCode> {
    let written = Arc::new(AtomicUsize::new(0));
    let mut burst = JoinSet::new();
    for req in requests {
        let (router, written) = (t.router.clone(), written.clone());
        burst.spawn(async move {
            let status = router.oneshot(req).await.unwrap().status();
            if status == StatusCode::NO_CONTENT {
                written.fetch_add(1, Ordering::SeqCst);
            }
            status
        });
    }
    let deadline = tokio::time::Instant::now() + WAIT;
    while written.load(Ordering::SeqCst) == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "aucune écriture n'a abouti"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    then.await;
    let mut statuses = Vec::new();
    while let Some(status) = burst.join_next().await {
        statuses.push(status.unwrap());
    }
    statuses
}

// Les rafales de R200 (le retrait de l'accès) et de R209 (la fin de la session) vivent dans
// `src/routes/put_mmd_tests.rs`, en tests de route qui voient l'acteur : pour `PUT /mmd` depuis
// R494, pour la restauration depuis R535 (`mmd_writes_queued_before_a_revocation_stop_with_it`,
// `restores_queued_before_a_revocation_stop_with_it`, et leurs pendants pour la fin de la
// session). Un compte n'a plus que deux places dans la file d'une page, une qui écrit, une qui
// attend : une rafale d'un compte n'y laisse qu'une requête en attente, et seul un test qui voit
// l'acteur sait faire tomber le retrait pendant qu'elle attend.

/// Ruling R485, après R494 : sur un disque lent (`SLOW_DISK`), pendant que Mona crée des pages
/// ailleurs dans l'espace, en six boucles, Tom récrit la page par deux `PUT /mmd` de front (ses
/// deux places dans la file de la page), et Olga lui retire l'accès dès que le premier a abouti.
/// L'écriture de l'acteur, le retrait d'Olga et les créations de Mona se disputent le verrou de
/// SQLite, et l'une d'elles l'attendait plus que le délai d'attente de sa connexion (`500
/// internal`, `database is locked`, vu sur la CI sous Windows sans les boucles de Mona, par
/// moments). Avec la file d'écriture du store, aucune n'échoue : chaque réponse de Tom est `204`
/// (avant le retrait) ou `404` (après), au plus une écriture sans l'accès, aucune création
/// refusée. Avant R494, ce test envoyait vingt-quatre `PUT /mmd` de Tom : vingt-deux y
/// recevraient `429`.
#[tokio::test]
async fn mmd_writes_amid_other_writes_on_a_slow_disk_never_fail() {
    let t = open_app_with(SLOW_DISK).await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Edit)];
    let page = team.page(None, "Récritures", &grants).await;
    let busy = team.page(None, "Agitée", &[members(Level::Edit)]).await;
    witness(&t, page, team.tom.id).await;
    let contents = [
        many_paragraphs("Alpha", PARAGRAPHS),
        many_paragraphs("Bêta", PARAGRAPHS),
    ];

    let requests = (0..mementum_server::MAX_ACCOUNT_WRITES)
        .map(|i| {
            request(
                &team.tom,
                Request::put(format!("/api/v1/pages/{page}/mmd"))
                    .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                Body::from(contents[i % 2].clone()),
            )
        })
        .collect();
    let creations = Loops::start(
        &t,
        &team.mona.browser,
        6,
        StatusCode::CREATED,
        move |_, _| {
            let body = serde_json::json!({ "parentId": busy.to_string(), "title": "Encore" });
            json_request(axum::http::Method::POST, "/api/v1/pages", &body)
        },
    );
    let statuses = burst_then_revoke(&t, &team, page, requests).await;
    let refused = creations.stop().await;
    assert!(refused.is_empty(), "créations refusées : {refused:?}");
    let (with, without) = (witnessed(&t, true).await, witnessed(&t, false).await);
    assert!(
        statuses
            .iter()
            .all(|s| *s == StatusCode::NO_CONTENT || *s == StatusCode::NOT_FOUND)
            && without <= 1,
        "statuts de Tom : {statuses:?} ; écritures au journal avec l'accès : {with}, sans l'accès : \
         {without}"
    );
}

/// Un client qui coupe chaque requête peu après l'avoir envoyée ne relâche pas le verrou avant
/// l'écriture : ce qui suit l'obtention du verrou va au bout dans sa propre tâche, verrou tenu.
/// Sinon, la requête suivante passait aussitôt ses contrôles, et la file se reformait dans la
/// boîte de l'acteur. Les requêtes coupées pendant qu'elles attendent le verrou ne le gardent
/// pas, et n'écrivent rien. Une écriture d'Olga, qui prend le même verrou, sert de borne : quand
/// elle a répondu, tout ce qui l'avait avant elle est écrit.
#[tokio::test]
async fn requests_cut_by_the_client_keep_the_lock_until_written() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Edit)];
    let page = team.page(None, "Coupées", &grants).await;
    witness(&t, page, team.tom.id).await;
    // Une écriture bien plus longue que l'écart entre deux coupures : sans le verrou tenu par
    // la tâche, chaque coupure laisserait la suivante passer ses contrôles pendant l'écriture.
    let contents = [
        many_paragraphs("Alpha", 10 * PARAGRAPHS),
        many_paragraphs("Bêta", 10 * PARAGRAPHS),
    ];

    let mut burst = JoinSet::new();
    for i in 0..BURST {
        let req = request(
            &team.tom,
            Request::put(format!("/api/v1/pages/{page}/mmd"))
                .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
            Body::from(contents[i % 2].clone()),
        );
        let router = t.router.clone();
        // Chacune coupée un peu plus tard que la précédente.
        let cut_after = Duration::from_millis(5 * (i as u64 + 1));
        burst.spawn(async move { tokio::time::timeout(cut_after, router.oneshot(req)).await });
    }
    let deadline = tokio::time::Instant::now() + WAIT;
    while witnessed(&t, true).await == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "aucune écriture n'a abouti"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    team.olga
        .browser
        .delete(&format!(
            "/api/v1/pages/{page}/access/users/{}",
            team.tom.id
        ))
        .await
        .assert_status(StatusCode::OK);
    while burst.join_next().await.is_some() {}
    team.olga
        .put_mmd(&page.to_string(), "Olga\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let (with, without) = (witnessed(&t, true).await, witnessed(&t, false).await);
    assert!(
        without <= 1,
        "écritures de Tom avec l'accès : {with}, sans l'accès : {without}"
    );
}

// Le verrou par page (R200) : `two_pages_never_wait_for_each_other`, dans
// `src/routes/put_mmd_tests.rs` (R535 : une rafale de restaurations d'un même compte y reçoit
// `429` au-delà de deux).

/// Constat I3 de la relecture de la piste S (Ruling R259) : l'écriture complète d'une page est
/// quadratique en nombre de blocs frères (30 000 paragraphes d'un caractère, 90 Ko : 50 s
/// d'acteur en release). Un `PUT /mmd` de plus de `MAX_MMD_BLOCKS` blocs (tous niveaux, lignes de
/// tableau comprises) est refusé avant le verrou et avant que la page soit chargée : `422
/// too_many_blocks`, avec un message qui dit la borne ; rien n'est écrit.
#[tokio::test]
async fn too_many_blocks_are_refused_before_anything_is_written() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Trop", &[user(&team.olga, Level::Full)])
        .await;
    assert_eq!(mementum_server::MAX_MMD_BLOCKS, 12_000);
    let lines: Vec<&str> = vec!["a"; mementum_server::MAX_MMD_BLOCKS + 1];
    let reply = team
        .olga
        .put_mmd(&page.to_string(), lines.join("\n\n") + "\n")
        .await;
    let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "too_many_blocks");
    common::assert_french(&body, &["12 001", "12 000"]);
    let sql = format!("SELECT COUNT(*) FROM doc_updates WHERE node_id = '{page}'");
    assert_eq!(query_i64(&t.db_path(), &sql).await, 0, "rien d'écrit");
    let sql = format!("SELECT COUNT(*) FROM doc_snapshots WHERE node_id = '{page}'");
    assert_eq!(
        query_i64(&t.db_path(), &sql).await,
        0,
        "la page n'est pas chargée"
    );
}

/// Un tableau MMD de `rows` lignes sur `cols` colonnes, sans en-tête.
fn table(rows: usize, cols: usize) -> String {
    let row = format!(
        "|{}
",
        " a |".repeat(cols)
    );
    let mut mmd = row.clone();
    mmd.push_str(&format!(
        "|{}
",
        " --- |".repeat(cols)
    ));
    for _ in 1..rows {
        mmd.push_str(&row);
    }
    mmd
}

/// Un tableau MMD de `rows` lignes sur `cols` colonnes de cellules vides, sans en-tête.
fn empty_table(rows: usize, cols: usize) -> String {
    let row = format!("|{}\n", "  |".repeat(cols));
    let mut mmd = row.clone();
    mmd.push_str(&format!("|{}\n", " --- |".repeat(cols)));
    for _ in 1..rows {
        mmd.push_str(&row);
    }
    mmd
}

/// Constat I3 de la re-relecture de la piste S (Ruling R368) : sous le plafond de blocs, un
/// paragraphe de 20 000 plages en gras (120 Ko), un de 40 000 sauts de ligne (240 Ko) et un
/// tableau de 100 lignes sur 4 000 colonnes (1,6 Mo) occupaient l'acteur 91 s, 112 à 145 s et
/// 20 à 25 s : chaque liste que l'écriture bâtit par insertions successives coûte le carré de sa
/// longueur. Un `PUT /mmd` dont le coût estimé dépasse le budget est refusé avant le verrou et
/// avant que la page soit chargée : `422 too_long_to_write`, avec un message qui dit ce qui est
/// trop long et quoi faire ; rien n'est écrit.
#[tokio::test]
async fn mmd_too_long_to_write_is_refused_before_anything_is_written() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Trop long", &[user(&team.olga, Level::Full)])
        .await;
    let entries = [
        (
            "**a**b".repeat(20_000)
                + "
",
            vec![
                "un même bloc",
                "40 000 morceaux",
                "découpe-le en plusieurs blocs",
            ],
        ),
        (
            "a<br/>".repeat(40_000)
                + "
",
            vec!["un même bloc", "80 000 morceaux", "sauts de ligne"],
        ),
        (
            table(100, 4_000),
            vec![
                "une même ligne de tableau",
                "4 000 cellules",
                "tableaux moins larges",
            ],
        ),
    ];
    for (mmd, words) in entries {
        let reply = team.olga.put_mmd(&page.to_string(), mmd).await;
        let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "too_long_to_write");
        let mut words = words;
        words.extend(["trop long à écrire", "8 s au plus", "rien n'a été écrit"]);
        common::assert_french(&body, &words);
    }
    let sql = format!("SELECT COUNT(*) FROM doc_updates WHERE node_id = '{page}'");
    assert_eq!(query_i64(&t.db_path(), &sql).await, 0, "rien d'écrit");
    let sql = format!("SELECT COUNT(*) FROM doc_snapshots WHERE node_id = '{page}'");
    assert_eq!(
        query_i64(&t.db_path(), &sql).await,
        0,
        "la page n'est pas chargée"
    );
}

/// Une forme de MMD, de taille `n`.
type Shape = dyn Fn(usize) -> String;

/// Le plus grand `n` pour lequel `shape(n)` passe les deux bornes de `PUT /mmd` (blocs et coût
/// estimé), entre 1 et `cap`.
fn largest_accepted(shape: &Shape, cap: usize) -> usize {
    let accepted = |n: usize| {
        let blocks = mementum_md::parse(&shape(n)).unwrap();
        let count = {
            let mut count = 0;
            let mut stack: Vec<&mementum_md::Block> = blocks.iter().collect();
            while let Some(block) = stack.pop() {
                count += 1;
                if let mementum_md::Content::Table(table) = &block.content {
                    count += table.rows.len();
                }
                stack.extend(block.children.iter());
            }
            count
        };
        count <= mementum_server::MAX_MMD_BLOCKS
            && u128::from(mementum_crdt::write_cost(&blocks).nanos)
                <= mementum_server::MMD_WRITE_BUDGET.as_nanos()
    };
    let (mut low, mut high) = (1, cap);
    if accepted(high) {
        return high;
    }
    assert!(accepted(low));
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if accepted(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

/// Mesure, à lancer en release (Ruling R368) : la pire écriture que les bornes de `PUT /mmd`
/// acceptent occupe l'acteur moins de dix secondes. Pour chaque forme, le plus grand MMD
/// accepté, cherché sur l'estimation elle-même, est écrit deux fois de suite sur une page neuve
/// d'un serveur neuf (la seconde récrit la première, et retire autant qu'elle écrit) ; chaque
/// requête est chronométrée, acteur compris. Des poids trop bas laisseraient passer de plus grands MMD, plus
/// lents : la mesure ne peut pas passer à tort.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "mesure, à lancer en --release"]
async fn measure_worst_accepted_writes() {
    // Un `assert!` sur `cfg!` est refusé par clippy (`assertions_on_constants`).
    if cfg!(debug_assertions) {
        panic!("mesure sans valeur en debug : lancer `cargo test --release`");
    }
    let rich = "**du texte** ordinaire et **du texte** ordinaire et **du texte** fin";
    let shapes: Vec<(&str, Box<Shape>)> = vec![
        (
            "plages grasses et maigres d'un paragraphe",
            Box::new(|n| {
                "**a**b".repeat(n)
                    + "
"
            }),
        ),
        (
            "lettres et sauts de ligne d'un paragraphe",
            Box::new(|n| {
                "a<br/>".repeat(n)
                    + "
"
            }),
        ),
        ("cellules d'une ligne", Box::new(|n| table(1, n))),
        ("lignes de 100 cellules", Box::new(|n| table(n, 100))),
        (
            "lignes de 100 cellules vides",
            Box::new(|n| empty_table(n, 100)),
        ),
        (
            "cellules vides d'une ligne",
            Box::new(|n| empty_table(1, n)),
        ),
        (
            "paragraphes de six plages",
            Box::new(move |n| {
                vec![rich; n].join(
                    "

",
                ) + "
"
            }),
        ),
        (
            "un paragraphe de plages et une ligne de cellules",
            Box::new(|n| {
                "**a**b".repeat(n)
                    + "

" + &table(1, 2 * n)
            }),
        ),
    ];
    // `MEMENTUM_I3_SHAPE` : ne mesurer que les formes dont le nom contient ce texte.
    let only = std::env::var("MEMENTUM_I3_SHAPE").unwrap_or_default();
    let mut worst = Duration::ZERO;
    for (what, shape) in shapes.into_iter().filter(|(what, _)| what.contains(&only)) {
        let n = largest_accepted(shape.as_ref(), 200_000);
        let mmd = shape(n);
        // Un serveur neuf par forme : les acteurs des pages d'avant (leur compactage, leur
        // mémoire) ne pèsent pas sur la mesure.
        let t = open_app().await;
        let team = Team::new(&t).await;
        let page = team
            .page(None, what, &[user(&team.olga, Level::Full)])
            .await;
        let mut times = Vec::new();
        for _ in 0..2 {
            let start = std::time::Instant::now();
            let reply = team.olga.put_mmd(&page.to_string(), mmd.clone()).await;
            assert_eq!(reply.status, StatusCode::NO_CONTENT, "{what}, n = {n}");
            times.push(start.elapsed());
        }
        println!(
            "MESURE pire écriture acceptée, {what} (n = {n}, {} octets) : {times:?}",
            mmd.len()
        );
        worst = worst.max(*times.iter().max().unwrap());
    }
    println!("MESURE pire écriture acceptée, toutes formes : {worst:?}");
    assert!(worst < Duration::from_secs(10), "{worst:?}");
}

/// Ruling R383 : la lecture d'un MMD et l'estimation de son écriture se font hors des fils du
/// serveur. Sur un seul fil (le moteur de ce test), un `PUT /mmd` long à lire (1,8 Mo de plages
/// alternées, refusé ensuite comme trop long à écrire) laissait toute autre requête attendre la
/// fin de sa lecture. Les lectures de page faites pendant ce temps répondent maintenant chacune
/// bien avant. Le test ne peut pas passer à tort : une lecture faite sur le fil du serveur
/// arrêterait celle des pages en cours (le seul fil est pris, et la boucle n'a pas d'autre
/// attente que ses requêtes) ; il vérifie que la lecture du MMD a assez duré pour le voir.
///
/// La taille du MMD et la borne ne sont pas libres (Ruling R585). Quand le fil du test partage un
/// cœur avec un fil qui calcule (celui qui lit le MMD, ou un autre test), le système laisse ce
/// dernier calculer par tranches de 120 à 350 ms, et une lecture de page attend la fin de la
/// tranche, plusieurs quand les fils sont nombreux (un fil qui calcule sans rapport avec le
/// serveur fait de même). Lue sur le fil du serveur, la lecture du MMD fait attendre une lecture
/// de page toute sa durée (99,8 à 100 %, sur un, deux ou vingt-quatre cœurs). Lue à part, la pire
/// attente est restée sous 35 % de sa durée avec 1,8 Mo, même à quatre tests sur deux cœurs ; avec
/// 600 Ko et une borne au quart, le test échouait sur un cœur dix-neuf fois sur vingt.
#[tokio::test]
async fn mmd_is_read_off_the_server_threads() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Lue à part", &[user(&team.olga, Level::Full)])
        .await;
    let mmd = "a*b*".repeat(450_000) + "\n";
    let put = request(
        &team.olga,
        Request::put(format!("/api/v1/pages/{page}/mmd"))
            .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
        Body::from(mmd),
    );
    let router = t.router.clone();
    let start = std::time::Instant::now();
    let writing = tokio::spawn(async move { router.oneshot(put).await.unwrap().status() });
    let (mut worst, mut reads) = (Duration::ZERO, 0);
    while !writing.is_finished() {
        let get = request(
            &team.olga,
            Request::get(format!("/api/v1/pages/{page}")),
            Body::empty(),
        );
        let begin = std::time::Instant::now();
        let status = t.router.clone().oneshot(get).await.unwrap().status();
        assert_eq!(status, StatusCode::OK);
        worst = worst.max(begin.elapsed());
        reads += 1;
    }
    let took = start.elapsed();
    assert_eq!(writing.await.unwrap(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        took >= Duration::from_millis(300),
        "la lecture du MMD a pris {took:?} : trop peu pour rien prouver"
    );
    assert!(
        worst < took / 2,
        "une lecture de page a attendu {worst:?} pendant les {took:?} du PUT /mmd ({reads} \
         lectures)"
    );
}

/// Constat C2 de la re-relecture du tour 4 (Ruling R434) : 2 Mio de plages alternées sous deux
/// couleurs permises, refusés comme non canoniques, coûtaient 1,2 Gio à lire, et le message du
/// refus recopiait la ligne attendue : une réponse de 249 Mo pour 2 Mio envoyés. Le refus vient
/// maintenant dès le deuxième texte sous le `<span>`, et son message ne cite qu'un extrait.
#[tokio::test]
async fn a_refused_mmd_answers_in_a_few_hundred_bytes() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let page = team
        .page(None, "Refus bref", &[user(&team.olga, Level::Full)])
        .await;
    let open = format!(
        "<span bg=\"{}\" color=\"{}\">",
        "b".repeat(mementum_md::MAX_COLOR_BYTES),
        "c".repeat(mementum_md::MAX_COLOR_BYTES)
    );
    let n = ((2 << 20) - open.len() - 10) / 4;
    let body = format!("{open}{}</span>\n", "a*b*".repeat(n));
    assert!(body.len() <= 2 << 20);
    let reply = team.olga.put_mmd(&page.to_string(), body).await;
    let refusal = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_mmd");
    let text = refusal.to_string();
    let start: String = text.chars().take(200).collect();
    assert!(
        text.len() < 1 << 10,
        "réponse de {} octets : {start}…",
        text.len()
    );
}

/// Constat I1 de la re-relecture du tour 4 (Ruling R435) : chaque `PUT /mmd` accepté gardait
/// l'arbre lu de son MMD jusqu'à son tour au verrou de la page, sans borne du nombre d'attentes
/// (64 requêtes vers une page : 2,2 Gio). La file d'une page admet `MAX_PAGE_WRITES` requêtes, et
/// `MAX_ACCOUNT_WRITES` d'un même compte (R494) ; une fois son MMD lu, une requête de plus répond
/// `429 page_busy` sur-le-champ, avec `Retry-After`, sans rien écrire, et la page reprend des
/// écritures une fois la file vidée. L'ordre des refus : les droits d'abord (un compte sans accès
/// reçoit `404`, même la file pleine).
#[tokio::test]
async fn a_full_page_queue_refuses_at_once() {
    let t = open_app().await;
    let team = Team::new(&t).await;
    let grants = [user(&team.olga, Level::Full), user(&team.tom, Level::Edit)];
    let page = team.page(None, "File pleine", &grants).await;
    witness(&t, page, team.tom.id).await;
    // Des écritures longues, envoyées d'un coup : la file se remplit, puis déborde.
    let long = many_paragraphs("Long", 4_000);
    let requests: Vec<Request<Body>> = (0..BURST)
        .map(|_| {
            request(
                &team.tom,
                Request::put(format!("/api/v1/pages/{page}/mmd"))
                    .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                Body::from(long.clone()),
            )
        })
        .collect();
    let mut burst = JoinSet::new();
    for req in requests {
        let router = t.router.clone();
        burst.spawn(async move {
            let reply = router.oneshot(req).await.unwrap();
            let retry = reply.headers().get(header::RETRY_AFTER).cloned();
            let status = reply.status();
            let body = axum::body::to_bytes(reply.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, retry, body)
        });
    }
    // Pendant que la file est pleine, Mona, sans accès à la page, reçoit le refus des droits.
    let (mut busy, mut written, mut mona_checked) = (0, 0, false);
    while let Some(reply) = burst.join_next().await {
        let (status, retry, body) = reply.unwrap();
        match status {
            StatusCode::NO_CONTENT => written += 1,
            StatusCode::TOO_MANY_REQUESTS => {
                busy += 1;
                assert_eq!(retry.as_ref().map(|v| v.to_str().unwrap()), Some("1"));
                let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["error"], "page_busy", "{body}");
                common::assert_french(&body, &["réessaie", "rien n'a été écrit"]);
            }
            other => panic!("statut inattendu : {other}"),
        }
        if busy > 0 && !mona_checked {
            mona_checked = true;
            team.mona
                .put_mmd(&page.to_string(), "Mona\n")
                .await
                .assert_error(StatusCode::NOT_FOUND, "not_found");
        }
    }
    assert!(
        (1..=mementum_server::MAX_PAGE_WRITES).contains(&written),
        "{written} écritures"
    );
    assert_eq!(written + busy, BURST);
    assert!(
        busy >= BURST - mementum_server::MAX_PAGE_WRITES,
        "{busy} refus"
    );
    assert_eq!(
        witnessed(&t, true).await,
        written as i64,
        "écritures au journal"
    );
    // La file vidée, la page reprend des écritures.
    team.tom
        .put_mmd(&page.to_string(), "Après\n")
        .await
        .assert_status(StatusCode::NO_CONTENT);
}
