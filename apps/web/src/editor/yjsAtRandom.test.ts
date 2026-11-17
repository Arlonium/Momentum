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

type Counts = { keyed: number; manyValued: number; manyValuedAlive: number; sameOrigins: number; placedByBlocks: number; updates: number };

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

type Id = { client: number; clock: number };

const sameId = (a: Id | null | undefined, b: Id | null | undefined): boolean => {
  const [x, y] = [a ?? null, b ?? null];
  return x === null || y === null ? x === y : x.client === y.client && x.clock === y.clock;
};

/**
 * N3 (Ruling R825) : l'origine droite `right` est du client de l'origine `origin`, plus basse, et
 * une même suite de blocs peut porter l'unité d'avant l'origine droite et l'origine (chacune a pour
 * origine l'unité d'avant, toutes ont la même origine droite) : Yjs range alors l'élément selon le
 * découpage de ses blocs. Lu sur les éléments que garde un document de Yjs.
 */
function placedByBlocks(clients: Map<number, Stored[]>, origin: Id, right: Id): boolean {
  if (origin.client !== right.client || right.clock >= origin.clock || right.clock === 0) return false;
  const structs = clients.get(origin.client);
  if (structs === undefined) return false;
  const at = (clock: number): Stored | undefined => structs.find((s) => s.id.clock <= clock && clock < s.id.clock + s.length);
  let clock = origin.clock;
  let rightOrigin: Id | null | undefined;
  for (let step = 0; step < 64; step += 1) {
    const s = at(clock);
    if (s === undefined || s.content === undefined) return false;
    if (rightOrigin !== undefined && !sameId(s.rightOrigin, rightOrigin)) return false;
    rightOrigin = s.rightOrigin ?? null;
    if (s.id.clock <= right.clock - 1) return true;
    if (!sameId(s.origin, { client: origin.client, clock: s.id.clock - 1 })) return false;
    clock = s.id.clock - 1;
  }
  return true;
}

/** Les règles de la grammaire, sur les éléments que garde un document de Yjs. */
function judge(doc: Y.Doc, counts: Counts, what: string): void {
  const clients = (doc as unknown as { store: { clients: Map<number, Stored[]> } }).store.clients;
  for (const structs of clients.values()) {
    for (const s of structs) {
      if (s.content === undefined) continue;
      const kind = s.content.constructor.name;
      // N3 : une origine droite du client de l'origine, plus basse, dans une même suite de blocs
      // que l'origine. Yjs l'écrit-il ? Compté (la garde récrit l'élément sans son origine droite).
      if (s.origin != null && s.rightOrigin != null && placedByBlocks(clients, s.origin, s.rightOrigin)) {
        counts.placedByBlocks += 1;
      }
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

/**
 * Les seuils cumulés (sur 100) du tirage d'un geste : frappe, effacement, marque, attribut, bloc
 * ajouté, bloc supprimé, annuler, rétablir ; au-delà, la capture est coupée.
 */
type Weights = [number, number, number, number, number, number, number, number];

/** Le tirage ordinaire. */
const ORDINARY: Weights = [28, 38, 46, 70, 80, 85, 92, 96];

/**
 * Un tirage penché vers les annulations : moins d'attributs, plus de blocs supprimés, d'annuler et
 * de rétablir. C'est lui qui a montré la forme N3 (Ruling R825), que le tirage ordinaire ne
 * montrait pas en 12 000 scènes.
 */
const UNDO_HEAVY: Weights = [28, 38, 46, 50, 60, 70, 82, 96];

/**
 * Le tirage de l'essai élargi (`MEMENTUM_FUZZ_SEEDS`) : `MEMENTUM_FUZZ_WEIGHTS=undo` le penche
 * vers les annulations ; les 150 scènes du dépôt gardent le tirage ordinaire.
 */
const LARGE: Weights = import.meta.env.MEMENTUM_FUZZ_WEIGHTS === "undo" ? UNDO_HEAVY : ORDINARY;

function scene(seed: number, base: Uint8Array, counts: Counts, weights: Weights = ORDINARY): { received: Uint8Array[]; server: Y.Doc } {
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
    const [typing, erasing, marking, attributes, adding, dropping, undoing, redoing] = weights;
    if (roll < typing && text instanceof Y.XmlText) {
      const at = below(text.length + 1);
      text.insert(at, chance(15) ? "😀" : "abcdefgh".slice(below(4), 4 + below(4)));
    } else if (roll < erasing && text instanceof Y.XmlText && text.length > 0) {
      const at = below(text.length);
      text.delete(at, 1 + below(Math.min(3, text.length - at)));
    } else if (roll < marking && text instanceof Y.XmlText && text.length > 0) {
      const at = below(text.length);
      const mark = chance(50) ? "bold" : "italic";
      text.format(at, 1 + below(text.length - at), { [mark]: chance(70) ? {} : null });
    } else if (roll < attributes && paragraph !== undefined) {
      const key = KEYS[below(KEYS.length)];
      if (chance(20)) paragraph.removeAttribute(key);
      else paragraph.setAttribute(key, VALUES[key][below(VALUES[key].length)]);
    } else if (roll < adding) {
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
    } else if (roll < dropping && containers.length > 1) {
      g.delete(below(containers.length), 1);
    } else if (roll < undoing) {
      tab.undo.undo();
    } else if (roll < redoing) {
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

// Les scènes où un onglet envoie un élément dont l'origine droite est son origine, que
// `redoItem` écrit quand il retrouve le même voisin des deux côtés (N2, Ruling R802). Trouvées
// en tirant 12 000 scènes (trois seulement) : les 150 du dépôt n'en portent aucune, et rien ne
// gardait la récriture du serveur (constat I-6.4 de la re-relecture 6). Gardées dans
// `redo.json`, que `crates/mementum-crdt/tests/yjs_updates.rs` donne à une vraie page : elles
// sont acceptées, et ce que la page relaie ramène un vrai Yjs à la même page.
const REDO_SCENES = [3192, 3851, 8832];

// Les scènes où un onglet rétablit un élément dont les deux voisins mènent, par `redone`, au même
// élément X de plusieurs unités : `redoItem` écrit l'origine `X.lastId` et l'origine droite `X.id`,
// et quand la première unité de X a pour origine l'unité d'avant (avec la même origine droite),
// Yjs range l'élément selon le découpage de ses blocs (N3, Ruling R825). Trouvées par le tirage
// penché vers les annulations (`UNDO_HEAVY`, 20 000 scènes : trois) ; le tirage ordinaire n'en
// montrait aucune en 12 000. Gardées dans `redo-blocks.json`, que
// `crates/mementum-crdt/tests/yjs_updates.rs` donne à une vraie page : elles sont acceptées (la
// page récrit l'élément sans son origine droite), et la page relue de son état est la page
// vivante.
const REDO_BLOCK_SCENES = [4376, 8403, 12763];

// Les scènes où un onglet rétablit un élément O dont l'origine droite est une unité de la suite de
// son origine, avant elle (O est donc à droite de son origine droite), puis où un onglet pose un
// élément entre O et l'unité qui le suit. Une forme de la règle I3 tenait l'origine droite d'O pour
// à sa droite, et ramassait cet élément honnête (Ruling R834). Trouvées par le tirage penché vers
// les annulations (`UNDO_HEAVY`, 20 000 scènes : deux). Gardées dans `redo-crossing.json`, que
// `crates/mementum-crdt/tests/yjs_updates.rs` donne à une vraie page : elles sont acceptées, et
// ce que la page relaie ramène un vrai Yjs à la page du serveur Yjs.
const REDO_CROSSING_SCENES = [8172, 13429];

type MaybeId = { client: number; clock: number } | null;

/** Les rangs des mises à jour qui portent un élément dont l'origine droite est du client de son
 * origine, plus basse : les candidats de N3. */
function backwardRightOrigins(received: Uint8Array[]): number[] {
  const ranks: number[] = [];
  received.forEach((update, n) => {
    const { structs } = Y.decodeUpdate(update) as unknown as { structs: { origin?: MaybeId; rightOrigin?: MaybeId }[] };
    const backward = (o: MaybeId, r: MaybeId): boolean => o !== null && r !== null && o.client === r.client && r.clock < o.clock;
    if (structs.some((s) => backward(s.origin ?? null, s.rightOrigin ?? null))) ranks.push(n);
  });
  return ranks;
}

/** Les rangs des mises à jour qui portent un élément dont l'origine droite est son origine. */
function sameOriginUpdates(received: Uint8Array[]): number[] {
  const same = (a: MaybeId, b: MaybeId): boolean => a !== null && b !== null && a.client === b.client && a.clock === b.clock;
  const ranks: number[] = [];
  received.forEach((update, n) => {
    const { structs } = Y.decodeUpdate(update) as unknown as { structs: { origin?: MaybeId; rightOrigin?: MaybeId }[] };
    if (structs.some((s) => same(s.origin ?? null, s.rightOrigin ?? null))) ranks.push(n);
  });
  return ranks;
}

test("ce que Yjs écrit au hasard suit la grammaire d'un client", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", "crates/mementum-crdt/tests/fixtures/yjs_updates");
  const base = Uint8Array.from(JSON.parse(new TextDecoder().decode(await fs.readFile(path.join(dir, "base.json")))) as number[]);
  const asked = Number(import.meta.env.MEMENTUM_FUZZ_SEEDS ?? 0);
  const counts: Counts = { keyed: 0, manyValued: 0, manyValuedAlive: 0, sameOrigins: 0, placedByBlocks: 0, updates: 0 };
  const scenes: string[][] = [];
  const blocksSeeds: number[] = [];
  for (let seed = 1; seed <= SCENES; seed += 1) {
    const before = counts.placedByBlocks;
    scenes.push(scene(seed, base, counts).received.map(hex));
    if (counts.placedByBlocks > before) blocksSeeds.push(seed);
  }
  // Ce que l'essai doit avoir vu pour valoir quelque chose.
  expect(counts.keyed).toBeGreaterThan(2_000);
  expect(counts.manyValued).toBeGreaterThan(5);
  // L'essai élargi : les scènes du dépôt, puis les suivantes, ou toutes tirées de nouveau si son
  // tirage n'est pas l'ordinaire.
  const large: string[][] = LARGE === ORDINARY ? [...scenes] : [];
  for (let seed = large.length + 1; seed <= asked; seed += 1) {
    const before = counts.placedByBlocks;
    large.push(scene(seed, base, counts, LARGE).received.map(hex));
    if (counts.placedByBlocks > before) blocksSeeds.push(seed);
  }
  const written = `${JSON.stringify(scenes)}\n`;
  const file = path.join(dir, "random.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(file, written);
  else expect(new TextDecoder().decode(await fs.readFile(file)) === written, `${file} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
  if (asked > SCENES) await fs.writeFile(path.join(dir, "random-large.json"), `${JSON.stringify(large)}\n`);
  console.log(`ESSAI Yjs : ${Math.max(scenes.length, large.length)} scènes, ${counts.updates} mises à jour, ${counts.keyed} entrées de map dont ${counts.manyValued} de plusieurs valeurs (${counts.manyValuedAlive} vivantes), ${counts.sameOrigins} éléments d'origine droite leur origine, ${counts.placedByBlocks} rangés selon le découpage des blocs (N3, scènes ${blocksSeeds.slice(0, 20).join(", ")})`);
  // Les scènes d'un rétablissement qui écrit N2 : chacune, ses mises à jour, et les rangs de
  // celles qui portent la forme.
  const redo = REDO_SCENES.map((seed) => {
    const { received } = scene(seed, base, counts);
    const ranks = sameOriginUpdates(received);
    expect(ranks.length, `scène ${seed} : plus d'élément d'origine droite son origine`).toBeGreaterThan(0);
    return [seed, received.map(hex), ranks];
  });
  const redoWritten = `${JSON.stringify(redo)}\n`;
  const redoFile = path.join(dir, "redo.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(redoFile, redoWritten);
  else expect(new TextDecoder().decode(await fs.readFile(redoFile)) === redoWritten, `${redoFile} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
  // Les scènes d'un rétablissement qui écrit N3 : chacune, ses mises à jour, et les rangs de
  // celles qui portent une origine droite du client de l'origine, plus basse.
  const blocks = REDO_BLOCK_SCENES.map((seed) => {
    const before = counts.placedByBlocks;
    const { received } = scene(seed, base, counts, UNDO_HEAVY);
    expect(counts.placedByBlocks, `scène ${seed} : plus d'élément rangé selon le découpage des blocs`).toBeGreaterThan(before);
    return [seed, received.map(hex), backwardRightOrigins(received)];
  });
  const blocksWritten = `${JSON.stringify(blocks)}\n`;
  const blocksFile = path.join(dir, "redo-blocks.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(blocksFile, blocksWritten);
  else expect(new TextDecoder().decode(await fs.readFile(blocksFile)) === blocksWritten, `${blocksFile} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
  // Les scènes d'un rétablissement dont un voisin pose un élément juste après lui : chacune, ses
  // mises à jour, et les rangs de celles qui portent une origine droite du client de l'origine,
  // plus basse.
  const crossing = REDO_CROSSING_SCENES.map((seed) => {
    const { received } = scene(seed, base, counts, UNDO_HEAVY);
    const ranks = backwardRightOrigins(received);
    expect(ranks.length, `scène ${seed} : plus d'origine droite du client de l'origine, plus basse`).toBeGreaterThan(0);
    return [seed, received.map(hex), ranks];
  });
  const crossingWritten = `${JSON.stringify(crossing)}\n`;
  const crossingFile = path.join(dir, "redo-crossing.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(crossingFile, crossingWritten);
  else expect(new TextDecoder().decode(await fs.readFile(crossingFile)) === crossingWritten, `${crossingFile} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
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
  const counts: Counts = { keyed: 0, manyValued: 0, manyValuedAlive: 0, sameOrigins: 0, placedByBlocks: 0, updates: 0 };
  let compared = 0;
  const apart: number[] = [];
  // Les scènes où le serveur Yjs vivant ne lit pas ce qu'un Yjs neuf lit de son état : Yjs lui-même
  // range un élément de la forme N3 selon le découpage de ses blocs (Ruling R825).
  const apartInYjs: number[] = [];
  relayed.forEach((entries, n) => {
    const { server } = scene(n + 1, base, counts, large ? LARGE : ORDINARY);
    if (entries === null) return;
    const browser = new Y.Doc();
    Y.applyUpdate(browser, base);
    for (const entry of entries) Y.applyUpdate(browser, unhex(entry));
    const store = (browser as unknown as { store: { pendingStructs: unknown; pendingDs: unknown } }).store;
    expect(store.pendingStructs, `scène ${n + 1} : des éléments restent en attente`).toBe(null);
    expect(store.pendingDs, `scène ${n + 1} : des suppressions restent en attente`).toBe(null);
    compared += 1;
    const reading = (doc: Y.Doc): string => doc.getXmlFragment("document-store").toString();
    if (reading(browser) === reading(server)) return;
    // Le navigateur doit lire la page du serveur Yjs : celle de son état, si le serveur Yjs
    // vivant ne la lit pas lui-même (N3).
    const fresh = new Y.Doc();
    Y.applyUpdate(fresh, Y.encodeStateAsUpdate(server));
    if (reading(server) !== reading(fresh)) apartInYjs.push(n + 1);
    if (reading(browser) !== reading(fresh)) apart.push(n + 1);
  });
  expect(compared).toBeGreaterThan(relayed.length / 2);
  console.log(`ESSAI Yjs : ${compared} scènes relayées relues par un vrai Yjs, ${apart.length} écarts ; ${counts.manyValuedAlive} entrées de plusieurs valeurs vivantes, ${counts.sameOrigins} origines droites égales à l'origine ; ${apartInYjs.length} scènes où Yjs vivant ne lit pas son propre état (${apartInYjs.slice(0, 10).join(", ")})`);
  expect(apart, "scènes où le navigateur ne lit pas la page du serveur Yjs").toEqual([]);
  // Les scènes d'un rétablissement qui écrit N2 (`redo-relayed.json`, dans l'ordre de
  // `REDO_SCENES`) : chacune est relayée, et lue par un vrai Yjs comme le serveur Yjs la lit.
  const redoRelayed = (await read("redo-relayed.json")) as (string[] | null)[];
  expect(redoRelayed.length).toBe(REDO_SCENES.length);
  REDO_SCENES.forEach((seed, n) => {
    const { server } = scene(seed, base, counts);
    const entries = redoRelayed[n];
    expect(entries, `scène ${seed} : le serveur y a écrit lui-même`).not.toBe(null);
    const browser = new Y.Doc();
    Y.applyUpdate(browser, base);
    for (const entry of entries ?? []) Y.applyUpdate(browser, unhex(entry));
    const store = (browser as unknown as { store: { pendingStructs: unknown; pendingDs: unknown } }).store;
    expect(store.pendingStructs, `scène ${seed} : des éléments restent en attente`).toBe(null);
    expect(store.pendingDs, `scène ${seed} : des suppressions restent en attente`).toBe(null);
    expect(browser.getXmlFragment("document-store").toString(), `scène ${seed}`).toBe(server.getXmlFragment("document-store").toString());
  });
  // Les scènes d'un rétablissement qui écrit N3 (`redo-blocks-relayed.json`, dans l'ordre de
  // `REDO_BLOCK_SCENES`) : chacune est relayée, et lue par un vrai Yjs comme la lit un Yjs neuf qui
  // ouvre la page du serveur Yjs (son état). Pas comme le serveur Yjs vivant : sur cette forme, Yjs
  // lui-même ne lit pas pareil le document vivant et son état relu (scène 8403 : « bgc » vivant,
  // « bcg » relu), et c'est l'écart que la page ferme (constat I-7.1, Ruling R825).
  const blocksRelayed = (await read("redo-blocks-relayed.json")) as (string[] | null)[];
  expect(blocksRelayed.length).toBe(REDO_BLOCK_SCENES.length);
  const blocksApartInYjs: number[] = [];
  REDO_BLOCK_SCENES.forEach((seed, n) => {
    const { server } = scene(seed, base, counts, UNDO_HEAVY);
    const entries = blocksRelayed[n];
    expect(entries, `scène ${seed} : le serveur y a écrit lui-même`).not.toBe(null);
    const browser = new Y.Doc();
    Y.applyUpdate(browser, base);
    for (const entry of entries ?? []) Y.applyUpdate(browser, unhex(entry));
    const store = (browser as unknown as { store: { pendingStructs: unknown; pendingDs: unknown } }).store;
    expect(store.pendingStructs, `scène ${seed} : des éléments restent en attente`).toBe(null);
    expect(store.pendingDs, `scène ${seed} : des suppressions restent en attente`).toBe(null);
    const fresh = new Y.Doc();
    Y.applyUpdate(fresh, Y.encodeStateAsUpdate(server));
    const reading = (doc: Y.Doc): string => doc.getXmlFragment("document-store").toString();
    if (reading(server) !== reading(fresh)) blocksApartInYjs.push(seed);
    expect(reading(browser), `scène ${seed}`).toBe(reading(fresh));
  });
  // Ce que la garde ferme : au moins une scène où le vrai Yjs ne relit pas son propre état comme
  // il le vit.
  expect(blocksApartInYjs, "plus de scène où Yjs vivant et Yjs relu divergent").not.toEqual([]);
  // Les scènes d'un rétablissement dont un voisin pose un élément juste après lui
  // (`redo-crossing-relayed.json`, dans l'ordre de `REDO_CROSSING_SCENES`) : chacune est relayée,
  // sans rien ramasser de ce que Yjs a écrit, et lue par un vrai Yjs comme le serveur Yjs la lit.
  const crossingRelayed = (await read("redo-crossing-relayed.json")) as (string[] | null)[];
  expect(crossingRelayed.length).toBe(REDO_CROSSING_SCENES.length);
  REDO_CROSSING_SCENES.forEach((seed, n) => {
    const { server } = scene(seed, base, counts, UNDO_HEAVY);
    const entries = crossingRelayed[n];
    expect(entries, `scène ${seed} : le serveur y a écrit lui-même`).not.toBe(null);
    const browser = new Y.Doc();
    Y.applyUpdate(browser, base);
    for (const entry of entries ?? []) Y.applyUpdate(browser, unhex(entry));
    const store = (browser as unknown as { store: { pendingStructs: unknown; pendingDs: unknown } }).store;
    expect(store.pendingStructs, `scène ${seed} : des éléments restent en attente`).toBe(null);
    expect(store.pendingDs, `scène ${seed} : des suppressions restent en attente`).toBe(null);
    expect(browser.getXmlFragment("document-store").toString(), `scène ${seed}`).toBe(server.getXmlFragment("document-store").toString());
  });
}, 600_000);
