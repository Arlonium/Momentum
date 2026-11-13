// Ctrl+Alt+M (Cmd+Option+M) : commenter le bloc où l'on est. La lettre tapée fait foi, pas la
// position de la touche (`code` nomme les touches d'après un clavier QWERTY) ; sur Mac, Option
// change le caractère, et la touche se reconnaît alors à son code (comme le keymap de
// ProseMirror). Un caractère tapé avec AltGr n'est jamais pris.
import { expect, test } from "vitest";
import { commentedBlock, isCommentShortcut, type ShortcutEvent } from "./shortcut";

/** Un événement clavier construit : la touche, ses modificateurs, et AltGr s'il est tenu. */
function key(init: Partial<Omit<ShortcutEvent, "getModifierState">> & { altGraph?: boolean }): ShortcutEvent {
  const { altGraph = false, ...rest } = init;
  return {
    key: "",
    keyCode: 0,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    ...rest,
    getModifierState: (modifier: string) => modifier === "AltGraph" && altGraph,
  };
}

test("azerty_m_is_the_letter_m_whatever_its_position", () => {
  // La touche marquée « M » d'un clavier AZERTY : `code` vaut `Semicolon`.
  expect(isCommentShortcut(key({ key: "m", code: "Semicolon", keyCode: 77, ctrlKey: true, altKey: true }))).toBe(true);
  // La touche à la place du « M » d'un QWERTY, qui tape « , » sur un AZERTY.
  expect(isCommentShortcut(key({ key: ",", code: "KeyM", keyCode: 188, ctrlKey: true, altKey: true }))).toBe(false);
});

test("qwerty_ctrl_alt_m", () => {
  expect(isCommentShortcut(key({ key: "m", code: "KeyM", keyCode: 77, ctrlKey: true, altKey: true }))).toBe(true);
  expect(isCommentShortcut(key({ key: "M", code: "KeyM", keyCode: 77, ctrlKey: true, altKey: true }))).toBe(true);
});

test("mac_cmd_option_m_gives_another_character", () => {
  // Option change le caractère (« µ ») : la touche se reconnaît à son code.
  expect(isCommentShortcut(key({ key: "µ", code: "KeyM", keyCode: 77, metaKey: true, altKey: true }))).toBe(true);
  // Une autre touche qui donne un autre caractère n'est pas prise.
  expect(isCommentShortcut(key({ key: "ﬁ", code: "KeyO", keyCode: 79, metaKey: true, altKey: true }))).toBe(false);
});

test("altgr_m_types_a_character_and_is_left_alone", () => {
  // Clavier allemand : AltGr+M écrit « µ », et AltGr se signale comme Ctrl+Alt sous Windows.
  expect(
    isCommentShortcut(key({ key: "µ", code: "KeyM", keyCode: 77, ctrlKey: true, altKey: true, altGraph: true })),
  ).toBe(false);
  expect(
    isCommentShortcut(key({ key: "m", code: "KeyM", keyCode: 77, ctrlKey: true, altKey: true, altGraph: true })),
  ).toBe(false);
});

test("ctrl_m_or_alt_m_alone_is_not_the_shortcut", () => {
  expect(isCommentShortcut(key({ key: "m", keyCode: 77, ctrlKey: true }))).toBe(false);
  expect(isCommentShortcut(key({ key: "m", keyCode: 77, metaKey: true }))).toBe(false);
  expect(isCommentShortcut(key({ key: "m", keyCode: 77, altKey: true }))).toBe(false);
  expect(isCommentShortcut(key({ key: "m", keyCode: 77 }))).toBe(false);
});

test("another_letter_with_the_code_of_m_is_not_taken", () => {
  // Une disposition où la touche de code 77 tape une autre lettre : la lettre fait foi.
  expect(isCommentShortcut(key({ key: "q", keyCode: 77, ctrlKey: true, altKey: true }))).toBe(false);
});

// Dans un éditeur en lecture seule, le raccourci vise le bloc de la sélection du document ; tapé
// dans un champ de saisie hors de l'éditeur (le panneau, la barre latérale), il ne fait rien et
// n'avale pas la touche, même si la sélection du document est restée dans l'éditeur (Firefox la
// garde quand on passe dans un champ par Tab).

function editorScene() {
  const editor = document.createElement("div");
  editor.className = "bn-editor";
  const outer = document.createElement("div");
  outer.setAttribute("data-node-type", "blockOuter");
  outer.setAttribute("data-id", "bloc");
  const content = document.createElement("p");
  const words = document.createTextNode("du texte");
  content.append(words);
  outer.append(content);
  editor.append(outer);
  const field = document.createElement("textarea");
  const input = document.createElement("input");
  const editable = document.createElement("div");
  editable.setAttribute("contenteditable", "true");
  document.body.append(editor, field, input, editable);
  const range = document.createRange();
  range.setStart(words, 1);
  range.setEnd(words, 4);
  const selection = document.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);
  return { editor, content, field, input, editable, selection };
}

const shortcut = (target: EventTarget | null) => ({
  ...key({ key: "m", keyCode: 77, ctrlKey: true, altKey: true }),
  target,
});

test("the_shortcut_names_the_block_of_the_selection", () => {
  const { editor, content, selection } = editorScene();
  expect(commentedBlock(shortcut(document.body), editor, selection)).toBe("bloc");
  expect(commentedBlock(shortcut(content), editor, selection)).toBe("bloc");
  // Une autre touche : rien.
  expect(
    commentedBlock({ ...key({ key: "k", keyCode: 75, ctrlKey: true, altKey: true }), target: document.body }, editor, selection),
  ).toBeNull();
  document.body.replaceChildren();
});

test("the_shortcut_is_silent_in_a_field_outside_the_editor", () => {
  const { editor, field, input, editable, selection } = editorScene();
  for (const target of [field, input, editable]) {
    expect(commentedBlock(shortcut(target), editor, selection)).toBeNull();
  }
  document.body.replaceChildren();
});

// Sous une fenêtre modale (« Historique », une confirmation, la connexion), le reste de la page
// est inerte : le raccourci n'y fait rien et n'avale pas la touche, dans l'éditeur en lecture
// seule (la sélection du document peut y être restée) comme dans l'éditeur modifiable.

test("the_shortcut_is_silent_under_a_modal_window", () => {
  const { editor, content, selection } = editorScene();
  const modal = document.createElement("dialog");
  modal.setAttribute("open", "");
  document.body.append(modal);
  const event = key({ key: "m", keyCode: 77, ctrlKey: true, altKey: true });
  // L'éditeur modifiable ne lit que la reconnaissance ; l'éditeur en lecture seule, le bloc visé.
  expect(isCommentShortcut(event)).toBe(false);
  expect(commentedBlock(shortcut(content), editor, selection)).toBeNull();
  expect(commentedBlock(shortcut(document.body), editor, selection)).toBeNull();
  // Fermée, la fenêtre ne compte plus.
  modal.removeAttribute("open");
  expect(isCommentShortcut(event)).toBe(true);
  expect(commentedBlock(shortcut(content), editor, selection)).toBe("bloc");
  document.body.replaceChildren();
});
