// Le schéma BlockNote du client et le registre du serveur (`spec/schema/blocks.json`,
// généré par `mementum-schema`) décrivent les mêmes blocs, props, styles et contenus en
// ligne. Un écart veut dire qu'un client écrirait des nœuds que le serveur lit mal, ou
// l'inverse : y-prosemirror supprimerait alors chez tout le monde ce qu'il ne sait pas
// construire.
import { codeBlockOptions } from "@blocknote/code-block";
import { BlockNoteEditor } from "@blocknote/core";
import { describe, expect, test } from "vitest";
import registry from "../../../../spec/schema/blocks.json";
import { CLIENT_SCHEMA_VERSION, schema, supportedLanguages } from "./schema";

type RegistryProp = { name: string; type: string; default: unknown };

/** Type BlockNote d'une prop du registre. */
const PROP_TYPE: Record<string, string> = {
  string: "string",
  int: "number",
  float: "number",
  bool: "boolean",
};

type BlockNoteProp = { default?: unknown; type?: string };

/** Défaut d'une prop BlockNote, `undefined` lu comme le `null` du registre. */
function defaultOf(prop: BlockNoteProp): unknown {
  return prop.default === undefined ? null : prop.default;
}

/** Type d'une prop BlockNote : déclaré, sinon celui de son défaut. */
function typeOf(prop: BlockNoteProp): string {
  return prop.type ?? typeof prop.default;
}

function expectSameProps(
  where: string,
  actual: Record<string, BlockNoteProp>,
  expected: RegistryProp[],
) {
  expect(Object.keys(actual).sort(), `${where} : noms des props`).toEqual(
    expected.map((p) => p.name).sort(),
  );
  for (const p of expected) {
    const bn = actual[p.name];
    expect(defaultOf(bn), `${where}.${p.name} : défaut`).toEqual(p.default);
    expect(typeOf(bn), `${where}.${p.name} : type`).toBe(PROP_TYPE[p.type]);
  }
}

const editor = BlockNoteEditor.create({ schema });

describe("registry_matches_blocknote", () => {
  test("la version de schéma du client est celle du registre", () => {
    expect(CLIENT_SCHEMA_VERSION).toBe(registry.version);
  });

  test("chaque type du registre existe dans BlockNote, avec les mêmes props et défauts", () => {
    const blockSchema = editor.schema.blockSchema as Record<
      string,
      { content: string; propSchema: Record<string, BlockNoteProp> }
    >;
    for (const block of registry.blocks) {
      const bn = blockSchema[block.name];
      expect(bn, `type ${block.name} absent du schéma BlockNote`).toBeDefined();
      expectSameProps(block.name, bn.propSchema, block.props);
      // `plain` (texte brut, `codeBlock`) est un contenu en ligne sans marques (R19).
      const content = bn.content === "plain" ? "inline" : bn.content;
      expect(content, `${block.name} : contenu`).toBe(block.content);
    }
  });

  test("aucun type BlockNote n'est absent du registre", () => {
    const known = new Set(registry.blocks.map((b) => b.name));
    const missing = Object.keys(editor.schema.blockSchema).filter((name) => !known.has(name));
    expect(missing).toEqual([]);
  });

  test("les props de cellule de tableau sont celles du nœud tableCell de BlockNote", () => {
    // Vue « blocs » de BlockNote : les props d'une cellule lue dans le document.
    const withTable = BlockNoteEditor.create({
      schema,
      initialContent: [
        { type: "table", content: { type: "tableContent", rows: [{ cells: ["a"] }] } },
      ],
    });
    const table = withTable.document[0] as unknown as {
      content: { rows: { cells: { type: string; props: Record<string, unknown> }[] }[] };
    };
    const cell = table.content.rows[0].cells[0];
    expect(cell.type).toBe("tableCell");
    expect(Object.keys(cell.props).sort()).toEqual(
      registry.tableCell.props.map((p) => p.name).sort(),
    );
    for (const p of registry.tableCell.props) {
      expect(cell.props[p.name], `tableCell.${p.name} : défaut`).toEqual(p.default);
      expect(typeof cell.props[p.name], `tableCell.${p.name} : type`).toBe(PROP_TYPE[p.type]);
    }

    // Vue ProseMirror (celle que Yjs transporte) : `tableCell` et `tableHeader` ont les
    // mêmes attributs. Le seul attribut de plus, `colwidth`, porte la largeur de colonne,
    // écrite `widths` sur le tableau en MMD (R15, R19), et pas une prop de cellule.
    for (const node of ["tableCell", "tableHeader"]) {
      const attrs = editor.pmSchema.nodes[node].spec.attrs as Record<string, { default?: unknown }>;
      const names = registry.tableCell.props.map((p) => p.name);
      expect(Object.keys(attrs).filter((a) => !names.includes(a)), `${node} : attributs de plus`).toEqual([
        "colwidth",
      ]);
      for (const p of registry.tableCell.props) {
        expect(attrs[p.name]?.default, `${node}.${p.name} : défaut`).toEqual(p.default);
      }
    }
  });

  test("les styles sont ceux du registre", () => {
    expect(Object.keys(editor.schema.styleSchema).sort()).toEqual([...registry.styles].sort());
  });

  test("les contenus en ligne sont ceux du registre", () => {
    // `text` est le texte stylé lui-même ; `hardBreak` est un nœud ProseMirror de BlockNote,
    // pas un contenu en ligne déclaré.
    const inline = Object.keys(editor.schema.inlineContentSchema).filter((k) => k !== "text");
    expect(editor.pmSchema.nodes.hardBreak, "nœud hardBreak").toBeDefined();
    expect([...inline, "hardBreak"].sort()).toEqual([...registry.inline].sort());

    // Attributs que `mementum-crdt` lit et écrit (`blocknote.rs`).
    const specs = editor.schema.inlineContentSchema as unknown as Record<
      string,
      { propSchema: Record<string, BlockNoteProp> }
    >;
    expect(Object.keys(specs.mention.propSchema).sort()).toEqual(["id", "kind", "label"]);
    expect(Object.keys(specs.math.propSchema)).toEqual(["latex"]);
  });
});

// BlockNote refuse d'afficher un bloc de code dont la langue n'est pas dans le menu
// (`language in supportedLanguages`) : toute langue écrite par MMD ou par un autre client
// doit y passer, et le menu lui-même (`Object.keys`, `Object.entries`) ne change pas.
describe("supported_languages_admit_any_language", () => {
  test("toute chaîne est une langue admise", () => {
    for (const language of ["rust", "text", "mon langage", "", "x".repeat(100_000), "__proto__"]) {
      expect(language in supportedLanguages, JSON.stringify(language.slice(0, 20))).toBe(true);
    }
  });

  test("le menu propose les langues de @blocknote/code-block, sans plus", () => {
    expect(Object.keys(supportedLanguages)).toEqual(Object.keys(codeBlockOptions.supportedLanguages));
    expect(Object.entries(supportedLanguages)).toEqual(
      Object.entries(codeBlockOptions.supportedLanguages),
    );
    expect(Object.prototype.hasOwnProperty.call(supportedLanguages, "mon langage")).toBe(false);
  });
});
