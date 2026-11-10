// Client REST du serveur, typé par les types générés de `packages/protocol` (ts-rs).
// Les corps refusent les champs inconnus (`deny_unknown_fields`, R30) : on n'envoie que
// ceux des types de `packages/protocol`.
//
// La session est un cookie (`HttpOnly`) que le navigateur joint à chaque appel. Un appel qui
// répond `401 unauthenticated` dit qu'elle est perdue : `onSessionLost` prévient
// `SessionGate`, qui demande la connexion. Les autres `401` (`invalid_credentials` : mot de
// passe faux) ne disent rien de la session.
import type {
  ChangePassword,
  CreatePage,
  ErrorBody,
  ErrorCode,
  Info,
  LogIn,
  Me,
  PageNode,
  SignUp,
  Tree,
  UpdatePage,
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
  // `204` : pas de corps.
  if (res.ok) return (res.status === 204 ? undefined : await res.json()) as T;
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

/** `GET /api/v1/info` : version du serveur et de son schéma de blocs. */
export function getInfo(): Promise<Info> {
  return call("GET", "/api/v1/info");
}

/** `GET /api/v1/tree` : toutes les pages du coffre. */
export function getTree(): Promise<Tree> {
  return call("GET", "/api/v1/tree");
}

/** `POST /api/v1/pages` : sans `afterId`, la page va en fin de ses frères (R23). */
export function createPage(body: CreatePage): Promise<PageNode> {
  return call("POST", "/api/v1/pages", body);
}

/** `PATCH /api/v1/pages/{id}` : un champ absent ne change rien. */
export function updatePage(id: string, body: UpdatePage): Promise<PageNode> {
  return call("PATCH", `/api/v1/pages/${encodeURIComponent(id)}`, body);
}

/** Longueur maximale d'un titre, en caractères Unicode (le serveur fait foi, R30). */
export const MAX_TITLE_CHARS = 500;

/** Longueur d'un titre comme le serveur la compte : en caractères, pas en unités UTF-16. */
export function titleLength(title: string): number {
  return Array.from(title.trim()).length;
}
