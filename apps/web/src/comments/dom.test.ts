// Ce que les commentaires lisent dans l'éditeur affiché : le bloc où est la sélection (pour
// Ctrl+Alt+M dans un éditeur en lecture seule) et le texte propre d'un bloc.
import { afterEach, expect, test } from "vitest";
import { blockOfSelection, blockText, isolate, snippet } from "./dom";

/** Un bloc tel que BlockNote le rend : `blockOuter` > `blockContainer` > contenu, puis ses enfants. */
function block(id: string, content: Node, children: HTMLElement[] = []): HTMLElement {
  const outer = document.createElement("div");
  outer.setAttribute("data-node-type", "blockOuter");
  outer.setAttribute("data-id", id);
  const container = document.createElement("div");
  container.className = "bn-block";
  container.setAttribute("data-node-type", "blockContainer");
  container.setAttribute("data-id", id);
  const blockContent = document.createElement("div");
  blockContent.className = "bn-block-content";
  blockContent.append(content);
  container.append(blockContent);
  if (children.length > 0) {
    const group = document.createElement("div");
    group.className = "bn-block-group";
    group.append(...children);
    container.append(group);
  }
  outer.append(container);
  return outer;
}

function text(value: string): Text {
  return document.createTextNode(value);
}

/** L'éditeur, et à côté de lui un texte qui n'en est pas. */
function scene() {
  const editor = document.createElement("div");
  editor.className = "bn-editor";
  const nestedText = text("enfant");
  const cell = document.createElement("td");
  const cellText = text("cellule");
  cell.append(cellText);
  const row = document.createElement("tr");
  row.append(cell);
  const table = document.createElement("table");
  table.append(row);
  const topText = text("premier");
  editor.append(
    block("premier", topText),
    block("parent", text("parent"), [block("enfant", nestedText)]),
    block("tableau", table),
  );
  const outside = document.createElement("p");
  const outsideText = text("ailleurs");
  outside.append(outsideText);
  document.body.append(editor, outside);
  return { editor, topText, nestedText, cellText, outsideText };
}

afterEach(() => {
  document.getSelection()?.removeAllRanges();
  document.body.replaceChildren();
});

/** Sélectionne `node` du caractère `from` au caractère `to`. */
function select(node: Node, from = 0, to = from) {
  const range = document.createRange();
  range.setStart(node, from);
  range.setEnd(node, to);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  return selection;
}

test("a_selection_in_a_block_names_that_block", () => {
  const { editor, topText } = scene();
  expect(blockOfSelection(editor, select(topText, 1, 4))).toBe("premier");
  // Un curseur sans étendue (la navigation au curseur du navigateur) aussi.
  expect(blockOfSelection(editor, select(topText, 2))).toBe("premier");
});

test("a_selection_in_a_nested_block_names_the_nested_block", () => {
  const { editor, nestedText } = scene();
  expect(blockOfSelection(editor, select(nestedText, 0, 3))).toBe("enfant");
});

test("a_selection_in_a_table_cell_names_the_table", () => {
  const { editor, cellText } = scene();
  expect(blockOfSelection(editor, select(cellText, 1))).toBe("tableau");
});

test("a_selection_outside_the_editor_names_nothing", () => {
  const { editor, outsideText } = scene();
  expect(blockOfSelection(editor, select(outsideText, 0, 3))).toBeNull();
});

test("no_selection_names_nothing", () => {
  const { editor } = scene();
  document.getSelection()!.removeAllRanges();
  expect(blockOfSelection(editor, document.getSelection())).toBeNull();
  expect(blockOfSelection(editor, null)).toBeNull();
});

test("a_selection_on_the_editor_itself_names_nothing", () => {
  const { editor } = scene();
  expect(blockOfSelection(editor, select(editor, 0))).toBeNull();
});

test("the_text_of_a_block_is_its_own", () => {
  const { editor } = scene();
  const parent = editor.querySelector('[data-id="parent"]')!;
  expect(blockText(parent)).toBe("parent");
});

test("the_text_of_a_toggle_block_is_its_own", () => {
  // Un bloc repliable : son contenu et ses enfants sont dans le cadre du repli
  // (`.bn-block > .bn-toggle-frame > .bn-toggle-slot`).
  const outer = document.createElement("div");
  outer.setAttribute("data-node-type", "blockOuter");
  outer.setAttribute("data-id", "repli");
  const container = document.createElement("div");
  container.className = "bn-block";
  const frame = document.createElement("div");
  frame.className = "bn-toggle-wrapper bn-toggle-frame";
  const toggle = document.createElement("button");
  toggle.className = "bn-toggle-button";
  const slot = document.createElement("div");
  slot.className = "bn-toggle-slot";
  const content = document.createElement("div");
  content.className = "bn-block-content";
  content.textContent = "Titre du repli";
  const group = document.createElement("div");
  group.className = "bn-block-group";
  group.append(block("dedans", text("enfant caché")));
  slot.append(content, group);
  frame.append(toggle, slot);
  container.append(frame);
  outer.append(container);
  expect(blockText(outer)).toBe("Titre du repli");
});

test("isolate_wraps_text_in_first_strong_isolates", () => {
  expect(isolate("‮abc")).toBe("⁨‮abc⁩");
});

test("a_snippet_is_one_line_cut_after_a_word", () => {
  // Les sauts de ligne et les suites d'espaces deviennent une espace ; un texte court reste tel quel.
  expect(snippet("  ligne un\n\n  ligne deux  ", 60)).toBe("ligne un ligne deux");
  // Trop long : coupé à la fin du dernier mot entier, suivi de « … ».
  expect(snippet("un deux trois quatre cinq", 16)).toBe("un deux trois…");
  // Un seul mot trop long : coupé à la borne.
  expect(snippet("anticonstitutionnellement", 10)).toBe("anticonsti…");
});

test("a_snippet_never_splits_a_character", () => {
  // Une famille (quatre emojis joints) et une lettre suivie de son accent combinant comptent
  // chacune pour un caractère, et ne sont jamais coupées en deux.
  const family = "👨‍👩‍👧‍👦";
  expect(snippet(family.repeat(5), 3)).toBe(`${family.repeat(3)}…`);
  const accented = "e\u0301";
  expect(snippet(accented.repeat(8), 4)).toBe(`${accented.repeat(4)}…`);
});
