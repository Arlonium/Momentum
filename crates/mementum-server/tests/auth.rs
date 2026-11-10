//! Authentification : comptes locaux, session par cookie, inscription ouverte
//! ou sur invitation. Les aides (application en mémoire, navigateur de test,
//! journaux) sont dans `common/mod.rs`.
//!
//! Les valeurs du contrat (nom du cookie, durées, seuils) sont écrites ici en
//! clair, pas reprises des constantes du serveur : un changement de constante
//! doit faire échouer ces tests.

mod common;

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{
    Logs, PASSWORD, Reply, SESSION_COOKIE, TestApp, all_logs, assert_french, exec_sql,
    json_request, overtaken, query_i64, tree_nodes,
};
use mementum_acl::{Grant, Level, Role, Subject};
use mementum_server::{Me, PageNode, SignupMode};
use mementum_store::{InvitationGrant, NewInvitation, NewNode, NodeKind, Store};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use uuid::Uuid;

/// Un autre mot de passe de test, tout aussi faux.
const NEW_PASSWORD: &str = "nouveau-mot-de-passe-de-test";

/// Sept jours, en millisecondes : la durée d'une invitation.
const WEEK_MS: i64 = 7 * 24 * 3600 * 1000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Une application où tout le monde peut s'inscrire.
async fn open_app() -> TestApp {
    TestApp::with_config(|cfg| cfg.signup = SignupMode::Open).await
}

/// Un jeton au format du serveur (32 octets écrits en base64url, sans
/// remplissage) et ce que la base en garde : son SHA-256.
fn token(seed: u8) -> (String, [u8; 32]) {
    let bytes = [seed; 32];
    (URL_SAFE_NO_PAD.encode(bytes), Sha256::digest(bytes).into())
}

/// Le SHA-256 que la base doit garder pour ce jeton de cookie.
fn stored_hash(token: &str) -> String {
    let bytes = URL_SAFE_NO_PAD.decode(token).expect("jeton en base64url");
    assert_eq!(bytes.len(), 32, "un jeton fait 32 octets");
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

fn user_id(me: &Value) -> Uuid {
    Uuid::parse_str(me["user"]["id"].as_str().expect("user.id")).expect("user.id en UUID")
}

fn space_id(me: &Value, index: usize) -> Uuid {
    Uuid::parse_str(me["spaces"][index]["id"].as_str().expect("spaces[].id"))
        .expect("spaces[].id en UUID")
}

/// Une invitation d'espace écrite par le store, comme le feront les routes
/// d'invitation.
async fn invite(
    store: &Store,
    space: Uuid,
    by: Uuid,
    role: Role,
    seed: u8,
    expires_at: i64,
) -> (String, Uuid) {
    let (token, hash) = token(seed);
    let invitation = store
        .create_invitation(NewInvitation {
            space_id: space,
            grant: InvitationGrant::Space(role),
            // Une autre adresse que celle du compte qui s'inscrira : c'est le
            // jeton qui fait foi (Ruling R47).
            email: "invitee@exemple.fr".to_string(),
            token_hash: hash,
            created_by: by,
            expires_at,
        })
        .await
        .unwrap();
    (token, invitation.id)
}

/// La valeur et les attributs d'une en-tête `Set-Cookie` de session.
fn cookie_parts(set_cookie: &str) -> (String, BTreeSet<String>) {
    let mut parts = set_cookie.split(';').map(str::trim);
    let value = parts
        .next()
        .and_then(|pair| pair.strip_prefix(&format!("{SESSION_COOKIE}=")))
        .unwrap_or_else(|| panic!("cookie {SESSION_COOKIE} attendu : {set_cookie}"))
        .to_string();
    (value, parts.map(str::to_string).collect())
}

fn attributes(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[tokio::test]
async fn first_signup_is_instance_admin_and_gets_a_space() {
    // Inscriptions sur invitation (le défaut) : le premier compte passe.
    let t = TestApp::new().await;
    let info = t.get("/api/v1/info").await.json();
    assert_eq!(info["signup"], "invite");
    assert_eq!(info["setup"], true);
    // Rien n'est créé au démarrage : aucun espace sans membre n'attend le
    // premier compte (Ruling R55).
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        0
    );
    t.browser()
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");

    let alice = t.browser();
    let reply = alice
        .sign_up("alice@exemple.fr", "Alice", PASSWORD, None)
        .await;
    reply.assert_status(StatusCode::CREATED);
    let me = reply.json();
    let id = user_id(&me);
    let space = space_id(&me, 0);
    assert_eq!(
        me,
        json!({
            "user": {
                "id": id.to_string(),
                "email": "alice@exemple.fr",
                "name": "Alice",
                "instanceAdmin": true,
            },
            "spaces": [{ "id": space.to_string(), "name": "Espace de Alice", "role": "owner" }],
        })
    );
    let typed: Me = serde_json::from_slice(&reply.body).unwrap();
    assert!(typed.user.instance_admin);

    // L'inscription ouvre la session.
    let again = alice.me().await;
    again.assert_status(StatusCode::OK);
    assert_eq!(again.json(), me);
    let info = t.get("/api/v1/info").await.json();
    assert_eq!(info["setup"], false);

    // Un seul espace, dont le compte est propriétaire, et dont il lit
    // l'arbre, encore vide.
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        1
    );
    assert_eq!(tree_nodes(&alice, space).await, [] as [Value; 0]);
    let store = Store::open(&t.db_path()).await.unwrap();
    assert_eq!(store.role_in(space, id).await.unwrap(), Some(Role::Owner));
    let user = store.get_user(id).await.unwrap().expect("le compte");
    assert!(user.instance_admin);
    store.close().await;
}

/// Review Focus 4 : les pages d'une base du lot 1 reviennent au premier
/// compte créé. La base est une vraie base du lot 1, migrée au démarrage :
/// son coffre est devenu un espace sans membre, avec ses deux pages.
#[tokio::test]
async fn first_account_adopts_lot1_space() {
    let (t, lot1) = TestApp::on_lot1_database().await;
    // Avant tout compte, personne ne lit ces pages par l'API : elles sont
    // relues en base.
    let store = Store::open(&t.db_path()).await.unwrap();
    let before = store.list_nodes(lot1.vault).await.unwrap();
    store.close().await;
    let mut titles: Vec<&str> = before.iter().map(|n| n.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Racine", "Sous-page"]);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM members").await,
        0
    );
    assert_eq!(t.get("/api/v1/info").await.json()["setup"], true);

    let (alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    assert_eq!(me["user"]["instanceAdmin"], true);
    // Lui seul : pas d'« Espace de Alice » en plus.
    assert_eq!(
        me["spaces"],
        json!([{ "id": lot1.vault.to_string(), "name": "Mon espace", "role": "owner" }])
    );
    assert_eq!(alice.me().await.json(), me);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        1
    );

    // Les pages sont toujours là, telles quelles, et celle de la racine va
    // aux membres : le compte les voit dans la section « Espace », en accès
    // complet.
    let store = Store::open(&t.db_path()).await.unwrap();
    assert_eq!(store.list_nodes(lot1.vault).await.unwrap(), before);
    let seen = tree_nodes(&alice, lot1.vault).await;
    assert_eq!(seen.len(), 2);
    for node in &seen {
        assert_eq!(node["level"], "full", "{node}");
        assert_eq!(node["section"], "space", "{node}");
    }
    let child = seen
        .iter()
        .find(|n| n["id"] == lot1.child.to_string())
        .expect("la sous-page");
    assert_eq!(child["parentId"], lot1.root.to_string());
    assert_eq!(
        store.role_in(lot1.vault, user_id(&me)).await.unwrap(),
        Some(Role::Owner)
    );
    assert_eq!(
        store.node_grants(lot1.root).await.unwrap(),
        [Grant {
            subject: Subject::Members,
            level: Level::Full,
        }]
    );
    assert_eq!(store.node_grants(lot1.child).await.unwrap(), []);
    store.close().await;
}

/// Une erreur de la base au milieu de l'inscription du premier compte (ici, à
/// la reprise de l'espace du lot 1) ne laisse rien derrière elle : ni compte,
/// ni adresse prise. L'inscription relancée est encore celle du premier
/// compte, et les pages du lot 1 lui reviennent.
#[tokio::test]
async fn failed_signup_leaves_no_account_behind() {
    let (t, lot1) = TestApp::on_lot1_database().await;
    exec_sql(
        &t.db_path(),
        "CREATE TRIGGER panne BEFORE INSERT ON members \
         BEGIN SELECT RAISE(ABORT, 'panne simulée'); END",
    )
    .await;
    let alice = t.browser();
    let reply = alice
        .sign_up("alice@exemple.fr", "Alice", PASSWORD, None)
        .await;
    reply.assert_error(StatusCode::INTERNAL_SERVER_ERROR, "internal");
    assert!(alice.session().is_none());
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM users").await,
        0
    );
    assert_eq!(t.get("/api/v1/info").await.json()["setup"], true);

    // La panne passée, la même adresse s'inscrit, et tout lui revient.
    exec_sql(&t.db_path(), "DROP TRIGGER panne").await;
    let reply = alice
        .sign_up("alice@exemple.fr", "Alice", PASSWORD, None)
        .await;
    reply.assert_status(StatusCode::CREATED);
    let me = reply.json();
    assert_eq!(me["user"]["instanceAdmin"], true);
    assert_eq!(
        me["spaces"],
        json!([{ "id": lot1.vault.to_string(), "name": "Mon espace", "role": "owner" }])
    );
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM users").await,
        1
    );
}

/// Le même sort pour un espace sans membre qui ne vient pas d'une base du
/// lot 1 : il revient au premier compte, qui n'en reçoit pas un second. (Les
/// routes de pages en créaient un quand on s'en servait avant tout compte,
/// Ruling R55 ; elles demandent maintenant une session, et l'espace est ici
/// posé par le store.)
#[tokio::test]
async fn first_account_adopts_the_space_made_before_it() {
    let t = TestApp::new().await;
    let store = Store::open(&t.db_path()).await.unwrap();
    let orphan = store.create_space("Mon espace", None).await.unwrap();
    let page = |parent, title: &str, grants| NewNode {
        space_id: orphan.id,
        parent_id: parent,
        after: None,
        kind: NodeKind::Page,
        title: title.to_string(),
        created_by: None,
        grants,
        doc: None,
    };
    let to_members = Grant {
        subject: Subject::Members,
        level: Level::Full,
    };
    let root = store
        .create_node(page(None, "Racine", vec![to_members]))
        .await
        .unwrap();
    store
        .create_node(page(Some(root.id), "Sous-page", vec![]))
        .await
        .unwrap();
    let before = store.list_nodes(orphan.id).await.unwrap();
    assert_eq!(before.len(), 2);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM members").await,
        0
    );

    let (alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    assert_eq!(
        me["spaces"],
        json!([{ "id": orphan.id.to_string(), "name": "Mon espace", "role": "owner" }])
    );
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        1
    );
    assert_eq!(store.list_nodes(orphan.id).await.unwrap(), before);
    store.close().await;
    let mut titles: Vec<String> = tree_nodes(&alice, orphan.id)
        .await
        .iter()
        .map(|n| n["title"].as_str().unwrap().to_string())
        .collect();
    titles.sort();
    assert_eq!(titles, ["Racine", "Sous-page"]);
}

/// Ruling R69 : une invitation consultée avec succès peut ne plus valoir un
/// instant plus tard, à l'acceptation (révoquée, ou son auteur a perdu ses
/// droits). L'inscription se termine alors proprement : le compte existe,
/// sans espace ni droit, et sa session est ouverte.
#[tokio::test]
async fn invitation_withdrawn_during_signup_leaves_an_account_without_space() {
    let t = TestApp::new().await;
    let (alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let store = Store::open(&t.db_path()).await.unwrap();
    let (token, _) = invite(
        &store,
        space_id(&me, 0),
        user_id(&me),
        Role::Member,
        5,
        now_ms() + WEEK_MS,
    )
    .await;
    store.close().await;
    // L'invitation disparaît juste après la création du compte, donc entre
    // sa vérification et son acceptation.
    exec_sql(
        &t.db_path(),
        "CREATE TRIGGER retrait AFTER INSERT ON users BEGIN DELETE FROM invitations; END",
    )
    .await;

    let logs = Logs::capture();
    let bob = t.browser();
    let reply = bob
        .sign_up("bob@exemple.fr", "Bob", PASSWORD, Some(&token))
        .await;
    reply.assert_status(StatusCode::CREATED);
    let bob_me = reply.json();
    assert_eq!(bob_me["user"]["email"], "bob@exemple.fr");
    assert_eq!(bob_me["user"]["instanceAdmin"], false);
    assert_eq!(bob_me["spaces"], json!([]));
    let text = logs.text();
    assert!(
        text.contains("WARN") && text.contains("invitation"),
        "avertissement attendu : {text}"
    );
    assert!(!text.contains(&token), "le jeton est dans les journaux");
    drop(logs);

    // Sa session vaut, mais elle ne lui donne rien : ni l'espace d'Alice, ni
    // un espace à lui (il venait pour une invitation).
    assert_eq!(bob.me().await.json(), bob_me);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        1
    );
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM members").await,
        1
    );
    assert_eq!(alice.me().await.json(), me);
}

#[tokio::test]
async fn signup_is_closed_without_invitation() {
    let t = TestApp::new().await;
    let (alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let space = space_id(&me, 0);
    let users = "SELECT COUNT(*) FROM users";

    // Sans invitation : fermé, et rien n'est créé.
    let bob = t.browser();
    let body = bob
        .sign_up("bob@exemple.fr", "Bob", PASSWORD, None)
        .await
        .assert_error(StatusCode::FORBIDDEN, "signup_closed");
    assert_french(&body, &["invitation"]);
    assert!(bob.session().is_none());
    assert_eq!(query_i64(&t.db_path(), users).await, 1);
    // Fermé avant tout le reste : l'adresse d'un compte existant ne se
    // devine pas par là.
    bob.sign_up("alice@exemple.fr", "Bob", PASSWORD, None)
        .await
        .assert_error(StatusCode::FORBIDDEN, "signup_closed");

    // Avec le jeton d'une invitation créée par le store.
    let store = Store::open(&t.db_path()).await.unwrap();
    let (token, _) = invite(
        &store,
        space,
        user_id(&me),
        Role::Member,
        7,
        now_ms() + WEEK_MS,
    )
    .await;
    let reply = bob
        .sign_up("bob@exemple.fr", "Bob", PASSWORD, Some(&token))
        .await;
    reply.assert_status(StatusCode::CREATED);
    let bob_me = reply.json();
    assert_eq!(bob_me["user"]["instanceAdmin"], false);
    // Membre de l'espace où il est invité, sans espace à lui.
    assert_eq!(
        bob_me["spaces"],
        json!([{ "id": space.to_string(), "name": "Espace de Alice", "role": "member" }])
    );
    assert_eq!(bob.me().await.json(), bob_me);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        1
    );
    assert_eq!(
        store.role_in(space, user_id(&bob_me)).await.unwrap(),
        Some(Role::Member)
    );

    // Le jeton ne sert qu'une fois.
    let carol = t.browser();
    carol
        .sign_up("carol@exemple.fr", "Carol", PASSWORD, Some(&token))
        .await
        .assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
    assert!(carol.session().is_none());
    assert_eq!(query_i64(&t.db_path(), users).await, 2);
    assert_eq!(alice.me().await.json(), me, "rien n'a changé pour Alice");
    store.close().await;
}

/// Un jeton inconnu, mal formé, échu ou révoqué : la même réponse, et aucun
/// compte créé, que les inscriptions soient ouvertes ou non.
#[tokio::test]
async fn invalid_invitation_is_404_and_creates_nothing() {
    for mode in [SignupMode::Invite, SignupMode::Open] {
        let t = TestApp::with_config(|cfg| cfg.signup = mode).await;
        let (_alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
        let store = Store::open(&t.db_path()).await.unwrap();
        let (space, by) = (space_id(&me, 0), user_id(&me));
        let (expired, _) = invite(&store, space, by, Role::Member, 1, now_ms() - 1).await;
        let (revoked, id) = invite(&store, space, by, Role::Member, 2, now_ms() + WEEK_MS).await;
        store.revoke_invitation(id).await.unwrap();
        let (unknown, _) = token(3);

        let mut bodies = Vec::new();
        for bad in [
            expired.as_str(),
            revoked.as_str(),
            unknown.as_str(),
            "",
            "pas-un-jeton",
            // Le bon alphabet, mais pas 32 octets.
            "AAAA",
            &"A".repeat(4096),
        ] {
            let bob = t.browser();
            let reply = bob
                .sign_up("bob@exemple.fr", "Bob", PASSWORD, Some(bad))
                .await;
            reply.assert_error(StatusCode::NOT_FOUND, "invalid_invitation");
            assert!(bob.session().is_none());
            bodies.push(reply.body.clone());
        }
        bodies.dedup();
        assert_eq!(bodies.len(), 1, "une seule réponse pour tous les cas");
        assert_eq!(
            query_i64(&t.db_path(), "SELECT COUNT(*) FROM users").await,
            1,
            "{mode:?}"
        );
        store.close().await;
    }
}

#[tokio::test]
async fn signup_open_allows_anyone() {
    let t = open_app().await;
    assert_eq!(t.get("/api/v1/info").await.json()["signup"], "open");
    let (alice, alice_me) = t.signed_up("alice@exemple.fr", "Alice").await;
    assert_eq!(alice_me["user"]["instanceAdmin"], true);

    // Un espace sans membre apparu ensuite : seul le premier compte adopte.
    let store = Store::open(&t.db_path()).await.unwrap();
    let orphan = store.create_space("Orphelin", None).await.unwrap();

    let (bob, bob_me) = t.signed_up("bob@exemple.fr", "Bob").await;
    assert_eq!(bob_me["user"]["instanceAdmin"], false);
    assert_eq!(bob_me["user"]["email"], "bob@exemple.fr");
    let bob_space = space_id(&bob_me, 0);
    assert_eq!(
        bob_me["spaces"],
        json!([{ "id": bob_space.to_string(), "name": "Espace de Bob", "role": "owner" }])
    );
    assert_ne!(bob_space, space_id(&alice_me, 0));
    assert_eq!(bob.me().await.json(), bob_me);
    // Chacun chez soi.
    assert_eq!(alice.me().await.json(), alice_me);
    assert!(store.members(orphan.id).await.unwrap().is_empty());
    store.close().await;
}

#[tokio::test]
async fn login_sets_http_only_lax_cookie() {
    let expected = attributes(&["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=2592000"]);
    let mut secure = expected.clone();
    secure.insert("Secure".to_string());

    for (public_url, attrs) in [
        (None, &expected),
        (Some("http://192.168.1.10:8080"), &expected),
        (Some("https://notes.exemple.fr"), &secure),
    ] {
        let t = TestApp::with_config(|cfg| cfg.public_url = public_url.map(str::to_string)).await;
        // L'inscription ouvre une session par le même cookie.
        let reply = t
            .browser()
            .sign_up("alice@exemple.fr", "Alice", PASSWORD, None)
            .await;
        reply.assert_status(StatusCode::CREATED);
        let (signup_token, signup_attrs) = cookie_parts(&reply.session_set_cookie());
        assert_eq!(&signup_attrs, attrs, "{public_url:?}");

        let alice = t.browser();
        let reply = alice.log_in("alice@exemple.fr", PASSWORD).await;
        reply.assert_status(StatusCode::OK);
        let me: Me = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(me.user.email, "alice@exemple.fr");
        assert_eq!(me.spaces.len(), 1);
        let (value, login_attrs) = cookie_parts(&reply.session_set_cookie());
        assert_eq!(&login_attrs, attrs, "{public_url:?}");

        // Un jeton de 32 octets en base64url, neuf à chaque session.
        assert_eq!(value.len(), 43, "{value}");
        assert!(
            value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{value}"
        );
        assert_ne!(value, signup_token);
        assert_eq!(alice.session().as_deref(), Some(value.as_str()));
        assert_eq!(alice.me().await.status, StatusCode::OK);
    }
}

#[tokio::test]
async fn wrong_password_and_unknown_email_are_indistinguishable() {
    let t = open_app().await;
    t.signed_up("alice@exemple.fr", "Alice").await;
    // Un compte sans mot de passe (Ruling R60 : ceux qui entreront par OIDC).
    let (_, bob) = t.signed_up("bob@exemple.fr", "Bob").await;
    exec_sql(
        &t.db_path(),
        &format!(
            "UPDATE users SET password_hash = NULL WHERE id = '{}'",
            user_id(&bob)
        ),
    )
    .await;

    let wrong = t
        .browser()
        .log_in("alice@exemple.fr", "pas-le-bon-mot-de-passe")
        .await;
    let body = wrong.assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_french(&body, &["mot de passe"]);
    let same = |other: &Reply, case: &str| {
        assert_eq!(other.status, wrong.status, "{case}");
        assert_eq!(other.body, wrong.body, "{case}");
        assert_eq!(other.headers, wrong.headers, "{case}");
        assert!(other.set_cookies().is_empty(), "{case}");
    };
    same(
        &t.browser().log_in("personne@exemple.fr", PASSWORD).await,
        "adresse inconnue",
    );
    same(
        &t.browser().log_in("bob@exemple.fr", PASSWORD).await,
        "compte sans mot de passe",
    );
    same(
        &t.browser().log_in("bob@exemple.fr", "").await,
        "compte sans mot de passe, mot de passe vide",
    );
    same(
        &t.browser().log_in("pas une adresse", "").await,
        "ni adresse ni mot de passe",
    );
    assert!(wrong.set_cookies().is_empty());
}

/// Ruling R60 : un compte sans mot de passe ne s'en donne pas un par la route
/// de changement, qui exige l'actuel.
#[tokio::test]
async fn account_without_password_cannot_set_one() {
    let t = TestApp::new().await;
    let (alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    exec_sql(
        &t.db_path(),
        &format!(
            "UPDATE users SET password_hash = NULL WHERE id = '{}'",
            user_id(&me)
        ),
    )
    .await;
    for current in [PASSWORD, ""] {
        alice
            .change_password(current, NEW_PASSWORD)
            .await
            .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    assert_eq!(
        query_i64(
            &t.db_path(),
            "SELECT COUNT(*) FROM users WHERE password_hash IS NULL"
        )
        .await,
        1
    );
    // La session, elle, vaut toujours.
    assert_eq!(alice.me().await.status, StatusCode::OK);
}

#[tokio::test]
async fn login_is_throttled_after_10_failures() {
    let t = open_app().await;
    let (alice, _) = t.signed_up("alice@exemple.fr", "Alice").await;
    t.signed_up("bob@exemple.fr", "Bob").await;

    for attempt in 1..=10 {
        let reply = t.browser().log_in("alice@exemple.fr", "pas-le-bon").await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
        assert!(reply.headers.get(header::RETRY_AFTER).is_none());
    }
    // La 11ᵉ : refusée sans même regarder le mot de passe.
    let retry_after = |reply: &Reply| -> u64 {
        let body = reply.assert_error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
        assert_french(&body, &["tentatives"]);
        assert!(reply.set_cookies().is_empty());
        reply
            .headers
            .get(header::RETRY_AFTER)
            .expect("Retry-After")
            .to_str()
            .unwrap()
            .parse()
            .expect("Retry-After en secondes")
    };
    let seconds = retry_after(&t.browser().log_in("alice@exemple.fr", "pas-le-bon").await);
    assert!((1..=900).contains(&seconds), "Retry-After: {seconds}");
    // Le bon mot de passe aussi, tant que dure la fenêtre ; et la même
    // adresse écrite autrement.
    let browser = t.browser();
    retry_after(&browser.log_in("alice@exemple.fr", PASSWORD).await);
    assert!(browser.session().is_none());
    retry_after(&t.browser().log_in("  ALICE@Exemple.FR ", PASSWORD).await);

    // Le compteur est par adresse : Bob se connecte, et la session déjà
    // ouverte d'Alice vaut toujours.
    let reply = t.browser().log_in("bob@exemple.fr", PASSWORD).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(alice.me().await.status, StatusCode::OK);

    // Une adresse sans compte est freinée de même : le 429 ne dit pas si
    // l'adresse existe. Même corps et mêmes en-têtes que pour un compte, au
    // délai près (chaque adresse a sa fenêtre).
    for attempt in 1..=10 {
        let reply = t.browser().log_in("personne@exemple.fr", PASSWORD).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
    }
    let unknown = t.browser().log_in("personne@exemple.fr", PASSWORD).await;
    retry_after(&unknown);
    let known = t.browser().log_in("alice@exemple.fr", PASSWORD).await;
    retry_after(&known);
    // Le délai, en secondes dans l'en-tête et en minutes dans le message,
    // est le seul écart permis.
    let without_delay = |reply: &Reply| {
        let mut headers = reply.headers.clone();
        headers.remove(header::RETRY_AFTER);
        headers.remove(header::CONTENT_LENGTH);
        let body: String = reply
            .text()
            .chars()
            .filter(|c| !c.is_ascii_digit())
            .collect();
        (headers, body)
    };
    assert_eq!(without_delay(&unknown), without_delay(&known));
}

#[tokio::test]
async fn successful_login_resets_the_failure_count() {
    let t = TestApp::new().await;
    t.signed_up("alice@exemple.fr", "Alice").await;
    let fail = async |count: usize| {
        for attempt in 1..=count {
            let reply = t.browser().log_in("alice@exemple.fr", "pas-le-bon").await;
            assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
        }
    };
    fail(9).await;
    t.browser()
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    // Le compteur repart de zéro : dix nouveaux échecs avant le refus.
    fail(10).await;
    t.browser()
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
}

#[tokio::test]
async fn logout_kills_the_session() {
    let t = TestApp::new().await;
    let (alice, _) = t.signed_up("alice@exemple.fr", "Alice").await;
    let token = alice.session().expect("session ouverte");
    assert_eq!(alice.me().await.status, StatusCode::OK);

    let reply = alice.log_out().await;
    reply.assert_status(StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty());
    // Le cookie est effacé, avec les attributs sous lesquels il a été posé.
    let (value, attrs) = cookie_parts(&reply.session_set_cookie());
    assert_eq!(value, "");
    for attribute in ["Max-Age=0", "Path=/", "HttpOnly", "SameSite=Lax"] {
        assert!(attrs.contains(attribute), "{attribute} absent de {attrs:?}");
    }
    assert!(alice.session().is_none());
    alice
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");

    // Le cookie d'avant, rejoué, ne vaut plus rien : la session est supprimée.
    let replay = t.browser();
    replay.set_session(Some(&token));
    replay
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM sessions").await,
        0
    );

    // Se déconnecter sans session : pareil.
    for browser in [replay, t.browser()] {
        let reply = browser.log_out().await;
        reply.assert_status(StatusCode::NO_CONTENT);
        assert_eq!(cookie_parts(&reply.session_set_cookie()).0, "");
    }
}

#[tokio::test]
async fn expired_session_is_401() {
    let t = TestApp::new().await;
    let (alice, _) = t.signed_up("alice@exemple.fr", "Alice").await;
    assert_eq!(alice.me().await.status, StatusCode::OK);

    // Échue depuis une seconde.
    exec_sql(
        &t.db_path(),
        "UPDATE sessions SET expires_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000 - 1000",
    )
    .await;
    let body = alice
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_french(&body, &["session"]);
    alice
        .change_password(PASSWORD, NEW_PASSWORD)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");

    // Un cookie inconnu, mal formé ou vide : pas de session non plus.
    let (unknown, _) = token(9);
    for forged in [unknown.as_str(), "pas-un-jeton", "", &"A".repeat(4096)] {
        let browser = t.browser();
        browser.set_session(Some(forged));
        browser
            .me()
            .await
            .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
}

#[tokio::test]
async fn change_password_kills_other_sessions() {
    let t = TestApp::new().await;
    let (laptop, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let phone = t.browser();
    phone
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let before = laptop.session().unwrap();

    // Refus : rien ne change. Le message parle du mot de passe actuel, pas
    // d'une adresse que la personne n'a pas tapée (celui de la connexion).
    let body = laptop
        .change_password("pas-le-bon-mot-de-passe", NEW_PASSWORD)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_french(&body, &["mot de passe actuel"]);
    assert!(
        !body["message"].as_str().unwrap().contains("adresse"),
        "{body}"
    );
    laptop
        .change_password(PASSWORD, "neuf-car.")
        .await
        .assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_password");
    t.browser()
        .change_password(PASSWORD, NEW_PASSWORD)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(phone.me().await.status, StatusCode::OK);
    assert_eq!(laptop.session().as_deref(), Some(before.as_str()));

    let reply = laptop.change_password(PASSWORD, NEW_PASSWORD).await;
    reply.assert_status(StatusCode::NO_CONTENT);
    assert!(reply.body.is_empty());

    // La session qui a changé le mot de passe continue ; les autres sont
    // supprimées, comme le cookie d'avant le changement.
    let after = laptop.me().await;
    after.assert_status(StatusCode::OK);
    assert_eq!(after.json(), me);
    phone
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    let replay = t.browser();
    replay.set_session(Some(&before));
    replay
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM sessions").await,
        1
    );

    // L'ancien mot de passe ne passe plus, le nouveau si.
    t.browser()
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    t.browser()
        .log_in("alice@exemple.fr", NEW_PASSWORD)
        .await
        .assert_status(StatusCode::OK);
}

#[tokio::test]
async fn password_rules() {
    let t = open_app().await;
    let users = "SELECT COUNT(*) FROM users";
    let sign_up = async |email: &str, password: &str| {
        let browser = t.browser();
        let reply = browser.sign_up(email, "Quelqu'un", password, None).await;
        (reply, browser.session())
    };

    // 9 caractères : refusé, rien n'est créé.
    let (reply, session) = sign_up("a@exemple.fr", "123456789").await;
    let body = reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_password");
    assert_french(&body, &["10", "200"]);
    assert!(session.is_none());
    assert_eq!(query_i64(&t.db_path(), users).await, 0);
    assert_eq!(t.get("/api/v1/info").await.json()["setup"], true);

    // 10 : accepté.
    let (reply, session) = sign_up("a@exemple.fr", "1234567890").await;
    reply.assert_status(StatusCode::CREATED);
    assert!(session.is_some());

    // 201 : refusé ; 200 : accepté.
    let (reply, _) = sign_up("b@exemple.fr", &"a".repeat(201)).await;
    reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_password");
    let (reply, _) = sign_up("b@exemple.fr", &"a".repeat(200)).await;
    reply.assert_status(StatusCode::CREATED);

    // Des caractères, pas des octets : 10 « é » passent (20 octets), 200
    // aussi (400 octets), 9 non.
    let (reply, _) = sign_up("c@exemple.fr", &"é".repeat(9)).await;
    reply.assert_error(StatusCode::UNPROCESSABLE_ENTITY, "invalid_password");
    let (reply, _) = sign_up("c@exemple.fr", &"é".repeat(10)).await;
    reply.assert_status(StatusCode::CREATED);
    let (reply, _) = sign_up("d@exemple.fr", &"é".repeat(200)).await;
    reply.assert_status(StatusCode::CREATED);
    // Les espaces comptent : un mot de passe n'est pas rogné.
    let (reply, _) = sign_up("e@exemple.fr", "  123456  ").await;
    reply.assert_status(StatusCode::CREATED);
    assert_eq!(query_i64(&t.db_path(), users).await, 5);
    t.browser()
        .log_in("e@exemple.fr", "123456")
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    t.browser()
        .log_in("e@exemple.fr", "  123456  ")
        .await
        .assert_status(StatusCode::OK);

    // Le mot de passe se retrouve tel quel à la connexion.
    t.browser()
        .log_in("c@exemple.fr", &"é".repeat(10))
        .await
        .assert_status(StatusCode::OK);
    t.browser()
        .log_in("c@exemple.fr", &"e".repeat(10))
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
}

#[tokio::test]
async fn email_is_normalized() {
    let t = open_app().await;
    let reply = t
        .browser()
        .sign_up(" A@B.fr ", "Alice", PASSWORD, None)
        .await;
    reply.assert_status(StatusCode::CREATED);
    assert_eq!(reply.json()["user"]["email"], "a@b.fr");

    let other = t.browser();
    let body = other
        .sign_up("a@b.fr", "Une autre", PASSWORD, None)
        .await
        .assert_error(StatusCode::CONFLICT, "email_taken");
    assert_french(&body, &["adresse"]);
    assert!(other.session().is_none());
    other
        .sign_up("A@B.FR", "Une autre", PASSWORD, None)
        .await
        .assert_error(StatusCode::CONFLICT, "email_taken");
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM users").await,
        1
    );

    // À la connexion aussi.
    let reply = t.browser().log_in("  A@b.FR", PASSWORD).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.json()["user"]["email"], "a@b.fr");
}

#[tokio::test]
async fn signup_validates_email_and_name() {
    let t = open_app().await;
    let refused = async |body: Value, status: StatusCode, code: &str| -> Value {
        let browser = t.browser();
        let reply = browser.post("/api/v1/auth/signup", body).await;
        let body = reply.assert_error(status, code);
        assert!(browser.session().is_none());
        body
    };
    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;

    for email in [
        "",
        "   ",
        "alice",
        "@exemple.fr",
        "alice@",
        "alice@@exemple.fr",
        "alice@exemple.fr@exemple.fr",
        "alice smith@exemple.fr",
        "alice@exem\tple.fr",
        &format!("{}@exemple.fr", "a".repeat(250)),
    ] {
        let body = refused(
            json!({ "email": email, "name": "Alice", "password": PASSWORD }),
            unprocessable,
            "invalid_email",
        )
        .await;
        assert_french(&body, &["adresse"]);
    }
    for name in ["", "   ", &"é".repeat(101)] {
        let body = refused(
            json!({ "email": "alice@exemple.fr", "name": name, "password": PASSWORD }),
            unprocessable,
            "invalid_name",
        )
        .await;
        assert_french(&body, &["nom", "100"]);
    }
    // Corps incomplet ou inattendu : l'erreur JSON habituelle.
    let body = refused(
        json!({ "email": "alice@exemple.fr", "name": "Alice" }),
        unprocessable,
        "invalid_json",
    )
    .await;
    assert_french(&body, &["champ manquant", "password"]);
    let body = refused(
        json!({ "email": "alice@exemple.fr", "name": "Alice", "password": PASSWORD, "admin": true }),
        unprocessable,
        "invalid_json",
    )
    .await;
    assert_french(&body, &["champ inconnu", "admin"]);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM users").await,
        0
    );

    // Le nom est rogné ; 100 caractères passent.
    let name = "é".repeat(100);
    let reply = t
        .browser()
        .sign_up("alice@exemple.fr", &format!("  {name} "), PASSWORD, None)
        .await;
    reply.assert_status(StatusCode::CREATED);
    assert_eq!(reply.json()["user"]["name"], name);
    assert_eq!(
        reply.json()["spaces"][0]["name"],
        format!("Espace de {name}")
    );
}

/// Ni les réponses ni les journaux ne portent un mot de passe, un hachage ou
/// un jeton de session ; la base ne garde que des hachages.
#[tokio::test]
async fn secrets_never_leave() {
    // Avant toute requête : les journaux de tous les fils, au niveau TRACE.
    let logs = all_logs();
    let t = TestApp::new().await;
    let mut replies = Vec::new();
    let mut tokens = Vec::new();

    // Inscription.
    let laptop = t.browser();
    let reply = laptop
        .sign_up("alice@exemple.fr", "Alice", PASSWORD, None)
        .await;
    reply.assert_status(StatusCode::CREATED);
    let id = user_id(&reply.json());
    tokens.push(laptop.session().unwrap());
    replies.push(reply);
    // Connexion ratée, puis réussie.
    let phone = t.browser();
    let reply = phone
        .log_in("alice@exemple.fr", "pas-le-bon-mot-de-passe")
        .await;
    reply.assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    replies.push(reply);
    let reply = phone.log_in("alice@exemple.fr", PASSWORD).await;
    reply.assert_status(StatusCode::OK);
    tokens.push(phone.session().unwrap());
    replies.push(reply);
    replies.push(phone.me().await);
    // Changement de mot de passe, raté puis réussi.
    let reply = laptop
        .change_password("pas-le-bon-mot-de-passe", NEW_PASSWORD)
        .await;
    reply.assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    replies.push(reply);
    let reply = laptop.change_password(PASSWORD, NEW_PASSWORD).await;
    reply.assert_status(StatusCode::NO_CONTENT);
    tokens.push(laptop.session().unwrap());
    replies.push(reply);
    replies.push(laptop.me().await);
    let last = tokens.last().unwrap().clone();
    // Déconnexion.
    let reply = laptop.log_out().await;
    reply.assert_status(StatusCode::NO_CONTENT);
    replies.push(reply);

    tokens.sort();
    tokens.dedup();
    assert_eq!(tokens.len(), 3, "un jeton neuf par session ouverte");
    let secrets: Vec<&str> = ["$argon2", PASSWORD, NEW_PASSWORD, "pas-le-bon-mot-de-passe"]
        .into_iter()
        .chain(tokens.iter().map(String::as_str))
        .collect();

    // Les réponses : un jeton ne voyage que dans `Set-Cookie`.
    for reply in &replies {
        let mut headers = HeaderMap::new();
        for (name, value) in &reply.headers {
            if name != header::SET_COOKIE {
                headers.append(name, value.clone());
            }
        }
        let text = format!("{headers:?}\n{}", String::from_utf8_lossy(&reply.body));
        for secret in &secrets {
            assert!(
                !text.contains(secret),
                "« {secret} » dans une réponse : {text}"
            );
        }
    }

    // Les journaux, lus pour de bon : ceux de ce parcours y sont.
    let text = logs.text();
    for expected in [
        "compte créé",
        "connexion refusée",
        "session ouverte",
        "mot de passe changé",
        &id.to_string(),
    ] {
        assert!(
            text.contains(expected),
            "« {expected} » absent des journaux"
        );
    }
    for secret in &secrets {
        assert!(!text.contains(secret), "« {secret} » dans les journaux");
    }

    // La base : un hachage Argon2id pour le mot de passe, le SHA-256 du jeton
    // pour la session ; ni l'un ni l'autre en clair dans les fichiers.
    let store = Store::open(&t.db_path()).await.unwrap();
    let user = store.get_user(id).await.unwrap().expect("le compte");
    let hash = user.password_hash.expect("un hachage");
    assert!(
        hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{hash}"
    );
    store.close().await;
    // La session de l'ordinateur est fermée ; reste celle du téléphone,
    // supprimée par le changement de mot de passe : plus aucune.
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM sessions").await,
        0
    );
    let reply = t.browser().log_in("alice@exemple.fr", NEW_PASSWORD).await;
    reply.assert_status(StatusCode::OK);
    let (value, _) = cookie_parts(&reply.session_set_cookie());
    assert_eq!(
        query_i64(
            &t.db_path(),
            &format!(
                "SELECT COUNT(*) FROM sessions WHERE hex(token_hash) = '{}'",
                stored_hash(&value)
            )
        )
        .await,
        1,
        "la base garde le SHA-256 du jeton"
    );
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&t.cfg.data_dir).unwrap() {
        files.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    assert!(!files.is_empty());
    for secret in [PASSWORD, NEW_PASSWORD, value.as_str(), last.as_str()] {
        assert!(
            !files
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "« {secret} » en clair dans les fichiers de la base"
        );
    }
}

/// Toute requête qui écrit et porte une `Origin` étrangère est refusée, avant
/// même de chercher la route ; une lecture passe (Ruling R51).
#[tokio::test]
async fn mutating_request_with_foreign_origin_is_403() {
    let t = TestApp::new().await;
    let (alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let space = me["spaces"][0]["id"]
        .as_str()
        .expect("un espace")
        .to_string();
    let reply = alice
        .post(
            "/api/v1/pages",
            json!({ "spaceId": space, "title": "Page" }),
        )
        .await;
    reply.assert_status(StatusCode::CREATED);
    let page: PageNode = serde_json::from_slice(&reply.body).unwrap();
    let tree = format!("/api/v1/spaces/{space}/tree");
    let with_origin = |mut request: Request<Body>, origin: &str| {
        request
            .headers_mut()
            .insert(header::ORIGIN, origin.parse().unwrap());
        request
    };
    let login = json!({ "email": "alice@exemple.fr", "password": PASSWORD });
    let writes = |origin: &str| {
        [
            json_request(Method::POST, "/api/v1/auth/login", &login),
            json_request(Method::POST, "/api/v1/auth/signup", &json!({})),
            Request::post("/api/v1/auth/logout")
                .body(Body::empty())
                .unwrap(),
            json_request(
                Method::POST,
                "/api/v1/auth/password",
                &json!({ "current": PASSWORD, "new": NEW_PASSWORD }),
            ),
            json_request(
                Method::POST,
                "/api/v1/pages",
                &json!({ "spaceId": space, "title": "Volée" }),
            ),
            json_request(
                Method::PATCH,
                &format!("/api/v1/pages/{}", page.id),
                &json!({ "title": "Volée" }),
            ),
            Request::put(format!("/api/v1/pages/{}/mmd", page.id))
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from("Volé\n"))
                .unwrap(),
            Request::delete(format!("/api/v1/pages/{}", page.id))
                .body(Body::empty())
                .unwrap(),
            Request::post(format!("/api/v1/pages/{}/trash", page.id))
                .body(Body::empty())
                .unwrap(),
            Request::post("/api/v1/inconnue")
                .body(Body::empty())
                .unwrap(),
        ]
        .map(|request| with_origin(request, origin))
    };

    for origin in [
        "http://evil.example",
        "https://localhost.evil.example",
        "null",
        "file://",
    ] {
        for request in writes(origin) {
            let line = format!("{} {} ({origin})", request.method(), request.uri());
            // Avec la session d'Alice : c'est elle qu'un site tiers viserait.
            let reply = alice.send(request).await;
            let body = reply.assert_error(StatusCode::FORBIDDEN, "bad_origin");
            // Le message nomme le réglage qui admet une autre origine.
            assert_french(&body, &["Origin", "MEMENTUM_PUBLIC_URL"]);
            assert!(reply.set_cookies().is_empty(), "{line}");
        }
    }
    // Rien n'a été écrit, et la session vaut toujours ; le mot de passe n'a
    // pas changé.
    let seen: mementum_server::Tree = serde_json::from_slice(&alice.get(&tree).await.body).unwrap();
    assert_eq!(seen.nodes, std::slice::from_ref(&page));
    assert_eq!(alice.get_mmd(&page.id).await.text(), "<p/>\n");
    assert_eq!(alice.me().await.status, StatusCode::OK);
    assert_eq!(
        t.browser()
            .log_in("alice@exemple.fr", NEW_PASSWORD)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    // Une lecture avec la même `Origin` passe : elle n'écrit rien, et un
    // site tiers ne lit pas la réponse (pas de CORS).
    for uri in ["/api/v1/me", tree.as_str(), "/api/v1/info"] {
        let request = with_origin(
            Request::get(uri).body(Body::empty()).unwrap(),
            "http://evil.example",
        );
        assert_eq!(alice.send(request).await.status, StatusCode::OK, "{uri}");
    }
    let head = with_origin(
        Request::head("/health").body(Body::empty()).unwrap(),
        "http://evil.example",
    );
    assert_eq!(t.send(head).await.status, StatusCode::OK);

    // Le client lui-même (servi par la boucle locale), et un client hors
    // navigateur, sans `Origin`.
    let request = with_origin(
        json_request(Method::POST, "/api/v1/auth/login", &login),
        "http://localhost:5173",
    );
    assert_eq!(t.browser().send(request).await.status, StatusCode::OK);
    let request = with_origin(
        json_request(
            Method::POST,
            "/api/v1/pages",
            &json!({ "spaceId": space, "title": "Locale" }),
        ),
        "http://127.0.0.1:5174",
    );
    assert_eq!(alice.send(request).await.status, StatusCode::CREATED);
    assert_eq!(
        t.browser()
            .log_in("alice@exemple.fr", PASSWORD)
            .await
            .status,
        StatusCode::OK
    );
}

/// Derrière un mandataire, l'origine de `MEMENTUM_PUBLIC_URL` est admise sur
/// les écritures et sur la synchronisation, quel que soit l'en-tête `Host`
/// que le mandataire transmet (Ruling R51).
#[tokio::test]
async fn public_origin_is_allowed_to_write() {
    let t = TestApp::with_config(|cfg| {
        cfg.bind = "0.0.0.0:0".parse().unwrap();
        cfg.public_url = Some("https://notes.exemple.fr/".to_string());
    })
    .await;
    let signup = |origin: &str, email: &str| {
        Request::post("/api/v1/auth/signup")
            .header(header::HOST, "10.0.0.5:8080")
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "email": email, "name": "Alice", "password": PASSWORD }).to_string(),
            ))
            .unwrap()
    };
    // L'origine publique, exactement : ni un autre schéma, ni un autre port,
    // ni un sous-domaine.
    for origin in [
        "http://notes.exemple.fr",
        "https://notes.exemple.fr:8443",
        "https://evil.notes.exemple.fr",
        "https://notes.exemple.fr.evil.example",
    ] {
        t.send_raw(signup(origin, "alice@exemple.fr"))
            .await
            .assert_error(StatusCode::FORBIDDEN, "bad_origin");
    }
    let reply = t
        .send_raw(signup("https://notes.exemple.fr", "alice@exemple.fr"))
        .await;
    reply.assert_status(StatusCode::CREATED);
    // Et toujours l'hôte même du serveur, avec la session que l'inscription
    // vient d'ouvrir.
    let cookie = reply.session_set_cookie();
    let cookie = cookie.split(';').next().expect("nom=valeur");
    let space = reply.json()["spaces"][0]["id"].clone();
    let page = Request::post("/api/v1/pages")
        .header(header::HOST, "10.0.0.5:8080")
        .header(header::ORIGIN, "http://10.0.0.5:8080")
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "spaceId": space }).to_string()))
        .unwrap();
    let reply = t.send_raw(page).await;
    reply.assert_status(StatusCode::CREATED);
    let id = reply.json()["id"].as_str().unwrap().to_string();

    // `/sync` : l'origine publique passe le contrôle (puis, avec la session
    // et sans poignée de main en mémoire, la requête n'est pas une WebSocket).
    let sync = |origin: &str| {
        Request::get(format!("/sync/{id}?schema=1"))
            .header(header::HOST, "10.0.0.5:8080")
            .header(header::ORIGIN, origin)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    };
    t.send_raw(sync("https://notes.exemple.fr"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid_request");
    t.send_raw(sync("http://notes.exemple.fr"))
        .await
        .assert_error(StatusCode::FORBIDDEN, "bad_origin");
}

/// Tant qu'aucun compte n'existe, le démarrage prévient : le premier compte
/// créé administrera l'instance (Ruling R50).
#[tokio::test]
async fn startup_warns_while_no_account_exists() {
    let logs = Logs::capture();
    let t = TestApp::new().await;
    let text = logs.text();
    assert!(
        text.contains("WARN") && text.contains("aucun compte") && text.contains("admin"),
        "avertissement attendu : {text}"
    );
    drop(logs);
    t.signed_up("alice@exemple.fr", "Alice").await;

    // Au démarrage suivant, sur les mêmes données : plus rien à dire.
    let logs = Logs::capture();
    let _router = mementum_server::app(&t.cfg).await.unwrap();
    let text = logs.text();
    assert!(!text.contains("aucun compte"), "{text}");
}

/// Une adresse publique invalide arrête le démarrage, même posée sans passer
/// par `Config::resolve`.
#[tokio::test]
async fn invalid_public_url_stops_startup() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = mementum_server::Config {
        data_dir: dir.path().join("donnees"),
        public_url: Some("notes.exemple.fr".to_string()),
        ..mementum_server::Config::default()
    };
    let error = mementum_server::app(&cfg)
        .await
        .expect_err("démarrage refusé");
    let message = format!("{error:#}");
    assert!(message.contains("adresse publique invalide"), "{message}");
}

/// Un navigateur ne rend que son propre cookie : deux sessions ne se mêlent
/// pas, et une session ne donne que son compte.
#[tokio::test]
async fn sessions_are_per_account() {
    let t = open_app().await;
    let (alice, alice_me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let (bob, bob_me) = t.signed_up("bob@exemple.fr", "Bob").await;
    assert_ne!(alice.session(), bob.session());
    assert_eq!(alice.me().await.json(), alice_me);
    assert_eq!(bob.me().await.json(), bob_me);

    // Un cookie de session parmi d'autres cookies.
    let request = |cookies: String| {
        Request::get("/api/v1/me")
            .header(header::COOKIE, cookies)
            .body(Body::empty())
            .unwrap()
    };
    let token = bob.session().unwrap();
    let reply = t
        .send(request(format!(
            "theme=sombre; {SESSION_COOKIE}={token}; x=1"
        )))
        .await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.json(), bob_me);
    // Un autre nom de cookie ne compte pas.
    t.send(request(format!("session={token}")))
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
}

/// Huit inscriptions lancées de front ; rend le statut et le code d'erreur de
/// chacune.
async fn eight_signups(t: &TestApp, invitation: Option<&str>) -> Vec<(StatusCode, String)> {
    let sign_up = |i: usize| async move {
        let reply = t
            .browser()
            .sign_up(
                &format!("compte{i}@exemple.fr"),
                &format!("Compte {i}"),
                PASSWORD,
                invitation,
            )
            .await;
        let code = match reply.status {
            StatusCode::CREATED => String::new(),
            _ => reply.json()["error"].as_str().unwrap_or("").to_string(),
        };
        (reply.status, code)
    };
    let (a, b, c, d, e, f, g, h) = tokio::join!(
        sign_up(0),
        sign_up(1),
        sign_up(2),
        sign_up(3),
        sign_up(4),
        sign_up(5),
        sign_up(6),
        sign_up(7),
    );
    vec![a, b, c, d, e, f, g, h]
}

/// Le verrou des inscriptions : sur une instance neuve, en mode `invite`, le
/// droit du premier compte ne vaut qu'une fois, même pour des inscriptions
/// qui arrivent ensemble (chacune voit « aucun compte » tant que le hachage
/// de la première n'est pas fini).
#[tokio::test]
async fn concurrent_first_signups_make_one_account_in_invite_mode() {
    let t = TestApp::new().await;
    let replies = eight_signups(&t, None).await;
    let created = replies
        .iter()
        .filter(|(status, _)| *status == StatusCode::CREATED)
        .count();
    let closed = replies
        .iter()
        .filter(|reply| **reply == (StatusCode::FORBIDDEN, "signup_closed".to_string()))
        .count();
    assert_eq!((created, closed), (1, 7), "{replies:?}");
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM users").await,
        1
    );
    assert_eq!(
        query_i64(
            &t.db_path(),
            "SELECT COUNT(*) FROM users WHERE instance_admin = 1"
        )
        .await,
        1
    );
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        1
    );
}

/// Le verrou des inscriptions : un jeton d'invitation sert une fois, même
/// pour des inscriptions qui arrivent ensemble. Les autres ne créent pas de
/// compte.
#[tokio::test]
async fn one_invitation_makes_one_account() {
    let t = TestApp::new().await;
    let (_alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let space = space_id(&me, 0);
    let store = Store::open(&t.db_path()).await.unwrap();
    let (token, _) = invite(
        &store,
        space,
        user_id(&me),
        Role::Member,
        6,
        now_ms() + WEEK_MS,
    )
    .await;

    let replies = eight_signups(&t, Some(&token)).await;
    let created = replies
        .iter()
        .filter(|(status, _)| *status == StatusCode::CREATED)
        .count();
    let invalid = replies
        .iter()
        .filter(|reply| **reply == (StatusCode::NOT_FOUND, "invalid_invitation".to_string()))
        .count();
    assert_eq!((created, invalid), (1, 7), "{replies:?}");
    // Alice, et un seul compte de plus, entré dans son espace.
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM users").await,
        2
    );
    assert_eq!(store.members(space).await.unwrap().len(), 2);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM spaces").await,
        1
    );
    store.close().await;
}

/// Ruling R76 : le changement de mot de passe a son propre frein. Dix
/// connexions ratées par un tiers ne bloquent pas le titulaire qui change son
/// mot de passe ; dix mots de passe actuels faux ne bloquent pas la
/// connexion.
#[tokio::test]
async fn password_change_has_its_own_throttle() {
    let t = open_app().await;
    let (alice, _) = t.signed_up("alice@exemple.fr", "Alice").await;
    let (bob, _) = t.signed_up("bob@exemple.fr", "Bob").await;
    let too_many = |reply: &Reply| -> String {
        let body = reply.assert_error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
        let seconds: u64 = reply
            .headers
            .get(header::RETRY_AFTER)
            .expect("Retry-After")
            .to_str()
            .unwrap()
            .parse()
            .expect("Retry-After en secondes");
        assert!((1..=900).contains(&seconds), "Retry-After: {seconds}");
        body["message"].as_str().unwrap().to_string()
    };

    // Un tiers épuise les connexions d'Alice.
    for attempt in 1..=10 {
        let reply = t.browser().log_in("alice@exemple.fr", "pas-le-bon").await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
    }
    let message = too_many(&t.browser().log_in("alice@exemple.fr", PASSWORD).await);
    assert!(message.contains("adresse"), "{message}");
    // Alice, déjà connectée, change quand même son mot de passe.
    alice
        .change_password(PASSWORD, NEW_PASSWORD)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert_eq!(alice.me().await.status, StatusCode::OK);

    // Dix mots de passe actuels faux avec la session de Bob : le changement
    // est refusé, même avec le bon, jusqu'à la fin de la fenêtre.
    for attempt in 1..=10 {
        let reply = bob.change_password("pas-le-bon", NEW_PASSWORD).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
        assert!(reply.headers.get(header::RETRY_AFTER).is_none());
    }
    too_many(&bob.change_password("pas-le-bon", NEW_PASSWORD).await);
    // Le refus parle de mots de passe actuels, pas d'une adresse que
    // personne n'a tapée ici (le message de la connexion).
    let message = too_many(&bob.change_password(PASSWORD, NEW_PASSWORD).await);
    assert!(message.contains("mots de passe actuels"), "{message}");
    assert!(!message.contains("adresse"), "{message}");
    // Sa connexion, elle, n'est pas freinée, et son mot de passe n'a pas
    // changé.
    t.browser()
        .log_in("bob@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    assert_eq!(bob.me().await.status, StatusCode::OK);
}

/// Les réponses des routes d'authentification et de `GET /api/v1/me` ne se
/// gardent dans aucun cache, réussites et refus compris.
#[tokio::test]
async fn auth_responses_are_never_cached() {
    let t = TestApp::new().await;
    let no_store = |reply: &Reply, expected: StatusCode, what: &str| {
        assert_eq!(reply.status, expected, "{what} : {}", reply.text());
        assert_eq!(
            reply
                .headers
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "{what}"
        );
    };
    let alice = t.browser();
    no_store(
        &alice.me().await,
        StatusCode::UNAUTHORIZED,
        "me sans session",
    );
    no_store(
        &alice.sign_up("alice", "Alice", PASSWORD, None).await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "inscription refusée",
    );
    no_store(
        &alice
            .post("/api/v1/auth/signup", json!({ "email": 3 }))
            .await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "inscription au corps illisible",
    );
    no_store(
        &alice
            .sign_up("alice@exemple.fr", "Alice", PASSWORD, None)
            .await,
        StatusCode::CREATED,
        "inscription",
    );
    no_store(&alice.me().await, StatusCode::OK, "me");
    no_store(
        &t.browser()
            .sign_up("bob@exemple.fr", "Bob", PASSWORD, None)
            .await,
        StatusCode::FORBIDDEN,
        "inscription fermée",
    );
    no_store(
        &t.browser().log_in("alice@exemple.fr", "pas-le-bon").await,
        StatusCode::UNAUTHORIZED,
        "connexion refusée",
    );
    no_store(
        &t.browser().log_in("alice@exemple.fr", PASSWORD).await,
        StatusCode::OK,
        "connexion",
    );
    no_store(
        &t.browser().change_password(PASSWORD, NEW_PASSWORD).await,
        StatusCode::UNAUTHORIZED,
        "changement sans session",
    );
    no_store(
        &alice.change_password("pas-le-bon", NEW_PASSWORD).await,
        StatusCode::UNAUTHORIZED,
        "mot de passe actuel faux",
    );
    no_store(
        &alice.change_password(PASSWORD, "neuf-car.").await,
        StatusCode::UNPROCESSABLE_ENTITY,
        "nouveau mot de passe refusé",
    );
    no_store(
        &alice.change_password(PASSWORD, NEW_PASSWORD).await,
        StatusCode::NO_CONTENT,
        "changement de mot de passe",
    );
    no_store(
        &alice.log_out().await,
        StatusCode::NO_CONTENT,
        "déconnexion",
    );
    no_store(
        &alice.log_out().await,
        StatusCode::NO_CONTENT,
        "déconnexion sans session",
    );
}

/// Se connecter ferme la session que le navigateur portait déjà : l'ancien
/// cookie, rejoué, ne vaut plus rien. Une connexion ratée ne ferme rien.
#[tokio::test]
async fn login_closes_the_session_the_browser_carried() {
    let t = open_app().await;
    let (alice, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let first = alice.session().expect("session de l'inscription");
    let sessions = "SELECT COUNT(*) FROM sessions";

    // Ratée : la session portée vaut toujours.
    alice
        .log_in("alice@exemple.fr", "pas-le-bon")
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    assert_eq!(alice.session().as_deref(), Some(first.as_str()));
    assert_eq!(alice.me().await.status, StatusCode::OK);

    // Réussie, dans le même navigateur : un jeton neuf, l'ancien est fermé.
    alice
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let second = alice.session().expect("session de la connexion");
    assert_ne!(second, first);
    assert_eq!(alice.me().await.json(), me);
    let replay = t.browser();
    replay.set_session(Some(&first));
    replay
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(query_i64(&t.db_path(), sessions).await, 1);

    // Un autre compte se connecte dans ce navigateur : la session d'Alice
    // qu'il portait est fermée, pas celles d'ailleurs.
    let elsewhere = t.browser();
    elsewhere
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let (_bob, _) = t.signed_up("bob@exemple.fr", "Bob").await;
    let reply = alice.log_in("bob@exemple.fr", PASSWORD).await;
    reply.assert_status(StatusCode::OK);
    assert_eq!(reply.json()["user"]["email"], "bob@exemple.fr");
    replay.set_session(Some(&second));
    replay
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    assert_eq!(elsewhere.me().await.json(), me);
    // Alice ailleurs, Bob à son inscription, Bob ici.
    assert_eq!(query_i64(&t.db_path(), sessions).await, 3);
}

/// Ruling R78 : derrière un tunnel qui garde l'en-tête `Host` publique, le
/// serveur n'écoute que sur la boucle locale. L'hôte de
/// `MEMENTUM_PUBLIC_URL` est alors admis par le contrôle de `Host`, et lui
/// seul.
#[tokio::test]
async fn public_url_host_is_admitted_on_loopback() {
    let health = |host: &'static str| {
        Request::get("/health")
            .header(header::HOST, host)
            .body(Body::empty())
            .unwrap()
    };
    let t = TestApp::with_config(|cfg| {
        cfg.public_url = Some("https://notes.exemple.fr".to_string());
    })
    .await;
    for host in [
        "notes.exemple.fr",
        "Notes.Exemple.FR",
        // Le port par défaut du schéma, écrit en toutes lettres.
        "notes.exemple.fr:443",
        // La boucle locale, comme avant.
        "localhost:8080",
        "127.0.0.1",
    ] {
        let reply = t.send_raw(health(host)).await;
        assert_eq!(reply.status, StatusCode::OK, "Host: {host}");
    }
    for host in [
        "evil.example",
        "notes.exemple.fr.evil.example",
        "sub.notes.exemple.fr",
        "exemple.fr",
        "notes.exemple.fr:8443",
        "notes.exemple.fr:80",
        "notes.exemple.fr:",
        "alex@notes.exemple.fr",
    ] {
        let body = t
            .send_raw(health(host))
            .await
            .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
        // Le message nomme le réglage qui admet un autre hôte.
        assert_french(&body, &["Host", "MEMENTUM_PUBLIC_URL"]);
    }
    // De bout en bout : une écriture venue de l'adresse publique.
    let signup = Request::post("/api/v1/auth/signup")
        .header(header::HOST, "notes.exemple.fr")
        .header(header::ORIGIN, "https://notes.exemple.fr")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "email": "alice@exemple.fr", "name": "Alice", "password": PASSWORD })
                .to_string(),
        ))
        .unwrap();
    t.send_raw(signup).await.assert_status(StatusCode::CREATED);

    // Avec un port dans l'adresse publique, c'est cet hôte-là, port compris.
    let t = TestApp::with_config(|cfg| {
        cfg.public_url = Some("http://notes.exemple.fr:8443/".to_string());
    })
    .await;
    assert_eq!(
        t.send_raw(health("notes.exemple.fr:8443")).await.status,
        StatusCode::OK
    );
    for host in ["notes.exemple.fr", "notes.exemple.fr:80"] {
        t.send_raw(health(host))
            .await
            .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
    }

    // Sans adresse publique : la boucle locale seulement.
    let t = TestApp::new().await;
    t.send_raw(health("notes.exemple.fr"))
        .await
        .assert_error(StatusCode::MISDIRECTED_REQUEST, "bad_host");
}

// ---------------------------------------------------------------------------------------------
// Une session ne s'ouvre que si le hachage vérifié est encore celui du compte (Ruling R120).
//
// Entre la lecture du hachage et l'ouverture de la session, il y a une vérification Argon2 et
// l'attente de son tour. Un changement de mot de passe qui se glisse là supprime les sessions
// du compte, mais pas celle qui naît après lui : la connexion en vol, vérifiée contre l'ancien
// mot de passe, rendait une session qui lui survivait. La session est donc liée au hachage
// vérifié, dans l'instruction qui l'écrit.
//
// Pour forcer l'entrelacement sans dépendre d'un minutage, ces tests font comme ceux du partage
// (`overtaken`) : la requête part pendant que le test tient le verrou d'écriture de SQLite. Elle
// lit le hachage d'avant, le vérifie, puis attend pour écrire ; le test écrit alors ce qu'écrit
// un changement de mot de passe, et rend le verrou. Le journal dit que la requête avait bien
// vérifié l'ancien mot de passe : arrivée après, elle serait refusée pour un mot de passe faux,
// et le test ne prouverait rien.

/// Le mot de passe que se choisit l'intrus.
const INTRUDER_PASSWORD: &str = "mot-de-passe-de-l-intrus";

/// Ce qu'écrit un changement de mot de passe (`Store::set_password`), en SQL : le hachage de
/// `account` devient celui de `donor` (un compte inscrit avec le nouveau mot de passe), et ses
/// sessions sont supprimées.
fn password_replaced(account: Uuid, donor: Uuid) -> [String; 2] {
    [
        format!(
            "UPDATE users SET password_hash = (SELECT password_hash FROM users WHERE id = \
             '{donor}') WHERE id = '{account}'"
        ),
        format!("DELETE FROM sessions WHERE user_id = '{account}'"),
    ]
}

/// Alice, et Bob, inscrit avec [`NEW_PASSWORD`] : son hachage sert de nouveau mot de passe à
/// Alice. Rend le `Me` d'Alice et l'id de Bob.
async fn alice_and_a_donor(t: &TestApp) -> (Value, Uuid) {
    let (_, me) = t.signed_up("alice@exemple.fr", "Alice").await;
    let reply = t
        .browser()
        .sign_up("bob@exemple.fr", "Bob", NEW_PASSWORD, None)
        .await;
    reply.assert_status(StatusCode::CREATED);
    (me, user_id(&reply.json()))
}

/// La même réponse, corps et en-têtes compris, et sans cookie.
fn assert_same_refusal(raced: &Reply, wrong: &Reply) {
    assert_eq!(raced.status, wrong.status, "réponse : {}", raced.text());
    assert_eq!(raced.body, wrong.body);
    assert_eq!(raced.headers, wrong.headers);
    assert!(raced.set_cookies().is_empty());
}

/// Une connexion en vol avec l'ancien mot de passe, vérifiée avant le changement et qui écrit
/// après : aucune session. Elle reçoit la réponse d'un mot de passe faux, et compte comme un
/// échec pour le frein.
#[tokio::test]
async fn login_verified_before_a_password_change_opens_no_session() {
    let t = open_app().await;
    let (me, bob) = alice_and_a_donor(&t).await;
    let (alice, space) = (user_id(&me), space_id(&me, 0));
    // La session qu'Alice avait, et ce que répond un mot de passe faux (un échec au compteur).
    let laptop = t.browser();
    laptop
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let wrong = t
        .browser()
        .log_in("alice@exemple.fr", "pas-le-bon-mot-de-passe")
        .await;
    wrong.assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");

    // Le navigateur d'où part la connexion porte déjà une session, celle de Bob : une connexion
    // refusée ne doit rien fermer.
    let intruder = t.browser();
    intruder
        .log_in("bob@exemple.fr", NEW_PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    let carried = intruder.session();
    let raced = overtaken(
        &t,
        space,
        intruder.log_in("alice@exemple.fr", PASSWORD),
        &password_replaced(alice, bob),
    )
    .await;

    // Elle avait bien vérifié l'ancien mot de passe, et c'est à l'écriture qu'elle est refusée.
    assert!(
        raced.journal.contains("mot de passe changé pendant"),
        "réponse {} : {}\njournal : {}",
        raced.reply.status,
        raced.reply.text(),
        raced.journal
    );
    assert!(
        !raced.journal.contains("mot de passe invalide"),
        "la connexion est arrivée après le changement : {}",
        raced.journal
    );
    assert_same_refusal(&raced.reply, &wrong);
    // La session que portait le navigateur n'est pas fermée : c'est toujours celle de Bob.
    assert_eq!(intruder.session(), carried);
    let still = intruder.me().await;
    still.assert_status(StatusCode::OK);
    assert_eq!(user_id(&still.json()), bob);
    // Plus aucune session pour le compte : celle d'Alice est partie avec le changement, et
    // celle de l'intrus n'est pas née.
    let sessions = format!("SELECT COUNT(*) FROM sessions WHERE user_id = '{alice}'");
    assert_eq!(query_i64(&t.db_path(), &sessions).await, 0);
    laptop
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");

    // Elle compte comme un échec : avec celui d'avant et huit autres, le compte est à dix, et
    // même le bon mot de passe est refusé jusqu'à la fin de la fenêtre.
    for attempt in 1..=8 {
        let reply = t.browser().log_in("alice@exemple.fr", "pas-le-bon").await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
    }
    t.browser()
        .log_in("alice@exemple.fr", NEW_PASSWORD)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
}

/// Le même entrelacement sur le changement de mot de passe, qui rouvre une session : l'intrus
/// tient une session du compte et connaît l'ancien mot de passe ; sa demande, vérifiée avant le
/// changement d'Alice, écrit après. Elle ne change rien : ni le mot de passe (celui d'Alice
/// reste), ni aucune session. Sans cela, l'intrus reprenait le compte.
#[tokio::test]
async fn password_change_verified_before_another_one_changes_nothing() {
    let t = open_app().await;
    let (me, bob) = alice_and_a_donor(&t).await;
    let (alice, space) = (user_id(&me), space_id(&me, 0));
    let intruder = t.browser();
    intruder
        .log_in("alice@exemple.fr", PASSWORD)
        .await
        .assert_status(StatusCode::OK);
    // Ce que répond un mot de passe actuel faux.
    let wrong = intruder
        .change_password("pas-le-bon-mot-de-passe", INTRUDER_PASSWORD)
        .await;
    wrong.assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");

    let raced = overtaken(
        &t,
        space,
        intruder.change_password(PASSWORD, INTRUDER_PASSWORD),
        &password_replaced(alice, bob),
    )
    .await;

    assert!(
        raced.journal.contains("mot de passe changé pendant"),
        "réponse {} : {}\njournal : {}",
        raced.reply.status,
        raced.reply.text(),
        raced.journal
    );
    assert!(
        !raced.journal.contains("mot de passe actuel invalide"),
        "la demande est arrivée après le changement : {}",
        raced.journal
    );
    assert_same_refusal(&raced.reply, &wrong);
    // Le mot de passe est celui du changement passé devant, et le compte n'a aucune session.
    let kept = format!(
        "SELECT COUNT(*) FROM users WHERE id = '{alice}' AND password_hash = (SELECT \
         password_hash FROM users WHERE id = '{bob}')"
    );
    assert_eq!(query_i64(&t.db_path(), &kept).await, 1);
    let sessions = format!("SELECT COUNT(*) FROM sessions WHERE user_id = '{alice}'");
    assert_eq!(query_i64(&t.db_path(), &sessions).await, 0);
    intruder
        .me()
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "unauthenticated");
    t.browser()
        .log_in("alice@exemple.fr", INTRUDER_PASSWORD)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid_credentials");
    let alice_again = t.browser();
    alice_again
        .log_in("alice@exemple.fr", NEW_PASSWORD)
        .await
        .assert_status(StatusCode::OK);

    // La demande refusée compte comme un échec : avec celui d'avant et huit autres, le compte
    // est à dix, et même le bon mot de passe actuel est refusé jusqu'à la fin de la fenêtre.
    for attempt in 1..=8 {
        let reply = alice_again
            .change_password("pas-le-bon", INTRUDER_PASSWORD)
            .await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "essai {attempt}");
    }
    alice_again
        .change_password(NEW_PASSWORD, INTRUDER_PASSWORD)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "too_many_attempts");
}

/// Le test de charge de la relecture de sécurité : 150 connexions avec l'ancien mot de passe,
/// une toutes les deux millisecondes, pendant qu'Alice change le sien. Après le `204`, aucune
/// des sessions que ces connexions ont reçues ne vaut encore : celles d'avant le changement
/// sont supprimées par lui, et celles qui étaient en vol ne naissent pas.
///
/// Sur le code d'avant, la moitié environ lui survivait (73 sur 150 à la relecture). Ce test-ci
/// dépend du minutage pour échouer, pas pour passer : les deux tests au-dessus forcent
/// l'entrelacement.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logins_in_flight_never_survive_a_password_change() {
    let t = TestApp::new().await;
    let (alice, _) = t.signed_up("alice@exemple.fr", "Alice").await;

    let mut logins = tokio::task::JoinSet::new();
    for i in 0..150u64 {
        let router = t.router.clone();
        logins.spawn(async move {
            tokio::time::sleep(Duration::from_millis(2 * i)).await;
            let mut request = json_request(
                Method::POST,
                "/api/v1/auth/login",
                &json!({ "email": "alice@exemple.fr", "password": PASSWORD }),
            );
            request
                .headers_mut()
                .insert(header::HOST, "localhost".parse().unwrap());
            let response = router.oneshot(request).await.unwrap();
            let session = response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .filter_map(|raw| raw.strip_prefix(&format!("{SESSION_COOKIE}=")))
                .filter_map(|rest| rest.split(';').next())
                .map(str::to_string)
                .next();
            (response.status(), session)
        });
    }
    tokio::time::sleep(Duration::from_millis(40)).await;
    alice
        .change_password(PASSWORD, NEW_PASSWORD)
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let outcomes = logins.join_all().await;

    // Le `204` est reçu, et toutes les connexions ont répondu.
    let opened: Vec<&String> = outcomes
        .iter()
        .filter_map(|(_, session)| session.as_ref())
        .collect();
    assert!(
        !opened.is_empty(),
        "aucune connexion n'a abouti avant le changement : {:?}",
        outcomes
            .iter()
            .map(|(status, _)| status)
            .collect::<Vec<_>>()
    );
    let mut survivors = 0;
    for session in &opened {
        let browser = t.browser();
        browser.set_session(Some(session.as_str()));
        if browser.me().await.status != StatusCode::UNAUTHORIZED {
            survivors += 1;
        }
    }
    assert_eq!(
        survivors,
        0,
        "{survivors} sessions ouvertes avec l'ancien mot de passe valent encore après le \
         changement, sur {} connexions abouties",
        opened.len()
    );
    // La session qui a changé le mot de passe est la seule du compte.
    assert_eq!(alice.me().await.status, StatusCode::OK);
    assert_eq!(
        query_i64(&t.db_path(), "SELECT COUNT(*) FROM sessions").await,
        1
    );
}
