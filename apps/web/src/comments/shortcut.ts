// Ctrl+Alt+M (Cmd+Option+M sur Mac) : commenter le bloc où l'on est (R176).
//
// La lettre tapée fait foi (`key`), pas la position de la touche : `code` nomme les touches
// d'après un clavier QWERTY, et la touche marquée « M » d'un clavier AZERTY a le code
// `Semicolon`. Quand la combinaison ne donne pas une lettre ASCII (Option, sur Mac, tape « µ »),
// la touche se reconnaît à son code de touche, 77, comme le fait le keymap de ProseMirror
// (`w3c-keyname`). Un caractère tapé avec AltGr (Ctrl+Alt sous Windows) n'est jamais pris : sur
// un clavier allemand, AltGr+M écrit « µ ».
//
// Sous une fenêtre modale (« Historique », une confirmation, la connexion), le reste de la page
// est inerte : le raccourci n'y vaut rien, et la touche n'est pas avalée. C'est dit ici, dans la
// reconnaissance, pour les deux éditeurs (le modifiable, `Editor.tsx`, et celui en lecture
// seule, `BlockMarks.tsx`). Toutes les fenêtres de l'application s'ouvrent par `showModal` : une
// `dialog` ouverte est une fenêtre modale.
import { blockOfSelection } from "./dom";

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

/** Une fenêtre modale est ouverte : le reste de la page est inerte. */
function underModal(): boolean {
  return globalThis.document?.querySelector("dialog[open]") != null;
}

export function isCommentShortcut(event: ShortcutEvent): boolean {
  if (!event.altKey || !(event.ctrlKey || event.metaKey)) return false;
  if (underModal()) return false;
  if (event.getModifierState("AltGraph")) return false;
  const typed = event.key.length === 1 ? event.key.toLowerCase() : "";
  if (typed === "m") return true;
  // Une autre lettre ASCII : c'est elle qui a été tapée.
  if (/^[a-z]$/.test(typed)) return false;
  return event.keyCode === KEY_M;
}

/** Un champ où l'on tape : le raccourci n'y est pas pris. */
const TYPING = 'input, textarea, select, [contenteditable]:not([contenteditable="false"])';

/**
 * Le bloc que vise Ctrl+Alt+M dans un éditeur en lecture seule : celui qui contient la sélection
 * du document (`blockOfSelection`). `null` si l'événement n'est pas le raccourci, ou s'il est tapé
 * dans un champ de saisie hors de l'éditeur (le panneau, la barre latérale) : la touche y garde
 * son sens, même quand la sélection du document est restée dans l'éditeur.
 */
export function commentedBlock(
  event: ShortcutEvent & { target: EventTarget | null },
  editor: Element,
  selection: Selection | null,
): string | null {
  if (!isCommentShortcut(event)) return null;
  const { target } = event;
  if (target instanceof Element && !editor.contains(target) && target.closest(TYPING) !== null) {
    return null;
  }
  return blockOfSelection(editor, selection);
}
