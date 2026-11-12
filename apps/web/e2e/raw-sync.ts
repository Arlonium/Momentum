// Une connexion de synchro brute, ouverte par Node avec la session d'un compte : ce qu'un test
// y écrit part tel quel au serveur, comme d'un client écrit pour l'occasion.
import { expect, type APIRequestContext } from "@playwright/test";
import { REGISTRY_VERSION } from "./helpers";

/** Le nom du cookie de session du serveur. */
const SESSION_COOKIE = "mementum_session";

/** Un entier variable de lib0 (7 bits par octet, poids faibles d'abord). */
export function varUint(value: number): number[] {
  const bytes: number[] = [];
  let rest = value;
  while (rest > 0x7f) {
    bytes.push(0x80 | (rest % 0x80));
    rest = Math.floor(rest / 0x80);
  }
  bytes.push(rest);
  return bytes;
}

/** Une connexion de synchro brute : ce qu'on y écrit part tel quel. */
export type RawSync = {
  send(bytes: Uint8Array<ArrayBuffer>): void;
  /**
   * Attend que le serveur ait traité tout ce qui a été envoyé, ou qu'il ait fermé la
   * connexion, et dit lequel.
   */
  settled(): Promise<"servie" | "fermée">;
  /** Le serveur a traité tout ce qui a été envoyé, et la connexion est encore ouverte. */
  processed(): Promise<void>;
  close(): void;
};

/**
 * Ouvre `/sync/{pageId}` avec la session de `api`. Une WebSocket ouverte par Node ne joint pas
 * le cookie d'elle-même : il est posé dans les en-têtes (comme `webSocketWithSession` de
 * `helpers.ts`).
 */
export async function rawSync(
  api: APIRequestContext,
  baseURL: string,
  pageId: string,
): Promise<RawSync> {
  const { cookies } = await api.storageState();
  const session = cookies.find((cookie) => cookie.name === SESSION_COOKIE);
  if (!session) throw new Error("ce client de l'API n'a pas de session");
  const url = `${baseURL.replace(/^http/, "ws")}/sync/${pageId}?schema=${REGISTRY_VERSION}`;
  // Le second argument de la WebSocket de Node (undici) admet des en-têtes ; le type du DOM,
  // lui, ne connaît que les sous-protocoles.
  const options = { headers: { cookie: `${session.name}=${session.value}` } };
  const socket = new WebSocket(url, options as unknown as string[]);
  socket.binaryType = "arraybuffer";
  let answers = 0;
  let closed = false;
  socket.addEventListener("message", (event) => {
    const bytes = new Uint8Array(event.data as ArrayBuffer);
    // Une étape 2 : la réponse du serveur à une étape 1.
    if (bytes[0] === 0 && bytes[1] === 1) answers += 1;
  });
  socket.addEventListener("close", () => {
    closed = true;
  });
  await new Promise<void>((resolve, reject) => {
    socket.addEventListener("open", () => resolve());
    socket.addEventListener("error", () => reject(new Error(`connexion refusée : ${url}`)));
  });
  const settled = async (): Promise<"servie" | "fermée"> => {
    const before = answers;
    // Une étape 1 au vecteur d'état vide : le serveur sert une connexion dans l'ordre, et sa
    // réponse dit que tout ce qui précède est traité.
    if (!closed) socket.send(Uint8Array.from([0, 0, 1, 0]));
    await expect.poll(() => closed || answers > before, { timeout: 10_000 }).toBe(true);
    return closed ? "fermée" : "servie";
  };
  return {
    send: (bytes) => socket.send(bytes),
    settled,
    async processed() {
      expect(await settled()).toBe("servie");
    },
    close: () => socket.close(),
  };
}
