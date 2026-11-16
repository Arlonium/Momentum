// Client REST du serveur, typé par les types générés de `packages/protocol` (ts-rs).
// Les corps refusent les champs inconnus (`deny_unknown_fields`, R30) : on n'envoie que
// ceux des types de `packages/protocol`.
//
// La session est un cookie (`HttpOnly`) que le navigateur joint à chaque appel. Un appel qui
// répond `401 unauthenticated` dit qu'elle est perdue : `onSessionLost` prévient
// `SessionGate`, qui demande la connexion. Les autres `401` (`invalid_credentials` : mot de
// passe faux) ne disent rien de la session.
//
// Sauf un refus qui vise une session que l'onglet a remplacée depuis (Rulings R347, R348, R460).
// Chaque appel part daté de la génération de session de l'onglet, qui avance à chaque session
// qu'il ouvre (connexion, inscription, changement de mot de passe : le cookie change) ou ferme
// (déconnexion). Un appel parti avant, dont le `401` revient après, ne dit rien de la session en
// cours : il ne demande pas la connexion, et il repart une fois, tel quel, écritures comprises,
// sous la session en cours, aux conditions de `judge` : jamais une route de session, jamais pour
// un autre compte que celui de l'application, pour qui il est parti (le compte de la session est
// relu juste avant), jamais s'il est parti avant une déconnexion de l'onglet, ou pendant. Un `401
// unauthenticated` est le premier refus du serveur, avant toute lecture ou écriture : l'appel
// refusé n'a rien fait, et son rejeu le fait une fois. Un refus reçu pendant qu'une session
// s'ouvre ou se ferme attend son issue.
//
// Le compte attendu (Rulings R540, R543). Le cookie est commun à tous les onglets du navigateur,
// et la page ne le lit pas : un autre onglet qui se connecte sous un autre compte fait partir les
// appels de celui-ci sous ce compte, un rejeu comme tout autre appel (R115, R460). Chaque appel
// nomme donc le compte de l'application, tel qu'il était à son premier envoi, par l'en-tête
// `X-Mementum-Account` (son rejeu aussi, jamais le compte du moment) ; le serveur refuse par `409
// account_changed`, sans rien faire, un appel qui ne nomme pas le compte de la session. Pas
// d'en-tête tant que l'application n'a pas de compte, ni pour `GET /api/v1/me`, qui sert à
// l'apprendre, ni pour la connexion, l'inscription et la déconnexion. Reçu, ce refus ne repart
// jamais : le compte de la session est relu, et s'il est un autre, l'application le sait
// (`onOtherAccount`) et repart de zéro (R464). La relecture d'avant un rejeu reste : elle évite
// d'envoyer ce que le serveur refuserait, et dit l'autre compte plus tôt (R544).
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
const otherAccount = new Set<(me: Me) => void>();

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

/**
 * Appelle `listener` quand la relecture du compte qu'`api.ts` fait avant un rejeu trouve la
 * session à un autre compte que celui de l'application : un autre onglet du navigateur s'y est
 * connecté. L'application repart alors de zéro (R115, `SessionGate`), sans attendre de relire le
 * compte elle-même (R464). Rend la fonction qui le retire.
 */
export function onOtherAccount(listener: (me: Me) => void): () => void {
  otherAccount.add(listener);
  return () => otherAccount.delete(listener);
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

/** L'en-tête qui nomme le compte sous lequel l'appel part (R540). */
export const ACCOUNT_HEADER = "x-mementum-account";
/**
 * Les routes qui ne nomment pas de compte : la relecture du compte (elle sert à l'apprendre), et
 * celles qui ouvrent ou ferment une session sans en lire (le serveur n'y regarde pas l'en-tête ;
 * se déconnecter ferme la session du navigateur, quel que soit son compte, R542). Le changement de
 * mot de passe, lui, en nomme un : il vise le compte de la session.
 */
const UNNAMED_ROUTES: ReadonlySet<string> = new Set([ME, SIGNUP, LOGIN, LOGOUT]);

/**
 * Combien de temps un refus de session attend, au plus, l'issue d'une session qui s'ouvre ou se
 * ferme (ms) : au-delà, il est jugé sans elle (R462).
 */
const SESSION_CHANGE_WAIT_MS = 10_000;

/** La session de l'onglet au moment où un appel part (Ruling R347). */
type Stamp = {
  generation: number;
  /**
   * `acting` : le compte pour lequel l'appel part, s'il est sûr ; sinon `null` (pendant une
   * déconnexion aussi, R461).
   */
  account: string | null;
  epoch: number;
};

/**
 * La génération de session de l'onglet : avance à chaque session qu'il ouvre (connexion,
 * inscription, changement de mot de passe) ou ferme (déconnexion, réussie ou non).
 */
let generation = 0;
/**
 * Le compte de l'application : celui qu'elle a chargé (premier `GET /api/v1/me` qui répond), ou
 * celui de la session que l'onglet a ouverte ; `null` : pas encore chargée. Ni une relecture qui
 * dit un autre compte, ni une déconnexion ne le changent : l'application va repartir de zéro
 * (R115 ; rechargée après une déconnexion), et elle agit jusque-là pour celui-ci, après une
 * déconnexion qui échoue aussi (R461).
 */
let appAccount: string | null = null;
/**
 * Le compte de la session, tel que le serveur l'a dit en dernier pour la génération en cours
 * (un autre onglet du navigateur a pu s'y connecter sous un autre) ; `null` : pas encore dit.
 */
let sessionAccount: string | null = null;
/**
 * Le compte pour lequel un appel part, quand l'application et la session s'accordent ; `null`
 * sinon : un appel parti alors n'est jamais rejoué (R348).
 */
let acting: string | null = null;
/** Avance chaque fois que `acting` change : un autre compte, ou la déconnexion. */
let epoch = 0;
/**
 * Les déconnexions de l'onglet en route (R461). Tant qu'il y en a une, aucun appel n'est daté
 * d'un compte : une relecture du compte répondue pendant elle ne le réarme pas.
 */
let loggingOut = 0;
/** Les sessions qui s'ouvrent ou se ferment en ce moment : un refus de session attend leur issue. */
const changing = new Set<Promise<void>>();
/**
 * La relecture du compte en route, et la génération où elle est partie : les refus de cette
 * génération qui arrivent pendant qu'elle est en route la partagent (R463). Une relecture
 * répondue ne sert plus : le cookie a pu changer depuis.
 */
let reading: { generation: number; account: Promise<string> } | null = null;

const stamp = (): Stamp => ({ generation, account: loggingOut > 0 ? null : acting, epoch });

/**
 * Le compte que les appels de l'onglet nomment (R540, R541) : celui de l'application, `null` tant
 * qu'elle n'en a pas. Pas `acting` : celui-ci ne vaut `null` que là où un rejeu est défendu
 * (déconnexion en route, session connue d'un autre compte), et c'est justement là que le serveur
 * doit refuser ce qui partirait sous un autre compte (R543). Les connexions de synchro et le canal
 * de l'espace le donnent en paramètre d'adresse, faute d'en-tête.
 */
export function expectedAccount(): string | null {
  return appAccount;
}

/** Recalcule `acting`, et avance `epoch` s'il change. */
function settleAccounts() {
  const now = sessionAccount !== null && sessionAccount === appAccount ? appAccount : null;
  if (now === acting) return;
  acting = now;
  epoch += 1;
}

/**
 * Le serveur vient de dire le compte de la session en cours. `loads` : c'est une lecture de
 * l'application (`getMe`), qui la charge sous ce compte si elle ne l'est pas encore.
 */
function sessionIs(id: string, loads: boolean) {
  if (loads && appAccount === null) appAccount = id;
  sessionAccount = id;
  settleAccounts();
}

/** L'onglet vient d'ouvrir une session pour le compte `id` : l'application est à lui. */
function opened(id: string) {
  generation += 1;
  appAccount = id;
  sessionAccount = id;
  settleAccounts();
}

/**
 * Une ouverture de session de l'onglet est restée sans réponse lisible : le serveur a pu l'ouvrir,
 * et le navigateur poser son cookie, sans que la page le sache. Le compte de la session n'est plus
 * sûr jusqu'à la prochaine relecture de l'application, et rien de ce qui était parti avant ne
 * repart (R480).
 */
function sessionUncertain() {
  generation += 1;
  sessionAccount = null;
  settleAccounts();
}

/**
 * Mène `run`, qui ouvre une session. Seul un refus du serveur (une `ApiError` d'un statut, comme un
 * mot de passe faux) laisse la session sûre ; toute autre issue la rend incertaine : pas de
 * réponse (statut 0), ou une réponse dont les en-têtes sont arrivés, cookie compris, et le corps
 * non (la lecture du corps lève une `TypeError`).
 */
async function opening<T>(run: () => Promise<T>): Promise<T> {
  try {
    return await run();
  } catch (e) {
    if (!(e instanceof ApiError) || e.status === 0) sessionUncertain();
    throw e;
  }
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
 * Attend que les sessions qui s'ouvrent ou se ferment en ce moment aient fini,
 * `SESSION_CHANGE_WAIT_MS` au plus : `false` si la borne est passée avant (R462).
 */
async function sessionChangesSettle(): Promise<boolean> {
  if (changing.size === 0) return true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const bound = new Promise<false>((done) => {
    timer = setTimeout(() => done(false), SESSION_CHANGE_WAIT_MS);
  });
  try {
    while (changing.size > 0) {
      const settled = await Promise.race([Promise.all(changing).then(() => true as const), bound]);
      if (!settled) return false;
    }
    return true;
  } finally {
    clearTimeout(timer);
  }
}

/**
 * Ce que dit le `401 unauthenticated` d'un appel parti sous `sent` (Rulings R347, R348, R460 à
 * R462) :
 * - `lost` : la session en cours est perdue, la connexion est demandée ;
 * - `stale` : le refus vise une session que l'onglet a remplacée ou fermée depuis, et ne dit
 *   rien de la session en cours ; l'appel ne repart pas ;
 * - `replay` : de même, et l'appel repart une fois sous la session en cours. Il faut qu'il ne
 *   soit pas déjà reparti, que ce ne soit pas une route de session, qu'il soit parti pour un
 *   compte sûr (`acting` : celui de l'application, quand la session était à lui, hors d'une
 *   déconnexion), que ce compte n'ait pas changé depuis (ni un autre, ni la déconnexion), et que
 *   le compte relu à l'instant (`GET /api/v1/me`) soit le sien.
 * Hors des routes de session, une session qui s'ouvre ou se ferme est attendue avant de juger : sa
 * réponse peut rendre le refus ancien, ou changer le compte ; à la borne, le refus est jugé sans
 * elle, et ne repart pas. Une session qui s'ouvre pendant la relecture est attendue de même, puis
 * le compte est relu une seconde fois (R462). Un échec de la relecture
 * est levé à la place du refus : l'appel le dit (un refus de session de la relecture a été jugé
 * par elle, et a demandé la connexion s'il le fallait).
 */
async function judge(
  sent: Stamp,
  path: string,
  replays: number,
): Promise<"lost" | "stale" | "replay"> {
  // Une route de session n'attend pas : elle attendrait sa propre issue.
  if (SESSION_ROUTES.has(path)) return sent.generation === generation ? "lost" : "stale";
  if (!(await sessionChangesSettle())) return sent.generation === generation ? "lost" : "stale";
  if (sent.generation === generation) return "lost";
  if (replays > 0 || sent.account === null || sent.epoch !== epoch) return "stale";
  let verified = await readSessionAccount();
  if (changing.size > 0) {
    // Une session de l'onglet s'ouvre ou se ferme pendant la relecture : le serveur a pu poser son
    // cookie avant que la page lise sa réponse, et la relecture, partie avant, ne le dit pas. On
    // attend son issue, puis on relit : le rejeu suit toujours de près la relecture qui le permet.
    if (!(await sessionChangesSettle()) || sent.epoch !== epoch) return "stale";
    verified = await readSessionAccount();
    if (changing.size > 0) return "stale";
  }
  return verified === sent.account && sent.epoch === epoch ? "replay" : "stale";
}

/**
 * Le compte de la session, relu, sans rejeu : celui que le navigateur porte à l'instant. Une
 * relecture déjà en route pour la génération en cours est partagée (R463). Si elle trouve un
 * autre compte que celui de l'application, `onOtherAccount` le dit (R464), à l'issue des sessions
 * de l'onglet en route : une connexion de l'onglet peut rendre la session à l'application (R73).
 */
function readSessionAccount(): Promise<string> {
  if (reading?.generation === generation) return reading.account;
  const at = generation;
  const account = (async () => {
    const me = (await (await send("GET", ME, undefined, undefined, 1)).json()) as Me;
    if (at === generation) {
      sessionIs(me.user.id, false);
      if (appAccount !== null && me.user.id !== appAccount) tellOtherAccount(me);
    }
    return me.user.id;
  })();
  const entry = { generation: at, account };
  reading = entry;
  void account
    .then(
      () => undefined,
      () => undefined,
    )
    .then(() => {
      if (reading === entry) reading = null;
    });
  return account;
}

/**
 * Dit `me` à `onOtherAccount`, aussitôt si aucune session de l'onglet n'est en route ; sinon à leur
 * issue, si la session est toujours à ce compte et que ce n'est pas celui de l'application. À la
 * borne de l'attente, rien n'est dit : l'application l'apprendra en relisant le compte (R126).
 */
function tellOtherAccount(me: Me) {
  const tell = () => {
    for (const listener of [...otherAccount]) listener(me);
  };
  if (changing.size === 0) {
    tell();
    return;
  }
  void sessionChangesSettle().then((settled) => {
    if (settled && sessionAccount === me.user.id && appAccount !== null && appAccount !== me.user.id) {
      tell();
    }
  });
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
 * (1 : il ne repartira pas). Le corps est écrit une fois : un rejeu part tel que l'appel est
 * parti, même si l'appelant a changé l'objet depuis.
 */
async function send(
  method: string,
  path: string,
  body?: unknown,
  signal?: AbortSignal,
  replays = 0,
): Promise<Response> {
  // Le compte nommé est celui du premier envoi : le rejeu le garde (R540, R543).
  const account = UNNAMED_ROUTES.has(path) ? null : appAccount;
  const headers: Record<string, string> = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (account !== null) headers[ACCOUNT_HEADER] = account;
  const payload = body === undefined ? undefined : JSON.stringify(body);
  for (; ; replays += 1) {
    const sent = stamp();
    let res: Response;
    try {
      res = await fetch(path, { method, headers, body: payload, signal });
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
    if (failure.code === "account_changed") {
      // La session est celle d'un autre compte : l'appel n'a rien fait, et ne repart jamais. Le
      // compte est relu ; si c'est bien un autre, `onOtherAccount` le dit (R464, R543).
      await readSessionAccount().catch(() => {});
      throw failure;
    }
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
  if (at === generation) sessionIs(me.user.id, true);
  return me;
}

/** `POST /api/v1/auth/signup` : crée le compte et ouvre sa session (cookie). */
export function signUp(body: SignUp): Promise<Me> {
  return changeSession(() =>
    opening(async () => {
      const me = await call<Me>("POST", SIGNUP, body);
      opened(me.user.id);
      return me;
    }),
  );
}

/** `POST /api/v1/auth/login` : ouvre une session (cookie) ; `401 invalid_credentials`. */
export function logIn(body: LogIn): Promise<Me> {
  return changeSession(() =>
    opening(async () => {
      const me = await call<Me>("POST", LOGIN, body);
      opened(me.user.id);
      return me;
    }),
  );
}

/** `POST /api/v1/auth/logout` : supprime la session sur le serveur et efface le cookie. */
export function logOut(): Promise<void> {
  // Rien de ce qui est parti avant, ou part pendant, ne sera rejoué : ni sous ce compte, même
  // reconnecté, ni sous un autre (R460, R461). La session n'a plus de compte sûr ; l'application
  // garde le sien.
  loggingOut += 1;
  sessionAccount = null;
  settleAccounts();
  return changeSession(async () => {
    try {
      await call<void>("POST", LOGOUT);
    } finally {
      // Échouée, elle a pu fermer la session quand même (réponse perdue) : les refus de ce qui
      // était parti avant ne disent rien de plus.
      generation += 1;
      loggingOut -= 1;
    }
  });
}

/**
 * `POST /api/v1/auth/password` : les autres sessions du compte sont fermées, celle-ci
 * repart avec un cookie neuf. Mot de passe actuel faux : `401 invalid_credentials`.
 */
export function changePassword(body: ChangePassword): Promise<void> {
  return changeSession(() =>
    opening(async () => {
      await call<void>("POST", PASSWORD, body);
      // Le même compte, sous une session neuve : ce qui était parti sous l'ancienne est refusé.
      generation += 1;
    }),
  );
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
