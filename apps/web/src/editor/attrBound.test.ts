// Ruling R695 : l'éditeur n'écrit pas un attribut que la garde des attributs du serveur
// refuserait (`crates/mementum-crdt/src/attrs.rs`, R689) : la connexion se fermerait et
// rouvrirait sans fin. Un navigateur y arrive par un collage (BlockNote lit `colspan`, `rowspan`,
// `colwidth` et le niveau d'un titre tels que le HTML les écrit) ou par une fusion de plus de
// cent colonnes. Ce qui arrive du document partagé n'est jamais refusé.
import { afterEach, describe, expect, test } from "vitest";
import * as Y from "yjs";
import registry from "../../../../spec/schema/blocks.json";
import { REMOTE, elements, peer, seedState, settle, type Peer, type Seed } from "../test/editors";
import { ATTR_REFUSED, MAX_COLSPAN, TYPED, soundAttr, type Kind } from "./attrBound";
import { linkBound } from "./linkBound";

const extensions = [linkBound];

afterEach(() => {
  for (const element of document.querySelectorAll(".mm-attr-refused")) element.remove();
});

const TABLE: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  {
    id: "t",
    type: "table",
    content: {
      type: "tableContent",
      rows: [{ cells: ["a", "b"] }, { cells: ["c", "d"] }],
    },
  },
  { id: "h", type: "heading", content: "Titre" },
] as Seed;

function alert(): string | null {
  return document.querySelector(".mm-attr-refused[role=alert]")?.textContent ?? null;
}

/** La position et les attributs du premier nœud `name` de l'éditeur. */
function firstNode(p: Peer, name: string): { pos: number; attrs: Record<string, unknown> } {
  let found: { pos: number; attrs: Record<string, unknown> } | null = null;
  p.editor.prosemirrorState.doc.descendants((node, pos) => {
    if (found === null && node.type.name === name) found = { pos, attrs: node.attrs };
    return found === null;
  });
  if (found === null) throw new Error(`aucun nœud ${name}`);
  return found;
}

/** Le nom du nœud de la première cellule du tableau (une cellule d'en-tête, ou non). */
function cellName(p: Peer): string {
  let name: string | null = null;
  p.editor.prosemirrorState.doc.descendants((node) => {
    if (name === null && (node.type.name === "tableHeader" || node.type.name === "tableCell")) {
      name = node.type.name;
    }
    return name === null;
  });
  if (name === null) throw new Error("aucune cellule");
  return name;
}

/** Pose `attrs` sur le premier nœud `name`, comme un geste de l'éditeur (`setNodeMarkup`). */
function setAttrs(p: Peer, name: string, attrs: Record<string, unknown>): void {
  const node = firstNode(p, name);
  p.editor.transact((tr) => tr.setNodeMarkup(node.pos, null, { ...node.attrs, ...attrs }));
}

/** Les attributs du document partagé que le serveur refuserait, « nœud.attribut ». */
function refusedByTheServer(doc: Y.Doc): string[] {
  return elements(doc).flatMap((element) =>
    Object.entries(element.getAttributes() as Record<string, unknown>)
      .filter(([name, value]) => !soundAttr(element.nodeName, name, value))
      .map(([name]) => `${element.nodeName}.${name}`),
  );
}

/** La valeur de l'attribut `name` du premier élément `node` du document partagé. */
function storedAttr(doc: Y.Doc, node: string, name: string): unknown {
  const element = elements(doc).find((e) => e.nodeName === node);
  if (element === undefined) throw new Error(`aucun élément ${node}`);
  return (element.getAttributes() as Record<string, unknown>)[name];
}

describe("attr_bound", () => {
  test("la table des attributs typés est celle du registre des blocs", () => {
    const kinds: Record<string, Kind> = { bool: "bool", int: "int", float: "float" };
    const wanted: Record<string, Record<string, Kind>> = {};
    for (const block of registry.blocks) {
      for (const prop of block.props) {
        if (prop.type === "string") continue;
        (wanted[block.name] ??= {})[prop.name] = kinds[prop.type];
      }
    }
    const cell: Record<string, Kind> = { colspan: "colspan", rowspan: "rowspan", colwidth: "widths" };
    expect(registry.tableCell.props.filter((p) => p.type !== "string").map((p) => p.name)).toEqual([
      "colspan",
      "rowspan",
    ]);
    wanted.tableCell = cell;
    wanted.tableHeader = cell;
    expect(TYPED).toEqual(wanted);
  });

  test("le jugement est celui du serveur, type par type", () => {
    const object = { toString: 1 };
    for (const [node, name, value, sound] of [
      ["heading", "level", 3, true],
      ["heading", "level", 2.5, false],
      ["heading", "level", 1e21, false],
      ["heading", "level", "2", false],
      ["heading", "level", undefined, true],
      ["heading", "level", null, true],
      ["checkListItem", "checked", true, true],
      ["checkListItem", "checked", "true", false],
      ["image", "previewWidth", 320.5, true],
      ["image", "previewWidth", Number.NaN, false],
      ["image", "previewWidth", "320", false],
      ["image", "url", "u".repeat(100_000), true],
      ["image", "url", object, false],
      ["paragraph", "backgroundColor", object, false],
      ["paragraph", "backgroundColor", ["red"], false],
      ["paragraph", "mienne", 7, true],
      ["blockContainer", "id", "x", true],
      ["blockContainer", "id", object, false],
      ["tableCell", "colspan", 1, true],
      ["tableCell", "colspan", MAX_COLSPAN, true],
      ["tableCell", "colspan", MAX_COLSPAN + 1, false],
      ["tableCell", "colspan", 0, false],
      ["tableCell", "colspan", "abc", false],
      ["tableHeader", "rowspan", 250_000, true],
      ["tableHeader", "rowspan", 0, false],
      ["tableCell", "colwidth", [120, undefined, null], true],
      ["tableCell", "colwidth", [Number.NaN], false],
      ["tableCell", "colwidth", ["120"], false],
      ["tableCell", "colwidth", Array.from({ length: MAX_COLSPAN + 1 }, () => 1), false],
      ["tableCell", "colwidth", 120, false],
    ] as [string, string, unknown, boolean][]) {
      expect(soundAttr(node, name, value), `${node}.${name} = ${String(JSON.stringify(value))}`).toBe(sound);
    }
  });

  test("une fusion de cent colonnes est écrite ; d'une de plus, elle est refusée et le message le dit", async () => {
    const mona = await peer("Mona", 2, await seedState(TABLE), extensions);
    const header = cellName(mona);
    setAttrs(mona, header, { colspan: MAX_COLSPAN });
    await settle();
    expect(storedAttr(mona.doc, header, "colspan")).toBe(MAX_COLSPAN);
    expect(alert()).toBeNull();

    setAttrs(mona, header, { colspan: MAX_COLSPAN + 1 });
    await settle();
    expect(firstNode(mona, header).attrs.colspan).toBe(MAX_COLSPAN);
    expect(storedAttr(mona.doc, header, "colspan")).toBe(MAX_COLSPAN);
    expect(alert()).toBe(ATTR_REFUSED);
    expect(refusedByTheServer(mona.doc)).toEqual([]);
  });

  test("un attribut d'un autre type que le sien n'est pas écrit", async () => {
    const mona = await peer("Mona", 2, await seedState(TABLE), extensions);
    const before = Y.encodeStateAsUpdate(mona.doc);
    for (const [node, attrs] of [
      ["heading", { level: 2.5 }],
      ["heading", { level: 1e21 }],
      ["heading", { backgroundColor: { toString: 1 } }],
      [cellName(mona), { colwidth: [Number.NaN] }],
      [cellName(mona), { colspan: "abc" }],
    ] as [string, Record<string, unknown>][]) {
      setAttrs(mona, node, attrs);
      await settle();
      expect(alert(), `${node} ${JSON.stringify(attrs)}`).toBe(ATTR_REFUSED);
      document.querySelector(".mm-attr-refused")?.remove();
    }
    expect(Y.encodeStateAsUpdate(mona.doc)).toEqual(before);
    // Un niveau de titre honnête passe.
    setAttrs(mona, "heading", { level: 2 });
    await settle();
    expect(storedAttr(mona.doc, "heading", "level")).toBe(2);
    expect(alert()).toBeNull();
  });

  test("ce qui arrive du document partagé n'est jamais refusé", async () => {
    const mona = await peer("Mona", 2, await seedState(TABLE), extensions);
    const cell = elements(mona.doc).find((e) => e.nodeName === "tableHeader" || e.nodeName === "tableCell");
    if (cell === undefined) throw new Error("aucune cellule");
    mona.doc.transact(() => cell.setAttribute("colspan", (MAX_COLSPAN + 1) as unknown as string), REMOTE);
    await settle();
    const shown = firstNode(mona, cell.nodeName).attrs.colspan;
    expect(shown).toBe(MAX_COLSPAN + 1);
    expect(alert()).toBeNull();
  });
});
