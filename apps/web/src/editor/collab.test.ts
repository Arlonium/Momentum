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
  // Les espions de `console.error` ne restent pas muets pour les tests suivants.
  vi.restoreAllMocks();
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

/** Trame de page du serveur : type, puis un `varUint8Array` (longueur, octets). */
function pageFrame(payload: unknown, type = 100): Uint8Array {
  const body =
    typeof payload === "string" ? new TextEncoder().encode(payload) : payload instanceof Uint8Array
      ? payload
      : new TextEncoder().encode(JSON.stringify(payload));
  return Uint8Array.from([...varUint(type), ...varUint(body.length), ...body]);
}

const ALICE = { id: "11111111-1111-1111-1111-111111111111", name: "Alice", canWrite: true };
const BOB = { id: "22222222-2222-2222-2222-222222222222", name: "Bob", canWrite: false };

describe("page_frame", () => {
  test("page_frame_never_reaches_y_websocket", async () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    for (const readOnly of [false, true]) {
      FakeSocket.instances = [];
      const session = open(readOnly);
      const socket = await connected();
      const seen = vi.fn();
      // y-websocket lirait la trame par son gestionnaire de type 100, s'il en avait un.
      session.provider.messageHandlers[100] = seen;
      socket.receive(pageFrame({ type: "roster", users: [ALICE] }));
      socket.receive(pageFrame({ type: "comments" }));
      socket.receive(pageFrame({ type: "versions" }));
      expect(seen, `lecture seule : ${readOnly}`).not.toHaveBeenCalled();
      expect(errors, `lecture seule : ${readOnly}`).not.toHaveBeenCalled();
    }
  });

  test("roster_frame_updates_the_session", async () => {
    for (const readOnly of [false, true]) {
      FakeSocket.instances = [];
      const session = open(readOnly);
      const changed = vi.fn();
      session.subscribe(changed);
      expect(session.roster).toEqual([]);
      const socket = await connected();
      socket.receive(pageFrame({ type: "roster", users: [ALICE, BOB] }));
      expect(session.roster, `lecture seule : ${readOnly}`).toEqual([ALICE, BOB]);
      expect(changed).toHaveBeenCalled();
      socket.receive(pageFrame({ type: "roster", users: [BOB] }));
      expect(session.roster).toEqual([BOB]);
      // Une ligne mal formée est laissée de côté, les autres restent.
      socket.receive(
        pageFrame({ type: "roster", users: [{ id: 3, name: "x", canWrite: true }, ALICE, null] }),
      );
      expect(session.roster).toEqual([ALICE]);
      // La liste d'une connexion tombée ne reste pas affichée : elle revient avec la suivante.
      socket.readyState = 3;
      socket.onclose?.({ code: 1006 } as CloseEvent);
      expect(session.roster).toEqual([]);
    }
  });

  test("comments_and_versions_frames_reach_the_page_listeners", async () => {
    const session = open(false);
    const seen: string[] = [];
    const stop = session.onPageEvent((event) => seen.push(event));
    const socket = await connected();
    socket.receive(pageFrame({ type: "comments" }));
    socket.receive(pageFrame({ type: "versions" }));
    stop();
    socket.receive(pageFrame({ type: "comments" }));
    expect(seen).toEqual(["comments", "versions"]);
  });

  test("unknown_page_frame_is_dropped_without_an_error", async () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    for (const readOnly of [false, true]) {
      FakeSocket.instances = [];
      const session = open(readOnly);
      const socket = await connected();
      socket.receive(pageFrame({ type: "roster", users: [ALICE] }));
      const events = vi.fn();
      session.onPageEvent(events);
      // Ni effet ni erreur : type inconnu, JSON illisible, tronquée, UTF-8 invalide, sans liste.
      socket.receive(pageFrame({ type: "inconnu" }));
      socket.receive(pageFrame("{pas du json"));
      socket.receive(pageFrame(Uint8Array.from([0xff, 0xfe])));
      socket.receive(pageFrame({ type: "roster" }));
      socket.receive(pageFrame({ type: "roster", users: "Alice" }));
      socket.receive(Uint8Array.from([100]));
      socket.receive(Uint8Array.from([100, 50, 123]));
      expect(events).not.toHaveBeenCalled();
      expect(session.roster, `lecture seule : ${readOnly}`).toEqual([ALICE]);
      expect(errors, `lecture seule : ${readOnly}`).not.toHaveBeenCalled();
    }
  });

  test("a_message_of_another_type_still_goes_to_y_websocket", async () => {
    const session = open(false);
    const socket = await connected();
    const seen = vi.fn();
    session.provider.messageHandlers[101] = seen;
    socket.receive(Uint8Array.from([101, 0]));
    expect(seen).toHaveBeenCalledTimes(1);
  });

  test("page_frame_before_the_first_sync_is_kept", async () => {
    const session = open(false);
    const socket = await connected();
    expect(session.synced).toBe(false);
    socket.receive(pageFrame({ type: "roster", users: [ALICE] }));
    expect(session.roster).toEqual([ALICE]);
    // Et la synchro se fait comme d'habitude derrière elle.
    socket.receive(syncFrame(1, Y.encodeStateAsUpdate(serverDoc())));
    expect(session.synced).toBe(true);
    expect(session.roster).toEqual([ALICE]);
  });

  test("un_message_qui_commence_par_une_trame_est_pris_en_entier", async () => {
    // y-websocket ne lit que le premier message d'un message WebSocket : le reste n'a pas
    // de lecteur, il n'est pas rendu (R163, retiré).
    const session = open(false);
    const socket = await connected();
    const seen = vi.fn();
    session.provider.messageHandlers[101] = seen;
    socket.receive(Uint8Array.from([...pageFrame({ type: "roster", users: [ALICE] }), 101, 0]));
    expect(session.roster).toEqual([ALICE]);
    expect(seen).not.toHaveBeenCalled();
  });

  test("duplicate_accounts_in_a_roster_are_shown_once", async () => {
    const session = open(false);
    const socket = await connected();
    socket.receive(
      pageFrame({ type: "roster", users: [ALICE, BOB, { ...ALICE, name: "Alice bis" }] }),
    );
    expect(session.roster).toEqual([ALICE, BOB]);
  });
});

describe("presence_at_reconnect", () => {
  test("une connexion rétablie annonce un état neuf, pas à la première connexion seulement", async () => {
    const session = open(false);
    session.provider.awareness.setLocalState({ user: { name: "A", color: "#23704F" } });
    const clock = () => session.provider.awareness.meta.get(session.provider.doc.clientID)!.clock;
    const socket = await connected();
    const first = clock();
    // Coupure puis retour, comme y-websocket le fait.
    socket.readyState = 3;
    socket.onclose?.({ code: 1006 } as CloseEvent);
    FakeSocket.instances = [];
    session.provider.disconnect();
    session.provider.connect();
    const next = await connected();
    expect(next).not.toBe(socket);
    expect(clock()).toBeGreaterThan(first);
  });

  test("rien n'est annoncé en lecture seule, ni à la connexion ni au retour", async () => {
    const session = open(true);
    const first = await connected();
    first.readyState = 3;
    first.onclose?.({ code: 1006 } as CloseEvent);
    FakeSocket.instances = [];
    session.provider.disconnect();
    session.provider.connect();
    const next = await connected();
    expect(session.provider.awareness.getLocalState()).toBeNull();
    // Ce qui part sur les sockets : l'étape 1 de la synchro, jamais de présence (type 1).
    const types = [first, next].flatMap((socket) =>
      socket.sent.map((message) => (message as Uint8Array)[0]),
    );
    expect(types).not.toEqual([]);
    expect(types.filter((type) => type !== 0)).toEqual([]);
  });
});

describe("watchdog", () => {
  test("une trame de page compte pour le chien de garde de y-websocket", async () => {
    // y-websocket ferme sa connexion après 30 s sans message qu'il ait vu (il vérifie toutes
    // les 3 s), et le serveur compte ses trames de page dans son délai d'entretien (R107) :
    // une trame qui arrive 28 s après le dernier message visible ne doit pas laisser tomber
    // la connexion. En temps réel : les faux minuteurs ne pilotent pas ce chien de garde.
    const session = open(false);
    const socket = await connected();
    const statuses: string[] = [];
    session.provider.on("status", ({ status }: { status: string }) => statuses.push(status));
    session.provider.wsLastMessageReceived = Date.now() - 28_000;
    socket.receive(pageFrame({ type: "roster", users: [ALICE] }));
    await new Promise((done) => setTimeout(done, 3_500));
    expect(statuses).toEqual([]);
    expect(Date.now() - session.provider.wsLastMessageReceived).toBeLessThan(5_000);
    expect(session.roster).toEqual([ALICE]);
  }, 10_000);
});

describe("read_only_copy", () => {
  test("an_exception_in_the_editor_never_blocks_the_document_that_receives", async () => {
    // Une session en lecture seule : l'éditeur est lié à une copie locale, qui suit le document
    // du serveur (`provider.doc`) dans son événement `update`. Un observateur de la copie qui lève
    // (ce que ferait y-prosemirror sur un changement qu'il ne sait pas montrer) ne doit pas
    // remonter dans la transaction du document qui reçoit : il resterait bloqué, sans plus rien
    // émettre ni appeler, même après la fermeture de l'éditeur.
    const session = open(true, () => new Promise<boolean>(() => {}));
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    const shared = session.provider.doc;
    expect(session.doc).not.toBe(shared);
    let failing = true;
    session.doc.getXmlFragment(FRAGMENT).observeDeep(() => {
      if (failing) throw new Error("éditeur en erreur");
    });
    let updates = 0;
    let observed = 0;
    shared.on("update", () => {
      updates += 1;
    });
    shared.getXmlFragment(FRAGMENT).observeDeep(() => {
      observed += 1;
    });

    const server = serverDoc();
    expect(() => Y.applyUpdate(shared, Y.encodeStateAsUpdate(server))).not.toThrow();
    expect(logged).toHaveBeenCalledTimes(1);
    // Le document qui reçoit n'est pas bloqué : le changement suivant est émis et observé.
    const before = Y.encodeStateVector(server);
    server.getXmlFragment(FRAGMENT).insert(1, [new Y.XmlElement("blockGroup")]);
    failing = false;
    expect(() => Y.applyUpdate(shared, Y.encodeStateAsUpdate(server, before))).not.toThrow();
    expect(updates).toBe(2);
    expect(observed).toBe(2);
    // La copie a reçu les deux : rien n'est perdu pour l'éditeur, qui montre ce qu'il peut.
    expect(session.doc.getXmlFragment(FRAGMENT).length).toBe(2);
  });
});
