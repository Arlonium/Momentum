// L'écriture d'une page par différence (Task S2 du lot 5, Rulings R1227, R1255) contre le vrai
// Yjs, celui du client.
//
// `crates/mementum-crdt/tests/fixtures/diff/scenes.json`, écrit par
// `crates/mementum-crdt/tests/diff.rs` (`the_scenes_are_written_on_demand`), porte pour chaque
// scène la page de départ, écrite par le serveur, l'écriture par différence que le serveur en
// fait, et des places de frappe. Pour chacune, ce test fait taper « Z » au vrai Yjs, par un onglet
// qui n'a que la page de départ, puis joue les deux onglets qui reçoivent ce qui leur manque :
// celui qui a tapé (`typist`) reçoit l'écriture ; un autre (`observer`) reçoit l'écriture, puis la
// frappe. Yjs nettoie de lui-même les marques qu'une mise à jour distante laisse inutiles
// (`cleanupYTextAfterTransaction`), dans une transaction locale qu'un navigateur renvoie au
// serveur : ce nettoyage est gardé, avec ce que lit l'onglet. Le tout va dans `keys.json`, que
// relit le test Rust (la page du serveur doit lire ce que lit chaque onglet, et garder ou perdre
// chaque frappe comme il le dit).
//
// `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/diffWrites.test.ts`
// réécrit `keys.json` (après `scenes.json`) ; sans la variable, le test compare. À relancer à une
// montée de Yjs, et après toute réécriture de `scenes.json`.
import { expect, test } from "vitest";
import * as Y from "yjs";

import { nodeModule, type Fs, type Path, type Url } from "../test/node";

type Place = { path: number[]; child: number; at: number };
type Scenes = Record<string, { base: number[]; write: number[]; keys: Record<string, Place> }>;
type Tab = { cleanup: number[] | null; read: unknown[] };

const FRAGMENT = "document-store";
/** L'onglet qui tape, et un autre. */
const TYPIST = 77;
const OBSERVER = 88;

/** Le conteneur du bloc au bout de `path` (rangs depuis la racine, puis dans chaque groupe). */
function blockAt(doc: Y.Doc, path: number[]): Y.XmlElement {
  let list = doc.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement;
  let container: Y.XmlElement | null = null;
  for (const at of path) {
    if (container) {
      list = container
        .toArray()
        .find((n): n is Y.XmlElement => n instanceof Y.XmlElement && n.nodeName === "blockGroup")!;
    }
    container = list.get(at) as Y.XmlElement;
  }
  return container!;
}

/** Une valeur sans `null` ni `undefined` dans ses objets (un `undefined` d'une liste devient `null`). */
function clean(value: unknown): unknown {
  if (Array.isArray(value)) return value.map((v) => (v === undefined ? null : clean(v)));
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([, v]) => v !== null && v !== undefined)
        .map(([k, v]) => [k, clean(v)]),
    );
  }
  return value;
}

/** Une valeur écrite clés triées, pour comparer deux marques. */
function stable(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (value && typeof value === "object") {
    const entries = Object.entries(value).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${stable(v)}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

/** Ce qu'un document lit, de la forme de `reading` du test Rust. */
function read(nodes: unknown[]): unknown[] {
  return nodes.map((node) => {
    if (node instanceof Y.XmlText) {
      const runs: [string, unknown][] = [];
      for (const d of node.toDelta() as { insert: unknown; attributes?: Record<string, unknown> }[]) {
        if (typeof d.insert !== "string" || d.insert === "") continue;
        const marks = clean(d.attributes ?? {});
        const last = runs[runs.length - 1];
        if (last && stable(last[1]) === stable(marks)) last[0] += d.insert;
        else runs.push([d.insert, marks]);
      }
      return { text: runs };
    }
    const element = node as Y.XmlElement;
    return { el: element.nodeName, attrs: clean(element.getAttributes()), children: read(element.toArray()) };
  });
}

/** Les mises à jour locales (le nettoyage de Yjs) que `apply` fait écrire au document, fondues. */
function local(doc: Y.Doc, apply: () => void): number[] | null {
  const written: Uint8Array[] = [];
  const listen = (update: Uint8Array, _origin: unknown, _doc: Y.Doc, tr: Y.Transaction) => {
    if (tr.local) written.push(update);
  };
  doc.on("update", listen);
  apply();
  doc.off("update", listen);
  return written.length > 0 ? [...Y.mergeUpdates(written)] : null;
}

function docOf(client: number, base: Uint8Array): Y.Doc {
  const doc = new Y.Doc();
  doc.clientID = client;
  Y.applyUpdate(doc, base, "remote");
  return doc;
}

test("chaque frappe du vrai Yjs, et ce que deux onglets en lisent après l'écriture par différence", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(
    path.dirname(url.fileURLToPath(import.meta.url)),
    "../../../..",
    "crates/mementum-crdt/tests/fixtures/diff",
  );
  const scenes = JSON.parse(await fs.readFile(path.join(dir, "scenes.json"), "utf8")) as Scenes;
  const out: Record<string, Record<string, { update: number[]; typist: Tab; observer: Tab }>> = {};

  for (const [name, scene] of Object.entries(scenes)) {
    const base = Uint8Array.from(scene.base);
    const write = Uint8Array.from(scene.write);
    out[name] = {};
    for (const [keyName, place] of Object.entries(scene.keys)) {
      const what = `${name} / ${keyName}`;
      const typist = docOf(TYPIST, base);
      const before = Y.encodeStateVector(typist);
      const content = blockAt(typist, place.path).get(0) as Y.XmlElement;
      const target = content.get(place.child);
      expect(target instanceof Y.XmlText, what).toBe(true);
      // Sans marques données : Yjs prend celles de la place, comme une frappe dans l'éditeur.
      (target as Y.XmlText).insert(place.at, "Z");
      const update = Y.encodeStateAsUpdate(typist, before);
      expect(Y.decodeUpdate(update).structs.length, what).toBeGreaterThan(0);

      // L'onglet qui a tapé reçoit l'écriture du serveur.
      const typistCleanup = local(typist, () => Y.applyUpdate(typist, write, "remote"));
      // Un autre a reçu l'écriture, puis reçoit la frappe.
      const observer = docOf(OBSERVER, base);
      const observerCleanup = local(observer, () => {
        Y.applyUpdate(observer, write, "remote");
        Y.applyUpdate(observer, update, "remote");
      });
      out[name][keyName] = {
        update: [...update],
        typist: { cleanup: typistCleanup, read: read(typist.getXmlFragment(FRAGMENT).toArray()) },
        observer: { cleanup: observerCleanup, read: read(observer.getXmlFragment(FRAGMENT).toArray()) },
      };
    }
  }

  const written = `${JSON.stringify(out)}\n`;
  const file = path.join(dir, "keys.json");
  if (import.meta.env.EXPORT_FIXTURES) await fs.writeFile(file, written);
  else expect(new TextDecoder().decode(await fs.readFile(file)) === written, `${file} (EXPORT_FIXTURES=1 le réécrit)`).toBe(true);
});
