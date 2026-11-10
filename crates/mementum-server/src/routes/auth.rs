//! Routes des comptes : inscription, connexion, déconnexion, changement de
//! mot de passe, et `GET /api/v1/me`.
//!
//! Ce que ces routes garantissent (détails dans `auth.rs`) :
//! - on s'inscrit si aucun compte n'existe, si les inscriptions sont ouvertes,
//!   ou avec le jeton d'une invitation valide, acceptée dans la foulée ; c'est
//!   le jeton qui fait foi, pas l'adresse (Ruling R47) ;
//! - une adresse inconnue, un mot de passe faux et un compte sans mot de passe
//!   reçoivent la même réponse, après la même vérification Argon2 ;
//! - rien de secret ne sort : ni mot de passe, ni hachage, ni jeton dans une
//!   réponse (le jeton ne voyage que dans `Set-Cookie`) ou dans un journal.
//!   Une adresse refusée à la connexion n'est pas journalisée non plus : on y
//!   tape parfois son mot de passe ;
//! - aucune de leurs réponses, réussite ou refus, ne se garde dans un cache
//!   (`Cache-Control: no-store`) ;
//! - une connexion de synchro ne survit pas à la session de son compte : la
//!   déconnexion et le changement de mot de passe ferment celles du compte,
//!   après leur écriture en base (`Hub::kick_user`, Ruling R73).
//!
//! Le compte et ce qui vient avec lui (espaces repris, invitation, premier
//! espace) s'écrivent en une transaction du store (`Store::sign_up`) : une
//! panne au milieu ne laisse pas un compte sans espace. Ce qui suit une
//! écriture et ne doit pas s'arrêter en route (la session, après le compte ou
//! le nouveau mot de passe) tourne dans sa propre tâche : elle va au bout
//! même si le client coupe la connexion.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use mementum_store::{NewUser, StoreError};
use uuid::Uuid;

use crate::auth::{self, AuthUser, Guarded, SESSION_TTL_MS};
use crate::error::ApiError;
use crate::routes::invitations;
use crate::types::{ChangePassword, ErrorCode, LogIn, Me, SignUp, SignupMode, SpaceInfo, UserInfo};
use crate::{AppState, now_ms, to_completion};

/// `POST /api/v1/auth/signup` : `201` et le compte, session ouverte.
pub(crate) async fn signup(
    State(state): State<AppState>,
    body: Result<Json<SignUp>, JsonRejection>,
) -> Response {
    no_store(sign_up(state, body).await)
}

async fn sign_up(
    state: AppState,
    body: Result<Json<SignUp>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;
    let email = auth::normalize_email(&body.email);
    if !auth::email_is_valid(&email) {
        return Err(ApiError::invalid_email());
    }
    let name = auth::account_name(&body.name)?;
    auth::check_password(&body.password)?;
    // Un jeton donné doit valoir, même là où l'on s'inscrit sans : la
    // personne vient pour une invitation, pas pour un compte sans elle.
    let invitation = match body.invitation.as_deref() {
        Some(token) => Some(auth::token_hash(token).ok_or_else(ApiError::invalid_invitation)?),
        None => None,
    };
    let secure = state.secure_cookies;
    let (me, token) =
        to_completion(register(state, email, name, body.password, invitation)).await?;
    Ok((
        StatusCode::CREATED,
        [(header::SET_COOKIE, auth::session_cookie(&token, secure))],
        Json(me),
    )
        .into_response())
}

/// Crée le compte, lui donne son espace et ouvre sa session. Rend le `Me` et
/// le jeton de session.
async fn register(
    state: AppState,
    email: String,
    name: String,
    password: String,
    invitation: Option<[u8; 32]>,
) -> Result<(Me, String), ApiError> {
    // Une inscription à la fois : entre la vérification du droit de
    // s'inscrire et la création du compte, aucune autre ne passe. Deux
    // « premiers comptes » ne se croisent pas, et un jeton d'invitation ne
    // crée pas deux comptes.
    let _turn = state.signups.lock().await;
    if let Some(hash) = invitation {
        // La consultation par laquelle passe toute acceptation (Ruling R69).
        invitations::valid_invitation(&state, hash)
            .await?
            .ok_or_else(ApiError::invalid_invitation)?;
    } else if state.signup != SignupMode::Open
        && state.store.count_users().await.map_err(internal)? > 0
    {
        return Err(ApiError::signup_closed());
    }
    // Avant le hachage : une adresse prise ne coûte pas un calcul Argon2.
    // `sign_up` le revérifie.
    if state
        .store
        .user_by_email(&email)
        .await
        .map_err(internal)?
        .is_some()
    {
        return Err(ApiError::email_taken());
    }
    let password_hash = auth::hash_password(password).await.map_err(internal)?;
    // Le compte, les espaces que reprend le premier compte, l'invitation,
    // sinon un espace à lui : tout, ou rien.
    let first_space = format!("Espace de {name}");
    let new_user = NewUser {
        email,
        name,
        password_hash,
    };
    let signed_up = state
        .store
        .sign_up(new_user, invitation, &first_space, now_ms())
        .await
        .map_err(|e| match e {
            // Prise entre-temps.
            StoreError::EmailTaken => ApiError::email_taken(),
            e => internal(e),
        })?;
    let user = signed_up.user;
    tracing::info!(user_id = %user.id, instance_admin = user.instance_admin, "compte créé");
    if !signed_up.adopted.is_empty() {
        tracing::info!(
            user_id = %user.id,
            spaces = signed_up.adopted.len(),
            "espaces sans membre repris par le premier compte"
        );
    }
    if let Some(invitation) = &signed_up.accepted {
        tracing::info!(
            invitation_id = %invitation.id,
            space_id = %invitation.space_id,
            user_id = %user.id,
            "invitation acceptée"
        );
    }
    if signed_up.invitation_withdrawn {
        // Révoquée, échue ou prise par un compte déjà inscrit depuis la
        // vérification, quelques millisecondes plus tôt : le compte existe,
        // sans elle, donc sans aucun droit.
        tracing::warn!(
            user_id = %user.id,
            "invitation retirée pendant l'inscription : compte créé sans elle"
        );
    }
    let token = open_session(&state, user.id).await?;
    let me = me_of(&state, user.into()).await?;
    Ok((me, token))
}

/// `POST /api/v1/auth/login` : `200` et le compte, session ouverte. La
/// session que portait déjà le navigateur est fermée.
pub(crate) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<LogIn>, JsonRejection>,
) -> Response {
    no_store(log_in(state, headers, body).await)
}

async fn log_in(
    state: AppState,
    headers: HeaderMap,
    body: Result<Json<LogIn>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;
    let email = auth::normalize_email(&body.email);
    // Le compte d'abord : le frein ne garde pas de la même façon la fenêtre
    // d'une adresse qui en a un (Ruling R81). Cette lecture a lieu dans tous
    // les cas, comme la vérification Argon2 plus bas : ni la réponse ni son
    // délai ne disent si le compte existe.
    let user = state.store.user_by_email(&email).await.map_err(internal)?;
    attempt(&state, Guarded::Login, &email, user.as_ref().map(|u| u.id))
        .map_err(ApiError::too_many_attempts)?;
    let hash = user.as_ref().and_then(|u| u.password_hash.clone());
    // Toujours une vérification Argon2, qu'il y ait un compte ou non.
    let verified = auth::verify_password(body.password, hash)
        .await
        .map_err(internal)?;
    let user = match user {
        Some(user) if verified => user,
        // Le journal dit pourquoi ; la réponse, non. Pour un compte, le frein
        // borne ces lignes (dix par fenêtre) ; pour une adresse sans compte,
        // rien ne les borne : elles ne sortent qu'au niveau `debug`.
        refused => {
            match refused {
                Some(user) => tracing::info!(
                    user_id = %user.id,
                    "connexion refusée : mot de passe invalide"
                ),
                None => tracing::debug!("connexion refusée : aucun compte pour cette adresse"),
            }
            return Err(ApiError::invalid_credentials());
        }
    };
    state.logins.succeeded(Guarded::Login, &email);
    // Le cookie que portait ce navigateur va être remplacé : sa session ne
    // doit plus rien valoir, pour personne.
    if let Some(carried) = auth::session_hash(&headers) {
        state
            .store
            .delete_session(carried)
            .await
            .map_err(internal)?;
    }
    let token = open_session(&state, user.id).await?;
    tracing::info!(user_id = %user.id, "session ouverte");
    let me = me_of(&state, user.into()).await?;
    Ok((
        [(
            header::SET_COOKIE,
            auth::session_cookie(&token, state.secure_cookies),
        )],
        Json(me),
    )
        .into_response())
}

/// `POST /api/v1/auth/logout` : `204`, session supprimée et cookie effacé.
/// Sans session : pareil.
///
/// Les connexions de synchro du compte sont fermées ensuite (Ruling R73) :
/// toutes, celles de ses autres appareils comprises, qui reviennent aussitôt
/// avec leur propre session.
pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: Result<AuthUser, ApiError>,
) -> Response {
    // Sans session valide, il n'y a pas de compte dont fermer les connexions.
    no_store(log_out(state, headers, user.ok()).await)
}

async fn log_out(
    state: AppState,
    headers: HeaderMap,
    user: Option<AuthUser>,
) -> Result<Response, ApiError> {
    let secure = state.secure_cookies;
    if let Some(hash) = auth::session_hash(&headers) {
        // La session, puis les connexions de synchro du compte : jusqu'au
        // bout, même si le client coupe la connexion.
        to_completion(async move {
            state.store.delete_session(hash).await.map_err(internal)?;
            if let Some(user) = user {
                state.hub.kick_user(user.id).await;
            }
            Ok(())
        })
        .await?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::removal_cookie(secure))],
    )
        .into_response())
}

/// `POST /api/v1/auth/password` : `204`. Toutes les sessions du compte sont
/// supprimées, et celle qui a fait la demande repart avec un jeton neuf (le
/// cookie de la réponse). Un mot de passe actuel faux, ou un compte qui n'en
/// a pas : `401 invalid_credentials`. Ces refus ont leur propre frein, à part
/// de celui de la connexion (Ruling R76) : `429 too_many_attempts`.
///
/// Les connexions de synchro du compte sont fermées avec ses sessions
/// (Ruling R73) : qui change son mot de passe pour chasser un intrus ferme
/// aussi ce que l'intrus avait ouvert. La commande `mementum user
/// set-password`, elle, tourne dans un autre processus : elle supprime les
/// sessions, mais ne peut pas fermer les connexions de synchro déjà ouvertes
/// sur le serveur. Elles vivent jusqu'à leur prochaine coupure (réseau,
/// droits de la page qui changent, redémarrage du serveur), et ne reviennent
/// pas alors, faute de session.
pub(crate) async fn change_password(
    State(state): State<AppState>,
    user: Result<AuthUser, ApiError>,
    body: Result<Json<ChangePassword>, JsonRejection>,
) -> Response {
    no_store(new_password(state, user, body).await)
}

async fn new_password(
    state: AppState,
    user: Result<AuthUser, ApiError>,
    body: Result<Json<ChangePassword>, JsonRejection>,
) -> Result<Response, ApiError> {
    let user = user?;
    let Json(body) = body?;
    auth::check_password(&body.new)?;
    // Sans ce frein, une session volée essaierait des mots de passe actuels
    // sans limite. Il porte toujours sur un compte : celui de la session.
    if let Err(retry_after) = attempt(&state, Guarded::PasswordChange, &user.email, Some(user.id)) {
        return Ok(too_many_wrong_passwords(retry_after));
    }
    let account = state
        .store
        .get_user(user.id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;
    let verified = auth::verify_password(body.current, account.password_hash)
        .await
        .map_err(internal)?;
    if !verified {
        tracing::info!(
            user_id = %user.id,
            "changement de mot de passe refusé : mot de passe actuel invalide"
        );
        // Le code de la connexion, mais pas son message : ici, personne n'a
        // tapé d'adresse.
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::InvalidCredentials,
            "le mot de passe actuel est incorrect",
        ));
    }
    state.logins.succeeded(Guarded::PasswordChange, &user.email);
    let password_hash = auth::hash_password(body.new).await.map_err(internal)?;
    let secure = state.secure_cookies;
    let token = to_completion(async move {
        match state.store.set_password(user.id, &password_hash).await {
            Ok(()) => {}
            Err(StoreError::NotFound) => return Err(ApiError::unauthenticated()),
            Err(e) => return Err(internal(e)),
        }
        tracing::info!(user_id = %user.id, "mot de passe changé : autres sessions fermées");
        // Les sessions du compte n'existent plus : ses connexions de synchro
        // ne leur survivent pas.
        state.hub.kick_user(user.id).await;
        open_session(&state, user.id).await
    })
    .await?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, auth::session_cookie(&token, secure))],
    )
        .into_response())
}

/// `GET /api/v1/me` : le compte de la session et ses espaces.
pub(crate) async fn me(
    State(state): State<AppState>,
    user: Result<AuthUser, ApiError>,
) -> Response {
    let me = async {
        me_of(&state, user?)
            .await
            .map(|me| Json(me).into_response())
    };
    no_store(me.await)
}

/// La réponse, réussite ou refus, avec `Cache-Control: no-store` : ce que
/// disent ces routes (un compte, ses espaces, un refus daté) ne doit rester
/// dans aucun cache, ni celui du navigateur, ni celui d'un mandataire.
fn no_store(result: Result<Response, ApiError>) -> Response {
    let mut response = result.unwrap_or_else(IntoResponse::into_response);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Compte une tentative pour cette adresse, ou rend les secondes à attendre
/// avant la suivante (`429`). `account` est le compte qui porte l'adresse,
/// s'il y en a un, lu par l'appelant : la fenêtre d'un compte n'est jamais
/// oubliée avant sa fin, celle d'une adresse sans compte peut l'être (Ruling
/// R81).
///
/// Le premier refus d'une fenêtre est journalisé, sans l'adresse : en
/// avertissement pour un compte, au niveau `debug` sinon (des adresses
/// inventées ne remplissent pas le journal).
fn attempt(state: &AppState, what: Guarded, email: &str, account: Option<Uuid>) -> Result<(), u64> {
    let Err(throttled) = state
        .logins
        .attempt(what, email, account.is_some(), now_ms())
    else {
        return Ok(());
    };
    if throttled.first {
        match account {
            Some(user_id) => tracing::warn!(
                %user_id,
                guarded = ?what,
                retry_after_s = throttled.retry_after,
                "trop d'échecs pour ce compte : tentatives refusées jusqu'à la fin de la fenêtre"
            ),
            None => tracing::debug!(
                retry_after_s = throttled.retry_after,
                "trop d'échecs de connexion pour une adresse sans compte"
            ),
        }
    }
    Err(throttled.retry_after)
}

/// Le `429 too_many_attempts` du changement de mot de passe, avec son
/// `Retry-After`. Le code de la connexion, mais pas son message : ici,
/// personne n'a tapé d'adresse.
fn too_many_wrong_passwords(retry_after: u64) -> Response {
    let mut response = ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::TooManyAttempts,
        format!(
            "trop de mots de passe actuels incorrects : réessayer dans {} min",
            retry_after.div_ceil(60)
        ),
    )
    .into_response();
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from(retry_after));
    response
}

/// Ouvre une session pour ce compte et rend son jeton, à poser en cookie.
async fn open_session(state: &AppState, user_id: Uuid) -> Result<String, ApiError> {
    let (token, hash) = auth::new_token();
    state
        .store
        .create_session(user_id, hash, now_ms() + SESSION_TTL_MS)
        .await
        .map_err(internal)?;
    Ok(token)
}

async fn me_of(state: &AppState, user: AuthUser) -> Result<Me, ApiError> {
    let spaces = state.store.spaces_of(user.id).await.map_err(internal)?;
    Ok(Me {
        user: UserInfo {
            id: user.id.to_string(),
            email: user.email,
            name: user.name,
            instance_admin: user.instance_admin,
        },
        spaces: spaces
            .into_iter()
            .map(|(space, role)| SpaceInfo {
                id: space.id.to_string(),
                name: space.name,
                role: role.into(),
            })
            .collect(),
    })
}

/// Une erreur du store qui ne désigne ici aucune page : `500 internal`.
/// (`From<StoreError>` lirait un `NotFound` comme une page introuvable.)
fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(&error)
}
