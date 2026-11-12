// Client REST du serveur, typé par les types générés de `packages/protocol` (ts-rs).
// Les corps refusent les champs inconnus (`deny_unknown_fields`, R30) : on n'envoie que
// ceux des types de `packages/protocol`.
//
// La session est un cookie (`HttpOnly`) que le navigateur joint à chaque appel. Un appel qui
// répond `401 unauthenticated` dit qu'elle est perdue : `onSessionLost` prévient
// `SessionGate`, qui demande la connexion. Les autres `401` (`invalid_credentials` : mot de
// passe faux) ne disent rien de la session.
import type {
  Accepted,
  ChangePassword,
  Comment,
  CreatePage,
  CreateSpace,
  ErrorBody,
  ErrorCode,
  Info,
  InvitationCreated,
  InvitationInfo,
  InvitationToken,
  InviteToPage,
  InviteToSpace,
  Level,
  LogIn,
  Me,
  Member,
  NewComment,
  NewThread,
  PageAccess,
  PageNode,
  PendingInvitation,
  ResolveThread,
  SetGeneral,
  SetLevel,
  SignUp,
  SpaceInfo,
  Thread,
  Tree,
  UpdateMember,
  UpdatePage,
  UpdateSpace,
  Version,
} from "@mementum/protocol";

/** Erreur d'un appel : le code et le message (en français) du serveur, quand il répond. */
export class ApiError extends Error {
  readonly status: number;
  /** `null` : pas de réponse du serveur, ou une réponse qui n'est pas une erreur de l'API. */
  readonly code: ErrorCode | null;

  constructor(status: number, code: ErrorCode | null, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

const UNREACHABLE = "Le serveur Mementum ne répond pas. Vérifie qu'il est lancé, puis réessaie.";

const sessionLost = new Set<() => void>();

/**
 * Appelle `listener` chaque fois qu'un appel répond `401 unauthenticated` (session absente,
 * échue ou fermée ailleurs). Rend la fonction qui le retire.
 */
export function onSessionLost(listener: () => void): () => void {
  sessionLost.add(listener);
  return () => sessionLost.delete(listener);
}

/** L'erreur dit-elle que la session est perdue (et non, par exemple, un mot de passe faux) ? */
export function isSessionLost(error: unknown): boolean {
  return error instanceof ApiError && error.code === "unauthenticated";
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await send(method, path, body);
  // `204` : pas de corps.
  return (res.status === 204 ? undefined : await res.json()) as T;
}

/** Une réponse réussie, sinon l'erreur du serveur (ou `UNREACHABLE`), levée en `ApiError`. */
async function send(method: string, path: string, body?: unknown): Promise<Response> {
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      headers: body === undefined ? undefined : { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, null, UNREACHABLE);
  }
  if (res.ok) return res;
  let error: Partial<ErrorBody> = {};
  try {
    error = (await res.json()) as ErrorBody;
  } catch {
    // corps illisible (mandataire, page d'erreur) : message générique ci-dessous
  }
  const failure = new ApiError(
    res.status,
    error.error ?? null,
    error.message ?? (res.status >= 500 ? UNREACHABLE : `Erreur ${res.status} du serveur.`),
  );
  if (isSessionLost(failure)) for (const listener of [...sessionLost]) listener();
  throw failure;
}

/** `GET /api/v1/me` : le compte de la session et ses espaces ; sans session, `401`. */
export function getMe(): Promise<Me> {
  return call("GET", "/api/v1/me");
}

/** `POST /api/v1/auth/signup` : crée le compte et ouvre sa session (cookie). */
export function signUp(body: SignUp): Promise<Me> {
  return call("POST", "/api/v1/auth/signup", body);
}

/** `POST /api/v1/auth/login` : ouvre une session (cookie) ; `401 invalid_credentials`. */
export function logIn(body: LogIn): Promise<Me> {
  return call("POST", "/api/v1/auth/login", body);
}

/** `POST /api/v1/auth/logout` : supprime la session sur le serveur et efface le cookie. */
export function logOut(): Promise<void> {
  return call("POST", "/api/v1/auth/logout");
}

/**
 * `POST /api/v1/auth/password` : les autres sessions du compte sont fermées, celle-ci
 * repart avec un cookie neuf. Mot de passe actuel faux : `401 invalid_credentials`.
 */
export function changePassword(body: ChangePassword): Promise<void> {
  return call("POST", "/api/v1/auth/password", body);
}

// Espaces, membres et invitations. Dans un espace où l'on n'a aucun rôle, toute route répond
// `404`, comme pour un espace qui n'existe pas ; une action au-dessus de son rôle, `403`.

const space = (spaceId: string) => `/api/v1/spaces/${encodeURIComponent(spaceId)}`;

/** `POST /api/v1/spaces` : crée un espace, dont le compte est propriétaire. */
export function createSpace(body: CreateSpace): Promise<SpaceInfo> {
  return call("POST", "/api/v1/spaces", body);
}

/** `PATCH /api/v1/spaces/{id}` : renomme l'espace (`admin`). */
export function updateSpace(spaceId: string, body: UpdateSpace): Promise<SpaceInfo> {
  return call("PATCH", space(spaceId), body);
}

/** `GET /api/v1/spaces/{id}/members` : les personnes de l'espace, invités compris (`member`). */
export function getMembers(spaceId: string): Promise<Member[]> {
  return call("GET", `${space(spaceId)}/members`);
}

/**
 * `PATCH /api/v1/spaces/{id}/members/{userId}` : change un rôle (un `admin`, seulement entre
 * `admin` et `member`) ; `409 last_owner` si l'espace perdrait son dernier propriétaire.
 */
export function updateMember(spaceId: string, userId: string, body: UpdateMember): Promise<Member> {
  return call("PATCH", `${space(spaceId)}/members/${encodeURIComponent(userId)}`, body);
}

/**
 * `DELETE /api/v1/spaces/{id}/members/{userId}` : retire quelqu'un (`admin`, jamais un
 * propriétaire s'il n'en est pas un), ou part, pour soi ; `409 last_owner`.
 */
export function removeMember(spaceId: string, userId: string): Promise<void> {
  return call("DELETE", `${space(spaceId)}/members/${encodeURIComponent(userId)}`);
}

/** `POST /api/v1/spaces/{id}/invitations` : l'invitation et son jeton, rendu cette fois seulement. */
export function inviteToSpace(spaceId: string, body: InviteToSpace): Promise<InvitationCreated> {
  return call("POST", `${space(spaceId)}/invitations`, body);
}

/** `GET /api/v1/spaces/{id}/invitations` : celles qui attendent, sans leur jeton (`admin`). */
export function getSpaceInvitations(spaceId: string): Promise<PendingInvitation[]> {
  return call("GET", `${space(spaceId)}/invitations`);
}

/** `DELETE /api/v1/invitations/{id}` : le lien ne vaut plus. */
export function revokeInvitation(id: string): Promise<void> {
  return call("DELETE", `/api/v1/invitations/${encodeURIComponent(id)}`);
}

/**
 * `POST /api/v1/invitations/lookup`, sans session : ce que dit un lien. Inconnu, échu,
 * révoqué ou déjà utilisé : `404 invalid_invitation`, toujours la même réponse. Le jeton va
 * dans le corps, jamais dans une adresse.
 */
export function lookupInvitation(token: string): Promise<InvitationInfo> {
  const body: InvitationToken = { token };
  return call("POST", "/api/v1/invitations/lookup", body);
}

/** `POST /api/v1/invitations/accept` : le compte de la session reçoit l'invitation. */
export function acceptInvitation(token: string): Promise<Accepted> {
  const body: InvitationToken = { token };
  return call("POST", "/api/v1/invitations/accept", body);
}

/** `GET /api/v1/info` : version du serveur et de son schéma de blocs. */
export function getInfo(): Promise<Info> {
  return call("GET", "/api/v1/info");
}

// Pages. Une page que la personne ne voit pas répond `404`, comme une page qui n'existe pas ;
// une action au-dessus de son niveau sur une page qu'elle voit, `403 forbidden`.

const page = (id: string) => `/api/v1/pages/${encodeURIComponent(id)}`;

/**
 * `GET /api/v1/spaces/{id}/tree` : les pages de l'espace que la personne voit, hors
 * corbeille, chacune avec son niveau et sa section.
 */
export function getTree(spaceId: string): Promise<Tree> {
  return call("GET", `${space(spaceId)}/tree`);
}

/**
 * `POST /api/v1/pages` : à la racine d'un espace (`spaceId`, et `section` : `private` par
 * défaut) ou sous une page (`parentId`, sans `section` : une sous-page hérite). Sans
 * `afterId`, la page va en fin de ses frères (R23).
 */
export function createPage(body: CreatePage): Promise<PageNode> {
  return call("POST", "/api/v1/pages", body);
}

/** `GET /api/v1/pages/{id}` : la page, son espace, et depuis quand elle est à la corbeille. */
export function getPage(id: string): Promise<PageNode> {
  return call("GET", page(id));
}

/** `PATCH /api/v1/pages/{id}` : un champ absent ne change rien. */
export function updatePage(id: string, body: UpdatePage): Promise<PageNode> {
  return call("PATCH", page(id), body);
}

/**
 * `GET /api/v1/spaces/{id}/trash` : les pages mises à la corbeille (pas leurs sous-pages, qui
 * n'y sont que par elles) que la personne peut restaurer, la plus récente d'abord.
 */
export function getTrash(spaceId: string): Promise<PageNode[]> {
  return call("GET", `${space(spaceId)}/trash`);
}

/** `POST /api/v1/pages/{id}/trash` : la page part à la corbeille, avec ses sous-pages. */
export function trashPage(id: string): Promise<PageNode> {
  return call("POST", `${page(id)}/trash`);
}

/**
 * `POST /api/v1/pages/{id}/restore` : la page revient, avec ses sous-pages ; `409
 * parent_trashed` si une de ses pages parentes est à la corbeille (c'est elle qu'on restaure).
 */
export function restorePage(id: string): Promise<PageNode> {
  return call("POST", `${page(id)}/restore`);
}

/**
 * `DELETE /api/v1/pages/{id}` : supprime pour de bon la page et ses sous-pages (`full`) ; `409
 * not_trashed` si elle n'est pas à la corbeille.
 */
export function deletePage(id: string): Promise<void> {
  return call("DELETE", page(id));
}

// Partage d'une page : qui y a accès, et à quel niveau. Ces routes demandent l'accès complet
// (`full`) sur la page ; sous ce niveau, `403 forbidden`.

const access = (pageId: string) => `${page(pageId)}/access`;

/**
 * Un changement d'accès : le serveur rend les accès tels qu'ils sont devenus. `null` : il a
 * répondu `204`, sans corps. Le changement est fait, mais la personne connectée n'a plus
 * l'accès complet (elle s'est baissée ou retirée elle-même) : les accès ne lui sont plus rendus.
 */
async function changeAccess(method: string, path: string, body?: unknown): Promise<PageAccess | null> {
  return (await call<PageAccess | undefined>(method, path, body)) ?? null;
}

/**
 * `GET /api/v1/pages/{id}/access` : l'accès général et son plancher (celui d'une page parente),
 * les comptes nommés sur la page ou ses pages parentes, les invitations de la page qui attendent.
 */
export function getAccess(pageId: string): Promise<PageAccess> {
  return call("GET", access(pageId));
}

/**
 * `PUT /api/v1/pages/{id}/access/general` : le niveau de tous les membres de l'espace sur la
 * page ; `null` : seulement les personnes nommées. Réservé aux membres de l'espace (un invité :
 * `403`, R85). `409 inherited_access` sous le niveau qu'une page parente donne déjà (R93) ;
 * `409 last_full_access` si la page n'avait plus personne en accès complet.
 */
export function setGeneralAccess(pageId: string, level: Level | null): Promise<PageAccess | null> {
  const body: SetGeneral = { level };
  return changeAccess("PUT", `${access(pageId)}/general`, body);
}

/**
 * `PUT /api/v1/pages/{id}/access/users/{userId}` : le niveau d'un compte de l'espace sur la
 * page ; `409 last_full_access`.
 */
export function setUserLevel(pageId: string, userId: string, level: Level): Promise<PageAccess | null> {
  const body: SetLevel = { level };
  return changeAccess("PUT", `${access(pageId)}/users/${encodeURIComponent(userId)}`, body);
}

/**
 * `DELETE /api/v1/pages/{id}/access/users/{userId}` : retire l'accès posé sur la page pour ce
 * compte (un accès hérité se retire sur la page qui le porte : `404`) ; `409 last_full_access`.
 * Un invité qui n'a plus aucun accès sort de l'espace.
 */
export function removeUserAccess(pageId: string, userId: string): Promise<PageAccess | null> {
  return changeAccess("DELETE", `${access(pageId)}/users/${encodeURIComponent(userId)}`);
}

/**
 * `POST /api/v1/pages/{id}/invitations` : l'invitation et son jeton, rendu cette fois
 * seulement. La réponse est la même, que l'adresse ait un compte ou non.
 */
export function inviteToPage(pageId: string, body: InviteToPage): Promise<InvitationCreated> {
  return call("POST", `${page(pageId)}/invitations`, body);
}

// Commentaires d'une page (R134) : des fils attachés à la page, ou à un de ses blocs par son
// identifiant. Rien n'en est écrit dans le document. Lire demande `read` (à la corbeille
// aussi) ; écrire, `comment`, et rien ne s'écrit à la corbeille (`409 trashed`). Un fil ou un
// message qui n'est plus là (supprimé entre-temps) répond `404`.

const comments = (pageId: string) => `${page(pageId)}/comments`;
const thread = (pageId: string, threadId: string) =>
  `${comments(pageId)}/${encodeURIComponent(threadId)}`;
const message = (pageId: string, threadId: string, commentId: string) =>
  `${thread(pageId, threadId)}/messages/${encodeURIComponent(commentId)}`;

/** `GET /api/v1/pages/{id}/comments` : les fils de la page, résolus compris, du plus ancien au plus récent. */
export function getComments(pageId: string): Promise<Thread[]> {
  return call("GET", comments(pageId));
}

/**
 * `POST /api/v1/pages/{id}/comments` : un fil neuf et son premier message ; sur un bloc
 * (`422 invalid_block` s'il n'est plus dans la page), ou sur la page (`blockId` nul).
 */
export function openThread(pageId: string, body: NewThread): Promise<Thread> {
  return call("POST", comments(pageId), body);
}

/** `POST /api/v1/pages/{id}/comments/{threadId}/replies` : une réponse, même à un fil résolu (R171). */
export function replyToThread(pageId: string, threadId: string, body: NewComment): Promise<Comment> {
  return call("POST", `${thread(pageId, threadId)}/replies`, body);
}

/**
 * `PATCH /api/v1/pages/{id}/comments/{threadId}` : résout ou rouvre le fil (l'avoir ouvert
 * avec `comment`, ou `edit`). Déjà dans cet état : rien ne change (R170).
 */
export function resolveThread(pageId: string, threadId: string, body: ResolveThread): Promise<Thread> {
  return call("PATCH", thread(pageId, threadId), body);
}

/** `PATCH …/messages/{commentId}` : le texte d'un message, par son auteur. */
export function editComment(
  pageId: string,
  threadId: string,
  commentId: string,
  body: NewComment,
): Promise<Comment> {
  return call("PATCH", message(pageId, threadId, commentId), body);
}

/**
 * `DELETE …/messages/{commentId}` : par son auteur, ou avec l'accès complet. Le fil part avec
 * son dernier message.
 */
export function deleteComment(pageId: string, threadId: string, commentId: string): Promise<void> {
  return call("DELETE", message(pageId, threadId, commentId));
}

/**
 * `PUT` (`on`) ou `DELETE …/messages/{commentId}/reactions/{emoji}` : la réaction de la
 * personne, posée ou retirée. L'emoji va dans le chemin, encodé en UTF-8.
 */
export function setReaction(
  pageId: string,
  threadId: string,
  commentId: string,
  emoji: string,
  on: boolean,
): Promise<Comment> {
  const path = `${message(pageId, threadId, commentId)}/reactions/${encodeURIComponent(emoji)}`;
  return call(on ? "PUT" : "DELETE", path);
}

// Historique d'une page (R135) : ses versions, chacune l'état complet de la page à un instant.
// Lire demande `read` (à la corbeille aussi) ; restaurer, `edit`, et rien ne se restaure à la
// corbeille (`409 trashed`). Une version qui n'est pas de la page, ou qui n'est plus gardée,
// reçoit la réponse d'une page introuvable (`404`).

const versions = (pageId: string) => `${page(pageId)}/versions`;
const version = (pageId: string, versionId: string) =>
  `${versions(pageId)}/${encodeURIComponent(versionId)}`;

/** `GET /api/v1/pages/{id}/versions` : les versions de la page, la plus récente d'abord. */
export function getVersions(pageId: string): Promise<Version[]> {
  return call("GET", versions(pageId));
}

/**
 * `GET …/versions/{versionId}/state` : l'état Yjs (v1) de la version, plusieurs mégaoctets pour
 * une grande page. À appliquer dans un document à part, jamais dans celui d'une session.
 */
export async function getVersionState(pageId: string, versionId: string): Promise<Uint8Array> {
  const res = await send("GET", `${version(pageId, versionId)}/state`);
  try {
    return new Uint8Array(await res.arrayBuffer());
  } catch {
    // Connexion coupée pendant le corps.
    throw new ApiError(0, null, UNREACHABLE);
  }
}

/**
 * `POST …/versions/{versionId}/restore` : les blocs de la version sont écrits par-dessus le
 * contenu, et ce qu'ils remplacent est gardé dans une version (« avant une restauration »). Le
 * contenu arrive ensuite aux onglets ouverts par la synchro.
 */
export function restoreVersion(pageId: string, versionId: string): Promise<void> {
  return call("POST", `${version(pageId, versionId)}/restore`);
}

/** Longueur maximale d'un titre, en caractères Unicode (le serveur fait foi, R30). */
export const MAX_TITLE_CHARS = 500;

/** Longueur d'un titre comme le serveur la compte : en caractères, pas en unités UTF-16. */
export function titleLength(title: string): number {
  return Array.from(title.trim()).length;
}
