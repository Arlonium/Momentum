// Bench client : yjs 13 contre loro-crdt sur une page de 10 000 blocs.
// Lancer : node --expose-gc spike/crdtbench-js/bench.mjs
import * as Y from "yjs";
import { LoroDoc, LoroText } from "loro-crdt";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const N = 10_000;
const line = (i) => `Bloc ${String(i).padStart(5, "0")} : une ligne de texte de quatre-vingts caracteres environ, pour mesurer.`.slice(0, 80);
const median = (v) => [...v].sort((a, b) => a - b)[Math.floor(v.length / 2)];
const time = (f) => { const t = performance.now(); const r = f(); return [r, performance.now() - t]; };
const heapMb = () => { globalThis.gc?.(); return process.memoryUsage().heapUsed / 1048576; };

// --- yjs (structure BlockNote)
function yBuilt() {
  const doc = new Y.Doc();
  const frag = doc.getXmlFragment("document-store");
  const group = new Y.XmlElement("blockGroup");
  const containers = [];
  for (let i = 0; i < N; i++) {
    const c = new Y.XmlElement("blockContainer");
    c.setAttribute("id", `b${i}`);
    const p = new Y.XmlElement("paragraph");
    p.insert(0, [new Y.XmlText(line(i))]);
    c.insert(0, [p]);
    containers.push(c);
  }
  group.insert(0, containers);
  frag.insert(0, [group]);
  return doc;
}
const yLoad = (bytes) => { const d = new Y.Doc(); Y.applyUpdate(d, bytes); return d; };
const yRead = (d) => d.getXmlFragment("document-store").get(0).toArray().map((c) => [c.getAttribute("id"), c.get(0).get(0).toString()]);

// --- loro-crdt (arbre déplaçable)
function lBuilt() {
  const doc = new LoroDoc();
  doc.setPeerId(1000n);
  const tree = doc.getTree("blocks");
  tree.enableFractionalIndex(0);
  for (let i = 0; i < N; i++) {
    const node = tree.createNode();
    node.data.set("id", `b${i}`);
    node.data.set("type", "paragraph");
    node.data.setContainer("text", new LoroText()).insert(0, line(i));
  }
  doc.commit();
  return doc;
}
const lLoad = (bytes) => { const d = new LoroDoc(); d.import(bytes); return d; };
const lRead = (d) => d.getTree("blocks").roots().map((n) => [n.data.get("id"), n.data.get("text").toString()]);

const rows = [];
rows.push(["W1 construire 10 000 blocs (ms)", median([0, 1, 2].map(() => time(yBuilt)[1])), median([0, 1, 2].map(() => time(lBuilt)[1]))]);
const ys = Y.encodeStateAsUpdate(yBuilt());
const ls = lBuilt().export({ mode: "snapshot" });
rows.push(["W2 instantané (Ko)", Math.round(ys.length / 1024), Math.round(ls.length / 1024)]);
rows.push(["W3 charger l'instantané (ms)", median([0, 1, 2, 3, 4].map(() => time(() => yLoad(ys))[1])), median([0, 1, 2, 3, 4].map(() => time(() => lLoad(ls))[1]))]);
rows.push([
  "W3b charger puis lire tous les blocs (ms)",
  median([0, 1, 2, 3, 4].map(() => time(() => yRead(yLoad(ys)).length)[1])),
  median([0, 1, 2, 3, 4].map(() => time(() => lRead(lLoad(ls)).length)[1])),
]);
const h0 = heapMb();
const keepY = yLoad(ys); yRead(keepY);
const h1 = heapMb();
const keepL = lLoad(ls); lRead(keepL);
const h2 = heapMb();
rows.push(["W4 mémoire après chargement et lecture (Mo de tas JS ; Loro vit surtout en WASM)", h1 - h0, h2 - h1]);

const fmt = (v) => (typeof v === "number" ? (Number.isInteger(v) ? String(v) : v.toFixed(1)) : v);
console.log("| Charge | yjs 13.6 | loro-crdt 1.17 |\n|---|---|---|");
for (const [w, a, b] of rows) console.log(`| ${w} | ${fmt(a)} | ${fmt(b)} |`);
const out = join(dirname(fileURLToPath(import.meta.url)), "..", "crdtbench", "results");
mkdirSync(out, { recursive: true });
writeFileSync(join(out, "js.csv"), "charge;yjs;loro\n" + rows.map((r) => r.map(fmt).join(";")).join("\n") + "\n");
void keepY; void keepL;
