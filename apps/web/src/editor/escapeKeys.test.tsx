// Échap dans le texte d'une page (question 37 de la recette du lot 4, choix (a)) : sans menu ouvert, il
// ne fait rien, le focus reste au texte ; sur du texte sélectionné, la barre de mise en forme
// montrée, il réduit la sélection à sa tête, sans perdre le focus. Rien n'est écrit dans le document.
// La preuve dans un navigateur : `e2e/search-rights.spec.ts`, `escape_in_the_text_of_the_editor_…`.
import { FormattingToolbarExtension } from "@blocknote/core/extensions";
import { TextSelection } from "prosemirror-state";
import { act } from "react";
import { expect, test } from "vitest";
import { stored, type Seed } from "../test/editors";
import { mountEditor, press } from "../test/richEditor";

const PAGE: Seed = [{ id: "p", type: "paragraph", content: "Début de page" }];

test("escape_in_the_text_keeps_the_focus_and_writes_nothing", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const text = m.editor.prosemirrorView.dom as HTMLElement;
  act(() => {
    m.editor.setTextCursorPosition("p", "end");
    m.editor.focus();
  });
  expect(document.activeElement).toBe(text);
  const page = stored(m.doc);
  let event!: KeyboardEvent;
  act(() => {
    event = press(text, "Escape");
  });
  // Rien ne la prend : ni le focus retiré, ni la touche retenue (une fenêtre autour se fermerait).
  expect(document.activeElement).toBe(text);
  expect(event.defaultPrevented).toBe(false);
  expect(m.editor.getTextCursorPosition().block.id).toBe("p");
  expect(stored(m.doc)).toEqual(page);
  expect(m.writes).toEqual([]);
  expect(m.errors).toEqual([]);
});

test("escape_on_a_selection_shrinks_it_to_its_head_and_keeps_the_focus", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const view = m.editor.prosemirrorView;
  const text = view.dom as HTMLElement;
  // « de » sélectionné, de gauche à droite ; la barre de mise en forme montrée.
  let from = 0;
  view.state.doc.descendants((node, pos) => {
    if (node.isText && from === 0) from = pos + "Début ".length;
  });
  act(() => {
    m.editor.focus();
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, from, from + 2)));
    m.editor.getExtension(FormattingToolbarExtension)!.store.setState(true);
  });
  expect(view.state.doc.textBetween(view.state.selection.from, view.state.selection.to)).toBe("de");
  let event!: KeyboardEvent;
  act(() => {
    event = press(text, "Escape");
  });
  expect(event.defaultPrevented).toBe(true);
  expect(view.state.selection.empty).toBe(true);
  expect(view.state.selection.head).toBe(from + 2);
  expect(document.activeElement).toBe(text);
  expect(m.writes).toEqual([]);
});
