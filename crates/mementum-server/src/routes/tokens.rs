//! Routes des jetons d'API personnels (lot 5, Task S6 ; Rulings R1213, R1218 à R1221) : créer,
//! lister, révoquer, lire l'activité.
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/tokens` | une session | [`TokenInfo`]`[]` : les jetons du compte, vivants d'abord |
//! | `POST /api/v1/tokens`, corps [`CreateToken`] | une session, le mot de passe | `201`, [`TokenCreated`] : le jeton, montré cette fois seulement |
//! | `DELETE /api/v1/tokens/{id}` | une session, le jeton est le sien | `204` (déjà révoqué : `204`, rien d'écrit) |
//! | `GET /api/v1/tokens/{id}/calls?before=&outcome=` | une session, le jeton est le sien | [`TokenCalls`] : son journal d'activité, cinquante lignes au plus |
//!
//! **Une session seulement.** Ces routes sont dans `auth::SESSION_ONLY_ROUTES` : la porte des
//! jetons refuse un `Bearer mmt_…` (`403 token_not_allowed`) avant qu'elles ne lisent rien, et
//! [`SessionUser`] le refuse de nouveau (second verrou) : un jeton ne crée, ne liste ni ne
//! révoque un jeton, pas même le sien.
//!
//! **Le texte d'un jeton ne paraît qu'une fois** : dans la réponse de la création (`mmt_` et les
//! 43 caractères de `auth::new_token`). La base n'en garde que le SHA-256 et les quatre derniers
//! caractères (`hint`) ; ni la liste, ni l'activité, ni aucun journal du serveur ne le portent.
//!
//! **Créer redemande le mot de passe** (décision L2) : vérifié par Argon2, compté par le frein
//! des connexions dans des fenêtres à lui (`Guarded::TokenCreation`, Ruling R1366 : dix mots de
//! passe faux en quinze minutes, puis `429 too_many_attempts`), et le jeton n'est écrit que si le
//! hachage vérifié est encore celui du compte, dans la transaction qui l'écrit
//! (`Store::create_token_verified`, Ruling R120) : un changement de mot de passe passé entre-temps,
//! qui révoque tous les jetons du compte, ne laisse pas derrière lui un jeton créé contre l'ancien.
//!
//! **Refus de la création**, dans l'ordre : `401` sans session, `409 account_changed` ; le corps
//! (`400` ou `422 invalid_json`) ; `422 invalid_name`, `invalid_level` (tout autre niveau que
//! `read` ou `edit`, `full` compris, Ruling R1218), `invalid_scope` (une portée mal formée),
//! `invalid_expiry` ; `429 too_many_attempts` ; `401 invalid_credentials` (mot de passe faux, ou
//! remplacé pendant sa vérification) ; `422 invalid_scope` pour un espace où le compte n'a aucun
//! rôle, une page qu'il ne voit pas ou à la corbeille, un identifiant inconnu ou mal écrit : **une
//! seule réponse**, qui ne dit pas si l'espace ou la page existe (Ruling R1367) ; `409
//! token_limit` au-delà de vingt jetons vivants (Ruling R1220).
//!
//! **Ce que la personne voit d'une page ou d'un espace** (la portée d'un jeton, son activité) :
//! le titre d'une page n'est demandé au store que si `access.rs` la montre encore au compte,
//! corbeille comprise ; le nom d'un espace, que s'il y a encore un rôle (Ruling R1368). Un jeton
//! d'un autre compte, un jeton inconnu ou mal nommé, et un jeton mort depuis quatre-vingt-dix jours
//! reçoivent la même réponse, `404 not_found` (Ruling R1270).

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use mementum_store::{
    CallOutcome, CallRow, CallSurface, NewToken, StoreError, TokenLevel, TokenRow, TokenScope,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::{self, AuthUser, Guarded, SessionUser, TOKEN_PREFIX};
use crate::error::{ApiError, internal};
use crate::routes::auth::{attempt, wrong_current_password};
use crate::types::{
    self, CreateToken, ErrorCode, TokenCall, TokenCallOutcome, TokenCallSurface, TokenCalls,
    TokenCreated, TokenInfo, TokenPage, TokenSpace,
};
use crate::{AppState, now_ms, to_completion};

/// Le nom d'un jeton, au plus, en caractères (Ruling R1220).
pub const MAX_TOKEN_NAME_CHARS: usize = 60;

/// L'échéance d'un jeton, au plus, en jours depuis sa création : dix ans. Un jeton peut aussi
/// n'en avoir aucune (§6.7 : « une expiration en option ») ; la borne ne garde que de valeurs
/// absurdes (Ruling R1369).
pub const MAX_EXPIRY_DAYS: i64 = 3_650;

/// Les lignes du journal d'activité rendues par réponse.
pub const CALLS_PER_PAGE: usize = 50;

/// Un jour, en millisecondes.
const DAY_MS: i64 = 86_400_000;

/// `GET /api/v1/tokens` : les jetons du compte qui se lisent encore (vivants, ou morts depuis
/// moins de quatre-vingt-dix jours), les vivants d'abord, du plus récent au plus ancien.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: Result<SessionUser, ApiError>,
) -> Result<Json<Vec<TokenInfo>>, ApiError> {
    let user = caller?.user;
    let now = now_ms();
    let rows = state
        .store
        .tokens_of(user.id, now)
        .await
        .map_err(internal)?;
    let named = Named::read(&state, &user, &rows).await?;
    Ok(Json(
        rows.iter().map(|row| info(row, &named, now)).collect(),
    ))
}

/// `POST /api/v1/tokens` : `201`, [`TokenCreated`]. Refus dans l'ordre de la tête du module.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: Result<SessionUser, ApiError>,
    body: Result<Json<CreateToken>, JsonRejection>,
) -> Result<(StatusCode, Json<TokenCreated>), ApiError> {
    let user = caller?.user;
    let Json(body) = body?;
    let name = token_name(&body.name)?;
    let level = token_level(&body.level)?;
    let asked = asked_scope(&body)?;
    let expires_in = expiry(body.expires_in_days)?;
    // Le mot de passe redemandé (décision L2), dans les fenêtres de son frein (Ruling R1366).
    attempt(&state, Guarded::TokenCreation, &user.email, Some(user.id))
        .map_err(ApiError::too_many_wrong_passwords)?;
    let account = state
        .store
        .get_user(user.id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;
    let right_password = auth::verify_password(body.password, account.password_hash.clone())
        .await
        .map_err(internal)?;
    let Some(verified) = account.password_hash.filter(|_| right_password) else {
        tracing::info!(
            user_id = %user.id,
            "création d'un jeton d'API refusée : mot de passe invalide"
        );
        return Err(wrong_current_password());
    };
    let scope = match seen_scope(&state, &user, asked).await {
        Ok(scope) => scope,
        Err(refusal) => {
            // Le mot de passe était le bon : la fenêtre du frein repart de zéro.
            state.logins.succeeded(Guarded::TokenCreation, &user.email);
            return Err(refusal);
        }
    };
    let (secret, hash) = auth::new_token();
    let token = format!("{TOKEN_PREFIX}{secret}");
    let now = now_ms();
    let new = NewToken {
        user_id: user.id,
        name,
        hash,
        hint: token[token.len() - 4..].to_string(),
        level,
        scope,
        can_trash: body.can_trash,
        expires_at: expires_in.map(|days| now + days * DAY_MS),
        now,
    };
    let row = match state.store.create_token_verified(new, &verified).await {
        // Un autre changement de mot de passe est passé entre la vérification et l'écriture :
        // la réponse d'un mot de passe faux, et la tentative reste comptée comme un échec.
        Ok(None) => {
            tracing::info!(
                user_id = %user.id,
                "création d'un jeton d'API refusée : mot de passe changé pendant sa vérification"
            );
            return Err(wrong_current_password());
        }
        written => {
            state.logins.succeeded(Guarded::TokenCreation, &user.email);
            match written {
                Ok(Some(row)) => row,
                Ok(None) => unreachable!("traité plus haut"),
                Err(StoreError::TokenLimit) => return Err(ApiError::token_limit()),
                // Le rôle dans l'espace, ou une page, est parti entre le jugement et la
                // transaction : la réponse d'une portée qu'on ne voit pas.
                Err(StoreError::NotFound) => return Err(ApiError::unseen_scope()),
                Err(error) => return Err(internal(error)),
            }
        }
    };
    tracing::info!(
        user_id = %user.id,
        token_id = %row.id,
        level = row.level.as_str(),
        "jeton d'API créé"
    );
    let named = Named::read(&state, &user, std::slice::from_ref(&row)).await?;
    Ok((
        StatusCode::CREATED,
        Json(TokenCreated {
            token,
            info: info(&row, &named, now),
        }),
    ))
}

/// `DELETE /api/v1/tokens/{id}` : `204` ; le jeton ne vaut plus dès l'appel suivant (la porte le
/// relit à chaque appel). Déjà révoqué : `204`, rien n'est écrit. Un jeton d'un autre compte,
/// inconnu, mal nommé ou mort depuis quatre-vingt-dix jours : `404 not_found`, la même réponse.
///
/// La révocation va au bout même si le client coupe (`to_completion`, Ruling R1376), comme la
/// déconnexion : qui ferme l'onglet pendant qu'elle attend son tour d'écriture ne laisse pas le
/// jeton vivant.
pub(crate) async fn revoke(
    State(state): State<AppState>,
    caller: Result<SessionUser, ApiError>,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let user = caller?.user;
    let id = token_id(path)?;
    to_completion(async move {
        match state.store.revoke_token(user.id, id, now_ms()).await {
            Ok(revoked) => {
                if revoked {
                    tracing::info!(user_id = %user.id, token_id = %id, "jeton d'API révoqué");
                }
                Ok(StatusCode::NO_CONTENT)
            }
            Err(StoreError::NotFound) => Err(ApiError::token_not_found()),
            Err(error) => Err(internal(error)),
        }
    })
    .await
}

/// Ce que la lecture de l'activité accepte dans son adresse.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallsQuery {
    /// Le curseur : l'`id` de la dernière ligne déjà lue.
    before: Option<i64>,
    /// `ok`, `refused` ou `error` : les seules lignes de cette issue.
    outcome: Option<String>,
}

/// `GET /api/v1/tokens/{id}/calls?before=&outcome=` : [`TokenCalls`], le journal d'activité du
/// jeton, de la plus récente ligne à la plus ancienne, [`CALLS_PER_PAGE`] au plus ; un jeton mort
/// se lit tant qu'il est gardé (Ruling R1220). Le titre d'une page n'est lu que si `access.rs` la
/// montre encore au compte (Ruling R1368). Refus, dans l'ordre : `401`, `409 account_changed` ;
/// `400 invalid_request` pour une adresse qui ne se lit pas, **avant tout `404`** : elle ne dit
/// rien du jeton, qui n'est pas encore cherché ; puis `404 not_found` pour un jeton d'un autre
/// compte, inconnu, mal nommé ou mort depuis quatre-vingt-dix jours (Ruling R1378).
pub(crate) async fn calls(
    State(state): State<AppState>,
    caller: Result<SessionUser, ApiError>,
    path: Result<Path<String>, PathRejection>,
    query: Result<Query<CallsQuery>, QueryRejection>,
) -> Result<Json<TokenCalls>, ApiError> {
    let user = caller?.user;
    let Query(query) = query.map_err(|_| unreadable_query())?;
    let outcome = match query.outcome.as_deref() {
        None => None,
        Some("ok") => Some(CallOutcome::Ok),
        Some("refused") => Some(CallOutcome::Refused),
        Some("error") => Some(CallOutcome::Error),
        Some(_) => return Err(unreadable_query()),
    };
    let id = token_id(path)?;
    let now = now_ms();
    let mut rows = match state
        .store
        .calls_of(user.id, id, query.before, outcome, CALLS_PER_PAGE + 1, now)
        .await
    {
        Ok(rows) => rows,
        Err(StoreError::NotFound) => return Err(ApiError::token_not_found()),
        Err(error) => return Err(internal(error)),
    };
    let more = rows.len() > CALLS_PER_PAGE;
    rows.truncate(CALLS_PER_PAGE);
    let token = state
        .store
        .tokens_of(user.id, now)
        .await
        .map_err(internal)?
        .into_iter()
        .find(|token| token.id == id)
        .ok_or_else(ApiError::token_not_found)?;
    let pages: Vec<Uuid> = rows.iter().filter_map(|row| row.call.node_id).collect();
    let titles = seen_titles(&state, &user, &pages).await?;
    Ok(Json(TokenCalls {
        calls: rows.into_iter().map(|row| call(row, &titles)).collect(),
        more,
        busy_count: token.busy_count,
        purged: token.calls_purged,
    }))
}

/// L'identifiant d'un jeton dans le chemin ; mal écrit, la réponse d'un jeton inconnu.
fn token_id(path: Result<Path<String>, PathRejection>) -> Result<Uuid, ApiError> {
    path.ok()
        .and_then(|Path(id)| Uuid::try_parse(&id).ok())
        .ok_or_else(ApiError::token_not_found)
}

fn unreadable_query() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        ErrorCode::InvalidRequest,
        "l'adresse de l'activité ne prend que « before » (le rang d'une ligne) et « outcome » \
         (« ok », « refused » ou « error »)",
    )
}

/// Le nom d'un jeton, rogné : 1 à [`MAX_TOKEN_NAME_CHARS`] caractères, sans caractère de
/// contrôle (Ruling R1220).
fn token_name(raw: &str) -> Result<String, ApiError> {
    let name = raw.trim();
    let count = name.chars().count();
    if count == 0 || count > MAX_TOKEN_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidName,
            format!(
                "le nom d'un jeton a 1 à {MAX_TOKEN_NAME_CHARS} caractères, sans caractère de \
                 contrôle ; rien n'a été créé"
            ),
        ));
    }
    Ok(name.to_string())
}

/// `read` ou `edit` ; tout autre niveau, `full` compris, est refusé (Ruling R1218).
fn token_level(raw: &str) -> Result<TokenLevel, ApiError> {
    match raw {
        "read" => Ok(TokenLevel::Read),
        "edit" => Ok(TokenLevel::Edit),
        _ => Err(ApiError::invalid_level()),
    }
}

/// L'échéance, en jours : 1 à [`MAX_EXPIRY_DAYS`], ou aucune.
fn expiry(days: Option<i64>) -> Result<Option<i64>, ApiError> {
    match days {
        None => Ok(None),
        Some(days) if (1..=MAX_EXPIRY_DAYS).contains(&days) => Ok(Some(days)),
        Some(_) => Err(ApiError::invalid_expiry(MAX_EXPIRY_DAYS)),
    }
}

/// La portée demandée, telle que le corps l'écrit : sa forme est jugée ici, sans rien lire.
enum Asked {
    All,
    Space(String),
    Pages(String, Vec<String>),
}

/// La forme de la portée : un espace pour `space` et `pages`, rien pour `all`, 1 à 50 pages pour
/// `pages` seulement (Ruling R1219).
fn asked_scope(body: &CreateToken) -> Result<Asked, ApiError> {
    let space = || {
        body.space_id.clone().ok_or_else(|| {
            ApiError::invalid_scope(format!(
                "une portée « {} » nomme son espace (spaceId) ; rien n'a été créé",
                body.scope
            ))
        })
    };
    match body.scope.as_str() {
        "all" if body.space_id.is_none() && body.page_ids.is_none() => Ok(Asked::All),
        "all" => Err(ApiError::invalid_scope(
            "une portée « all » ne nomme ni espace ni page : retire spaceId et pageIds ; rien \
             n'a été créé",
        )),
        "space" if body.page_ids.is_some() => Err(ApiError::invalid_scope(
            "une portée « space » ne nomme pas de pages : retire pageIds, ou prends la portée \
             « pages » ; rien n'a été créé",
        )),
        "space" => Ok(Asked::Space(space()?)),
        "pages" => {
            let space = space()?;
            let mut pages = body.page_ids.clone().unwrap_or_default();
            pages.sort_unstable();
            pages.dedup();
            if pages.is_empty() || pages.len() > mementum_store::MAX_TOKEN_PAGES {
                return Err(ApiError::invalid_scope(format!(
                    "une portée « pages » nomme de 1 à {} pages d'un même espace (pageIds) ; rien \
                     n'a été créé",
                    mementum_store::MAX_TOKEN_PAGES
                )));
            }
            Ok(Asked::Pages(space, pages))
        }
        _ => Err(ApiError::invalid_scope(
            "la portée d'un jeton est « all », « space » ou « pages » ; rien n'a été créé",
        )),
    }
}

/// La portée, jugée par `access.rs` au nom du compte : un espace où il a un rôle (invité
/// compris), des pages de cet espace qu'il voit, hors corbeille. Tout le reste reçoit la même
/// réponse, [`ApiError::unseen_scope`], identifiant mal écrit compris (Ruling R1367).
async fn seen_scope(
    state: &AppState,
    user: &AuthUser,
    asked: Asked,
) -> Result<TokenScope, ApiError> {
    let space_of = |raw: &str| Uuid::try_parse(raw).map_err(|_| ApiError::unseen_scope());
    match asked {
        Asked::All => Ok(TokenScope::All),
        Asked::Space(raw) => {
            let space = space_of(&raw)?;
            state
                .find_space_access(user, space)
                .await?
                .ok_or_else(ApiError::unseen_scope)?;
            Ok(TokenScope::Space(space))
        }
        Asked::Pages(raw, raw_pages) => {
            let space = space_of(&raw)?;
            let access = state
                .find_space_access(user, space)
                .await?
                .ok_or_else(ApiError::unseen_scope)?;
            let mut pages = Vec::with_capacity(raw_pages.len());
            for raw in raw_pages {
                let page = Uuid::try_parse(&raw).map_err(|_| ApiError::unseen_scope())?;
                match access.node(page) {
                    Some(seen) if !seen.in_trash => pages.push(page),
                    _ => return Err(ApiError::unseen_scope()),
                }
            }
            pages.sort_unstable();
            pages.dedup();
            Ok(TokenScope::Pages { space, pages })
        }
    }
}

/// Les titres des pages de `pages` que `access.rs` montre encore au compte, corbeille comprise :
/// l'espace de chacune est lu sans son titre (`Store::node_spaces`), le compte jugé dans chacun
/// de ces espaces, puis seuls les titres des pages qu'il voit sont demandés (Ruling R1368) ; une
/// page qu'il ne voit plus, ou partie, n'en a pas.
async fn seen_titles(
    state: &AppState,
    user: &AuthUser,
    pages: &[Uuid],
) -> Result<HashMap<Uuid, String>, ApiError> {
    if pages.is_empty() {
        return Ok(HashMap::new());
    }
    let mut pages = pages.to_vec();
    pages.sort_unstable();
    pages.dedup();
    let spaces = state.store.node_spaces(&pages).await.map_err(internal)?;
    let mut by_space: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for (page, space) in spaces {
        by_space.entry(space).or_default().push(page);
    }
    let mut seen = Vec::new();
    for (space, pages) in by_space {
        let Some(access) = state.find_space_access(user, space).await? else {
            continue;
        };
        seen.extend(
            pages
                .into_iter()
                .filter(|page| access.node(*page).is_some()),
        );
    }
    state.store.node_titles(&seen).await.map_err(internal)
}

/// Ce que la liste des jetons nomme : les espaces où le compte a un rôle, et les titres des pages
/// des portées qu'il voit encore.
struct Named {
    spaces: HashMap<Uuid, String>,
    titles: HashMap<Uuid, String>,
}

impl Named {
    async fn read(state: &AppState, user: &AuthUser, rows: &[TokenRow]) -> Result<Named, ApiError> {
        let mut spaces = HashMap::new();
        if rows.iter().any(|row| row.scope != TokenScope::All) {
            spaces = state
                .store
                .spaces_of(user.id)
                .await
                .map_err(internal)?
                .into_iter()
                .map(|(space, _)| (space.id, space.name))
                .collect();
        }
        let pages: Vec<Uuid> = rows
            .iter()
            .flat_map(|row| match &row.scope {
                TokenScope::Pages { pages, .. } => pages.clone(),
                _ => Vec::new(),
            })
            .collect();
        let titles = seen_titles(state, user, &pages).await?;
        Ok(Named { spaces, titles })
    }

    fn space(&self, id: Uuid) -> TokenSpace {
        TokenSpace {
            id: id.to_string(),
            name: self.spaces.get(&id).cloned(),
        }
    }
}

/// Un jeton tel que son compte le voit : jamais son texte ni son hachage.
fn info(row: &TokenRow, named: &Named, now: i64) -> TokenInfo {
    let (scope, space, pages) = match &row.scope {
        TokenScope::All => (types::TokenScope::All, None, Vec::new()),
        TokenScope::Space(space) => (
            types::TokenScope::Space,
            Some(named.space(*space)),
            Vec::new(),
        ),
        TokenScope::Pages { space, pages } => (
            types::TokenScope::Pages,
            Some(named.space(*space)),
            pages
                .iter()
                .map(|page| TokenPage {
                    id: page.to_string(),
                    title: named.titles.get(page).cloned(),
                })
                .collect(),
        ),
    };
    TokenInfo {
        id: row.id.to_string(),
        name: row.name.clone(),
        hint: row.hint.clone(),
        level: match row.level {
            TokenLevel::Read => types::TokenLevel::Read,
            TokenLevel::Edit => types::TokenLevel::Edit,
        },
        scope,
        space,
        pages,
        can_trash: row.can_trash,
        created_at: row.created_at,
        expires_at: row.expires_at,
        last_used_at: row.last_used_at,
        revoked_at: row.revoked_at,
        // La règle de `live!` du store : ni révoqué, ni échu à son échéance même.
        live: row.revoked_at.is_none() && row.expires_at.is_none_or(|at| now < at),
        busy_count: row.busy_count,
        calls_purged: row.calls_purged,
    }
}

/// Une ligne du journal, le titre de sa page s'il est dans `titles`.
fn call(row: CallRow, titles: &HashMap<Uuid, String>) -> TokenCall {
    let call = row.call;
    TokenCall {
        id: row.id,
        at: call.at,
        surface: match call.surface {
            CallSurface::Mcp => TokenCallSurface::Mcp,
            CallSurface::Rest => TokenCallSurface::Rest,
        },
        tool: call.tool,
        space_id: call.space_id.map(|space| space.to_string()),
        page_id: call.node_id.map(|page| page.to_string()),
        page_title: call.node_id.and_then(|page| titles.get(&page).cloned()),
        outcome: match call.outcome {
            CallOutcome::Ok => TokenCallOutcome::Ok,
            CallOutcome::Refused => TokenCallOutcome::Refused,
            CallOutcome::Error => TokenCallOutcome::Error,
        },
        code: call.code,
        revision_before: call.revision_before,
        revision_after: call.revision_after,
        blocks_touched: call.blocks_touched,
    }
}
