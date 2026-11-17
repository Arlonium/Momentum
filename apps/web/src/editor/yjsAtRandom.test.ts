// Ce que le vrai Yjs écrit, tiré au hasard, rejoué dans la garde du serveur (garde des mises à
// jour, tour 4 : la grammaire d'un client, Rulings R801 à R806).
//
// Chaque scène : deux ou trois onglets (identifiants dans un ordre au hasard) écrivent une page
// servie : frappes, effacements, marques, attributs écrits, récrits et retirés, blocs ajoutés et
// supprimés, annuler et rétablir (`UndoManager`, qui garde les éléments supprimés : c'est lui qui
// fait fondre des entrées de map en une entrée de plusieurs valeurs), plusieurs gestes par
// transaction, onglets hors ligne puis revenus (étape 2 depuis l'état du serveur, ou depuis un
// état plus ancien : des éléments à cheval), mises à jour fusionnées, mises à jour redites. La
// scène est la suite des mises à jour que le serveur reçoit, dans l'ordre.
//
// Ici, sur les éléments que Yjs garde : chaque règle de la grammaire (`guard/grammar.rs`). Les
// scènes sont écrites dans `crates/mementum-crdt/tests/fixtures/yjs_updates/random.json`, que
// `crates/mementum-crdt/tests/yjs_updates.rs` donne à une vraie page comme celles d'un client :
// aucune n'est refusée, et le journal relu donne la page vivante.
//
// `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/yjsAtRandom.test.ts` les
// réécrit ; sans la variable, le test les compare. `MEMENTUM_FUZZ_SEEDS=6000` élargit l'essai, et
// écrit alors `random-large.json` (hors du dépôt, `.gitignore`), que le test Rust lit s'il est
// là. À relancer à une montée de Yjs.
import { expect, test } from "vitest";
import * as Y from "yjs";

import { nodeModule, type Fs, type Path, type Url } from "../test/node";

const SCENES = 150;
const REMOTE = "remote";

function random(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

type Tab = { doc: Y.Doc; undo: Y.UndoManager; online: boolean; seen: Uint8Array; sent: Uint8Array[] };

type Counts = { keyed: number; manyValued: number; manyValuedAlive: number; sameOrigins: number; updates: number };

const KEYS = ["backgroundColor", "textColor", "textAlignment"] as const;
const VALUES: Record<(typeof KEYS)[number], string[]> = {
  backgroundColor: ["default", "red", "blue", "green"],
  textColor: ["default", "red", "blue"],
  textAlignment: ["left", "center", "right"],
};

type Stored = {
  id: { client: number; clock: number };
  length: number;
  deleted: boolean;
  parentSub?: string | null;
  origin?: { client: number; clock: number } | null;
  rightOrigin?: { client: number; clock: number } | null;
  content?: { constructor: { name: string } };
};

/** Les règles de la grammaire, sur les éléments que garde un document de Yjs. */
function judge(doc: Y.Doc, counts: Counts, what: string): void {
  const clients = (doc as unknown as { store: { clients: Map<number, Stored[]> } }).store.clients;
  for (const structs of clients.values()) {
    for (const s of structs) {
      if (s.content === undefined) continue;
      const kind = s.content.constructor.name;
      // N2 : `redoItem` peut retrouver le même voisin des deux côtés : un élément dont l'origine
      // droite est son origine. La garde ne le refuse donc pas : le serveur retire l'origine
      // droite.
      if (s.origin != null && s.rightOrigin != null && s.origin.client === s.rightOrigin.client && s.origin.clock === s.rightOrigin.clock) {
        counts.sameOrigins += 1;
      }
      if (s.parentSub == null) {
        // E4 : une valeur `Any` est une entrée de map.
        expect(kind, `${what} : E4`).not.toBe("ContentAny");
        continue;
      }
      counts.keyed += 1;
      // E1 : une valeur `Any` ou un contenu supprimé.
      expect(["ContentAny", "ContentDeleted"], `${what} : E1`).toContain(kind);
      // E2 : pas d'origine droite.
      expect(s.rightOrigin ?? null, `${what} : E2`).toBe(null);
      // N1 : Yjs fond les entrées supprimées d'une clé en une entrée de plusieurs valeurs
      // (`Item.mergeWith`), et `redoItem` en recopie le contenu tel quel : une entrée de
      // plusieurs valeurs peut être **vivante**. La garde ne la refuse donc pas : le serveur
      // la récrit en entrées d'une valeur.
      if (kind === "ContentAny" && s.length > 1) {
        counts.manyValued += 1;
        if (!s.deleted) counts.manyValuedAlive += 1;
      }
    }
  }
}

function scene(seed: number, base: Uint8Array, counts: Counts): { received: Uint8Array[]; server: Y.Doc } {
  const next = random(seed);
  const below = (n: number): number => Math.floor(next() * n);
  const chance = (percent: number): boolean => next() * 100 < percent;
  const server = new Y.Doc();
  server.clientID = 1;
  Y.applyUpdate(server, base);
  const received: Uint8Array[] = [];
  const ids = [5, 9, 12, 700_000, 3];
  for (let n = ids.length - 1; n > 0; n -= 1) {
    const other = below(n + 1);
    [ids[n], ids[other]] = [ids[other], ids[n]];
  }
  ids.length = 2 + below(2);
  let blocks = 0;
  const tabs: Tab[] = [];
  const deliver = (from: Tab, update: Uint8Array): void => {
    const known = Y.encodeStateVector(server);
    received.push(update);
    from.sent.push(update);
    Y.applyUpdate(server, update, REMOTE);
    // Les autres onglets en ligne reçoivent ce que le serveur vient d'apprendre.
    const news = Y.encodeStateAsUpdate(server, known);
    for (const tab of tabs) if (tab !== from && tab.online) Y.applyUpdate(tab.doc, news, REMOTE);
  };
  for (const id of ids) {
    const doc = new Y.Doc();
    doc.clientID = id;
    Y.applyUpdate(doc, base, REMOTE);
    // Une capture sans fin, coupée au hasard : des gestes groupés ou non, sans horloge.
    const undo = new Y.UndoManager(doc.getXmlFragment("document-store"), { captureTimeout: 1e12 });
    const tab: Tab = { doc, undo, online: true, seen: Y.encodeStateVector(server), sent: [] };
    doc.on("update", (update: Uint8Array, origin: unknown) => {
      if (origin !== REMOTE && tab.online) deliver(tab, update);
    });
    tabs.push(tab);
  }
  const group = (doc: Y.Doc): Y.XmlElement => doc.getXmlFragment("document-store").get(0) as Y.XmlElement;
  const gesture = (tab: Tab): void => {
    const g = group(tab.doc);
    const containers = g.toArray() as Y.XmlElement[];
    const container = containers[below(containers.length)];
    const paragraph = container?.get(0) as Y.XmlElement | undefined;
    const text = paragraph?.get(0) as Y.XmlText | undefined;
    const roll = below(100);
    if (roll < 28 && text instanceof Y.XmlText) {
      const at = below(text.length + 1);
      text.insert(at, chance(15) ? "😀" : "abcdefgh".slice(below(4), 4 + below(4)));
    } else if (roll < 38 && text instanceof Y.XmlText && text.length > 0) {
      const at = below(text.length);
      text.delete(at, 1 + below(Math.min(3, text.length - at)));
    } else if (roll < 46 && text instanceof Y.XmlText && text.length > 0) {
      const at = below(text.length);
      const mark = chance(50) ? "bold" : "italic";
      text.format(at, 1 + below(text.length - at), { [mark]: chance(70) ? {} : null });
    } else if (roll < 70 && paragraph !== undefined) {
      const key = KEYS[below(KEYS.length)];
      if (chance(20)) paragraph.removeAttribute(key);
      else paragraph.setAttribute(key, VALUES[key][below(VALUES[key].length)]);
    } else if (roll < 80) {
      blocks += 1;
      const added = new Y.XmlElement("blockContainer");
      added.setAttribute("id", `0196f0a0-0000-7000-8000-${String(seed * 1000 + blocks).padStart(12, "0")}`);
      const p = new Y.XmlElement("paragraph");
      p.setAttribute("backgroundColor", "default");
      p.setAttribute("textColor", "default");
      p.setAttribute("textAlignment", "left");
      const t = new Y.XmlText();
      p.insert(0, [t]);
      added.insert(0, [p]);
      g.insert(below(containers.length + 1), [added]);
      t.insert(0, "bloc");
    } else if (roll < 85 && containers.length > 1) {
      g.delete(below(containers.length), 1);
    } else if (roll < 92) {
      tab.undo.undo();
    } else if (roll < 96) {
      tab.undo.redo();
    } else {
      tab.undo.stopCapturing();
    }
  };
  const comeBack = (tab: Tab): void => {
    // L'étape 2 : depuis l'état du serveur, ou depuis un état plus ancien qu'il a dépassé (un
    // onglet qui se croit en retard : des éléments à cheval sur ce que la page connaît).
    const from = chance(35) ? tab.seen : Y.encodeStateVector(server);
    tab.online = true;
    deliver(tab, Y.encodeStateAsUpdate(tab.doc, from));
    Y.applyUpdate(tab.doc, Y.encodeStateAsUpdate(server, Y.encodeStateVector(tab.doc)), REMOTE);
  };
  const steps = 8 + below(30);
  for (let step = 0; step < steps; step += 1) {
    const tab = tabs[below(tabs.length)];
    const roll = below(100);
    if (roll < 8 && tab.online) {
      tab.online = false;
      tab.seen = Y.encodeStateVector(server);
    } else if (roll < 20 && !tab.online) {
      comeBack(tab);
    } else if (roll < 24 && tab.online && tab.sent.length > 1) {
      // Une mise à jour redite, ou les dernières fondues en une (ce qu'un mandataire ferait).
      const last = tab.sent.slice(-(2 + below(3)));
      deliver(tab, chance(50) ? last[0] : Y.mergeUpdates(last));
    } else if (chance(30)) {
      tab.doc.transact(() => {
        for (let n = 1 + below(4); n > 0; n -= 1) gesture(tab);
      });
    } else {
      gesture(tab);
    }
  }
  for (const tab of tabs) if (!tab.online) comeBack(tab);
  for (const tab of tabs) judge(tab.doc, counts, `scène ${seed}`);
  judge(server, counts, `scène ${seed}, serveur`);
  counts.updates += received.length;
  return { received, server };
}

const hex = (bytes: Uint8Array): string => [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");

test("ce que Yjs écrit au hasard suit la grammaire d'un client", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", "crates/mementum-crdt/tests/fixtures/yjs_updates");
  const base = Uint8Array.from(JSON.parse(new TextDecoder().decode(await fs.readFile(path.join(dir, "base.json")))) as number[]);
  const asked = Number(import.meta.env.MEMENTUM_FUZZ_SEEDS ?? 0);
  const counts: Counts = { keyed: 0, manyValued: 0, manyValuedAlive: 0, sameOrigins: 0, updates: 0 };
  const scenes: string[][] = [];
  for (let seed = 1; seed <= Math.max(SCENES, asked); seed += 1) {
    scenes.push(scene(seed, base, counts).received.map(hex));
    if (seed === SCENES) {
      // Ce que l'essai doit avoir vu pour valoir quelque chose.
      expect(counts.keyed).toBeGreaterThan(2_000);
      expect(counts.manyValued).toBeGreaterThan(5);
    }
  }
  const written = `${JSON.stringify(scenes.slice(0, SCENES))}\n`;
  const file = path.join(dir, "random.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(file, written);
  else expect(new TextDecoder().decode(await fs.readFile(file)) === written, `${file} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
  if (asked > SCENES) await fs.writeFile(path.join(dir, "random-large.json"), `${JSON.stringify(scenes)}\n`);
  console.log(`ESSAI Yjs : ${scenes.length} scènes, ${counts.updates} mises à jour, ${counts.keyed} entrées de map dont ${counts.manyValued} de plusieurs valeurs (${counts.manyValuedAlive} vivantes), ${counts.sameOrigins} éléments d'origine droite leur origine`);
}, 600_000);

// Ce que la page relaie de ces scènes (`random-relayed.json`, écrit par
// `EXPORT_FIXTURES=1 cargo test -p mementum-crdt --test yjs_updates` après celui-ci), rejoué dans
// un vrai Yjs : un navigateur qui ne reçoit que cela lit la page que lit un serveur Yjs à qui
// les onglets auraient écrit directement. C'est la preuve que les deux récritures du serveur (une
// entrée de map de plusieurs valeurs en entrées d'une valeur, une origine droite égale à
// l'origine retirée : Rulings R801, R802) ne séparent pas les navigateurs du serveur.
test("ce que la page relaie mène un vrai Yjs à la page que Yjs aurait lue", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", "crates/mementum-crdt/tests/fixtures/yjs_updates");
  const read = async (name: string): Promise<unknown> => JSON.parse(new TextDecoder().decode(await fs.readFile(path.join(dir, name))));
  const base = Uint8Array.from((await read("base.json")) as number[]);
  // L'essai élargi, s'il a été mené (hors du dépôt), sinon les scènes du dépôt.
  const large = Number(import.meta.env.MEMENTUM_FUZZ_SEEDS ?? 0) > SCENES;
  const relayed = (await read(large ? "random-relayed-large.json" : "random-relayed.json")) as (string[] | null)[];
  expect(relayed.length).toBeGreaterThanOrEqual(SCENES);
  const unhex = (text: string): Uint8Array => Uint8Array.from(text.match(/../g) ?? [], (pair) => parseInt(pair, 16));
  const counts: Counts = { keyed: 0, manyValued: 0, manyValuedAlive: 0, sameOrigins: 0, updates: 0 };
  let compared = 0;
  const apart: number[] = [];
  relayed.forEach((entries, n) => {
    const { server } = scene(n + 1, base, counts);
    if (entries === null) return;
    const browser = new Y.Doc();
    Y.applyUpdate(browser, base);
    for (const entry of entries) Y.applyUpdate(browser, unhex(entry));
    const store = (browser as unknown as { store: { pendingStructs: unknown; pendingDs: unknown } }).store;
    expect(store.pendingStructs, `scène ${n + 1} : des éléments restent en attente`).toBe(null);
    expect(store.pendingDs, `scène ${n + 1} : des suppressions restent en attente`).toBe(null);
    compared += 1;
    if (browser.getXmlFragment("document-store").toString() !== server.getXmlFragment("document-store").toString()) apart.push(n + 1);
  });
  expect(compared).toBeGreaterThan(relayed.length / 2);
  console.log(`ESSAI Yjs : ${compared} scènes relayées relues par un vrai Yjs, ${apart.length} écarts ; ${counts.manyValuedAlive} entrées de plusieurs valeurs vivantes, ${counts.sameOrigins} origines droites égales à l'origine`);
  expect(apart, "scènes où le navigateur ne lit pas la page du serveur Yjs").toEqual([]);
}, 600_000);
