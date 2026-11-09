import * as Y from "yjs";
import { readFileSync } from "node:fs";
const doc = new Y.Doc();
Y.applyUpdate(doc, readFileSync(process.argv[2]));
const frag = doc.getXmlFragment("document-store");
function dump(node, depth) {
  const pad = "  ".repeat(depth);
  if (node instanceof Y.XmlText) { console.log(pad + "TEXT " + JSON.stringify(node.toDelta())); return; }
  const attrs = node.getAttributes ? node.getAttributes() : {};
  console.log(pad + "<" + (node.nodeName ?? "#frag") + "> " + JSON.stringify(attrs));
  for (const c of node.toArray()) dump(c, depth + 1);
}
dump(frag, 0);
