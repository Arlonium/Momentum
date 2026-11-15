// Une prop de bloc vient d'ailleurs : de MMD, d'un agent, d'un autre client. Yjs y garde toute
// valeur JSON (un nombre, un objet, une liste), et y-prosemirror la passe telle quelle à
// l'éditeur. Un bloc qui la rendait sans la vérifier cassait l'éditeur de tous ceux qui ouvrent
// la page (« Objects are not valid as a React child »). Toute prop se lit donc par `textProp`.
import { expect, test } from "vitest";
import * as Y from "yjs";
import { mountEditor } from "../../test/richEditor";
import { FRAGMENT } from "../collab";
import { textProp } from "./props";

test("text_prop_takes_only_a_string", () => {
  expect(textProp({ url: "https://a.fr/" }, "url")).toBe("https://a.fr/");
  expect(textProp({ url: "" }, "url", "x")).toBe("");
  for (const value of [undefined, null, 42, true, {}, [], ["a"], { toString: () => "a" }]) {
    expect(textProp({ url: value }, "url"), JSON.stringify(value)).toBe("");
    expect(textProp({ url: value }, "url", "défaut"), JSON.stringify(value)).toBe("défaut");
  }
  expect(textProp({}, "url", "défaut")).toBe("défaut");
});

/** Ce qu'aucun client honnête n'écrit dans une prop de texte. */
const NOT_TEXT: unknown[] = [{ a: 1 }, 42, true, ["https://a.fr/"], null];

/**
 * Une page : un bloc `type` dont les attributs sont `attrs` (écrits tels quels dans Yjs), puis un
 * paragraphe « Après ». `inline` : le bloc est un paragraphe qui porte un contenu en ligne `type`.
 */
function pageWith(type: string, attrs: Record<string, unknown>, { inline = false, text = false } = {}): Y.Doc {
  const doc = new Y.Doc();
  doc.transact(() => {
    const group = new Y.XmlElement("blockGroup");
    doc.getXmlFragment(FRAGMENT).insert(0, [group]);
    const container = new Y.XmlElement("blockContainer");
    group.insert(0, [container]);
    container.setAttribute("id", "b");
    const element = new Y.XmlElement(inline ? "paragraph" : type);
    container.insert(0, [element]);
    const target = inline ? new Y.XmlElement(type) : element;
    if (inline) {
      const before = new Y.XmlText();
      element.insert(0, [before, target]);
      before.insert(0, "Texte ");
    } else if (text) {
      const content = new Y.XmlText();
      element.insert(0, [content]);
      content.insert(0, "Texte du bloc");
    }
    for (const [name, value] of Object.entries(attrs)) {
      target.setAttribute(name, value as string);
    }
    const next = new Y.XmlElement("blockContainer");
    group.insert(1, [next]);
    next.setAttribute("id", "n");
    const paragraph = new Y.XmlElement("paragraph");
    next.insert(0, [paragraph]);
    const after = new Y.XmlText();
    paragraph.insert(0, [after]);
    after.insert(0, "Après");
  });
  return doc;
}

const CASES: { type: string; props: string[]; inline?: boolean; text?: boolean }[] = [
  { type: "bookmark", props: ["url"] },
  { type: "callout", props: ["icon", "color"], text: true },
  { type: "equation", props: ["latex"] },
  { type: "pageLink", props: ["pageId", "title"] },
  { type: "databaseRef", props: ["databaseId", "view"] },
  { type: "syncedBlock", props: ["ref"] },
  { type: "unknown", props: ["originalType", "data"] },
  { type: "mention", props: ["kind", "id", "label"], inline: true },
  { type: "math", props: ["latex"], inline: true },
];

test("custom_blocks_survive_props_that_are_not_text", async () => {
  for (const { type, props, inline, text } of CASES) {
    for (const value of NOT_TEXT) {
      const attrs = Object.fromEntries(props.map((name) => [name, value]));
      const where = `${type} ${JSON.stringify(attrs)}`;
      const page = await mountEditor({ doc: pageWith(type, attrs, { inline, text }) });
      expect(page.text(), where).toContain("Après");
      expect(page.errors.filter((e) => /not valid as a React child|is not a function|Cannot read/.test(e)), where).toEqual([]);
    }
  }
});
