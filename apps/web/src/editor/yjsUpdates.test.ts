// Ce que le vrai Yjs écrit, rejoué dans la garde du serveur (Rulings R723, R784 ; sondes de la
// re-relecture 3 de la garde des attributs, gardées).
//
// Deux clients (5 et 9, dans les deux ordres d'identifiants) écrivent une page servie : écritures
// concurrentes du même attribut, retrait puis écriture, annuler et rétablir par-dessus l'écriture
// d'un autre, frappe au milieu d'un texte et attributs dans la même transaction, longue session
// hors ligne. Chaque scénario donne ses mises à jour dans l'ordre, l'autre client d'abord, et en
// étapes 2 depuis l'état de base, dans les deux ordres. Ici : aucune entrée de map de Yjs ne porte
// d'origine droite. Les mises à jour sont écrites dans
// `crates/mementum-crdt/tests/fixtures/yjs_updates/scenarios.json`, que
// `crates/mementum-crdt/tests/yjs_updates.rs` donne à une vraie page comme celles d'un client :
// toutes passent (aucune entrée de map à origine droite), et le journal relu donne la page
// vivante. L'élément à cheval d'une frappe arrivée en retard est dans `astride.test.ts`.
//
// `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/yjsUpdates.test.ts` les
// réécrit (après `EXPORT_FIXTURES=1 cargo test -p mementum-crdt --test yjs_updates`, qui écrit
// `base.json`, l'état de la page de départ) ; sans la variable, le test les compare. À relancer à
// une montée de Yjs.
import { expect, test } from "vitest";
import * as Y from "yjs";

import { nodeModule, type Fs, type Path, type Url } from "../test/node";

let BASE: Uint8Array = new Uint8Array();
let BASE_SV: Uint8Array = new Uint8Array();

type Logged = { client: number; update: Uint8Array };
type Bench = {
  log: Logged[];
  docs: Y.Doc[];
  client: (id: number) => { doc: Y.Doc; paragraph: Y.XmlElement; text: Y.XmlText; undo: Y.UndoManager };
  sync: (from: Y.Doc, to: Y.Doc) => void;
};

function bench(): Bench {
  const log: Logged[] = [];
  const docs: Y.Doc[] = [];
  return {
    log,
    docs,
    client(id) {
      const doc = new Y.Doc();
      doc.clientID = id;
      Y.applyUpdate(doc, BASE, "remote");
      doc.on("update", (update: Uint8Array, origin: unknown) => {
        if (origin !== "remote") log.push({ client: id, update });
      });
      docs.push(doc);
      const fragment = doc.getXmlFragment("document-store");
      const group = fragment.get(0) as Y.XmlElement;
      const container = group.get(0) as Y.XmlElement;
      const paragraph = container.get(0) as Y.XmlElement;
      const text = paragraph.get(0) as Y.XmlText;
      const undo = new Y.UndoManager(fragment, { captureTimeout: 0 });
      return { doc, paragraph, text, undo };
    },
    sync(from, to) {
      Y.applyUpdate(to, Y.encodeStateAsUpdate(from, Y.encodeStateVector(to)), "remote");
    },
  };
}

const scenarios: [string, number[][]][] = [];
let keyedWithRightOrigin = 0;
let keyed = 0;

function record(name: string, b: Bench, concurrent: boolean): void {
  for (const doc of b.docs) {
    for (const structs of (doc as unknown as { store: { clients: Map<number, unknown[]> } }).store.clients.values()) {
      for (const s of structs) {
        const item = s as { parentSub?: string | null; rightOrigin?: unknown };
        if (item.parentSub != null) {
          keyed += 1;
          if (item.rightOrigin != null) keyedWithRightOrigin += 1;
        }
      }
    }
  }
  const bytes = (l: Logged[]): number[][] => l.map((e) => [...e.update]);
  scenarios.push([`${name}, dans l'ordre`, bytes(b.log)]);
  if (concurrent) {
    const ids = [...new Set(b.log.map((e) => e.client))].reverse();
    scenarios.push([`${name}, l'autre d'abord`, bytes(ids.flatMap((id) => b.log.filter((e) => e.client === id)))]);
    // Chaque onglet revient en ligne : son étape 2, depuis l'état de base, dans les deux ordres.
    const steps = b.docs.map((doc) => [...Y.encodeStateAsUpdate(doc, BASE_SV)]);
    scenarios.push([`${name}, étapes 2`, steps]);
    scenarios.push([`${name}, étapes 2 renversées`, [...steps].reverse()]);
  }
}

test("ce que Yjs écrit passe la garde du serveur, et aucune entrée de map n'a d'origine droite", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", "crates/mementum-crdt/tests/fixtures/yjs_updates");
  BASE = Uint8Array.from(JSON.parse(new TextDecoder().decode(await fs.readFile(path.join(dir, "base.json")))) as number[]);
  const base = new Y.Doc();
  Y.applyUpdate(base, BASE);
  BASE_SV = Y.encodeStateVector(base);
  for (const [low, high] of [
    [5, 9],
    [9, 5],
  ]) {
    {
      const b = bench();
      const a = b.client(low);
      const c = b.client(high);
      a.paragraph.setAttribute("backgroundColor", "red");
      c.paragraph.setAttribute("backgroundColor", "blue");
      record(`deux écritures concurrentes du même attribut (${low}, ${high})`, b, true);
    }
    {
      const b = bench();
      const a = b.client(low);
      const c = b.client(high);
      a.paragraph.setAttribute("backgroundColor", "red");
      b.sync(a.doc, c.doc);
      c.paragraph.removeAttribute("backgroundColor");
      a.paragraph.setAttribute("backgroundColor", "green");
      b.sync(a.doc, c.doc);
      b.sync(c.doc, a.doc);
      c.paragraph.setAttribute("backgroundColor", "blue");
      a.paragraph.setAttribute("textAlignment", "center");
      record(`retrait concurrent d'une écriture, puis écriture (${low}, ${high})`, b, false);
    }
    {
      const b = bench();
      const a = b.client(low);
      const c = b.client(high);
      a.paragraph.setAttribute("backgroundColor", "red");
      a.paragraph.setAttribute("backgroundColor", "blue");
      a.undo.undo();
      a.undo.undo();
      a.undo.redo();
      c.paragraph.setAttribute("backgroundColor", "green");
      b.sync(c.doc, a.doc);
      a.undo.redo();
      a.undo.undo();
      b.sync(a.doc, c.doc);
      c.undo.undo();
      c.undo.redo();
      record(`annuler et rétablir par-dessus l'écriture d'un autre (${low}, ${high})`, b, false);
    }
    {
      const b = bench();
      const a = b.client(low);
      const c = b.client(high);
      a.doc.transact(() => {
        a.text.insert(1, "XY");
        a.paragraph.setAttribute("backgroundColor", "red");
        a.paragraph.setAttribute("backgroundColor", "blue");
        a.paragraph.removeAttribute("textAlignment");
        a.paragraph.setAttribute("textAlignment", "right");
      });
      c.doc.transact(() => {
        c.text.insert(2, "Z");
        c.text.format(0, 2, { bold: {} });
        c.paragraph.setAttribute("backgroundColor", "green");
      });
      a.undo.undo();
      a.undo.redo();
      record(`frappe au milieu et attributs dans la même transaction (${low}, ${high})`, b, true);
    }
    {
      // Hors ligne longtemps : écritures, retraits, annulations, puis ramasse-miettes de Yjs.
      const b = bench();
      const a = b.client(low);
      const c = b.client(high);
      for (let n = 0; n < 6; n += 1) {
        a.paragraph.setAttribute("backgroundColor", n % 2 ? "red" : "blue");
        if (n % 3 === 0) a.paragraph.removeAttribute("backgroundColor");
        a.text.insert(n, "o");
        c.paragraph.setAttribute("textColor", n % 2 ? "red" : "blue");
        if (n === 4) c.undo.undo();
      }
      a.undo.undo();
      a.undo.undo();
      a.undo.redo();
      record(`hors ligne longtemps (${low}, ${high})`, b, true);
    }
  }
  const written = `${JSON.stringify(scenarios)}\n`;
  const file = path.join(dir, "scenarios.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(file, written);
  else expect(new TextDecoder().decode(await fs.readFile(file)) === written, `${file} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
  expect(keyed).toBeGreaterThan(50);
  expect(keyedWithRightOrigin).toBe(0);
});
