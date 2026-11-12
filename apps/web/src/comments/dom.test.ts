// Ce que les commentaires lisent dans l'éditeur affiché : le bloc où est la sélection (pour
// Ctrl+Alt+M dans un éditeur en lecture seule) et le texte propre d'un bloc.
import { afterEach, expect, test } from "vitest";
import { blockOfSelection } from "./dom";

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
