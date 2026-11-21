//! Le serveur MCP du binaire (lot 5, Task M2 ; conception §6.5 ; Rulings R1236, R1237, R1240) :
//! `POST /mcp` sert les six outils, `POST /mcp/readonly` les trois de lecture (`search`, `fetch`,
//! `workspace_info`).
//!
//! | Route | Il faut | Réponse |
//! |---|---|---|
//! | `POST /mcp`, un message JSON-RPC | un jeton d'API, jamais une session | la réponse du protocole (`mementum-mcp`), dans la révision de la requête (2026-07-28, ou 2025-11-25) : `200` et un résultat (le refus d'un outil est un résultat `isError`, jamais une erreur JSON-RPC), `202` pour une notification, `400` ou `404` pour une erreur JSON-RPC qui ne se lit pas ou une méthode inconnue (R1406, R1407) ; sans jeton, `401 invalid_token` (avec `WWW-Authenticate: Bearer`), le cookie n'étant pas lu ; `GET` et `DELETE` : `405` |
//! | `POST /mcp/readonly` | un jeton d'API | la même, avec les seuls outils de lecture : un outil d'écriture y est un outil inconnu (`-32602`), jamais appelé |
//!
//! **Un outil n'a aucun droit à lui** : chacun lit ses arguments (bornés par `mementum-mcp`),
//! mène la fonction d'`agent/` que mène la route REST de la même opération, au nom du compte du
//! jeton restreint à lui (`access.rs`), et met la réponse en texte (`mementum_mcp::text`) ; ce
//! module ne lit rien de la base lui-même. Un refus d'une route garde son code et son message ;
//! ses champs de plus (l'opération en cause, ses blocs, le conseil de sa raison, les candidats, la
//! révision et les blocs à jour d'un conflit) passent dans le texte, le contenu des pages entre
//! les lignes de borne (Ruling R1239). Le débit des jetons est jugé par la porte (`auth.rs`) : un
//! appel d'outil qu'il refuse est un résultat `token_busy`, avec `retry_after_seconds`, sans rien
//! faire de l'appel (Ruling R1240).
//!
//! **Ce que l'outil lit seul** : le nom d'un espace (sans casse, parmi ceux que le jeton ouvre ; un
//! espace hors de la portée répond comme un nom inconnu), l'espace implicite d'un jeton qui n'en
//! ouvre qu'un, l'adresse d'une page (`https://<instance>/p/<id>`, `/p/<id>`, et `#b=<bloc>` qui
//! fait commencer la lecture à ce bloc), une date (`AAAA-MM-JJ`, ou RFC 3339) et un membre par son
//! nom pour les filtres de `search`, le curseur `after` de `search`.
//!
//! **Le journal d'activité** : la porte écrit une ligne par appel admis ; la route la précise par
//! `CallDetails` (surface `mcp`, l'outil, sa page ou son espace, son issue et son code, les
//! révisions et les blocs touchés d'une écriture), refus du protocole compris (R1560). Une autre
//! méthode (`tools/list`, `server/discover`…) garde le nom de la route, avec le code d'une erreur
//! JSON-RPC.

use std::future::Future;
use std::sync::Mutex;

use axum::body::Bytes;
use axum::extract::State;
use axum::extract::rejection::BytesRejection;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use mementum_mcp::text::{
    self, Candidate, Changed, Created, Hit, Managed, Member, Outline, OutlineLine, PageRef,
    PageText, Refusal, SearchResults, SpaceDetail, SpaceLine, TokenLine, Updated, Workspace,
};
use mementum_mcp::tools::{
    CreatePagesInput, FetchInput, ManagePagesInput, PageAction, SearchInput, Section,
    UpdatePageInput, WorkspaceInfoInput,
};
use mementum_mcp::{Endpoint, Headers, Host, Reply, Served, ToolCall, ToolOutput};
use mementum_store::{CallOutcome, CallSurface};
use serde_json::Value;
use uuid::Uuid;

use crate::AppState;
use crate::agent;
use crate::agent::fetch::{DEFAULT_LIMIT, Fetch, MAX_LIMIT, MIN_LIMIT};
use crate::auth::{CallDetails, LiveCaller, McpCaller};
use crate::error::ApiError;
use crate::types::{
    CreatePages, ErrorCode, NewPageBody, PageDocument, PageNode, PageOperations, RootSection,
    SearchHit, SearchRequest, SpaceInfo,
};

/// Lignes du plan d'un espace (`fetch` d'un espace), au plus.
const MAX_PLAN_LINES: usize = 500;
/// Pages de premier niveau d'un espace nommé (`workspace_info`), au plus.
const MAX_SPACE_PAGES: usize = 200;
/// Caractères d'un fragment d'adresse (`#b=<bloc>`), au plus : un identifiant de bloc fait cent
/// octets au plus, trois cents caractères échappés (la règle du client, `router.ts`).
const MAX_FRAGMENT_CHARS: usize = 1_024;

/// `POST /mcp` : les six outils.
pub(crate) async fn full(
    State(state): State<AppState>,
    caller: Result<McpCaller, ApiError>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    answer(state, Endpoint::Full, caller, headers, body).await
}

/// `POST /mcp/readonly` : les trois outils de lecture.
pub(crate) async fn readonly(
    State(state): State<AppState>,
    caller: Result<McpCaller, ApiError>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    answer(state, Endpoint::ReadOnly, caller, headers, body).await
}

/// Le corps est reçu avant tout refus (un refus rendu avant de lire un corps se perd chez un client
/// qui l'envoie d'un coup, R505), puis l'appelant est jugé (`401` sans jeton), puis le corps, puis
/// le protocole.
async fn answer(
    state: AppState,
    endpoint: Endpoint,
    caller: Result<McpCaller, ApiError>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    let caller = match caller {
        Ok(caller) => caller,
        Err(error) => return error.into_response(),
    };
    let body = match body {
        Ok(body) => body,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let protocol_version = values(&headers, "mcp-protocol-version");
    let method = values(&headers, "mcp-method");
    let name = values(&headers, "mcp-name");
    let compared = Headers {
        protocol_version: &protocol_version,
        method: &method,
        name: &name,
    };
    match caller {
        McpCaller::Busy(seconds) => {
            let served = mementum_mcp::serve(&compared, &body, endpoint, &Busy { seconds }).await;
            reply(&served.reply)
        }
        McpCaller::Admitted(caller) => {
            let tools = Tools {
                state: &state,
                caller: &caller,
                seen: Mutex::default(),
            };
            let served = mementum_mcp::serve(&compared, &body, endpoint, &tools).await;
            let seen = tools
                .seen
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut response = reply(&served.reply);
            response.extensions_mut().insert(details(&served, seen));
            response
        }
    }
}

/// Les valeurs reçues d'un en-tête, toutes (une liste vide : absent ; plusieurs : répété).
fn values<'a>(headers: &'a HeaderMap, name: &str) -> Vec<&'a [u8]> {
    headers
        .get_all(name)
        .iter()
        .map(HeaderValue::as_bytes)
        .collect()
}

/// La réponse HTTP d'une réponse du protocole : son statut, et son corps JSON s'il en a un.
fn reply(reply: &Reply) -> Response {
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    match &reply.body {
        None => status.into_response(),
        Some(_) => (
            status,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            reply.to_bytes(),
        )
            .into_response(),
    }
}

/// Ce qu'un appel d'outil a touché, pour le journal d'activité.
#[derive(Debug, Default)]
struct Seen {
    node: Option<Uuid>,
    space: Option<Uuid>,
    /// Le refus est une erreur du serveur (`5xx`) : l'issue `error`, pas `refused`.
    failed: bool,
    /// Les révisions et les blocs touchés d'une écriture faite (`update_page`).
    write: Option<CallDetails>,
}

/// La ligne du journal d'activité d'un appel (Rulings R1244, R1560) : un appel d'outil par son
/// outil et son issue (refus du protocole compris), une autre méthode par la route, avec le code
/// d'une erreur JSON-RPC.
fn details(served: &Served, seen: Seen) -> CallDetails {
    let mut details = CallDetails {
        surface: Some(CallSurface::Mcp),
        ..CallDetails::default()
    };
    match &served.call {
        Some(call) => {
            if let Some(write) = seen.write {
                details.revision_before = write.revision_before;
                details.revision_after = write.revision_after;
                details.blocks_touched = write.blocks_touched;
            }
            details.tool = (!call.tool.is_empty()).then(|| call.tool.clone());
            details.node_id = seen.node;
            details.space_id = seen.space;
            details.code = call.code.clone();
            details.outcome = Some(match (&call.code, seen.failed) {
                (None, _) => CallOutcome::Ok,
                (Some(_), true) => CallOutcome::Error,
                (Some(_), false) => CallOutcome::Refused,
            });
        }
        None => {
            let code = rpc_error(&served.reply);
            details.outcome = Some(if code.is_some() {
                CallOutcome::Refused
            } else {
                CallOutcome::Ok
            });
            details.code = code;
        }
    }
    details
}

/// Le nom d'une erreur JSON-RPC de la réponse, s'il y en a une.
fn rpc_error(reply: &Reply) -> Option<String> {
    let code = reply.body.as_ref()?.pointer("/error/code")?.as_i64()?;
    use mementum_mcp::protocol as p;
    Some(
        match code {
            p::PARSE_ERROR => "parse_error",
            p::INVALID_REQUEST => "invalid_request",
            p::METHOD_NOT_FOUND => "method_not_found",
            p::INVALID_PARAMS => "invalid_params",
            p::RESOURCE_NOT_FOUND_2025 => "resource_not_found",
            p::HEADER_MISMATCH => "header_mismatch",
            p::UNSUPPORTED_PROTOCOL_VERSION => "unsupported_protocol_version",
            _ => "rpc_error",
        }
        .to_string(),
    )
}

/// Un jeton que son débit refuse (Ruling R1240) : chaque appel d'outil est un refus
/// `token_busy`, qui dit son délai ; rien n'est fait.
struct Busy {
    seconds: u64,
}

impl Host for Busy {
    fn call(&self, _call: ToolCall) -> impl Future<Output = ToolOutput> + Send {
        let output = text::refusal(&refusal_of(
            &ApiError::token_busy(self.seconds),
            Counted::Operation,
        ));
        async move { output }
    }
}

/// Les six outils, au nom de l'appelant de la requête.
struct Tools<'a> {
    state: &'a AppState,
    caller: &'a LiveCaller,
    seen: Mutex<Seen>,
}

/// Pourquoi un outil n'a pas abouti.
enum Failure {
    /// Le refus d'une route ou d'une fonction d'`agent/`, tel qu'elle le rend ; ce que compte son
    /// champ `index`.
    Api(ApiError, Counted),
    /// Un refus propre à l'outil (un espace à nommer, ou ambigu) : un code et un message écrits par
    /// le serveur, sans texte d'un tiers.
    Tool { code: &'static str, message: String },
}

/// Ce que compte le champ `index` d'un refus : une opération d'`update_page`, ou une page de
/// `create_pages`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Counted {
    Operation,
    Page,
}

impl From<ApiError> for Failure {
    fn from(error: ApiError) -> Failure {
        Failure::Api(error, Counted::Operation)
    }
}

impl Host for Tools<'_> {
    async fn call(&self, call: ToolCall) -> ToolOutput {
        let result = match call {
            ToolCall::Search(input) => self.search(input).await,
            ToolCall::Fetch(input) => self.fetch(input).await,
            ToolCall::CreatePages(input) => self.create_pages(input).await,
            ToolCall::UpdatePage(input) => self.update_page(input).await,
            ToolCall::ManagePages(input) => self.manage_pages(input).await,
            ToolCall::WorkspaceInfo(input) => self.workspace_info(input).await,
        };
        result.unwrap_or_else(|failure| self.refused(failure))
    }
}

impl Tools<'_> {
    fn note(&self, change: impl FnOnce(&mut Seen)) {
        change(
            &mut self
                .seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }

    /// Le refus d'un outil, mis en texte.
    fn refused(&self, failure: Failure) -> ToolOutput {
        match failure {
            Failure::Api(error, counted) => {
                if error.status().is_server_error() {
                    self.note(|seen| seen.failed = true);
                }
                text::refusal(&refusal_of(&error, counted))
            }
            Failure::Tool { code, message } => text::error(code, &message, None),
        }
    }

    /// L'espace que l'appel vise : nommé (son nom, sans casse, ou son identifiant, parmi ceux que le
    /// jeton ouvre), ou implicite si le jeton n'en ouvre qu'un.
    async fn space(&self, named: Option<&str>) -> Result<(Uuid, SpaceInfo), Failure> {
        let user = &self.caller.user;
        let space = match named {
            Some(name) => match agent::info::resolve_space(self.state, user, name).await? {
                Ok(Some(space)) => space,
                Ok(None) => return Err(unknown_space()),
                Err(ids) => {
                    return Err(Failure::Tool {
                        code: "ambiguous_space",
                        message: format!(
                            "plusieurs espaces que ton jeton ouvre portent ce nom : nomme celui \
                             que tu vises par son identifiant ({})",
                            ids.join(", ")
                        ),
                    });
                }
            },
            None => {
                let mut spaces = agent::info::opened_spaces(self.state, user).await?;
                match spaces.len() {
                    0 => return Err(unknown_space()),
                    1 => spaces.remove(0),
                    _ => {
                        return Err(Failure::Tool {
                            code: "space_required",
                            message:
                                "ton jeton ouvre plusieurs espaces : nomme celui que tu vises \
                                      (space), par son nom ou son identifiant"
                                    .into(),
                        });
                    }
                }
            }
        };
        let id = Uuid::try_parse(&space.id).map_err(|_| unknown_space())?;
        self.note(|seen| seen.space = Some(id));
        Ok((id, space))
    }

    async fn search(&self, input: SearchInput) -> Result<ToolOutput, Failure> {
        let user = &self.caller.user;
        let (space_id, space) = self.space(input.space.as_deref()).await?;
        let after = match input.after.as_deref() {
            None => None,
            Some(cursor) => Some(cursor.trim().parse::<u32>().map_err(|_| {
                ApiError::new(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    ErrorCode::InvalidQuery,
                    "« after » : le curseur que donne la ligne « Suite » d'une recherche \
                     précédente (un nombre)",
                )
            })?),
        };
        let created_by = match input.created_by.as_deref() {
            None => None,
            Some(who) => Some(self.person(space_id, who).await?),
        };
        let edited_after = input
            .edited_after
            .as_deref()
            .map(|date| filter_date("edited_after", date))
            .transpose()?;
        let edited_before = input
            .edited_before
            .as_deref()
            .map(|date| filter_date("edited_before", date))
            .transpose()?;
        let found = agent::search::search(
            self.state,
            user,
            space_id,
            SearchRequest {
                q: input.query,
                after,
                created_by,
                edited_after,
                edited_before,
            },
        )
        .await?;
        let received = after.unwrap_or(0) as usize + found.results.len();
        Ok(text::results(&SearchResults {
            space_id: space.id,
            hits: found.results.into_iter().map(hit).collect(),
            more: found.more.then(|| received.to_string()),
            indexing: found.indexing,
        }))
    }

    /// Le compte d'une personne du filtre `created_by` : son identifiant, ou son nom parmi les
    /// membres de l'espace (sans casse), tels que `workspace_info` les nomme.
    async fn person(&self, space: Uuid, who: &str) -> Result<String, Failure> {
        if let Ok(id) = Uuid::try_parse(who.trim()) {
            return Ok(id.to_string());
        }
        let refused = |message: &str| {
            Failure::from(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::InvalidFilter,
                message,
            ))
        };
        let members = match agent::info::member_names(self.state, &self.caller.user, space).await {
            Ok(members) => members,
            Err(error) if error.code() == ErrorCode::Forbidden => {
                return Err(refused(
                    "« created_by » : ton jeton ne lit pas les membres de cet espace ; donne \
                     l'identifiant du compte",
                ));
            }
            Err(error) => return Err(error.into()),
        };
        let wanted = who.trim().to_lowercase();
        let mut named = members
            .into_iter()
            .filter(|member| member.name.trim().to_lowercase() == wanted);
        match (named.next(), named.next()) {
            (Some(member), None) => Ok(member.user_id),
            (None, _) => Err(refused(
                "« created_by » : aucun membre de cet espace ne porte ce nom (workspace_info avec \
                 space les nomme)",
            )),
            (Some(_), Some(_)) => Err(refused(
                "« created_by » : plusieurs membres de cet espace portent ce nom ; donne \
                 l'identifiant du compte",
            )),
        }
    }

    async fn fetch(&self, input: FetchInput) -> Result<ToolOutput, Failure> {
        let (id, block, maybe_space) = match target(&input.id) {
            Target::Id(id) => (id, None, true),
            Target::Page(id, block) => (id, block, false),
            Target::Space(id) => return self.outline(&id.to_string()).await,
            Target::Name(name) => return self.outline(&name).await,
            Target::Unknown => return Err(ApiError::page_not_found().into()),
        };
        self.note(|seen| seen.node = Some(id));
        let fetch = fetch_of(&input, block);
        match agent::fetch::document(self.state, &self.caller.user, id, fetch).await {
            Ok(document) => Ok(text::page(&page_text(document, input.since))),
            // Un identifiant seul nomme une page ou un espace : une page introuvable est peut-être
            // un espace ; sinon, la réponse d'une page introuvable.
            Err(error) if maybe_space && error.code() == ErrorCode::NotFound => {
                match self.outline(&id.to_string()).await {
                    Ok(outline) => {
                        self.note(|seen| seen.node = None);
                        Ok(outline)
                    }
                    Err(_) => {
                        self.note(|seen| seen.space = None);
                        Err(error.into())
                    }
                }
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Le plan d'un espace tel que le jeton le voit (`fetch` d'un espace).
    async fn outline(&self, name: &str) -> Result<ToolOutput, Failure> {
        let (space_id, space) = self.space(Some(name)).await?;
        let (lines, more) = agent::info::space_plan(
            self.state,
            &self.caller.user,
            space_id,
            MAX_PLAN_LINES,
            usize::MAX,
        )
        .await?;
        Ok(text::outline(&Outline {
            space_id: space.id,
            space_name: space.name,
            lines: lines
                .into_iter()
                .map(|(depth, page)| OutlineLine {
                    depth,
                    id: page.id,
                    title: page.title,
                })
                .collect(),
            truncated: more,
        }))
    }

    async fn create_pages(&self, input: CreatePagesInput) -> Result<ToolOutput, Failure> {
        let user = &self.caller.user;
        let pages = |error: ApiError| Failure::Api(error, Counted::Page);
        let (space_id, parent) = match input.parent.as_deref() {
            Some(parent) => {
                let parent = page_of(parent).ok_or_else(|| pages(ApiError::invalid_parent()))?;
                let space_id = match input.space.as_deref() {
                    Some(name) => self.space(Some(name)).await?.0,
                    None => {
                        let space = agent::info::page_space(self.state, user, parent)
                            .await?
                            .ok_or_else(|| pages(ApiError::invalid_parent()))?;
                        self.note(|seen| seen.space = Some(space));
                        space
                    }
                };
                (space_id, Some(parent))
            }
            None => (self.space(input.space.as_deref()).await?.0, None),
        };
        let request = CreatePages {
            parent_id: parent.map(|parent| parent.to_string()),
            section: input.section.map(|section| match section {
                Section::Private => RootSection::Private,
                Section::Space => RootSection::Space,
            }),
            pages: input
                .pages
                .into_iter()
                .map(|page| NewPageBody {
                    title: Some(page.title),
                    markdown: page.markdown,
                })
                .collect(),
        };
        let created = agent::create::create_pages(self.state, self.caller, space_id, request)
            .await
            .map_err(pages)?;
        let public = self.state.public_origin.as_deref();
        let created: Vec<Created> = created
            .into_iter()
            .map(|node: PageNode| Created {
                url: page_url(public, &node.id),
                id: node.id,
                title: node.title,
            })
            .collect();
        Ok(text::created(&created))
    }

    async fn update_page(&self, input: UpdatePageInput) -> Result<ToolOutput, Failure> {
        let id = page_of(&input.page).ok_or_else(ApiError::page_not_found)?;
        self.note(|seen| seen.node = Some(id));
        let request = PageOperations {
            base_revision: input.base_revision,
            on_conflict: input.on_conflict,
            mode: input.mode,
            operations: input.operations,
        };
        match agent::update::update_page(self.state, self.caller, id, request).await {
            Ok(updated) => {
                self.note(|seen| seen.write = Some(updated.details));
                Ok(text::updated(&Updated {
                    page_id: id.to_string(),
                    revision: updated.result.revision,
                    mmd: updated.result.mmd,
                    truncated: updated.result.truncated,
                }))
            }
            Err(refused) => {
                // Le contenu écrit avant un titre refusé reste dit au journal (R1404).
                if let Some(details) = refused.details {
                    self.note(|seen| seen.write = Some(*details));
                }
                Err(refused.error.into())
            }
        }
    }

    async fn manage_pages(&self, input: ManagePagesInput) -> Result<ToolOutput, Failure> {
        let user = &self.caller.user;
        let page = page_of(&input.page).ok_or_else(ApiError::page_not_found)?;
        self.note(|seen| seen.node = Some(page));
        let (action, node) = match input.action {
            PageAction::Move => {
                // La sœur telle que l'agent l'a nommée : son identifiant, ou ce qu'il a donné, que
                // la route refuse (`invalid_after`) après les refus de la page.
                let after = input.after.map(|after| match page_of(&after) {
                    Some(id) => id.to_string(),
                    None => after,
                });
                (
                    "move",
                    agent::manage::reorder(self.state, user, page, after).await?,
                )
            }
            PageAction::Rename => (
                "rename",
                agent::manage::rename(self.state, user, page, input.title.unwrap_or_default())
                    .await?,
            ),
            PageAction::Duplicate => (
                "duplicate",
                agent::manage::duplicate(self.state, self.caller, page).await?,
            ),
            PageAction::Trash => ("trash", agent::manage::trash(self.state, user, page).await?),
            PageAction::Restore => (
                "restore",
                agent::manage::restore(self.state, user, page).await?,
            ),
        };
        Ok(text::managed(&Managed {
            action: action.to_string(),
            page: PageRef {
                id: node.id,
                title: node.title,
            },
            parent_id: node.parent_id,
            trashed: node.trashed_at.is_some(),
        }))
    }

    async fn workspace_info(&self, input: WorkspaceInfoInput) -> Result<ToolOutput, Failure> {
        let user = &self.caller.user;
        let me = agent::info::me(self.state, user)
            .await?
            .ok_or_else(ApiError::token_required)?;
        let space = match input.space.as_deref() {
            None => None,
            Some(name) => {
                let (space_id, space) = self.space(Some(name)).await?;
                // Les membres se lisent à partir du rôle de membre : une portée de pages, comme
                // un invité, n'en lit aucun (Ruling R1247).
                let members = match agent::info::member_names(self.state, user, space_id).await {
                    Ok(members) => members,
                    Err(error) if error.code() == ErrorCode::Forbidden => Vec::new(),
                    Err(error) => return Err(error.into()),
                };
                // Une page de plus que la réponse n'en montre : la mise en texte dit la coupe.
                let (pages, _) =
                    agent::info::space_plan(self.state, user, space_id, MAX_SPACE_PAGES + 1, 0)
                        .await?;
                Some(SpaceDetail {
                    space: space_line(space),
                    members: members
                        .into_iter()
                        .map(|member| Member {
                            name: member.name,
                            role: word_of(&member.role),
                        })
                        .collect(),
                    pages: pages
                        .into_iter()
                        .map(|(_, page)| PageRef {
                            id: page.id,
                            title: page.title,
                        })
                        .collect(),
                })
            }
        };
        let token = me.token;
        Ok(text::workspace(&Workspace {
            account: me.user.name,
            token: TokenLine {
                name: token.name,
                level: word_of(&token.level),
                scope: word_of(&token.scope),
                expires_at: token.expires_at.map(rfc3339),
                can_trash: token.can_trash,
            },
            spaces: me.spaces.into_iter().map(space_line).collect(),
            space,
        }))
    }
}

/// « Aucun espace de ce nom » : la même réponse pour un nom inconnu et pour un espace hors de la
/// portée du jeton (Review Focus de M2), sans recopier le nom.
fn unknown_space() -> Failure {
    Failure::Tool {
        code: "not_found",
        message: "aucun espace de ce nom ni de cet identifiant parmi ceux que ton jeton ouvre \
                  (workspace_info les nomme)"
            .into(),
    }
}

/// Un refus d'une route, tel que l'outil le dit : son code, son message, et ses champs de plus.
/// Le message et les champs que le serveur écrit (alias, révision, conseil) restent hors des
/// lignes de borne ; le texte des pages (candidats, blocs à jour) va entre elles (Ruling R1239).
fn refusal_of(error: &ApiError, counted: Counted) -> Refusal {
    let code = code_name(error.code());
    let mut message = error.message().to_string();
    if let Some(line) = error.line() {
        message.push_str(&format!(" (ligne {line} du MMD)"));
    }
    let index = error
        .field("index")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok());
    let mut operation = None;
    match (counted, index) {
        (Counted::Operation, Some(index)) => {
            let reason = error
                .field("reason")
                .and_then(Value::as_str)
                .unwrap_or(&code);
            operation = Some((index, reason.to_string()));
        }
        (Counted::Page, Some(index)) => {
            message = format!("page {} du lot : {message}", index.saturating_add(1));
        }
        (_, None) => {}
    }
    let aliases: Vec<&str> = error
        .field("blocks")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !aliases.is_empty() {
        message.push_str(&format!(" Blocs en cause : {}.", aliases.join(", ")));
    }
    let revision = error.field("revision").and_then(Value::as_str);
    let conflict = if error.code() == ErrorCode::RevisionConflict {
        if error.field("truncated") == Some(&Value::Bool(true)) {
            message.push_str(" Les blocs à jour sont coupés : relis la page par fetch.");
        }
        Some((
            revision.unwrap_or_default().to_string(),
            error
                .field("mmd")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ))
    } else {
        if let Some(revision) = revision {
            message.push_str(&format!(" Révision de la page : {revision}."));
        }
        None
    };
    let candidates = error
        .field("candidates")
        .and_then(Value::as_array)
        .map(|candidates| {
            candidates
                .iter()
                .filter_map(|candidate| {
                    Some(Candidate {
                        alias: candidate.get("alias")?.as_str()?.to_string(),
                        text: candidate.get("text")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Refusal {
        code,
        message,
        retry_after_seconds: error.retry_after(),
        operation,
        candidates,
        conflict,
        hint: error
            .field("hint")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

/// Le nom d'un code d'erreur, tel que le corps d'un refus l'écrit (`not_found`…).
fn code_name(code: ErrorCode) -> String {
    word_of(&code)
}

/// Une valeur sérialisée en texte (un code, un niveau, un rôle, une portée) : son nom.
fn word_of(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn space_line(space: SpaceInfo) -> SpaceLine {
    SpaceLine {
        role: word_of(&space.role),
        id: space.id,
        name: space.name,
    }
}

/// Une page trouvée : le bloc trouvé par un alias qui se donne sans relire la page (un identifiant
/// de bloc est écrit par un client, jamais montré tel quel).
fn hit(found: SearchHit) -> Hit {
    Hit {
        page_id: found.page_id,
        title: found.title,
        path: found.path.into_iter().map(|page| page.title).collect(),
        excerpt: found.excerpt,
        block: found
            .block_id
            .as_deref()
            .and_then(mementum_crdt::anchors::standalone_alias),
    }
}

/// Une page lue, mise en texte.
fn page_text(document: PageDocument, since: Option<String>) -> PageText {
    let changed = match (document.changed_since, document.changed) {
        (None, _) => None,
        (Some(false), _) => Some(Changed::Nothing),
        (Some(true), Some(Some(aliases))) if !aliases.is_empty() => Some(Changed::Blocks(aliases)),
        // Un changement qu'aucun alias ne nomme (un bloc arrivé en tête de page), ou des
        // empreintes qui ne sont plus gardées : relire depuis le début.
        (Some(true), _) => Some(Changed::Unknown),
    };
    PageText {
        id: document.id,
        space_id: document.space_id,
        title: document.title,
        path: document.path.into_iter().map(|page| page.title).collect(),
        level: word_of(&document.level),
        edited_at: document.edited_at.map(rfc3339),
        revision: document.revision,
        mmd: document.mmd,
        truncated: document.truncated,
        next: document.next,
        since,
        changed,
        trashed: document.trashed,
        children: document
            .children
            .into_iter()
            .map(|child| PageRef {
                id: child.id,
                title: child.title,
            })
            .collect(),
    }
}

/// Ce qu'une lecture demande. `limit` sous 1 024 octets est ramené à 1 024 (une tranche avance
/// toujours) ; le bloc d'une adresse (`#b=`) donne `from` quand l'agent ne le donne pas.
fn fetch_of(input: &FetchInput, block: Option<String>) -> Result<Fetch, ApiError> {
    let from = input.from.clone().or(block);
    if input.since.is_some() && from.is_none() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidRequest,
            "« since » ne sert qu'avec « from » : la révision de la première tranche, pour \
             reprendre la lecture",
        ));
    }
    Ok(Fetch {
        anchors: input.anchors.unwrap_or(false),
        from,
        since: input.since.clone(),
        limit: input.limit.map_or(DEFAULT_LIMIT, |limit| {
            (limit as usize).clamp(MIN_LIMIT, MAX_LIMIT)
        }),
    })
}

/// L'adresse d'une page dans l'application : sous l'adresse publique de l'instance quand elle est
/// réglée (`MEMENTUM_PUBLIC_URL`), sinon son seul chemin.
fn page_url(public: Option<&str>, id: &str) -> String {
    format!("{}/p/{id}", public.unwrap_or(""))
}

/// Ce que désigne un `id` de `fetch` (ou une page d'un autre outil).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// Un identifiant seul : une page, ou un espace.
    Id(Uuid),
    /// L'adresse d'une page, et le bloc que vise son fragment (`#b=`).
    Page(Uuid, Option<String>),
    /// L'adresse d'un espace (`/s/<id>`).
    Space(Uuid),
    /// Ni identifiant ni adresse : le nom d'un espace.
    Name(String),
    /// Une adresse qui ne désigne rien que l'outil connaisse.
    Unknown,
}

/// Ce que désigne `text` : un identifiant, l'adresse d'une page (`http(s)://<hôte>/p/<id>`, ou
/// `/p/<id>`, suivie de `#b=<bloc>`) ou d'un espace (`/s/<id>`), ou un nom.
fn target(text: &str) -> Target {
    let text = text.trim();
    if let Ok(id) = Uuid::try_parse(text) {
        return Target::Id(id);
    }
    let path = match text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
    {
        Some(rest) => match rest.find('/') {
            Some(slash) => &rest[slash..],
            None => return Target::Unknown,
        },
        None if text.starts_with('/') => text,
        None => return Target::Name(text.to_string()),
    };
    let (path, fragment) = match path.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (path, None),
    };
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let mut segments = path.trim_matches('/').split('/');
    match (segments.next(), segments.next(), segments.next()) {
        (Some("p"), Some(id), None) => match Uuid::try_parse(id) {
            Ok(id) => Target::Page(id, fragment.and_then(block_of)),
            Err(_) => Target::Unknown,
        },
        (Some("s"), Some(id), _) => match Uuid::try_parse(id) {
            Ok(id) => Target::Space(id),
            Err(_) => Target::Unknown,
        },
        _ => Target::Unknown,
    }
}

/// La page que désigne `text` (son identifiant, ou son adresse), pour un outil qui ne vise qu'une
/// page.
fn page_of(text: &str) -> Option<Uuid> {
    match target(text) {
        Target::Id(id) | Target::Page(id, _) => Some(id),
        _ => None,
    }
}

/// Le bloc que vise un fragment d'adresse (`b=<bloc échappé>`, la forme de `blockPath` du client) ;
/// `None` pour une autre forme, un bloc vide, mal échappé, ou un fragment trop long.
fn block_of(fragment: &str) -> Option<String> {
    if fragment.len() > MAX_FRAGMENT_CHARS {
        return None;
    }
    let escaped = fragment.strip_prefix("b=")?;
    let bytes = escaped.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out)
        .ok()
        .filter(|block| !block.is_empty())
}

/// Une date d'un filtre de `search`, en millisecondes Unix ; `422 invalid_filter` sinon.
fn filter_date(field: &str, text: &str) -> Result<i64, ApiError> {
    date_ms(text).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::InvalidFilter,
            format!(
                "« {field} » : une date AAAA-MM-JJ (minuit en temps universel), ou une date et \
                 une heure RFC 3339 (2026-11-20T14:30:00Z)"
            ),
        )
    })
}

/// Le nombre de jours depuis le 1970-01-01 d'une date civile (algorithme de Howard Hinnant).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// La date civile d'un nombre de jours depuis le 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Des millisecondes Unix en date et heure RFC 3339, en temps universel, à la seconde.
fn rfc3339(ms: i64) -> String {
    let (days, ms) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (year, month, day) = civil_from_days(days);
    let secs = ms / 1_000;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

/// Une date `AAAA-MM-JJ` (minuit en temps universel) ou une date et une heure RFC 3339
/// (`AAAA-MM-JJTHH:MM:SS`, des fractions de seconde, puis `Z` ou un décalage `±HH:MM`), en
/// millisecondes Unix ; `None` pour toute autre forme, ou une date qui n'existe pas.
fn date_ms(text: &str) -> Option<i64> {
    let text = text.trim();
    if !text.is_ascii() || text.len() < 10 {
        return None;
    }
    let b = text.as_bytes();
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = &text[range];
        digits
            .bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| digits.parse().ok())
            .flatten()
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    if !(1..=12).contains(&month) || day < 1 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    let mut ms = days * 86_400_000;
    let rest = &text[10..];
    if rest.is_empty() {
        return Some(ms);
    }
    let rest = rest.strip_prefix(['T', 't', ' '])?;
    if rest.len() < 8 || rest.as_bytes()[2] != b':' || rest.as_bytes()[5] != b':' {
        return None;
    }
    let field = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = rest.get(range)?;
        digits
            .bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| digits.parse().ok())
            .flatten()
    };
    let (hours, minutes, seconds) = (field(0..2)?, field(3..5)?, field(6..8)?);
    if hours > 23 || minutes > 59 || seconds > 60 {
        return None;
    }
    let mut rest = &rest[8..];
    let mut fraction = 0;
    if let Some(after) = rest.strip_prefix('.') {
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let kept = &after[..digits.min(3)];
        fraction = kept.parse::<i64>().ok()? * 10_i64.pow(3 - kept.len() as u32);
        rest = &after[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            if rest.len() != 6 || rest.as_bytes()[3] != b':' {
                return None;
            }
            let hours = rest[1..3].parse::<i64>().ok()?;
            let minutes = rest[4..6].parse::<i64>().ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 60 + minutes)
        }
    };
    ms += (hours * 3_600 + minutes * 60 + seconds.min(59)) * 1_000 + fraction - offset * 60_000;
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les dates des filtres se lisent en temps universel, et une date qui n'existe pas, ou une
    /// autre forme, est refusée ; les dates rendues s'écrivent en RFC 3339.
    #[test]
    fn dates_are_read_and_written_in_utc() {
        assert_eq!(date_ms("1970-01-01"), Some(0));
        assert_eq!(date_ms("2026-11-20"), Some(1_795_132_800_000));
        assert_eq!(date_ms(" 2026-11-20T00:00:00Z "), Some(1_795_132_800_000));
        assert_eq!(
            date_ms("2026-11-20T14:30:15.5Z"),
            Some(1_795_132_800_000 + (14 * 3_600 + 30 * 60 + 15) * 1_000 + 500)
        );
        assert_eq!(
            date_ms("2026-11-20T01:00:00+01:00"),
            Some(1_795_132_800_000)
        );
        assert_eq!(date_ms("2024-02-29"), Some(1_709_164_800_000));
        for wrong in [
            "2026-02-30",
            "2025-02-29",
            "2026-13-01",
            "2026-11-20T24:00:00Z",
            "2026-11-20T10:00:00",
            "2026-11-20T10:00Z",
            "20-11-2026",
            "hier",
            "2026-11-20T10:00:00+0100",
            "２０２６-11-20",
        ] {
            assert_eq!(date_ms(wrong), None, "{wrong}");
        }
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339(1_795_132_800_000 + 52_215_000),
            "2026-11-20T14:30:15Z"
        );
        assert_eq!(rfc3339(-1_000), "1969-12-31T23:59:59Z");
    }

    /// Une page se désigne par son identifiant ou son adresse, dont le fragment `#b=<bloc>`
    /// (échappé comme le client l'écrit) ; un espace par `/s/<id>` ; le reste est un nom, ou une
    /// adresse qui ne désigne rien.
    #[test]
    fn a_target_is_an_id_an_address_or_a_name() {
        let id = Uuid::now_v7();
        assert_eq!(target(&id.to_string()), Target::Id(id));
        assert_eq!(target(&format!(" {id} ")), Target::Id(id));
        for address in [
            format!("https://notes.exemple.fr/p/{id}"),
            format!("http://localhost:5173/p/{id}/"),
            format!("/p/{id}?x=1"),
        ] {
            assert_eq!(target(&address), Target::Page(id, None), "{address}");
        }
        assert_eq!(
            target(&format!("/p/{id}#b=bloc%20un%C3%A9")),
            Target::Page(id, Some("bloc uné".into()))
        );
        for fragment in ["#b=", "#x=1", "#b=%zz", "#b=%C3"] {
            assert_eq!(
                target(&format!("/p/{id}{fragment}")),
                Target::Page(id, None),
                "{fragment}"
            );
        }
        assert_eq!(
            target(&format!("/p/{id}#b={}", "a".repeat(2_000))),
            Target::Page(id, None)
        );
        assert_eq!(
            target(&format!("https://x/s/{id}/trash")),
            Target::Space(id)
        );
        assert_eq!(target("Pro"), Target::Name("Pro".into()));
        assert_eq!(
            target(" Espace d'Alex "),
            Target::Name("Espace d'Alex".into())
        );
        for unknown in [
            "https://notes.exemple.fr".to_string(),
            "/p/pas-une-page".to_string(),
            format!("/x/{id}"),
            format!("/p/{id}/plus"),
        ] {
            assert_eq!(target(&unknown), Target::Unknown, "{unknown}");
        }
        assert_eq!(page_of("Pro"), None);
        assert_eq!(page_of(&format!("/s/{id}")), None);
    }
}
