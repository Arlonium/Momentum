//! Les opérations d'édition d'un agent (lot 5, Task S7 ; Rulings R1211, R1228 à R1235) : la route
//! `POST /api/v1/pages/{id}/operations` et l'outil `update_page` (M2) appellent [`update_page`].
//!
//! Dans l'ordre :
//! 1. la forme de l'appel, sans rien lire (le mode, de une à cent opérations, une place par
//!    insertion ou déplacement, la révision exigée par `replace_page`, `replace_block` et
//!    `delete_block`, Ruling R1231, le titre de `set_properties`) ;
//! 2. les MMD des opérations, lus et vérifiés hors des fils du serveur (`mmd_reads.run`, R383 ;
//!    `ops::read_markdown`, puis les props du registre, R41) ;
//! 3. une place dans la file des écritures de la page, sur la part du jeton (R435, R1242) ;
//! 4. sous elle, verrou de la page tenu, mené au bout même si le client coupe : l'appel relu
//!    (session ou jeton, R89, R209), les droits relus, `replace_page` refusé à un jeton tant que
//!    quelqu'un écrit dans la page (R1233), puis `Hub::edit_page` : une commande de l'acteur, deux
//!    avec une version `before_agent` (R1572) ; le jeton relu dans la transaction qui écrit le
//!    journal (R1356) ;
//! 5. la révision écrite et les empreintes de ses blocs gardées (`revisions.rs`), puis le titre
//!    de `set_properties`, s'il y en a un.
//!
//! **Atomique pour le contenu** : toutes les opérations de contenu, ou aucune (le plan de S3). Le
//! titre n'est pas dans le document : il s'écrit après le contenu, et une page supprimée ou jetée
//! entre les deux garde le contenu écrit (Ruling R1390).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mementum_acl::Action;
use mementum_crdt::anchors::Anchors;
use mementum_crdt::ops::{self, Operation, Place, Reason};
use mementum_mcp::tools::{self as mcp, Mode, Position};
use mementum_store::{CallOutcome, Writer};
use mementum_sync::{EditRefusal, Edited, OnConflict};
use uuid::Uuid;

use crate::agent::fetch::{DEFAULT_LIMIT, MAX_LIMIT, slice, touched_mmd};
use crate::auth::{CallDetails, LiveCaller};
use crate::error::ApiError;
use crate::routes::pages::{
    MAX_MMD_BLOCKS, registered_props_only, too_long_to_write, unknown_not_foreign,
};
use crate::types::{ErrorCode, PageOperations, UpdatePage, UpdateResult};
use crate::{AppState, to_completion};

/// Opérations d'un appel, au plus (Ruling R1229 ; `mementum_mcp::tools::MAX_OPERATIONS`).
pub const MAX_OPERATIONS: usize = mcp::MAX_OPERATIONS;

/// Une écriture d'agent faite : la réponse, et ce que le journal d'activité en garde.
pub(crate) struct Updated {
    pub(crate) result: UpdateResult,
    pub(crate) details: CallDetails,
}

/// Une écriture d'agent refusée : la réponse, et, quand le contenu a été écrit avant le refus (le
/// titre de `set_properties`), ce que le journal d'activité en garde (R1404).
pub(crate) struct UpdateRefused {
    pub(crate) error: ApiError,
    /// En boîte : un `Result<_, UpdateRefused>` reste sous la borne de `clippy::result_large_err`.
    pub(crate) details: Option<Box<CallDetails>>,
}

impl From<ApiError> for UpdateRefused {
    fn from(error: ApiError) -> UpdateRefused {
        UpdateRefused {
            error,
            details: None,
        }
    }
}

impl From<mementum_sync::SyncError> for UpdateRefused {
    fn from(error: mementum_sync::SyncError) -> UpdateRefused {
        ApiError::from(error).into()
    }
}

impl From<mementum_store::StoreError> for UpdateRefused {
    fn from(error: mementum_store::StoreError) -> UpdateRefused {
        ApiError::from(error).into()
    }
}

/// `POST /api/v1/pages/{id}/operations`, et l'outil `update_page` : les opérations de `request`
/// sur la page `id`, toutes ou aucune : `404`, `403`, `409 trashed` d'abord (`edit`), puis la
/// forme de l'appel, et ainsi de suite (voir le module).
pub(crate) async fn update_page(
    state: &AppState,
    caller: &LiveCaller,
    id: Uuid,
    request: PageOperations,
) -> Result<Updated, UpdateRefused> {
    let user = &caller.user;
    // Les refus de la page d'abord (la route les a déjà dits avant de lire le corps ; un outil
    // arrive ici avec ses arguments lus).
    state.node_access(user, id, Action::Edit).await?;
    let checked = check(&request)?;
    let token = user.token().map(|grant| grant.token_id);
    // Les MMD des opérations, lus et vérifiés hors des fils du serveur, à leur tour (R383).
    let operations = request.operations;
    let (ops, title) = state
        .mmd_reads
        .run(user.party(), move || read_operations(operations))
        .await??;
    // Une place dans la file des écritures de la page, sur la part de l'appel (R435, R1242).
    let place = state
        .page_writes
        .reserve(id, user.party())
        .ok_or_else(ApiError::page_busy)?;
    let write = place.lock().await;
    let state = state.clone();
    let caller = caller.clone();
    let base_revision = request.base_revision;
    let on_conflict = match request.on_conflict {
        Some(mcp::OnConflict::Fail) => OnConflict::Fail,
        _ => OnConflict::Rebase,
    };
    to_completion(async move {
        Ok(write_under_the_lock(
            state,
            caller,
            write,
            id,
            ops,
            title,
            checked,
            base_revision,
            on_conflict,
            token,
        )
        .await)
    })
    .await?
}

/// La suite d'`update_page`, verrou de la page tenu, menée au bout même si le client coupe.
#[allow(clippy::too_many_arguments)]
async fn write_under_the_lock(
    state: AppState,
    caller: LiveCaller,
    write: crate::page_writes::PageWrite,
    id: Uuid,
    ops: Vec<Operation>,
    title: Option<String>,
    checked: Checked,
    base_revision: Option<String>,
    on_conflict: OnConflict,
    token: Option<Uuid>,
) -> Result<Updated, UpdateRefused> {
    {
        let _write = write;
        let LiveCaller { user, proof } = caller;
        // La page chargée, l'appel et les droits sont relus : c'est ce second contrôle qui
        // autorise l'écriture (R89, R209).
        state.hub.load_page(id).await?;
        state.caller_still_live(proof).await?;
        state.node_access(&user, id, Action::Edit).await?;
        if ops.is_empty() {
            // Seul le titre change.
            let read = state.hub.read_page(id).await?;
            state
                .revisions
                .served(id, &read.revision, read.hashes.clone());
            if let Some(title) = title {
                rename(&state, &user, id, title).await?;
            }
            return Ok(Updated {
                details: details(&read.revision, &read.revision, 0),
                result: UpdateResult {
                    revision: read.revision,
                    mmd: String::new(),
                    truncated: false,
                    blocks_touched: 0,
                    warnings: Vec::new(),
                },
            });
        }
        let commented = state.store.commented_blocks(id).await?;
        let base = base_revision.map(|revision| {
            let hashes = state.revisions.hashes(id, &revision);
            (revision, hashes)
        });
        let edit = mementum_sync::Edit {
            ops,
            base,
            on_conflict,
            writer: Writer {
                account: Some(user.id),
                token,
            },
            commented: Some(commented),
            // Ruling R1233 : un jeton ne récrit pas toute une page où quelqu'un écrit ; jugé par
            // l'acteur dans chaque commande qui décide d'écrire (tour de correction de S7).
            alone: checked.replaces_page && token.is_some(),
        };
        // La réponse, ses alias et son MMD se bâtissent hors des fils du serveur (R863, R1401).
        let party = user.party();
        let edited = match state.hub.edit_page(id, edit).await {
            Ok(edited) => edited,
            Err(refusal) => {
                let error = state
                    .mmd_reads
                    .run(party, move || refused(refusal, &checked))
                    .await?;
                return Err(error.into());
            }
        };
        state
            .revisions
            .served(id, &edited.revision, edited.hashes.clone());
        let updated = state.mmd_reads.run(party, move || written(edited)).await?;
        if let Some(title) = title {
            // Lot 5, tour de correction de S7 (R1404) : le contenu est écrit ; un titre refusé
            // ensuite le dit, avec la révision écrite, et le journal d'activité la garde.
            if let Err(error) = rename(&state, &user, id, title).await {
                return Err(UpdateRefused {
                    error: error
                        .prefixed("le contenu est écrit ; seul le titre ne l'est pas : ")
                        .with("revision", &updated.result.revision),
                    // L'issue et le code, la porte les lit du refus ; les révisions et les blocs
                    // touchés sont ceux de l'écriture faite.
                    details: Some(Box::new(CallDetails {
                        outcome: None,
                        ..updated.details
                    })),
                });
            }
        }
        Ok(updated)
    }
}

/// La réponse d'une écriture faite.
fn written(edited: Edited) -> Updated {
    let (mmd, truncated) = touched_mmd(&edited.blocks, &edited.touched, MAX_LIMIT);
    let touched = edited.touched.len();
    Updated {
        details: details(&edited.before, &edited.revision, touched),
        result: UpdateResult {
            revision: edited.revision,
            mmd,
            truncated,
            blocks_touched: touched,
            warnings: Vec::new(),
        },
    }
}

/// Ce que le journal d'activité garde d'une écriture (Ruling R1245) : jamais son contenu.
fn details(before: &str, after: &str, touched: usize) -> CallDetails {
    CallDetails {
        outcome: Some(CallOutcome::Ok),
        revision_before: Some(before.to_string()),
        revision_after: Some(after.to_string()),
        blocks_touched: Some(i64::try_from(touched).unwrap_or(i64::MAX)),
        ..CallDetails::default()
    }
}

/// Le titre de `set_properties`, écrit comme `PATCH /api/v1/pages/{id}` l'écrit (ses règles, son
/// droit, la corbeille).
async fn rename(
    state: &AppState,
    user: &crate::auth::AuthUser,
    id: Uuid,
    title: String,
) -> Result<(), ApiError> {
    crate::routes::pages::update(
        State(state.clone()),
        user.clone(),
        Ok(Path(id.to_string())),
        Ok(Json(UpdatePage {
            title: Some(title),
            parent_id: None,
            after_id: None,
        })),
    )
    .await
    .map(|_| ())
}

/// Ce que la forme de l'appel dit, une fois jugée.
#[derive(Debug, Clone, Copy)]
struct Checked {
    /// L'appel porte un `replace_page` (Ruling R1233).
    replaces_page: bool,
    /// Le rang de la première opération qui exige une révision, s'il y en a une.
    first_destructive: Option<usize>,
}

/// La forme de l'appel, sans rien lire (voir le module).
fn check(request: &PageOperations) -> Result<Checked, ApiError> {
    match request.mode {
        None | Some(Mode::Direct) => {}
        Some(Mode::Suggest | Mode::Branch) => return Err(ApiError::mode_unavailable()),
    }
    let ops = &request.operations;
    if ops.is_empty() || ops.len() > MAX_OPERATIONS {
        return Err(ApiError::too_many_operations(MAX_OPERATIONS));
    }
    if let Some(revision) = &request.base_revision
        && (revision.is_empty() || revision.chars().count() > mcp::MAX_TOKEN_CHARS)
    {
        return Err(invalid(
            None,
            "« baseRevision » : la révision rendue par la dernière lecture",
        ));
    }
    let mut first_destructive = None;
    let mut replaces_page = false;
    let mut titles = 0;
    for (i, op) in ops.iter().enumerate() {
        let anchor = |field: &str, value: Option<&String>| -> Result<(), ApiError> {
            match value {
                Some(value)
                    if value.is_empty() || value.chars().count() > mcp::MAX_ANCHOR_CHARS =>
                {
                    Err(invalid(
                        Some(i),
                        &format!(
                            "« {field} » : une ancre de 1 à {} caractères",
                            mcp::MAX_ANCHOR_CHARS
                        ),
                    ))
                }
                _ => Ok(()),
            }
        };
        match op {
            mcp::Operation::StrReplace {
                old, anchor: at, ..
            } => {
                if old.is_empty() || old.chars().count() > mcp::MAX_OLD_CHARS {
                    return Err(invalid(
                        Some(i),
                        &format!("« old » : de 1 à {} caractères", mcp::MAX_OLD_CHARS),
                    ));
                }
                anchor("anchor", at.as_ref())?;
            }
            mcp::Operation::Insert {
                after,
                before,
                position,
                ..
            } => {
                one_place(i, after.as_ref(), before.as_ref(), position.as_ref())?;
                anchor("after", after.as_ref())?;
                anchor("before", before.as_ref())?;
            }
            mcp::Operation::ReplaceBlock { anchor: at, .. }
            | mcp::Operation::DeleteBlock { anchor: at, .. } => {
                anchor("anchor", Some(at))?;
                first_destructive.get_or_insert(i);
            }
            mcp::Operation::MoveBlock {
                anchor: at,
                after,
                before,
                position,
            } => {
                anchor("anchor", Some(at))?;
                one_place(i, after.as_ref(), before.as_ref(), position.as_ref())?;
                anchor("after", after.as_ref())?;
                anchor("before", before.as_ref())?;
            }
            mcp::Operation::ReplacePage { .. } => {
                replaces_page = true;
                first_destructive.get_or_insert(i);
            }
            mcp::Operation::SetProperties { properties } => {
                titles += 1;
                if titles > 1 {
                    return Err(invalid(
                        Some(i),
                        "un seul « set_properties » par appel : le titre ne change qu'une fois",
                    ));
                }
                crate::routes::pages::title(Some(&properties.title))
                    .map_err(|e| e.with("index", i))?;
            }
        }
    }
    if let Some(i) = first_destructive
        && request.base_revision.is_none()
    {
        return Err(ApiError::revision_required(i));
    }
    Ok(Checked {
        replaces_page,
        first_destructive,
    })
}

/// Un seul de `after`, `before`, `position`.
fn one_place(
    i: usize,
    after: Option<&String>,
    before: Option<&String>,
    position: Option<&Position>,
) -> Result<(), ApiError> {
    let given = [after.is_some(), before.is_some(), position.is_some()];
    if given.iter().filter(|g| **g).count() != 1 {
        return Err(invalid(
            Some(i),
            "donne un seul de « after », « before » et « position »",
        ));
    }
    Ok(())
}

/// `422 invalid_request`, qui nomme l'opération par son rang.
fn invalid(index: Option<usize>, message: &str) -> ApiError {
    let error = ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::InvalidRequest,
        format!("{message} ; rien n'a été écrit"),
    );
    match index {
        Some(i) => error.with("index", i),
        None => error,
    }
}

/// Les opérations de contenu, leurs MMD lus et vérifiés, et le titre de `set_properties`.
fn read_operations(
    operations: Vec<mcp::Operation>,
) -> Result<(Vec<Operation>, Option<String>), ApiError> {
    let mut out = Vec::with_capacity(operations.len());
    let mut title = None;
    let mut blocks_written = 0usize;
    let read = |i: usize, markdown: &str, out_blocks: &mut usize| {
        let mut blocks = ops::read_markdown(i, markdown).map_err(operation_failed)?;
        registered_props_only(&blocks).map_err(|e| e.with("index", i))?;
        unknown_not_foreign(&mut blocks);
        *out_blocks += count(&blocks);
        if *out_blocks > MAX_MMD_BLOCKS {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::TooManyBlocks,
                format!(
                    "les opérations écrivent plus de {MAX_MMD_BLOCKS} blocs : découpe-les en \
                     plusieurs appels ; rien n'a été écrit"
                ),
            )
            .with("index", i));
        }
        Ok::<_, ApiError>(blocks)
    };
    for (i, op) in operations.into_iter().enumerate() {
        let at = |after: Option<String>, before: Option<String>, position: Option<Position>| match (
            after, before, position,
        ) {
            (Some(a), _, _) => Place::After(a),
            (_, Some(b), _) => Place::Before(b),
            (_, _, Some(Position::Start)) => Place::Start,
            _ => Place::End,
        };
        out.push(match op {
            mcp::Operation::StrReplace {
                old,
                new,
                replace_all,
                anchor,
            } => Operation::StrReplace {
                old,
                new,
                anchor,
                replace_all: replace_all.unwrap_or(false),
            },
            mcp::Operation::Insert {
                after,
                before,
                position,
                markdown,
            } => Operation::Insert {
                at: at(after, before, position),
                blocks: read(i, &markdown, &mut blocks_written)?,
            },
            mcp::Operation::ReplaceBlock {
                anchor,
                markdown,
                allow_deleting_content,
            } => Operation::ReplaceBlock {
                anchor,
                blocks: read(i, &markdown, &mut blocks_written)?,
                allow_deleting_content: allow_deleting_content.unwrap_or(false),
            },
            mcp::Operation::DeleteBlock {
                anchor,
                allow_deleting_content,
            } => Operation::DeleteBlock {
                anchor,
                allow_deleting_content: allow_deleting_content.unwrap_or(false),
            },
            mcp::Operation::MoveBlock {
                anchor,
                after,
                before,
                position,
            } => Operation::MoveBlock {
                anchor,
                to: at(after, before, position),
            },
            mcp::Operation::ReplacePage {
                markdown,
                allow_deleting_content,
            } => Operation::ReplacePage {
                blocks: read(i, &markdown, &mut blocks_written)?,
                allow_deleting_content: allow_deleting_content.unwrap_or(false),
            },
            mcp::Operation::SetProperties { properties } => {
                title = Some(properties.title);
                continue;
            }
        });
    }
    Ok((out, title))
}

/// Blocs d'un arbre, à tous les niveaux, lignes de tableau comprises.
fn count(blocks: &[mementum_md::Block]) -> usize {
    blocks
        .iter()
        .map(|b| {
            1 + count(&b.children)
                + match &b.content {
                    mementum_md::Content::Table(table) => table.rows.len(),
                    _ => 0,
                }
        })
        .sum()
}

/// `422 operation_failed` d'un refus d'opération lu hors de l'acteur (un MMD illisible).
fn operation_failed(refusal: ops::OpRefusal) -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::OperationFailed,
        refusal.message.clone(),
    )
    .with("index", refusal.index)
    .with("reason", refusal.reason.code())
    .with("hint", refusal.reason.hint())
}

/// La réponse d'une écriture d'agent refusée (voir « Pour S7 et M2 » au registre du lot, S4).
fn refused(refusal: EditRefusal, checked: &Checked) -> ApiError {
    match refusal {
        EditRefusal::Operation {
            refusal,
            revision,
            blocks,
        } => {
            let anchors = Anchors::of(&blocks);
            let aliases: Vec<String> = refusal
                .blocks
                .iter()
                .filter_map(|id| anchors.alias(id))
                .collect();
            let index = refusal.index;
            match refusal.reason {
                Reason::ExternalMedia => {
                    ApiError::external_media(refusal.message).with("index", index)
                }
                Reason::RevisionConflict => {
                    let (mmd, truncated) = touched_mmd(&blocks, &refusal.blocks, MAX_LIMIT);
                    ApiError::revision_conflict(&revision)
                        .with("index", index)
                        .with("blocks", aliases)
                        .with("mmd", mmd)
                        .with("truncated", truncated)
                }
                Reason::TooLong => ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorCode::TooLongToWrite,
                    refusal.message,
                )
                .with("index", index),
                reason => {
                    let candidates: Vec<serde_json::Value> = refusal
                        .candidates
                        .iter()
                        .map(|c| serde_json::json!({ "alias": c.alias, "text": c.text }))
                        .collect();
                    ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        ErrorCode::OperationFailed,
                        refusal.message,
                    )
                    .with("index", index)
                    .with("reason", reason.code())
                    .with("hint", reason.hint())
                    .with("candidates", candidates)
                    .with("blocks", aliases)
                }
            }
        }
        EditRefusal::Conflict { revision, blocks } => {
            // Le plan ne nomme pas les blocs en cause : la page a changé, la première tranche à
            // jour, avec ses ancres, en dit assez pour relire.
            let anchors = Anchors::of(&blocks);
            let head = slice(&blocks, &anchors, true, None, DEFAULT_LIMIT);
            ApiError::revision_conflict(&revision)
                .with("mmd", head.mmd)
                .with("truncated", head.next.is_some())
        }
        EditRefusal::RevisionRequired => {
            ApiError::revision_required(checked.first_destructive.unwrap_or(0))
        }
        EditRefusal::PageInUse => ApiError::page_in_use(),
        EditRefusal::TooLong(cost) => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::TooLongToWrite,
            too_long_to_write(&cost),
        ),
        EditRefusal::VersionRefused => ApiError::version_refused(),
        EditRefusal::Failed(error) => error.into(),
    }
}
