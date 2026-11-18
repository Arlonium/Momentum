// Ce que la page sert, relu par le vrai Yjs (Ruling R862). Une page relue par le serveur repasse par
// les règles de la garde, et ne voit pas ce qu'elles récrivent mal : le lecteur juge ici est celui
// d'un navigateur, sans garde.
//
// `crates/mementum-crdt/tests/fixtures/hostile/served.json` porte des pages écrites par des mises à
// jour hostiles (`crates/mementum-crdt/tests/placements_at_random.rs`), avec le plan que la page
// lit. `EXPORT_FIXTURES=1 cargo test -p mementum-crdt --test placements_at_random` le réécrit.
//
// L'essai élargi : `MEMENTUM_FUZZ_SERVED=<fichier>`, le fichier écrit par l'essai de Rust, avec la
// même variable.
import { expect, test } from "vitest";
import * as Y from "yjs";

import { nodeModule, type Fs, type Path, type Url } from "../test/node";

type Served = { seed: number; stored: boolean; state: string; outline: string[] };

const unhex = (text: string): Uint8Array => Uint8Array.from(text.match(/../g) ?? [], (pair) => parseInt(pair, 16));

// Le même relevé que `outline` (`crates/mementum-crdt/tests/common/live.rs`).
function outline(doc: Y.Doc): string[] {
  const out: string[] = [];
  const walk = (nodes: unknown[]): void => {
    for (const node of nodes) {
      if (node instanceof Y.XmlText) {
        const runs = node.toDelta() as { insert: unknown }[];
        out.push(`texte ${runs.map((run) => (typeof run.insert === "string" ? run.insert : "?")).join("")}`);
      } else if (node instanceof Y.XmlElement) {
        out.push(`<${node.nodeName}>`);
        walk(node.toArray());
        out.push(`</${node.nodeName}>`);
      } else {
        out.push("fragment");
      }
    }
  };
  walk(doc.getXmlFragment("document-store").toArray());
  return out;
}

function judge(scenes: Served[]): string[] {
  const gaps: string[] = [];
  for (const scene of scenes) {
    const doc = new Y.Doc();
    Y.applyUpdate(doc, unhex(scene.state));
    const name = `graine ${scene.seed}, voie ${scene.stored ? "journal" : "client"}`;
    // Un journal peut garder une suppression d'horloges que la page n'a pas (Ruling R199) : Yjs la
    // garde en attente comme la page. D'un client, rien n'en laisse (Ruling R198).
    if (doc.store.pendingStructs !== null || (!scene.stored && doc.store.pendingDs !== null)) gaps.push(`${name} : éléments en attente`);
    const read = outline(doc);
    if (JSON.stringify(read) !== JSON.stringify(scene.outline)) {
      const at = read.findIndex((line, n) => line !== scene.outline[n]);
      gaps.push(`${name}, ligne ${at} : la page lit ${JSON.stringify(scene.outline[at])}, Yjs ${JSON.stringify(read[at])}`);
    }
    doc.destroy();
  }
  return gaps;
}

test("un vrai Yjs lit la page que le serveur sert", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const large = import.meta.env.MEMENTUM_FUZZ_SERVED as string | undefined;
  const scenes = large
    ? (await fs.readFile(large, "utf8"))
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line) as Served)
    : (
        JSON.parse(
          await fs.readFile(
            path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", "crates/mementum-crdt/tests/fixtures/hostile/served.json"),
            "utf8",
          ),
        ) as { scenes: Served[] }
      ).scenes;
  expect(scenes.length).toBeGreaterThan(0);
  const gaps = judge(scenes);
  console.info(`ESSAI Yjs : ${scenes.length} pages servies relues, ${gaps.length} écarts`);
  expect(gaps.slice(0, 10)).toEqual([]);
}, 3_600_000);
