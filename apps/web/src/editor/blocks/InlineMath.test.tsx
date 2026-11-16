// La formule en ligne : posée par « Équation en ligne » au curseur, réglée dans sa fenêtre (rendue
// hors du document, tenue par la vue de la formule), rouverte d'un clic ou par Ctrl+Alt+Entrée
// quand elle est sélectionnée. Son brouillon vit dans la fenêtre de l'éditeur, tant que la page est
// ouverte, et la fenêtre le dit.
import { NodeSelection } from "prosemirror-state";
import { expect, test } from "vitest";
import * as Y from "yjs";
import { until } from "../../test/hooks";
import { cursor, elements, exchange, peer, REMOTE, seedState, type Seed } from "../../test/editors";
import { blockOf, blockWindow, fill, flush, mountEditor, press, type Mounted } from "../../test/richEditor";
import { FRAGMENT } from "../collab";
import { richSlashItems } from "../slashItems";
import { INLINE_DRAFT_NOTE, INLINE_GONE, openInlineMath, placeAt } from "./InlineMath";

const WINDOW = "Formule en ligne";
const FORMULA = 'button[aria-label="Modifier la formule"]';

/** Les formules en ligne du document `doc`, lues sans éditeur : leur `latex`. */
function storedFormulas(doc: Y.Doc): unknown[] {
  return elements(doc)
    .filter((element) => element.nodeName === "math")
    .map((element) => element.getAttribute("latex"));
}

/** Le champ de la fenêtre ouverte, quand la vue de la formule est trouvée (plus d'attente). */
async function field(_m: Mounted): Promise<HTMLTextAreaElement> {
  const dialog = blockWindow(WINDOW);
  expect(dialog, "la fenêtre de la formule").not.toBeNull();
  return dialog!.querySelector("textarea")!;
}

test("inline_math_opens_its_field_when_inserted_and_on_click", async () => {
  const m = await mountEditor({ blocks: [{ id: "x", type: "paragraph", content: "Du texte" }] });
  cursor(m, "x", "Du texte".length);
  richSlashItems(m.editor).find((item) => item.title === "Équation en ligne")!.onItemClick();
  await flush();
  // Posée vide au curseur ; sa fenêtre s'ouvre, le focus dans son champ.
  expect(storedFormulas(m.doc)).toEqual([""]);
  let typed = await field(m);
  expect(document.activeElement).toBe(typed);
  expect(blockWindow(WINDOW)!.textContent).toContain(INLINE_DRAFT_NOTE);
  fill(typed, "E = mc^2");
  await flush();
  expect(storedFormulas(m.doc), "rien ne s'écrit tant qu'on tape").toEqual([""]);
  press(typed, "Enter", { ctrlKey: true });
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  expect(storedFormulas(m.doc)).toEqual(["E = mc^2"]);
  // Posée depuis le texte : le focus y revient.
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);

  // Un clic sur la formule rouvre sa fenêtre ; Échap y rend le focus.
  blockOf(m, "x").querySelector<HTMLButtonElement>(FORMULA)!.click();
  await flush();
  typed = await field(m);
  expect(typed.value).toBe("E = mc^2");
  expect(document.activeElement).toBe(typed);
  fill(typed, "E = mc^3");
  press(typed, "Escape");
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  expect(document.activeElement).toBe(blockOf(m, "x").querySelector(FORMULA));
  // Échap n'a pas perdu le texte tapé.
  blockOf(m, "x").querySelector<HTMLButtonElement>(FORMULA)!.click();
  await flush();
  expect((await field(m)).value).toBe("E = mc^3");
  expect(storedFormulas(m.doc)).toEqual(["E = mc^2"]);
});

test("an_untouched_inline_field_writes_nothing", async () => {
  const m = await mountEditor({
    blocks: [{ id: "x", type: "paragraph", content: ["Avec ", { type: "math", props: { latex: "a" } }, " fin"] }],
  });
  blockOf(m, "x").querySelector<HTMLButtonElement>(FORMULA)!.click();
  await flush();
  press(await field(m), "Enter", { ctrlKey: true });
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  expect(m.writes).toEqual([]);
});

test("ctrl_alt_enter_opens_the_window_of_a_selected_inline_formula", async () => {
  const m = await mountEditor({
    blocks: [{ id: "x", type: "paragraph", content: ["Avec ", { type: "math", props: { latex: "a^2" } }, " fin"] }],
  });
  m.editor.focus();
  m.editor.transact((tr) => {
    let at = -1;
    tr.doc.descendants((node, pos) => {
      if (node.type.name === "math") at = pos;
    });
    tr.setSelection(NodeSelection.create(tr.doc, at));
  });
  const event = new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, altKey: true, bubbles: true, cancelable: true });
  m.editor.prosemirrorView.dom.dispatchEvent(event);
  await flush();
  expect(event.defaultPrevented).toBe(true);
  expect((await field(m)).value).toBe("a^2");
});

test("the_window_of_a_formula_another_deletes_keeps_the_text_and_the_focus", async () => {
  const seed: Seed = [
    { id: "x", type: "paragraph", content: ["Avec ", { type: "math", props: { latex: "a" } }, " fin"] },
    { id: "z", type: "paragraph", content: "Après" },
  ];
  const base = await seedState(seed);
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  blockOf(mona, "x").querySelector<HTMLButtonElement>(FORMULA)!.click();
  await flush();
  const typed = await field(mona);
  fill(typed, "a + b");
  // Tom supprime le paragraphe qui portait la formule.
  tom.editor.removeBlocks(["x"]);
  await exchange(server, [tom, mona]);
  await flush();
  await until(mona.host, () => blockWindow(WINDOW)?.querySelector('[role="status"]') != null, "la fenêtre le dit");
  expect(blockWindow(WINDOW)!.querySelector('[role="status"]')?.textContent).toBe(INLINE_GONE);
  expect(document.activeElement).toBe(typed);
  expect(typed.value).toBe("a + b");
  expect(typed.readOnly).toBe(true);
  expect(mona.writes).toEqual([]);
  press(typed, "Escape");
  await flush();
  expect(document.activeElement).toBe(mona.editor.prosemirrorView.dom);
  expect(server.getXmlFragment(FRAGMENT).toString()).not.toContain("a + b");
});

/** Mona et Tom sur une page dont le paragraphe `x` (`first`) porte des formules en ligne. */
async function duo(first: Seed[number]) {
  const seed: Seed = [first, { id: "z", type: "paragraph", content: "Après" }];
  const base = await seedState(seed);
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  return { mona, tom, server };
}

/** Le message de la fenêtre ouverte, s'il y en a un. */
const status = () => blockWindow(WINDOW)?.querySelector('[role="status"]')?.textContent ?? null;

test("the_window_of_a_formula_whose_paragraph_another_moves_stays_on_it", async () => {
  // Un déplacement supprime et recrée le paragraphe au même id (R220) : la vue de la formule part,
  // la formule reste. La fenêtre la retrouve par l'id de son bloc et son rang (relecture de C2, M1).
  const { mona, tom, server } = await duo({
    id: "x",
    type: "paragraph",
    content: ["Avec ", { type: "math", props: { latex: "a" } }, " fin"],
  });
  blockOf(mona, "x").querySelector<HTMLButtonElement>(FORMULA)!.click();
  await flush();
  const typed = await field(mona);
  fill(typed, "a + b");
  tom.editor.setTextCursorPosition("x");
  tom.editor.moveBlocksDown();
  await exchange(server, [tom, mona]);
  await flush();
  expect(mona.editor.document.map((block) => block.id)).toEqual(["z", "x"]);
  await until(mona.host, () => blockOf(mona, "x").querySelector(FORMULA) !== null, "la formule recréée");
  await flush();
  expect(status()).toBeNull();
  expect(typed.readOnly).toBe(false);
  expect(document.activeElement).toBe(typed);
  press(typed, "Enter", { ctrlKey: true });
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  await exchange(server, [mona, tom]);
  expect(storedFormulas(server)).toEqual(["a + b"]);
  expect(tom.editor.document.map((block) => block.id)).toEqual(["z", "x"]);
});

test("two_formulas_of_a_table_have_two_places", async () => {
  // Le rang d'une formule se compte dans tout son bloc, de son début jusqu'à elle : deux cellules
  // d'un tableau avaient la même place (re-relecture de C2, N1).
  const math = { type: "math", props: { latex: "x" } };
  const m = await mountEditor({
    blocks: [{ id: "x", type: "table", content: { type: "tableContent", rows: [{ cells: [["A ", math], ["B ", math]] }] } } as never],
  });
  const positions: number[] = [];
  m.editor.prosemirrorState.doc.descendants((node, pos) => {
    if (node.type.name === "math") positions.push(pos);
  });
  expect(positions.map((pos) => placeAt(m.editor, pos))).toEqual([
    { blockId: "x", rank: 0 },
    { blockId: "x", rank: 1 },
  ]);
});

for (const [opened, other] of [
  ["A", "B"],
  ["B", "A"],
] as const) {
  test(`in_a_table_moved_by_another_the_window_of_cell_${opened}_stays_on_it`, async () => {
    // Deux cellules d'un tableau portent la même formule : le rang se compte dans tout le bloc (le
    // tableau), pas dans le seul paragraphe de la cellule, sinon la fenêtre se rattachait à l'autre
    // cellule et « Terminer » y écrivait (re-relecture de C2, N1).
    const math = { type: "math", props: { latex: "x" } };
    const { mona, tom, server } = await duo({
      id: "x",
      type: "table",
      content: { type: "tableContent", rows: [{ cells: [["A ", math], ["B ", math]] }] },
    } as never);
    const buttons = blockOf(mona, "x").querySelectorAll<HTMLButtonElement>(FORMULA);
    expect(buttons).toHaveLength(2);
    buttons[opened === "A" ? 0 : 1]!.click();
    await flush();
    const typed = await field(mona);
    fill(typed, "y^2");
    tom.editor.setTextCursorPosition("x");
    tom.editor.moveBlocksDown();
    await exchange(server, [tom, mona]);
    await flush();
    expect(mona.editor.document.map((block) => block.id)).toEqual(["z", "x"]);
    await until(mona.host, () => blockOf(mona, "x").querySelectorAll(FORMULA).length === 2, "les formules recréées");
    await flush();
    press(typed, "Enter", { ctrlKey: true });
    await flush();
    expect(blockWindow(WINDOW)).toBeNull();
    await exchange(server, [mona, tom]);
    const cells = [...server.getXmlFragment(FRAGMENT).toString().matchAll(/(A|B) <math latex="([^"]*)"/g)].map(
      (match) => `${match[1]}:${match[2]}`,
    );
    expect(cells.sort()).toEqual([`${opened}:y^2`, `${other}:x`].sort());
  });
}

test("a_formula_deleted_by_another_is_not_taken_for_the_next_one", async () => {
  // Tom supprime la formule dont Mona remplit le champ : la suivante prend son rang, mais ce n'est
  // pas elle (sa formule diffère) ; la fenêtre la dit partie, et n'écrit rien.
  const { mona, tom, server } = await duo({
    id: "x",
    type: "paragraph",
    content: ["Avec ", { type: "math", props: { latex: "a" } }, " et ", { type: "math", props: { latex: "b" } }, " fin"],
  });
  blockOf(mona, "x").querySelector<HTMLButtonElement>(FORMULA)!.click();
  await flush();
  const typed = await field(mona);
  fill(typed, "a + 1");
  tom.editor.transact((tr) => {
    let at = -1;
    tr.doc.descendants((node, pos) => {
      if (at === -1 && node.type.name === "math") at = pos;
    });
    tr.delete(at, at + 1);
  });
  await exchange(server, [tom, mona]);
  await flush();
  await until(mona.host, () => status() === INLINE_GONE, "la fenêtre le dit");
  expect(typed.value).toBe("a + 1");
  expect(typed.readOnly).toBe(true);
  press(typed, "Enter", { ctrlKey: true });
  await flush();
  expect(mona.writes).toEqual([]);
  expect(storedFormulas(mona.doc)).toEqual(["b"]);
});

test("a_posed_formula_whose_view_never_comes_says_so_and_keeps_the_text", async () => {
  // Une fenêtre ouverte pour une place où aucune formule n'arrive (relecture de C2, M2) : elle le
  // dit, passe en lecture seule, et le texte tapé reste, même fermée par Échap.
  const m = await mountEditor({ blocks: [{ id: "x", type: "paragraph", content: "Du texte" }] });
  m.editor.focus();
  openInlineMath(m.editor, { pos: 3 }, "editor");
  await flush();
  const typed = await field(m);
  fill(typed, "E = mc^2");
  await flush();
  await until(m.host, () => status() === INLINE_GONE, "la fenêtre le dit");
  expect(typed.readOnly).toBe(true);
  const finish = [...blockWindow(WINDOW)!.querySelectorAll("button")].find((b) => b.textContent === "Terminer")!;
  expect(finish.getAttribute("aria-disabled")).toBe("true");
  press(typed, "Escape");
  await flush();
  expect(blockWindow(WINDOW)).toBeNull();
  openInlineMath(m.editor, { pos: 3 }, "editor");
  await flush();
  expect((await field(m)).value).toBe("E = mc^2");
  expect(m.writes).toEqual([]);
});
