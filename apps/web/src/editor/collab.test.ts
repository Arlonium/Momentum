// `CollabSession.onServerState` : l'éditeur se déverrouille quand l'état du serveur arrive
// (première étape 2 de la synchro), juste avant que y-websocket ne l'applique. Le test passe
// par une WebSocket simulée et de vraies trames y-websocket : si leur format changeait, le
// déverrouillage précoce cesserait sans bruit (l'éditeur se déverrouillerait plus tard, à la
// synchro), d'où ce test. Et la vérification qui précède chaque connexion, en écriture comme
// en lecture seule, et le seul chemin d'un onglet à l'autre : le serveur.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { deferred } from "../test/hooks";
import { CollabSession, FRAGMENT } from "./collab";
import type { ConnectionCheck } from "./schemaGate";

/** WebSocket simulée : le test ouvre la connexion et lui fait recevoir des trames. */
class FakeSocket {
  static instances: FakeSocket[] = [];
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  readonly url: string;
  binaryType: BinaryType = "blob";
  readyState = 0;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  sent: unknown[] = [];

  constructor(url: string | URL) {
    this.url = String(url);
    FakeSocket.instances.push(this);
  }

  send(data: unknown): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = 3;
  }

  open(): void {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }

  receive(frame: Uint8Array): void {
    const data = frame.buffer.slice(frame.byteOffset, frame.byteOffset + frame.byteLength);
    this.onmessage?.({ data } as MessageEvent);
  }
}

/** Entier variable de lib0 (7 bits par octet, poids faibles d'abord). */
function varUint(value: number): number[] {
  const bytes: number[] = [];
  while (value > 0x7f) {
    bytes.push((value & 0x7f) | 0x80);
    value = Math.floor(value / 0x80);
  }
  bytes.push(value);
  return bytes;
}

/** Trame y-websocket « sync » : type 0, étape (0, 1 ou 2), puis la charge et sa longueur. */
function syncFrame(step: 0 | 1 | 2, payload: Uint8Array): Uint8Array {
  return Uint8Array.from([0, step, ...varUint(payload.length), ...payload]);
}

/** Le document du serveur : la racine d'une page. */
function serverDoc(): Y.Doc {
  const doc = new Y.Doc();
  doc.getXmlFragment(FRAGMENT).insert(0, [new Y.XmlElement("blockGroup")]);
  return doc;
}

let sessions: CollabSession[] = [];
let room = 0;

/** L'id d'une page que nul autre test n'ouvre. */
function newPage(): string {
  return `page-${Date.now()}-${room++}`;
}

function open(
  readOnly: boolean,
  check: ConnectionCheck = async () => true,
  pageId = newPage(),
): CollabSession {
  const session = new CollabSession(
    pageId,
    { readOnly, check },
    { name: "Compte de test", color: "#23704F" },
  );
  sessions.push(session);
  return session;
}

/** Laisse passer les promesses déjà tenues, et celles qu'elles tiennent à leur tour. */
async function flush(): Promise<void> {
  await new Promise((done) => setTimeout(done, 0));
}

/** La WebSocket ouverte par la session (après la vérification qui précède la connexion). */
async function connected(): Promise<FakeSocket> {
  await vi.waitFor(() => expect(FakeSocket.instances).toHaveLength(1));
  const socket = FakeSocket.instances[0];
  socket.open();
  return socket;
}

beforeEach(() => {
  FakeSocket.instances = [];
  vi.stubGlobal("WebSocket", FakeSocket);
});

afterEach(() => {
  for (const session of sessions) session.destroy();
  sessions = [];
  vi.unstubAllGlobals();
});

describe("connection_check", () => {
  test("aucune connexion avant la vérification, en écriture comme en lecture seule", async () => {
    for (const readOnly of [false, true]) {
      FakeSocket.instances = [];
      const verdict = deferred<boolean>();
      open(readOnly, () => verdict.promise);
      await flush();
      expect(FakeSocket.instances, `lecture seule : ${readOnly}`).toHaveLength(0);
      verdict.resolve(true);
      await vi.waitFor(() => expect(FakeSocket.instances).toHaveLength(1));
    }
  });

  test("jamais de connexion quand la vérification dit non", async () => {
    for (const readOnly of [false, true]) {
      FakeSocket.instances = [];
      const check = vi.fn(async () => false);
      const session = open(readOnly, check);
      await flush();
      // Ni connexion, ni nouvel essai : l'application remplace ou ferme la session.
      expect(FakeSocket.instances, `lecture seule : ${readOnly}`).toHaveLength(0);
      expect(check).toHaveBeenCalledTimes(1);
      expect(session.synced).toBe(false);
    }
  });
});

describe("same_page_in_two_tabs", () => {
  /** Ce que porte la page dans un document : le nombre de nœuds à sa racine. */
  const written = (doc: Y.Doc) => doc.getXmlFragment(FRAGMENT).length;

  test("deux sessions de la même page ne s'échangent rien hors du serveur", async () => {
    // Deux onglets du même navigateur sur la même page, sans WebSocket : leur vérification
    // attend (page relue, compte à reconnecter, réseau coupé).
    const pageId = newPage();
    const waiting = () => new Promise<boolean>(() => {});
    const writer = open(false, waiting, pageId);
    const reader = open(true, waiting, pageId);
    await flush();

    writer.doc.getXmlFragment(FRAGMENT).insert(0, [new Y.XmlElement("blockGroup")]);
    await flush();
    expect(FakeSocket.instances).toHaveLength(0);
    // Ni le document du serveur de l'autre onglet, ni celui qu'il montre, ne l'ont reçu.
    expect(written(reader.provider.doc)).toBe(0);
    expect(written(reader.doc)).toBe(0);

    // Une session neuve de la page ne le tient pas non plus d'un autre onglet : elle n'aura
    // que ce que le serveur lui donnera, et rien à lui envoyer.
    const next = open(false, waiting, pageId);
    await flush();
    expect(written(next.doc)).toBe(0);
    expect(written(writer.doc)).toBe(1);
  });
});

describe("on_server_state", () => {
  test("appelé une fois, à la première étape 2, avant son application", async () => {
    const server = serverDoc();
    const session = open(false);
    const seen: { synced: boolean; applied: number }[] = [];
    session.onServerState(() =>
      seen.push({
        synced: session.provider.synced,
        applied: session.doc.getXmlFragment(FRAGMENT).length,
      }),
    );
    const socket = await connected();

    // L'étape 1 du serveur et une mise à jour ne déverrouillent rien.
    socket.receive(syncFrame(0, Y.encodeStateVector(server)));
    socket.receive(syncFrame(2, Y.encodeStateAsUpdate(new Y.Doc())));
    expect(seen).toEqual([]);

    socket.receive(syncFrame(1, Y.encodeStateAsUpdate(server)));
    // Avant que y-websocket n'applique l'état : ni synchro, ni racine dans le document.
    expect(seen).toEqual([{ synced: false, applied: 0 }]);
    // Puis l'état est appliqué dans la même tâche.
    expect(session.synced).toBe(true);
    expect(session.doc.getXmlFragment(FRAGMENT).length).toBe(1);

    // Une autre étape 2 (resynchronisation) ne rappelle personne.
    socket.receive(syncFrame(1, Y.encodeStateAsUpdate(server)));
    expect(seen).toHaveLength(1);
  });

  test("jamais après la première synchro", async () => {
    const server = serverDoc();
    const session = open(false);
    const socket = await connected();
    socket.receive(syncFrame(1, Y.encodeStateAsUpdate(server)));
    expect(session.synced).toBe(true);

    const late = vi.fn();
    session.onServerState(late);
    socket.receive(syncFrame(1, Y.encodeStateAsUpdate(server)));
    expect(late).not.toHaveBeenCalled();
  });

  test("jamais en lecture seule", async () => {
    const server = serverDoc();
    const session = open(true);
    const listener = vi.fn();
    session.onServerState(listener);
    const socket = await connected();
    socket.receive(syncFrame(0, Y.encodeStateVector(server)));
    socket.receive(syncFrame(1, Y.encodeStateAsUpdate(server)));
    // La session a bien reçu l'état du serveur, sans déverrouiller quoi que ce soit.
    expect(session.synced).toBe(true);
    expect(session.doc.getXmlFragment(FRAGMENT).length).toBe(1);
    expect(listener).not.toHaveBeenCalled();
  });

  test("un écouteur retiré n'est pas appelé", async () => {
    const session = open(false);
    const listener = vi.fn();
    const stop = session.onServerState(listener);
    stop();
    const socket = await connected();
    socket.receive(syncFrame(1, Y.encodeStateAsUpdate(serverDoc())));
    expect(session.synced).toBe(true);
    expect(listener).not.toHaveBeenCalled();
  });
});
