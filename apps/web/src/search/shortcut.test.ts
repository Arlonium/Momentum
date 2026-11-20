// Ctrl+K (Cmd+K sur Mac) ouvre la recherche (lot 4, Task C1, décision L5). La lettre tapée fait
// foi, jamais la position de la touche ; un caractère tapé avec AltGr n'est jamais pris ; sous une
// fenêtre ouverte, le raccourci se tait sans avaler la touche ; dans un éditeur modifiable dont la
// sélection n'est pas vide, il est laissé à l'éditeur (il y crée un lien).
import { afterEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { renderHook } from "../test/hooks";
import { isSearchKey, isSearchShortcut, leftToEditor, useSearchShortcut } from "./shortcut";

type Keys = {
  key: string;
  code?: string;
  keyCode?: number;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  altGraph?: boolean;
};

const key = ({ altGraph = false, ...keys }: Keys) => ({
  keyCode: 0,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...keys,
  getModifierState: (modifier: string) => modifier === "AltGraph" && altGraph,
});

afterEach(() => {
  document.body.replaceChildren();
  document.getSelection()?.removeAllRanges();
});

test("the_shortcut_is_read_from_the_key_typed", () => {
  expect(isSearchKey(key({ key: "k", code: "KeyK", keyCode: 75, ctrlKey: true }))).toBe(true);
  expect(isSearchKey(key({ key: "K", code: "KeyK", keyCode: 75, ctrlKey: true }))).toBe(true);
  expect(isSearchKey(key({ key: "k", code: "KeyK", keyCode: 75, metaKey: true }))).toBe(true);
  // Une disposition où « K » n'est pas à la place du « K » d'un QWERTY (Dvorak : la touche `KeyV`).
  expect(isSearchKey(key({ key: "k", code: "KeyV", keyCode: 75, ctrlKey: true }))).toBe(true);
  // Et la touche à la place du « K » d'un QWERTY, qui y tape « t » : ce n'est pas le raccourci.
  expect(isSearchKey(key({ key: "t", code: "KeyK", keyCode: 84, ctrlKey: true }))).toBe(false);
  // AZERTY : « K » est à la même place, mais la touche `Semicolon` y tape « m ».
  expect(isSearchKey(key({ key: "m", code: "Semicolon", keyCode: 77, ctrlKey: true }))).toBe(false);
  // Une autre lettre sous le code de touche de « K » : la lettre fait foi.
  expect(isSearchKey(key({ key: "q", keyCode: 75, ctrlKey: true }))).toBe(false);
  // Une disposition sans lettre latine (cyrillique : « л ») : la touche se reconnaît à son code.
  expect(isSearchKey(key({ key: "л", code: "KeyK", keyCode: 75, ctrlKey: true }))).toBe(true);
  expect(isSearchKey(key({ key: "д", code: "KeyL", keyCode: 76, ctrlKey: true }))).toBe(false);
});

test("k_alone_or_with_other_modifiers_is_not_the_shortcut", () => {
  expect(isSearchKey(key({ key: "k", keyCode: 75 }))).toBe(false);
  expect(isSearchKey(key({ key: "k", keyCode: 75, altKey: true }))).toBe(false);
  expect(isSearchKey(key({ key: "k", keyCode: 75, shiftKey: true }))).toBe(false);
  // Ctrl+Maj+K et Ctrl+Alt+K sont d'autres raccourcis (du navigateur, ou de personne).
  expect(isSearchKey(key({ key: "K", keyCode: 75, ctrlKey: true, shiftKey: true }))).toBe(false);
  expect(isSearchKey(key({ key: "k", keyCode: 75, ctrlKey: true, altKey: true }))).toBe(false);
});

test("a_character_typed_with_altgr_is_never_taken", () => {
  // Sous Windows, AltGr se signale comme Ctrl+Alt ; ailleurs, sans modificateur.
  expect(isSearchKey(key({ key: "k", keyCode: 75, ctrlKey: true, altKey: true, altGraph: true }))).toBe(false);
  expect(isSearchKey(key({ key: "ĸ", keyCode: 75, ctrlKey: true, altGraph: true }))).toBe(false);
  expect(isSearchKey(key({ key: "k", keyCode: 75, ctrlKey: true, altGraph: true }))).toBe(false);
});

test("the_shortcut_is_silent_under_an_open_dialog", () => {
  const event = key({ key: "k", keyCode: 75, ctrlKey: true });
  expect(isSearchShortcut(event)).toBe(true);
  const dialog = document.createElement("dialog");
  document.body.append(dialog);
  // Témoin : une fenêtre fermée ne retient rien.
  expect(isSearchShortcut(event)).toBe(true);
  dialog.setAttribute("open", "");
  expect(isSearchShortcut(event)).toBe(false);
  // La touche elle-même reste reconnue : la fenêtre de recherche, ouverte, s'en sert.
  expect(isSearchKey(event)).toBe(true);
});

/** Un éditeur de BlockNote, tel que le DOM le montre : `.bn-editor`, modifiable ou non. */
function editor(editable: boolean): { host: HTMLElement; text: Text } {
  const host = document.createElement("div");
  host.className = "bn-editor ProseMirror";
  host.setAttribute("contenteditable", String(editable));
  const paragraph = document.createElement("p");
  const text = document.createTextNode("le chat dort");
  paragraph.append(text);
  host.append(paragraph);
  document.body.append(host);
  return { host, text };
}

function select(text: Text, from: number, to: number): Selection {
  const selection = document.getSelection()!;
  selection.setBaseAndExtent(text, from, text, to);
  return selection;
}

test("with_a_selection_in_an_editable_editor_the_key_is_left_to_the_editor", () => {
  const { host, text } = editor(true);
  expect(leftToEditor(host, select(text, 3, 7))).toBe(true);
  // Sans sélection (un curseur), l'éditeur n'en fait rien : la recherche s'ouvre.
  expect(leftToEditor(host, select(text, 3, 3))).toBe(false);
  expect(leftToEditor(host, null)).toBe(false);
});

test("a_selection_is_left_to_nobody_outside_an_editable_editor", () => {
  // Un éditeur en lecture seule ne crée pas de lien.
  const reader = editor(false);
  expect(leftToEditor(reader.host, select(reader.text, 3, 7))).toBe(false);
  // La touche tapée ailleurs (un champ, la barre latérale), la sélection restée dans l'éditeur.
  const writer = editor(true);
  const field = document.createElement("input");
  document.body.append(field);
  expect(leftToEditor(field, select(writer.text, 3, 7))).toBe(false);
  // Une sélection hors de tout éditeur.
  const plain = document.createElement("p");
  const words = document.createTextNode("du texte de la page");
  plain.append(words);
  document.body.append(plain);
  expect(leftToEditor(writer.host, select(words, 0, 4))).toBe(false);
});

/** Tape Ctrl+K sur `target` ; rend l'événement, pour lire s'il a été annulé. */
function press(target: EventTarget, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key: "k",
    ctrlKey: true,
    bubbles: true,
    cancelable: true,
    ...init,
  });
  target.dispatchEvent(event);
  return event;
}

test("the_shortcut_opens_the_search_from_anywhere_and_takes_the_key", async () => {
  const open = vi.fn();
  await renderHook(useSearchShortcut, open);
  expect(press(document.body).defaultPrevented).toBe(true);
  expect(open).toHaveBeenCalledTimes(1);
  // Dans un champ : Ctrl+K n'y tape rien, la recherche s'ouvre aussi.
  const field = document.createElement("input");
  document.body.append(field);
  expect(press(field).defaultPrevented).toBe(true);
  expect(open).toHaveBeenCalledTimes(2);
  // Dans un éditeur modifiable sans sélection.
  const { host, text } = editor(true);
  select(text, 2, 2);
  expect(press(host).defaultPrevented).toBe(true);
  expect(open).toHaveBeenCalledTimes(3);
  // Une autre touche, ou « k » seul : rien, et la touche n'est pas avalée.
  expect(press(document.body, { key: "j" }).defaultPrevented).toBe(false);
  expect(press(document.body, { ctrlKey: false }).defaultPrevented).toBe(false);
  expect(open).toHaveBeenCalledTimes(3);
});

test("the_shortcut_neither_opens_nor_takes_the_key_where_it_is_not_its_own", async () => {
  const open = vi.fn();
  const hook = await renderHook(useSearchShortcut, open);
  // Du texte sélectionné dans l'éditeur modifiable : la touche est à lui.
  const { host, text } = editor(true);
  select(text, 3, 7);
  expect(press(host).defaultPrevented).toBe(false);
  // Sous une fenêtre ouverte.
  document.getSelection()!.removeAllRanges();
  const dialog = document.createElement("dialog");
  dialog.setAttribute("open", "");
  document.body.append(dialog);
  expect(press(dialog).defaultPrevented).toBe(false);
  dialog.remove();
  // Avec AltGr.
  expect(press(document.body, { modifierAltGraph: true }).defaultPrevented).toBe(false);
  expect(open).not.toHaveBeenCalled();
  // Sans rien à ouvrir (aucun espace), la touche reste au navigateur ; démonté, plus rien n'écoute.
  await hook.rerender(null);
  expect(press(document.body).defaultPrevented).toBe(false);
  await hook.rerender(open);
  await hook.unmount();
  expect(press(document.body).defaultPrevented).toBe(false);
  expect(open).not.toHaveBeenCalled();
});
