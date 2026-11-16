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
import { INLINE_DRAFT_NOTE, INLINE_GONE } from "./InlineMath";

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
