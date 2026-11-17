// Un élément à cheval sur ce que le serveur connaît, écrit par le vrai Yjs (Ruling R784 ; constat
// C1 de la re-relecture 4 de la garde des attributs).
//
// L'onglet (client 5) écrit : la mise à jour U1 part sur son ancienne connexion, et y reste
// (réseau coupé). Il écrit encore, hors ligne : Yjs fond les deux écritures en un seul élément.
// La nouvelle connexion s'ouvre : le serveur, qui n'a pas encore lu U1, envoie son étape 1 (l'état
// de base). U1 arrive enfin par l'ancienne connexion. Puis l'étape 2 de l'onglet, calculée depuis
// l'état de base : un élément dont le départ est sous ce que le serveur connaît maintenant.
//
// Chaque scène donne à `crates/mementum-crdt/tests/astride.rs`, par
// `crates/mementum-crdt/tests/fixtures/yjs_updates/astride.json` : U1, l'étape 2 à cheval, ce que
// Yjs écrirait pour un destinataire qui connaît U1 (`Item.write` avec un décalage : la coupe), et
// l'état qu'un vrai Yjs qui connaît U1 obtient en appliquant l'étape 2 à cheval. La page du
// serveur doit obtenir cet état, et relayer la coupe.
//
// `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/astride.test.ts` réécrit
// le fichier (après `base.json`, voir `yjsUpdates.test.ts`) ; sans la variable, le test compare. À
// relancer à une montée de Yjs.
import { expect, test } from "vitest";
import * as Y from "yjs";

import { nodeModule, type Fs, type Path, type Url } from "../test/node";

type Tab = { doc: Y.Doc; paragraph: Y.XmlElement; text: Y.XmlText };
type Scene = {
  name: string;
  /** Ce qui part sur l'ancienne connexion. */
  first: (tab: Tab) => void;
  /** Ce qui est écrit hors ligne. */
  then: (tab: Tab) => void;
  /** Longueur de l'élément à cheval, et où le serveur le coupe. */
  astride: { length: number; floor: number };
};

const scenes: Scene[] = [
  {
    name: "une frappe",
    first: (t) => t.text.insert(t.text.length, "a"),
    then: (t) => t.text.insert(t.text.length, "b"),
    astride: { length: 2, floor: 1 },
  },
  {
    name: "un collage",
    first: (t) => t.text.insert(t.text.length, "a"),
    then: (t) => t.text.insert(t.text.length, "bcdef"),
    astride: { length: 6, floor: 1 },
  },
  {
    name: "trois frappes",
    first: (t) => t.text.insert(t.text.length, "a"),
    then: (t) => {
      t.text.insert(t.text.length, "b");
      t.text.insert(t.text.length, "c");
    },
    astride: { length: 3, floor: 1 },
  },
  {
    name: "un emoji après une lettre",
    first: (t) => t.text.insert(t.text.length, "a"),
    then: (t) => t.text.insert(t.text.length, "😀é"),
    astride: { length: 4, floor: 1 },
  },
  {
    name: "deux lettres déjà parties, puis une",
    first: (t) => t.text.insert(t.text.length, "ab"),
    then: (t) => t.text.insert(t.text.length, "c"),
    astride: { length: 3, floor: 2 },
  },
  {
    // Les deux premières valeurs, écrasées, sont fondues en un élément supprimé qui porte la clé.
    name: "un attribut écrit trois fois",
    first: (t) => t.paragraph.setAttribute("backgroundColor", "red"),
    then: (t) => {
      t.paragraph.setAttribute("backgroundColor", "blue");
      t.paragraph.setAttribute("backgroundColor", "green");
    },
    astride: { length: 2, floor: 1 },
  },
  {
    // Tapé puis effacé : le ramasse-miettes de Yjs en fait un contenu supprimé d'un tenant.
    name: "un texte tapé puis effacé",
    first: (t) => t.text.insert(t.text.length, "a"),
    then: (t) => {
      const at = t.text.length;
      t.text.insert(at, "b");
      t.text.delete(at - 1, 2);
    },
    astride: { length: 2, floor: 1 },
  },
];

test("l'étape 2 d'un onglet dont une écriture est arrivée entre-temps porte un élément à cheval", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", "crates/mementum-crdt/tests/fixtures/yjs_updates");
  const BASE = Uint8Array.from(JSON.parse(new TextDecoder().decode(await fs.readFile(path.join(dir, "base.json")))) as number[]);
  const out: Record<string, { first: number[]; step2: number[]; cut: number[]; state: number[]; text: string; color: string | null }> = {};

  for (const scene of scenes) {
    const doc = new Y.Doc();
    doc.clientID = 5;
    Y.applyUpdate(doc, BASE, "remote");
    const serverSv = Y.encodeStateVector(doc);
    const log: Uint8Array[] = [];
    doc.on("update", (update: Uint8Array, origin: unknown) => {
      if (origin !== "remote") log.push(update);
    });
    const fragment = doc.getXmlFragment("document-store");
    const group = fragment.get(0) as Y.XmlElement;
    const container = group.get(0) as Y.XmlElement;
    const paragraph = container.get(0) as Y.XmlElement;
    const tab: Tab = { doc, paragraph, text: paragraph.get(0) as Y.XmlText };
    scene.first(tab);
    const first = log[0];
    expect(log.length, scene.name).toBe(1);
    scene.then(tab);

    // L'étape 2 de l'onglet, en réponse à l'étape 1 du serveur (son état : la base).
    const step2 = Y.encodeStateAsUpdate(doc, serverSv);
    const mine = Y.decodeUpdate(step2).structs.filter((s) => s.id.client === 5);
    // Yjs a fondu les écritures : le premier élément part de l'horloge 0 du client, et dépasse
    // ce que U1 portait.
    expect(mine[0].id.clock, scene.name).toBe(0);
    expect(mine[0].length, scene.name).toBe(scene.astride.length);

    // Le serveur : un vrai Yjs qui connaît U1, puis reçoit l'étape 2 à cheval.
    const server = new Y.Doc();
    Y.applyUpdate(server, BASE);
    Y.applyUpdate(server, first);
    expect(Y.decodeStateVector(Y.encodeStateVector(server)).get(5), scene.name).toBe(scene.astride.floor);
    // Ce que l'onglet écrirait pour lui s'il savait qu'il connaît U1 : la coupe.
    const cut = Y.encodeStateAsUpdate(doc, Y.encodeStateVector(server));
    Y.applyUpdate(server, step2);
    // Le serveur a tout ce que l'onglet a, rien en attente.
    expect((server as unknown as { store: { pendingStructs: unknown } }).store.pendingStructs, scene.name).toBeNull();
    expect([...Y.encodeStateAsUpdate(server)], scene.name).toEqual([...Y.encodeStateAsUpdate(doc)]);

    // Un autre vrai Yjs qui connaît U1 et reçoit la coupe arrive au même état.
    const other = new Y.Doc();
    Y.applyUpdate(other, BASE);
    Y.applyUpdate(other, first);
    Y.applyUpdate(other, cut);
    expect([...Y.encodeStateAsUpdate(other)], scene.name).toEqual([...Y.encodeStateAsUpdate(server)]);

    const read = ((server.getXmlFragment("document-store").get(0) as Y.XmlElement).get(0) as Y.XmlElement).get(0) as Y.XmlElement;
    out[scene.name] = {
      first: [...first],
      step2: [...step2],
      cut: [...cut],
      state: [...Y.encodeStateAsUpdate(server)],
      text: (read.get(0) as Y.XmlText).toString(),
      color: (read.getAttribute("backgroundColor") as string | undefined) ?? null,
    };
  }

  const written = `${JSON.stringify(out)}\n`;
  const file = path.join(dir, "astride.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(file, written);
  else expect(new TextDecoder().decode(await fs.readFile(file)) === written, `${file} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
});
