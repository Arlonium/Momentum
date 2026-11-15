// Client REST du serveur, typé par les types générés de `packages/protocol` (ts-rs).
// Les corps refusent les champs inconnus (`deny_unknown_fields`, R30) : on n'envoie que
// ceux des types de `packages/protocol`.
//
// La session est un cookie (`HttpOnly`) que le navigateur joint à chaque appel. Un appel qui
// répond `401 unauthenticated` dit qu'elle est perdue : `onSessionLost` prévient
// `SessionGate`, qui demande la connexion. Les autres `401` (`invalid_credentials` : mot de
// passe faux) ne disent rien de la session.
//
// Sauf un refus qui vise une session que l'onglet a remplacée depuis (Rulings R347, R348).
// Chaque appel part daté de la génération de session de l'onglet, qui avance à chaque session
// qu'il ouvre (connexion, inscription, changement de mot de passe : le cookie change) ou ferme
// (déconnexion). Un appel parti avant, dont le `401` revient après, ne dit rien de la session en
// cours : il ne demande pas la connexion, et il repart une fois, tel quel, sous la session en
// cours, aux conditions de `judge` : jamais une route de session, jamais sous un autre compte
// que celui sous lequel il est parti (le compte est relu juste avant), jamais s'il est parti
// avant une déconnexion de l'onglet, ou pendant. Un `401 unauthenticated` est le premier refus du
// serveur, avant toute lecture ou écriture : l'appel refusé n'a rien fait, et son rejeu le fait
// une fois. Un refus reçu pendant qu'une session s'ouvre ou se ferme attend son issue.
import type {
  Accepted,
  ChangePassword,
  Comment,
  CreatePage,
  CreateSpace,
  ErrorBody,
  ErrorCode,
  Favorites,
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
 * Appelle `listener` chaque fois qu'un appel parti sous la session en cours répond `401
 * unauthenticated` (session absente, échue ou fermée ailleurs). Pas pour un appel parti avant
 * que l'onglet ouvre ou ferme une session, que ce refus ne concerne plus (voir l'en-tête). Rend
 * la fonction qui le retire.
 */
export function onSessionLost(listener: () => void): () => void {
  sessionLost.add(listener);
  return () => sessionLost.delete(listener);
}

/** L'erreur dit-elle que la session est perdue (et non, par exemple, un mot de passe faux) ? */
export function isSessionLost(error: unknown): boolean {
  return error instanceof ApiError && error.code === "unauthenticated";
}

const ME = "/api/v1/me";
const SIGNUP = "/api/v1/auth/signup";
const LOGIN = "/api/v1/auth/login";
const LOGOUT = "/api/v1/auth/logout";
const PASSWORD = "/api/v1/auth/password";
/** Les routes qui ouvrent ou ferment la session : jamais rejouées. */
const SESSION_ROUTES: ReadonlySet<string> = new Set([SIGNUP, LOGIN, LOGOUT, PASSWORD]);

/** La session de l'onglet au moment où un appel part (Ruling R347). */
type Stamp = {
  generation: number;
  account: string | null;
  epoch: number;
};

/**
 * La génération de session de l'onglet : avance à chaque session qu'il ouvre (connexion,
 * inscription, changement de mot de passe) ou ferme (déconnexion, réussie ou non).
 */
let generation = 0;
/**
 * Le compte de la session, tel que le serveur l'a dit en dernier pour la génération en cours ;
 * `null` : pas encore dit, ou déconnexion demandée.
 */
let account: string | null = null;
/** Avance chaque fois que `account` change : un autre compte, ou la déconnexion. */
let epoch = 0;
/** Les sessions qui s'ouvrent ou se ferment en ce moment : un refus de session attend leur issue. */
const changing = new Set<Promise<void>>();

const stamp = (): Stamp => ({ generation, account, epoch });

function noteAccount(id: string | null) {
  if (id === account) return;
  account = id;
  epoch += 1;
}

/** L'onglet vient d'ouvrir une session pour le compte `id`. */
function opened(id: string) {
  generation += 1;
  noteAccount(id);
}

/** Mène `run`, qui ouvre ou ferme la session de l'onglet : un refus de session attend son issue. */
async function changeSession<T>(run: () => Promise<T>): Promise<T> {
  const done = run();
  const settled = done.then(
    () => undefined,
    () => undefined,
  );
  changing.add(settled);
  try {
    return await done;
  } finally {
    changing.delete(settled);
  }
}

/**
 * Ce que dit le `401 unauthenticated` d'un appel parti sous `sent` (Rulings R347, R348) :
 * - `lost` : la session en cours est perdue, la connexion est demandée ;
 * - `stale` : le refus vise une session que l'onglet a remplacée ou fermée depuis, et ne dit
 *   rien de la session en cours ; l'appel ne repart pas ;
 * - `replay` : de même, et l'appel repart une fois sous la session en cours. Il faut qu'il ne
 *   soit pas déjà reparti, que ce ne soit pas une route de session, qu'il soit parti sous un
 *   compte connu, que l'onglet n'ait vu depuis ni un autre compte ni une déconnexion, et que le
 *   compte relu à l'instant (`GET /api/v1/me`) soit celui sous lequel il est parti.
 * Un échec de cette relecture est levé à la place du refus : l'appel le dit (un refus de session
 * de la relecture a été jugé par elle, et a demandé la connexion s'il le fallait).
 */
async function judge(
  sent: Stamp,
  path: string,
  replays: number,
): Promise<"lost" | "stale" | "replay"> {
  const sessionRoute = SESSION_ROUTES.has(path);
  // Une session qui s'ouvre ou se ferme a pu causer ce refus : il attend de savoir. Pas une route
  // de session, qui attendrait sa propre issue.
  while (!sessionRoute && sent.generation === generation && changing.size > 0) {
    await Promise.all(changing);
  }
  if (sent.generation === generation) return "lost";
  if (sessionRoute || replays > 0 || sent.account === null || sent.epoch !== epoch) return "stale";
  const verified = await sessionAccount();
  // Le compte a pu changer pendant la relecture (une connexion de l'onglet sous un autre) : sa
  // réponse, partie avant, ne dit plus rien.
  return verified === sent.account && sent.epoch === epoch ? "replay" : "stale";
}

/** Le compte de la session, relu, sans rejeu : celui que le navigateur porte à l'instant. */
async function sessionAccount(): Promise<string> {
  const at = generation;
  const me = (await (await send("GET", ME, undefined, undefined, 1)).json()) as Me;
  if (at === generation) noteAccount(me.user.id);
  return me.user.id;
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await send(method, path, body);
  // `204` : pas de corps.
  return (res.status === 204 ? undefined : await res.json()) as T;
}

/**
 * Une réponse réussie, sinon l'erreur du serveur (ou `UNREACHABLE`), levée en `ApiError`.
 * Une requête annulée par `signal` rejette avec l'erreur de l'annulation, telle quelle. Un
 * refus de session est jugé par `judge` ; `replays` : combien de fois l'appel est déjà reparti
 * (1 : il ne repartira pas).
 */
async function send(
  method: string,
  path: string,
  body?: unknown,
  signal?: AbortSignal,
  replays = 0,
): Promise<Response> {
  for (; ; replays += 1) {
    const sent = stamp();
    let res: Response;
    try {
      res = await fetch(path, {
        method,
        headers: body === undefined ? undefined : { "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal,
      });
    } catch (e) {
      if (signal?.aborted) throw e;
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
    if (!isSessionLost(failure)) throw failure;
    const verdict = await judge(sent, path, replays);
    if (verdict === "replay") continue;
    if (verdict === "lost") for (const listener of [...sessionLost]) listener();
    throw failure;
  }
}

/** `GET /api/v1/me` : le compte de la session et ses espaces ; sans session, `401`. */
export async function getMe(): Promise<Me> {
  const at = generation;
  const me = await call<Me>("GET", ME);
  // Une réponse d'une session remplacée depuis ne dit pas le compte de la session en cours.
  if (at === generation) noteAccount(me.user.id);
  return me;
}

/** `POST /api/v1/auth/signup` : crée le compte et ouvre sa session (cookie). */
export function signUp(body: SignUp): Promise<Me> {
  return changeSession(async () => {
    const me = await call<Me>("POST", SIGNUP, body);
    opened(me.user.id);
    return me;
  });
}

/** `POST /api/v1/auth/login` : ouvre une session (cookie) ; `401 invalid_credentials`. */
export function logIn(body: LogIn): Promise<Me> {
  return changeSession(async () => {
    const me = await call<Me>("POST", LOGIN, body);
    opened(me.user.id);
    return me;
  });
}

/** `POST /api/v1/auth/logout` : supprime la session sur le serveur et efface le cookie. */
export function logOut(): Promise<void> {
  // Rien de ce qui est parti avant, ou part pendant, ne sera rejoué : ni sous ce compte, même
  // reconnecté, ni sous un autre.
  noteAccount(null);
  return changeSession(async () => {
    try {
      await call<void>("POST", LOGOUT);
    } finally {
      // Échouée, elle a pu fermer la session quand même (réponse perdue) : les refus de ce qui
      // était parti avant ne disent rien de plus.
      generation += 1;
    }
  });
}

/**
 * `POST /api/v1/auth/password` : les autres sessions du compte sont fermées, celle-ci
 * repart avec un cookie neuf. Mot de passe actuel faux : `401 invalid_credentials`.
 */
export function changePassword(body: ChangePassword): Promise<void> {
  return changeSession(async () => {
    await call<void>("POST", PASSWORD, body);
    // Le même compte, sous une session neuve : ce qui était parti sous l'ancienne est refusé.
    generation += 1;
  });
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
 * `DELETE /api/v1/pages/{id}/comments/{threadId}` : tout le fil part, avec les messages de tous
 * et leurs réactions, en une écriture (R352). Il faut l'accès complet (`403` en dessous) ; un fil
 * qui n'est plus là répond `404`.
 */
export function deleteThread(pageId: string, threadId: string): Promise<void> {
  return call("DELETE", thread(pageId, threadId));
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
 * `signal` annule le téléchargement (on a choisi une autre version, ou fermé la fenêtre).
 */
export async function getVersionState(
  pageId: string,
  versionId: string,
  signal?: AbortSignal,
): Promise<Uint8Array> {
  const res = await send("GET", `${version(pageId, versionId)}/state`, undefined, signal);
  try {
    return new Uint8Array(await res.arrayBuffer());
  } catch (e) {
    if (signal?.aborted) throw e;
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

// Favoris (R254) : à chaque compte les siens, par espace. Poser ou retirer un favori demande de
// voir la page (`read`, à la corbeille aussi), et n'avance pas la version de l'espace : la barre
// latérale des autres n'en sait rien, et l'onglet qui le pose relit sa liste lui-même.

/**
 * `GET /api/v1/spaces/{id}/favorites` : les pages favorites du compte dans l'espace, dans l'ordre
 * où il les a ajoutées ; seulement celles qu'il voit, hors corbeille.
 */
export function getFavorites(spaceId: string): Promise<Favorites> {
  return call("GET", `${space(spaceId)}/favorites`);
}

/**
 * `PUT` (`on`) ou `DELETE …/pages/{id}/favorite` : pose ou retire le favori, `204`, sans effet
 * s'il est déjà dans cet état ; une page qu'on ne voit plus répond `404`, comme une page inconnue.
 */
export function setFavorite(pageId: string, on: boolean): Promise<void> {
  return call(on ? "PUT" : "DELETE", `${page(pageId)}/favorite`);
}

/** Longueur maximale d'un titre, en caractères Unicode (le serveur fait foi, R30). */
export const MAX_TITLE_CHARS = 500;

/** Longueur d'un titre comme le serveur la compte : en caractères, pas en unités UTF-16. */
export function titleLength(title: string): number {
  return Array.from(title.trim()).length;
}
