// Ctrl+K (Cmd+K sur Mac) : ouvrir la recherche, de partout (lot 4, Task C1, décision L5).
//
// La lettre tapée fait foi (`key`), pas la position de la touche (`code`, nommée d'après un clavier
// QWERTY : sur un Dvorak, « K » est la touche `KeyV`). Quand la combinaison ne donne pas une lettre
// latine (une disposition cyrillique tape « л »), la touche se reconnaît à son code de touche, 75,
// comme le fait le keymap de ProseMirror (`w3c-keyname`) et comme `comments/shortcut.ts`. Ni Alt ni
// Maj : Ctrl+Alt+K et Ctrl+Maj+K sont d'autres raccourcis. Un caractère tapé avec AltGr n'est jamais
// pris (sous Windows, AltGr se signale comme Ctrl+Alt ; ailleurs, par `AltGraph` seul).
//
// Sous une fenêtre modale (« Historique », une confirmation, la connexion, la recherche elle-même),
// le reste de la page est inerte : le raccourci n'y vaut rien, et la touche n'est pas avalée.
//
// BlockNote prend Ctrl+K pour créer un lien, quand du texte est sélectionné dans l'éditeur
// modifiable. Son écouteur et le nôtre reçoivent tous deux la touche, dans un ordre qu'on ne
// suppose pas : c'est la sélection de l'éditeur qui décide (`leftToEditor`). Sélection non vide
// dans l'éditeur modifiable où la touche est tapée : elle est à lui, on ne l'ouvre ni ne l'annule.
// Partout ailleurs (un curseur sans sélection, un éditeur en lecture seule, un champ, la barre
// latérale), la recherche s'ouvre, et la touche est prise : le navigateur en ferait autre chose
// (sa propre barre de recherche).
import { useEffect } from "react";

/** Ce que la reconnaissance lit d'un événement clavier (du DOM, ou de React). */
export type SearchKeyEvent = {
  key: string;
  keyCode: number;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  getModifierState(modifier: string): boolean;
};

/** La touche du raccourci, telle que le clavier la nomme : « ⌘ » sur un Mac, « Ctrl » ailleurs. */
export const MOD = /Mac|iPhone|iPad/.test(globalThis.navigator?.platform ?? "") ? "⌘" : "Ctrl";

/**
 * La longueur du champ de recherche, au plus : la borne de la route (200 caractères, L16). Le champ
 * compte en unités UTF-16, la route en caractères : il n'en laisse jamais taper plus qu'elle n'en
 * prend.
 */
export const MAX_QUERY_LENGTH = 200;

/** Le code de touche de « K ». */
const KEY_K = 75;

/** La touche est-elle Ctrl+K (ou Cmd+K) ? Sans rien dire de l'endroit où elle est tapée. */
export function isSearchKey(event: SearchKeyEvent): boolean {
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return false;
  if (event.getModifierState("AltGraph")) return false;
  const typed = event.key.length === 1 ? event.key.toLowerCase() : "";
  if (typed === "k") return true;
  // Une autre lettre latine : c'est elle qui a été tapée.
  if (/^[a-z]$/.test(typed)) return false;
  return event.keyCode === KEY_K;
}

/** Une fenêtre modale est ouverte : le reste de la page est inerte. */
function underModal(): boolean {
  return globalThis.document?.querySelector("dialog[open]") != null;
}

/** Ctrl+K, hors de toute fenêtre ouverte. */
export function isSearchShortcut(event: SearchKeyEvent): boolean {
  return isSearchKey(event) && !underModal();
}

/** L'éditeur de BlockNote qui contient `node`, s'il est modifiable. */
function editableEditor(node: Node | null): Element | null {
  const element = node instanceof Element ? node : (node?.parentElement ?? null);
  const editor = element?.closest(".bn-editor") ?? null;
  return editor?.getAttribute("contenteditable") === "true" ? editor : null;
}

/**
 * Ctrl+K tapé sur `target` est-il à l'éditeur ? Oui quand `selection` n'est pas vide et tient dans
 * l'éditeur modifiable où la touche est tapée : BlockNote y ouvre la création d'un lien.
 */
export function leftToEditor(target: EventTarget | null, selection: Selection | null): boolean {
  if (selection === null || selection.rangeCount === 0 || selection.isCollapsed) return false;
  const editor = editableEditor(target instanceof Node ? target : null);
  if (editor === null) return false;
  return editor.contains(selection.anchorNode) && editor.contains(selection.focusNode);
}

/**
 * Écoute Ctrl+K sur la fenêtre tant que `open` est donné (`null` : rien à ouvrir, la touche reste
 * au navigateur). `open` peut changer d'identité d'un rendu à l'autre.
 */
export function useSearchShortcut(open: (() => void) | null): void {
  useEffect(() => {
    if (open === null) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isSearchShortcut(event)) return;
      if (leftToEditor(event.target, document.getSelection())) return;
      event.preventDefault();
      open();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);
}
