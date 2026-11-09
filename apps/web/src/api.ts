// Client REST du serveur, typé par les types générés de `packages/protocol` (ts-rs).
// Les corps refusent les champs inconnus (`deny_unknown_fields`, R30) : on n'envoie que
// ceux de `CreatePage` et `UpdatePage`.
import type {
  CreatePage,
  ErrorBody,
  ErrorCode,
  Info,
  PageNode,
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
  if (res.ok) return (await res.json()) as T;
  let error: Partial<ErrorBody> = {};
  try {
    error = (await res.json()) as ErrorBody;
  } catch {
    // corps illisible (mandataire, page d'erreur) : message générique ci-dessous
  }
  throw new ApiError(
    res.status,
    error.error ?? null,
    error.message ?? (res.status >= 500 ? UNREACHABLE : `Erreur ${res.status} du serveur.`),
  );
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
