//! Routes des commentaires d'une page (Ruling R134) : des fils attachés à la page, et au plus à
//! un bloc par son identifiant, avec réponses, réactions et résolution. Rien n'est écrit dans le
//! document : qui peut commenter sans pouvoir modifier commente, et le MMD n'en porte rien.
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `GET /api/v1/pages/{id}/comments` | `read` | [`Thread`]`[]`, fils résolus compris, du plus ancien au plus récent |
//! | `POST /api/v1/pages/{id}/comments` ([`NewThread`]) | `comment` | `201`, [`Thread`] ; `422 invalid_body` ; `422 invalid_block` si `blockId` ne nomme aucun bloc de la page |
//! | `POST …/comments/{threadId}/replies` ([`NewComment`]) | `comment` | `201`, [`Comment`] ; `422 invalid_body` |
//! | `PATCH …/comments/{threadId}` ([`ResolveThread`]) | l'avoir ouvert avec `comment`, ou `edit` | [`Thread`] |
//! | `PATCH …/comments/{threadId}/messages/{commentId}` ([`NewComment`]) | `comment`, et en être l'auteur | [`Comment`] ; `422 invalid_body` |
//! | `DELETE …/comments/{threadId}/messages/{commentId}` | en être l'auteur avec `comment`, ou `full` | `204` ; le fil part avec son dernier message |
//! | `PUT`, `DELETE …/messages/{commentId}/reactions/{emoji}` | `comment` | [`Comment`] ; `422 invalid_emoji` |
//!
//! **Refus**, dans cet ordre (Ruling R168 : celui des pages, écrit en tête de `access.rs`, où
//! passent tous les droits, avec les étapes 4 et 5 de plus entre le niveau et la corbeille) :
//! 1. `401` sans session ;
//! 2. `404 not_found` pour une page que la personne ne voit pas, la réponse d'une page qui
//!    n'existe pas, quels que soient le fil, le message ou le corps nommés ;
//! 3. `403 forbidden` sous le niveau de la route (`comment` pour toute écriture) ;
//! 4. `404 not_found` pour un fil qui n'est pas de cette page, puis un message qui n'est pas de
//!    ce fil : la même réponse que l'identifiant existe ailleurs, nulle part, ou qu'il soit mal
//!    formé. Rien ne dit à personne qu'un fil existe sur une page qu'il ne voit pas ;
//! 5. `403 forbidden` si la règle de l'auteur l'interdit (`access::permit_own`) ;
//! 6. `409 trashed` pour une écriture dans une page à la corbeille, dont les fils se lisent ;
//! 7. `422` pour le corps (`invalid_json`, `invalid_body`) ou l'emoji (`invalid_emoji`) ;
//! 8. pour un fil sur un bloc : la page chargée, les droits revérifiés (2, 3 et 6, Ruling
//!    R89), puis `422 invalid_block` si le document vivant n'a pas ce bloc. Un commentaire de
//!    la page ne la charge pas.
//!
//! **Ce qui fait foi est la transaction.** Les droits sont ceux lus juste avant l'écriture (la
//! fenêtre de quelques millisecondes que le projet accepte pour les écritures de page) ; mais
//! « le fil est de cette page », « le message est de ce fil » et « en être l'auteur » sont
//! revérifiés par le store dans la transaction qui écrit (`Authorship`, `StoreError::NotAuthor`).
//! Ce que la route lit avant (`Store::thread_authors`) ne sert qu'à dire les refus dans l'ordre.
//!
//! **Bornes.** Le texte d'un message : de 1 à [`MAX_COMMENT_CHARS`] caractères (des points de
//! code, pas des octets) une fois les espaces de bord retirées, qui ne sont pas gardées ; aucun
//! caractère de contrôle (`char::is_control`) autre que le saut de ligne. Un emoji : de 1 à
//! [`MAX_EMOJI_BYTES`] octets, sans caractère de contrôle ni espace (`char::is_whitespace`).
//!
//! **Plafonds** (Ruling R204), comptés par le store dans la transaction qui écrirait : 2 000 fils
//! par page, résolus compris (`409 thread_limit`), 500 messages par fil (`409 message_limit`), 32
//! emojis différents par message (`409 reaction_limit`, pour un emoji neuf seulement). Ils
//! viennent après tous les refus de droit et le corps.
//!
//! **Lecteurs prévenus.** Après chaque écriture validée qui change quelque chose, les
//! connexions de synchro de la page reçoivent la trame `{"type":"comments"}` (`Hub::page_event`,
//! Ruling R136), lecteurs compris, et relisent par `GET`. Une réaction déjà posée, un fil déjà
//! résolu, un message modifié avec le texte qu'il a déjà : rien ne change, rien n'est envoyé
//! (Ruling R170). L'écriture et la trame vont au bout même si le
//! client coupe la connexion. Une écriture de commentaire n'avance ni `spaces.rev` ni
//! `spaces.lsn` : elle ne change ni les droits ni ce que montre la barre latérale.

use std::future::Future;

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mementum_acl::{Action, NodeAccess};
use mementum_md::Block;
use mementum_store::StoreError;
use mementum_sync::{PageEvent, display_name};
use uuid::Uuid;

use crate::access::{OwnWork, permit, permit_level, permit_own};
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::routes::pages::page_id;
use crate::types::{Author, Comment, NewComment, NewThread, Reaction, ResolveThread, Thread};
use crate::{AppState, to_completion};

/// Longueur maximale du texte d'un message, en caractères Unicode, espaces de bord retirées.
pub const MAX_COMMENT_CHARS: usize = 10_000;

/// Longueur maximale d'un emoji de réaction, en octets (UTF-8).
pub const MAX_EMOJI_BYTES: usize = 16;

/// `GET /api/v1/pages/{id}/comments` : les fils de la page, résolus compris. Une page à la
/// corbeille se lit, ses fils aussi.
pub(crate) async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Vec<Thread>>, ApiError> {
    let id = page_id(path)?;
    state.node_access(&user, id, Action::Read).await?;
    let threads = state.store.comment_threads(id).await?;
    Ok(Json(threads.iter().map(thread).collect()))
}

/// `POST /api/v1/pages/{id}/comments` : ouvre un fil, sur la page ou sur un de ses blocs, avec
/// son premier message.
pub(crate) async fn open(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<NewThread>, JsonRejection>,
) -> Result<(StatusCode, Json<Thread>), ApiError> {
    let id = page_id(path)?;
    // Premier contrôle : les refus, avant que le corps soit lu.
    state.node_access(&user, id, Action::Comment).await?;
    let Json(body) = body?;
    let opened = open_thread(&state, &user, id, body).await?;
    Ok((StatusCode::CREATED, Json(opened)))
}

/// La suite de [`open`], une fois le premier contrôle passé et le corps lu.
///
/// Un fil sur un bloc suit l'ordre de `PUT /mmd` (Ruling R89) : la page est chargée (sa lecture
/// la charge), les droits relus, puis le bloc cherché dans ses blocs, juste avant d'écrire. Un
/// droit retiré pendant le chargement d'une page froide n'écrit donc plus, et n'apprend rien de
/// ses blocs. Sans bloc, la page n'est pas chargée : le fil ne touche pas au document.
async fn open_thread(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    body: NewThread,
) -> Result<Thread, ApiError> {
    let text = comment_body(&body.body)?;
    if let Some(block) = &body.block_id {
        let blocks = state.hub.read_blocks(id).await?;
        state.node_access(user, id, Action::Comment).await?;
        if block.is_empty() || !has_block(&blocks, block) {
            return Err(ApiError::invalid_block());
        }
    }
    let (store, by) = (state.store.clone(), user.id);
    let opened = written(state, id, ApiError::page_not_found, async move {
        let opened = store
            .create_thread(id, body.block_id.as_deref(), by, &text)
            .await?;
        Ok((opened, true))
    })
    .await?;
    Ok(thread(&opened))
}

/// `POST /api/v1/pages/{id}/comments/{threadId}/replies` : ajoute un message au fil, résolu ou
/// non.
pub(crate) async fn reply(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<NewComment>, JsonRejection>,
) -> Result<(StatusCode, Json<Comment>), ApiError> {
    let (id, thread_id) = thread_path(path)?;
    let access = commenter(&state, &user, id).await?;
    let found = located(&state, id, &thread_id, None).await?;
    permit(&access, Action::Comment)?;
    let Json(body) = body?;
    let text = comment_body(&body.body)?;
    let (store, by, thread_id) = (state.store.clone(), user.id, found.thread);
    let replied = written(&state, id, ApiError::thread_not_found, async move {
        let replied = store.reply_to_thread(id, thread_id, by, &text).await?;
        Ok((replied, true))
    })
    .await?;
    Ok((StatusCode::CREATED, Json(comment(&replied))))
}

/// `PATCH /api/v1/pages/{id}/comments/{threadId}` : résout le fil, ou le rouvre.
pub(crate) async fn resolve(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<ResolveThread>, JsonRejection>,
) -> Result<Json<Thread>, ApiError> {
    let (id, thread_id) = thread_path(path)?;
    let access = commenter(&state, &user, id).await?;
    let found = located(&state, id, &thread_id, None).await?;
    let own = found.opened_by == user.id;
    let who = permit_own(&access, OwnWork::ResolveThread, own, user.id)?;
    let Json(body) = body?;
    let (store, by, thread_id) = (state.store.clone(), user.id, found.thread);
    let resolved = written(&state, id, ApiError::thread_not_found, async move {
        store
            .resolve_thread(id, thread_id, by, body.resolved, who)
            .await
    })
    .await?;
    Ok(Json(thread(&resolved)))
}

/// `PATCH /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}` : remplace le texte d'un
/// message, par son auteur.
pub(crate) async fn edit(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String, String)>, PathRejection>,
    body: Result<Json<NewComment>, JsonRejection>,
) -> Result<Json<Comment>, ApiError> {
    let (id, thread_id, comment_id) = message_path(path)?;
    let access = commenter(&state, &user, id).await?;
    let found = located(&state, id, &thread_id, Some(&comment_id)).await?;
    let (message, author) = found.comment()?;
    permit_own(&access, OwnWork::EditMessage, author == user.id, user.id)?;
    let Json(body) = body?;
    let text = comment_body(&body.body)?;
    let (store, by, thread_id) = (state.store.clone(), user.id, found.thread);
    let edited = written(&state, id, ApiError::comment_not_found, async move {
        store.edit_comment(id, thread_id, message, by, &text).await
    })
    .await?;
    Ok(Json(comment(&edited)))
}

/// `DELETE /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}` : supprime un message
/// et ses réactions ; son fil avec lui si c'était le dernier.
pub(crate) async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let (id, thread_id, comment_id) = message_path(path)?;
    let access = commenter(&state, &user, id).await?;
    let found = located(&state, id, &thread_id, Some(&comment_id)).await?;
    let (message, author) = found.comment()?;
    let who = permit_own(&access, OwnWork::DeleteMessage, author == user.id, user.id)?;
    let (store, thread_id) = (state.store.clone(), found.thread);
    written(&state, id, ApiError::comment_not_found, async move {
        store.delete_comment(id, thread_id, message, who).await?;
        Ok(((), true))
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PUT /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}/reactions/{emoji}` : pose
/// la réaction de la personne ; déjà posée, rien ne change.
pub(crate) async fn react(
    state: State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String, String, String)>, PathRejection>,
) -> Result<Json<Comment>, ApiError> {
    reaction(state, user, path, true).await
}

/// `DELETE /api/v1/pages/{id}/comments/{threadId}/messages/{commentId}/reactions/{emoji}` :
/// retire la réaction de la personne, la sienne seulement ; absente, rien ne change.
pub(crate) async fn unreact(
    state: State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String, String, String)>, PathRejection>,
) -> Result<Json<Comment>, ApiError> {
    reaction(state, user, path, false).await
}

async fn reaction(
    State(state): State<AppState>,
    user: AuthUser,
    path: Result<Path<(String, String, String, String)>, PathRejection>,
    on: bool,
) -> Result<Json<Comment>, ApiError> {
    let (id, thread_id, comment_id, raw) = reaction_path(path)?;
    let access = commenter(&state, &user, id).await?;
    let found = located(&state, id, &thread_id, Some(&comment_id)).await?;
    let (message, _) = found.comment()?;
    permit(&access, Action::Comment)?;
    let emoji = emoji(&raw)?.to_string();
    let (store, by, thread_id) = (state.store.clone(), user.id, found.thread);
    let reacted = written(&state, id, ApiError::comment_not_found, async move {
        Ok(if on {
            store
                .add_reaction(id, thread_id, message, by, &emoji)
                .await?
        } else {
            store
                .remove_reaction(id, thread_id, message, by, &emoji)
                .await?
        })
    })
    .await?;
    Ok(Json(comment(&reacted)))
}

/// La page `id` vue par la personne, et ses accès : `404` si elle ne la voit pas, `403` sous
/// `comment`. La corbeille n'est pas encore regardée : d'autres refus passent avant elle.
async fn commenter(state: &AppState, user: &AuthUser, id: Uuid) -> Result<NodeAccess, ApiError> {
    let (_, access, _) = state
        .seen_node(user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    permit_level(&access, Action::Comment)?;
    Ok(access)
}

/// Un fil de la page, et le message demandé de ce fil, trouvés avant d'écrire.
struct Located {
    thread: Uuid,
    opened_by: Uuid,
    /// Le message demandé et son auteur.
    comment: Option<(Uuid, Uuid)>,
}

impl Located {
    /// Le message demandé et son auteur.
    fn comment(&self) -> Result<(Uuid, Uuid), ApiError> {
        self.comment
            .ok_or_else(|| ApiError::internal(&"aucun message n'était demandé"))
    }
}

/// Le fil `thread` de la page, s'il est d'elle, et son message `comment` s'il est demandé :
/// `404` sinon, le fil d'abord, puis le message. Un identifiant mal formé, inconnu ou d'une
/// autre page reçoit la même réponse.
async fn located(
    state: &AppState,
    page: Uuid,
    thread: &str,
    comment: Option<&str>,
) -> Result<Located, ApiError> {
    let thread = Uuid::parse_str(thread).map_err(|_| ApiError::thread_not_found())?;
    let asked = comment.map(|raw| Uuid::parse_str(raw).ok());
    let authors = state
        .store
        .thread_authors(page, thread, asked.flatten())
        .await
        .map_err(crate::error::internal)?
        .ok_or_else(ApiError::thread_not_found)?;
    let comment = asked
        .map(|id| {
            id.zip(authors.comment_author)
                .ok_or_else(ApiError::comment_not_found)
        })
        .transpose()?;
    Ok(Located {
        thread,
        opened_by: authors.opened_by,
        comment,
    })
}

/// Mène l'écriture `write` au bout, même si le client coupe la connexion, puis prévient les
/// connexions de synchro de la page si elle a changé quelque chose : après la validation,
/// jamais avant. `write` rend sa valeur, et si quelque chose a changé.
///
/// `NotFound` du store veut dire que ce que la route a trouvé avant d'écrire a disparu
/// entre-temps (le fil, le message, la page) : c'est `not_found` qui le dit.
async fn written<T: Send + 'static>(
    state: &AppState,
    page: Uuid,
    not_found: fn() -> ApiError,
    write: impl Future<Output = Result<(T, bool), StoreError>> + Send + 'static,
) -> Result<T, ApiError> {
    let hub = state.hub.clone();
    to_completion(async move {
        let (value, changed) = write.await.map_err(|error| match error {
            StoreError::NotFound => not_found(),
            error => error.into(),
        })?;
        if changed {
            hub.page_event(page, PageEvent::Comments);
        }
        Ok(value)
    })
    .await
}

/// Le texte d'un message, sans ses espaces de bord, s'il tient dans les bornes : de 1 à
/// [`MAX_COMMENT_CHARS`] caractères, sans caractère de contrôle autre que le saut de ligne.
fn comment_body(raw: &str) -> Result<String, ApiError> {
    let body = raw.trim();
    if body.is_empty() {
        return Err(ApiError::invalid_body(
            "le message est vide : écris au moins un caractère",
        ));
    }
    if body.chars().count() > MAX_COMMENT_CHARS {
        return Err(ApiError::invalid_body(
            "le message est trop long : 10 000 caractères au plus",
        ));
    }
    if body.chars().any(|c| c.is_control() && c != '\n') {
        return Err(ApiError::invalid_body(
            "le message porte un caractère de contrôle (tabulation, retour chariot…) : seul le \
             saut de ligne est permis",
        ));
    }
    Ok(body.to_string())
}

/// L'emoji d'une réaction, s'il tient dans les bornes : de 1 à [`MAX_EMOJI_BYTES`] octets,
/// sans caractère de contrôle ni espace. Le contrôle passe avant l'espace : une tabulation est
/// les deux.
fn emoji(raw: &str) -> Result<&str, ApiError> {
    if raw.is_empty() || raw.len() > MAX_EMOJI_BYTES {
        return Err(ApiError::invalid_emoji(
            "un emoji a de 1 à 16 octets (en UTF-8)",
        ));
    }
    if raw.chars().any(char::is_control) {
        return Err(ApiError::invalid_emoji(
            "un emoji ne porte aucun caractère de contrôle",
        ));
    }
    if raw.chars().any(char::is_whitespace) {
        return Err(ApiError::invalid_emoji("un emoji ne porte aucune espace"));
    }
    Ok(raw)
}

/// Le bloc `id` est-il parmi `blocks`, à n'importe quelle profondeur ?
fn has_block(blocks: &[Block], id: &str) -> bool {
    let mut stack = vec![blocks.iter()];
    while let Some(level) = stack.last_mut() {
        let Some(block) = level.next() else {
            stack.pop();
            continue;
        };
        if block.id == id {
            return true;
        }
        stack.push(block.children.iter());
    }
    false
}

/// La page et le fil d'un chemin `…/pages/{id}/comments/{threadId}…`. Une page illisible
/// n'existe pas (`404`) ; le fil est cherché ensuite, sur la page.
fn thread_path(
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(Uuid, String), ApiError> {
    let Path((page, thread)) = path.map_err(|_| ApiError::page_not_found())?;
    Ok((parse_page(&page)?, thread))
}

/// La page, le fil et le message d'un chemin `…/{threadId}/messages/{commentId}`.
fn message_path(
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Result<(Uuid, String, String), ApiError> {
    let Path((page, thread, comment)) = path.map_err(|_| ApiError::page_not_found())?;
    Ok((parse_page(&page)?, thread, comment))
}

/// La page, le fil, le message et l'emoji (décodé) d'un chemin `…/reactions/{emoji}`. Un
/// chemin qu'axum ne sait pas décoder (un emoji qui n'est pas de l'UTF-8) reçoit la réponse de
/// tout chemin illisible : celle d'une page introuvable.
fn reaction_path(
    path: Result<Path<(String, String, String, String)>, PathRejection>,
) -> Result<(Uuid, String, String, String), ApiError> {
    let Path((page, thread, comment, emoji)) = path.map_err(|_| ApiError::page_not_found())?;
    Ok((parse_page(&page)?, thread, comment, emoji))
}

fn parse_page(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(raw).map_err(|_| ApiError::page_not_found())
}

fn author(author: &mementum_store::Author) -> Author {
    Author {
        id: author.id.to_string(),
        name: display_name(&author.name),
    }
}

fn comment(comment: &mementum_store::Comment) -> Comment {
    Comment {
        id: comment.id.to_string(),
        author: author(&comment.author),
        body: comment.body.clone(),
        created_at: comment.created_at,
        edited_at: comment.edited_at,
        reactions: comment
            .reactions
            .iter()
            .map(|reaction| Reaction {
                emoji: reaction.emoji.clone(),
                users: reaction.users.iter().map(author).collect(),
            })
            .collect(),
    }
}

fn thread(thread: &mementum_store::Thread) -> Thread {
    Thread {
        id: thread.id.to_string(),
        block_id: thread.block_id.clone(),
        opened_by: author(&thread.opened_by),
        created_at: thread.created_at,
        resolved_at: thread.resolved_at,
        resolved_by: thread.resolved_by.as_ref().map(author),
        comments: thread.comments.iter().map(comment).collect(),
    }
}

#[cfg(test)]
mod tests {
    use mementum_acl::{Grant, Level, Subject};
    use mementum_store::{NewNode, NodeKind};

    use super::*;
    use crate::access::tests::{account, state};

    /// Un commentaire de la page ne la charge pas : son acteur ne démarre pas. Un fil sur un
    /// bloc la charge, pour chercher le bloc dans son document, même quand il n'y est pas.
    #[tokio::test]
    async fn page_comment_does_not_load_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(&dir).await;
        let olga = account(&state, "olga").await;
        let space = state
            .store
            .create_space("Équipe", Some(olga.id))
            .await
            .unwrap();
        let page = state
            .store
            .create_node(NewNode {
                space_id: space.id,
                parent_id: None,
                after: None,
                kind: NodeKind::Page,
                title: "P".to_string(),
                created_by: Some(olga.id),
                grants: vec![Grant {
                    subject: Subject::Members,
                    level: Level::Full,
                }],
                doc: None,
            })
            .await
            .unwrap()
            .id;
        let new = |block_id: Option<&str>| NewThread {
            block_id: block_id.map(str::to_string),
            body: "Un".to_string(),
        };

        let opened = open_thread(&state, &olga, page, new(None)).await.unwrap();
        assert_eq!(opened.block_id, None);
        assert!(state.hub.live_pages().await.is_empty());

        let refused = open_thread(&state, &olga, page, new(Some("inexistant"))).await;
        assert!(refused.is_err());
        assert_eq!(state.hub.live_pages().await, [page]);
        assert_eq!(state.store.comment_threads(page).await.unwrap().len(), 1);
    }

    /// Les bornes du texte se comptent en caractères, espaces de bord retirées ; celles d'un
    /// emoji en octets.
    #[test]
    fn bounds_count_characters_for_a_body_and_bytes_for_an_emoji() {
        let longest = "é".repeat(MAX_COMMENT_CHARS);
        assert_eq!(
            comment_body(&format!(" {longest}\n")).ok().as_deref(),
            Some(longest.as_str())
        );
        assert!(comment_body(&format!("{longest}é")).is_err());
        assert!(comment_body("a\nb").is_ok());
        assert!(comment_body("a\u{1b}b").is_err());
        assert_eq!(emoji("🙂🙂🙂🙂").ok(), Some("🙂🙂🙂🙂"));
        assert!(emoji("🙂🙂🙂🙂a").is_err());
        assert!(emoji("").is_err());
    }
}
