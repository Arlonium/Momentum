// Sonde du Step 3 (Task S0 du lot 5) : le vrai Yjs relit ce que le serveur écrit par préfixe et
// suffixe communs, avec une frappe concurrente du vrai Yjs, et le compare à ce que relit yrs.
//
//   node probe/inline.mjs <agents-bench.exe> <dossier jetable> <chemin de dist/yjs.mjs>
//
// Le Yjs donné doit être celui du client (apps/web, `yjs@13.6.33` du verrou de pnpm).
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [exe, dir, yjsPath] = process.argv.slice(2);
const Y = await import(pathToFileURL(yjsPath).href);

execFileSync(exe, ["inline-base", dir], { stdio: "inherit" });

const FRAGMENT = "document-store";

function containers(doc) {
  return doc.getXmlFragment(FRAGMENT).get(0).toArray();
}
function contentOf(container) {
  return container.toArray().find((n) => n.nodeName !== "blockGroup");
}

function readPage(doc) {
  return containers(doc).map((c) => {
    const content = contentOf(c);
    return {
      type: content.nodeName,
      children: content.toArray().map((child) =>
        child instanceof Y.XmlText
          ? { text: child.toDelta().map((d) => ({ insert: d.insert, attributes: d.attributes ?? {} })) }
          : { el: child.nodeName, attrs: child.getAttributes() },
      ),
    };
  });
}

function stable(v) {
  if (Array.isArray(v)) return v.map(stable);
  if (v && typeof v === "object") {
    return Object.fromEntries(
      Object.keys(v)
        .sort()
        .filter((k) => v[k] !== null && v[k] !== undefined)
        .map((k) => [k, stable(v[k])]),
    );
  }
  return v;
}

// Les plages voisines de mêmes marques fondues, les marques nulles retirées, les plages vides ôtées.
function normalize(page) {
  return page.map((b) => ({
    type: b.type,
    children: b.children.map((c) => {
      if (!c.text) return { el: c.el, attrs: stable(c.attrs) };
      const runs = [];
      for (const r of c.text) {
        const attributes = stable(r.attributes ?? {});
        if (r.insert === "") continue;
        const last = runs[runs.length - 1];
        if (last && JSON.stringify(last.attributes) === JSON.stringify(attributes)) last.insert += r.insert;
        else runs.push({ insert: r.insert, attributes });
      }
      return { text: runs };
    }),
  }));
}

const same = (a, b) => JSON.stringify(normalize(a)) === JSON.stringify(normalize(b));

// Retire la frappe « Z » et rend la page sans elle, et les marques de la plage qui la portait.
function withoutZ(page) {
  let marks = null;
  const out = normalize(page).map((b) => ({
    type: b.type,
    children: b.children.map((c) => {
      if (!c.text) return c;
      return {
        text: c.text.map((r) => {
          if (r.insert.includes("Z")) {
            marks = r.attributes;
            return { ...r, insert: r.insert.replace("Z", "") };
          }
          return r;
        }),
      };
    }),
  }));
  return { page: normalize(out), marks };
}

// Les frappes du vrai Yjs, chacune faite sur l'état de départ : (bloc, enfant, place).
const KEYS = {
  prefix: (doc) => [0, 0, 1],
  suffix: (doc) => {
    const children = contentOf(containers(doc)[0]).toArray();
    const last = children.length - 1;
    return [0, last, children[last].length - 1];
  },
  untouched: (doc) => [1, 0, 3],
  // Juste avant et juste après ce que l'écriture retire (ses voisins immédiats).
  before: (doc, edit) => [0, edit.child, edit.at],
  after: (doc, edit) => [0, edit.child, edit.at + edit.len],
};

function keystroke(base, which, edit) {
  const doc = new Y.Doc();
  Y.applyUpdate(doc, base);
  const sv = Y.encodeStateVector(doc);
  const [block, child, at] = KEYS[which](doc, edit);
  const text = contentOf(containers(doc)[block]).toArray()[child];
  // Les marques en vigueur à la place, comme une frappe dans l'éditeur.
  const before = text.toDelta();
  text.insert(at, "Z");
  return { update: Y.encodeStateAsUpdate(doc, sv), before };
}

const combos = [];
const checks = [];
for (const scene of readdirSync(dir)) {
  const at = join(dir, scene);
  const base = readFileSync(join(at, "base.bin"));
  const expected = JSON.parse(readFileSync(join(at, "expected.json"), "utf8"));
  const edit = JSON.parse(readFileSync(join(at, "edit.json"), "utf8"));
  for (const which of Object.keys(KEYS)) {
    writeFileSync(join(at, `key-${which}.bin`), keystroke(base, which, edit).update);
  }
  for (const mode of ["affix", "whole"]) {
    const runs = [[`${mode}.bin`], ...Object.keys(KEYS).flatMap((k) => [
      [`${mode}.bin`, `key-${k}.bin`],
      [`key-${k}.bin`, `${mode}.bin`],
    ])];
    for (const files of runs) {
      const doc = new Y.Doc();
      // Ce que Yjs écrit de lui-même en appliquant des mises à jour distantes : le nettoyage des
      // marques (`cleanupYTextAfterTransaction`), une transaction locale qu'un navigateur
      // enverrait au serveur.
      const local = [];
      doc.on("update", (u, origin, d, tr) => {
        if (tr.local) local.push(u);
      });
      for (const f of ["base.bin", ...files]) Y.applyUpdate(doc, readFileSync(join(at, f)));
      const name = files.join("+").replaceAll(".bin", "");
      const out = `yrs-${name}.json`;
      combos.push({ scene, files: ["base.bin", ...files], out });
      let cleanupOut = null;
      if (local.length > 0) {
        writeFileSync(join(at, `cleanup-${name}.bin`), Y.mergeUpdates(local));
        cleanupOut = `yrs-${name}+cleanup.json`;
        combos.push({ scene, files: ["base.bin", ...files, `cleanup-${name}.bin`], out: cleanupOut });
      }
      checks.push({ scene, mode, files, out, cleanupOut, yjs: readPage(doc), expected });
    }
  }
}
writeFileSync(join(dir, "combos.json"), JSON.stringify(combos, null, 1));
execFileSync(exe, ["inline-apply", dir], { stdio: "inherit" });

let failures = 0;
const rows = [];
for (const c of checks) {
  const yrs = JSON.parse(readFileSync(join(dir, c.scene, c.out), "utf8"));
  const agree = same(c.yjs, yrs);
  const key = c.files.find((f) => f.startsWith("key-"));
  let verdict;
  if (!key) {
    verdict = same(c.yjs, c.expected) ? "écriture lue comme voulu" : "ÉCART avec la page voulue";
  } else {
    const hasZ = JSON.stringify(c.yjs).includes("Z");
    const { page, marks } = withoutZ(c.yjs);
    const rest = JSON.stringify(page) === JSON.stringify(normalize(c.expected));
    verdict = `frappe ${hasZ ? "gardée" : "PERDUE"} ; écriture ${rest ? "entière" : "ALTÉRÉE"}${
      hasZ ? ` ; marques de la frappe ${JSON.stringify(marks)}` : ""
    }`;
  }
  if (!agree) failures++;
  let cleanup = "";
  if (c.cleanupOut) {
    const after = JSON.parse(readFileSync(join(dir, c.scene, c.cleanupOut), "utf8"));
    cleanup = ` ; Yjs a nettoyé des marques (transaction locale) ; yrs, nettoyage reçu, ${
      same(c.yjs, after) ? "=" : "≠"
    } Yjs`;
  }
  rows.push(
    `${c.scene.padEnd(17)} ${c.mode.padEnd(5)} ${c.files.join(" puis ").padEnd(32)} Yjs ${agree ? "=" : "≠"} yrs ; ${verdict}${cleanup}`,
  );
}
console.log(rows.join("\n"));
console.log(`\n${checks.length} combinaisons, ${failures} où le vrai Yjs ne lit pas comme yrs`);
for (const c of checks.filter((c) => !same(c.yjs, JSON.parse(readFileSync(join(dir, c.scene, c.out), "utf8")))).slice(0, 3)) {
  console.log("Yjs :", JSON.stringify(normalize(c.yjs)[0]));
  console.log("yrs :", JSON.stringify(normalize(JSON.parse(readFileSync(join(dir, c.scene, c.out), "utf8")))[0]));
}
