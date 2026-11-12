// Ctrl+Alt+M (Cmd+Option+M) : commenter le bloc où l'on est. La lettre tapée fait foi, pas la
// position de la touche (`code` nomme les touches d'après un clavier QWERTY) ; sur Mac, Option
// change le caractère, et la touche se reconnaît alors à son code (comme le keymap de
// ProseMirror). Un caractère tapé avec AltGr n'est jamais pris.
import { expect, test } from "vitest";
import { isCommentShortcut, type ShortcutEvent } from "./shortcut";

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
