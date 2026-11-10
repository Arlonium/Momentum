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
//!   tape parfois son mot de passe.
//!
//! Une suite d'écritures qui ne doit pas s'arrêter au milieu (compte, puis
//! espace, puis session) tourne dans sa propre tâche : elle va au bout même
//! si le client coupe la connexion.

use std::future::Future;

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use mementum_store::{NewUser, StoreError};
use uuid::Uuid;

use crate::auth::{self, AuthUser, SESSION_TTL_MS};
use crate::error::ApiError;
use crate::types::{ChangePassword, LogIn, Me, SignUp, SignupMode, SpaceInfo, UserInfo};
use crate::{AppState, now_ms};

/// `POST /api/v1/auth/signup` : `201` et le compte, session ouverte.
pub(crate) async fn signup(
    State(state): State<AppState>,
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
        state
            .store
            .invitation_by_token(hash, now_ms())
            .await
            .map_err(internal)?
            .ok_or_else(ApiError::invalid_invitation)?;
    } else if state.signup != SignupMode::Open
        && state.store.count_users().await.map_err(internal)? > 0
    {
        return Err(ApiError::signup_closed());
    }
    // Avant le hachage : une adresse prise ne coûte pas un calcul Argon2.
    // `create_user` le revérifie.
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
    let user = state
        .store
        .create_user(NewUser {
            email,
            name,
            password_hash,
        })
        .await
        .map_err(account_error)?;
    tracing::info!(user_id = %user.id, instance_admin = user.instance_admin, "compte créé");

    // Le premier compte reprend les espaces sans membre : ceux d'une base du
    // lot 1.
    if user.instance_admin {
        let adopted = state
            .store
            .adopt_orphan_spaces(user.id)
            .await
            .map_err(internal)?;
        if !adopted.is_empty() {
            tracing::info!(
                user_id = %user.id,
                spaces = adopted.len(),
                "espaces sans membre repris par le premier compte"
            );
        }
    }
    match invitation {
        Some(hash) => match state.store.accept_invitation(hash, user.id, now_ms()).await {
            Ok(_) => {}
            // Révoquée, échue ou prise par un compte déjà inscrit depuis la
            // vérification, quelques millisecondes plus tôt : le compte
            // existe, sans elle, donc sans aucun droit.
            Err(StoreError::NotFound) => tracing::warn!(
                user_id = %user.id,
                "invitation retirée pendant l'inscription : compte créé sans elle"
            ),
            Err(e) => return Err(internal(e)),
        },
        // Sans invitation, chacun a un espace à lui.
        None => {
            let spaces = state.store.spaces_of(user.id).await.map_err(internal)?;
            if spaces.is_empty() {
                state
                    .store
                    .create_space(&format!("Espace de {}", user.name), Some(user.id))
                    .await
                    .map_err(internal)?;
            }
        }
    }
    let token = open_session(&state, user.id).await?;
    let me = me_of(&state, user.into()).await?;
    Ok((me, token))
}

/// `POST /api/v1/auth/login` : `200` et le compte, session ouverte.
pub(crate) async fn login(
    State(state): State<AppState>,
    body: Result<Json<LogIn>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;
    let email = auth::normalize_email(&body.email);
    attempt(&state, &email).await?;
    let user = state.store.user_by_email(&email).await.map_err(internal)?;
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
    state.logins.succeeded(&email);
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
pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(hash) = auth::session_hash(&headers) {
        state.store.delete_session(hash).await.map_err(internal)?;
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            auth::removal_cookie(state.secure_cookies),
        )],
    )
        .into_response())
}

/// `POST /api/v1/auth/password` : `204`. Toutes les sessions du compte sont
/// supprimées, et celle qui a fait la demande repart avec un jeton neuf (le
/// cookie de la réponse). Un mot de passe actuel faux, ou un compte qui n'en
/// a pas : `401 invalid_credentials`, compté par le frein des connexions
/// comme une connexion ratée.
pub(crate) async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    body: Result<Json<ChangePassword>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;
    auth::check_password(&body.new)?;
    // Sans ce frein, une session volée essaierait des mots de passe actuels
    // sans limite.
    attempt(&state, &user.email).await?;
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
        return Err(ApiError::invalid_credentials());
    }
    state.logins.succeeded(&user.email);
    let password_hash = auth::hash_password(body.new).await.map_err(internal)?;
    let secure = state.secure_cookies;
    let token = to_completion(async move {
        match state.store.set_password(user.id, &password_hash).await {
            Ok(()) => {}
            Err(StoreError::NotFound) => return Err(ApiError::unauthenticated()),
            Err(e) => return Err(internal(e)),
        }
        tracing::info!(user_id = %user.id, "mot de passe changé : autres sessions fermées");
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
    user: AuthUser,
) -> Result<Json<Me>, ApiError> {
    me_of(&state, user).await.map(Json)
}

/// Compte une tentative pour cette adresse, ou la refuse (`429`). Le premier
/// refus d'une fenêtre est journalisé, sans l'adresse : en avertissement si
/// elle est celle d'un compte, au niveau `debug` sinon (des adresses
/// inventées ne remplissent pas le journal).
async fn attempt(state: &AppState, email: &str) -> Result<(), ApiError> {
    let Err(throttled) = state.logins.attempt(email, now_ms()) else {
        return Ok(());
    };
    if throttled.first {
        match state.store.user_by_email(email).await.map_err(internal)? {
            Some(user) => tracing::warn!(
                user_id = %user.id,
                retry_after_s = throttled.retry_after,
                "trop d'échecs de connexion pour ce compte : tentatives refusées jusqu'à la \
                 fin de la fenêtre"
            ),
            None => tracing::debug!(
                retry_after_s = throttled.retry_after,
                "trop d'échecs de connexion pour une adresse sans compte"
            ),
        }
    }
    Err(ApiError::too_many_attempts(throttled.retry_after))
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

/// Mène `work` au bout dans sa propre tâche, même si la requête est
/// abandonnée en route (client parti).
async fn to_completion<T: Send + 'static>(
    work: impl Future<Output = Result<T, ApiError>> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::spawn(work).await.map_err(internal)?
}

/// Une erreur du store qui ne désigne ici aucune page : `500 internal`.
/// (`From<StoreError>` lirait un `NotFound` comme une page introuvable.)
fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(&error)
}

/// Erreur de `create_user` : l'adresse a pu être prise entre-temps.
fn account_error(error: StoreError) -> ApiError {
    match error {
        StoreError::EmailTaken => ApiError::email_taken(),
        e => internal(e),
    }
}
