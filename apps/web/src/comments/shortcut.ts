// Ctrl+Alt+M (Cmd+Option+M sur Mac) : commenter le bloc où l'on est (R176).
//
// La lettre tapée fait foi (`key`), pas la position de la touche : `code` nomme les touches
// d'après un clavier QWERTY, et la touche marquée « M » d'un clavier AZERTY a le code
// `Semicolon`. Quand la combinaison ne donne pas une lettre ASCII (Option, sur Mac, tape « µ »),
// la touche se reconnaît à son code de touche, 77, comme le fait le keymap de ProseMirror
// (`w3c-keyname`). Un caractère tapé avec AltGr (Ctrl+Alt sous Windows) n'est jamais pris : sur
// un clavier allemand, AltGr+M écrit « µ ».

/** Ce que la reconnaissance lit d'un événement clavier (du DOM, ou de React). */
export type ShortcutEvent = {
  key: string;
  /** Ignoré : la position de la touche ne dit pas la lettre. */
  code?: string;
  keyCode: number;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  getModifierState(modifier: string): boolean;
};

/** Le code de touche de « M ». */
const KEY_M = 77;

export function isCommentShortcut(event: ShortcutEvent): boolean {
  if (!event.altKey || !(event.ctrlKey || event.metaKey)) return false;
  if (event.getModifierState("AltGraph")) return false;
  const typed = event.key.length === 1 ? event.key.toLowerCase() : "";
  if (typed === "m") return true;
  // Une autre lettre ASCII : c'est elle qui a été tapée.
  if (/^[a-z]$/.test(typed)) return false;
  return event.keyCode === KEY_M;
}
