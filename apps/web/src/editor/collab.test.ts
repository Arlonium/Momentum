// `CollabSession.onServerState` : l'éditeur se déverrouille quand l'état du serveur arrive
// (première étape 2 de la synchro), juste avant que y-websocket ne l'applique. Le test passe
// par une WebSocket simulée et de vraies trames y-websocket : si leur format changeait, le
// déverrouillage précoce cesserait sans bruit (l'éditeur se déverrouillerait plus tard, à la
// synchro), d'où ce test.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { CollabSession, FRAGMENT } from "./collab";

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

function open(readOnly: boolean): CollabSession {
  // Une salle par session : y-websocket relie par BroadcastChannel les onglets d'une salle.
  const pageId = `page-${Date.now()}-${room++}`;
  const session = new CollabSession(
    pageId,
    readOnly ? { readOnly: true } : { readOnly: false, check: async () => true },
    { name: "Compte de test", color: "#23704F" },
  );
  sessions.push(session);
  return session;
}

/** La WebSocket ouverte par la session (après la vérification du schéma). */
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
