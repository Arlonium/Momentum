// La fenêtre de réglage d'un bloc riche (l'icône et la couleur d'un callout, l'adresse d'un signet ;
// la formule d'une équation, Task C2) : une seule à la fois par éditeur, tenue par **l'id du bloc**,
// jamais par l'élément affiché du bloc ni par une position dans le document.
//
// Pourquoi : un bloc affiché se recrée sans que personne ici n'y touche. Un autre qui le déplace le
// supprime et le récrit au même id (R223), un autre qui change son type recrée son élément de
// contenu, une version restaurée récrit la page. Une fenêtre rendue dans la vue du bloc partait avec
// elle : elle se fermait sous les doigts, le focus tombait, et la frappe qui suivait partait dans le
// document. Tenue ici, par id, elle est rendue hors du document (`BlockWindowHost`), qui relit le
// bloc par son id à chaque changement.
//
// Les brouillons (ce qu'on tape dans une fenêtre, pas encore écrit dans le document) vivent ici
// aussi, par éditeur, par id de bloc et par sorte de fenêtre, tant que l'éditeur vit, c'est-à-dire
// tant que la page est ouverte : fermer la fenêtre (Échap, un clic dehors) ne détruit jamais un
// texte tapé. Rien ne va dans le stockage du navigateur.
import { useCallback, useSyncExternalStore } from "react";
import type { MementumEditor } from "../debug";

/** Un bloc de l'éditeur, tel que BlockNote le lit. */
export type EditorBlock = MementumEditor["document"][number];

/**
 * Ce que reçoit le contenu de la fenêtre d'une sorte de bloc (`BLOCK_WINDOWS`, `BlockWindowHost`).
 * `block` : le bloc, relu par son id à chaque changement du document, s'il existe encore et qu'il
 * est toujours de la sorte de la fenêtre ; sinon `null`, et rien ne s'écrit plus (la fenêtre reste,
 * avec ce qu'on y a tapé). `close("done")` : le réglage est écrit, le focus revient.
 */
export type BlockWindowProps = {
  editor: MementumEditor;
  blockId: string;
  block: EditorBlock | null;
  editable: boolean;
  close: (how: "done") => void;
};

/** Où rendre le focus quand la fenêtre se ferme : au bouton du bloc, ou à l'éditeur. */
export type ReturnTo = "anchor" | "editor";

/** La fenêtre ouverte : le bloc, par son id, et où rendre le focus à la fermeture. */
export type OpenWindow = { readonly blockId: string; readonly returnTo: ReturnTo };

type State = {
  open: OpenWindow | null;
  readonly drafts: Map<string, string>;
  readonly listeners: Set<() => void>;
};

/** Par éditeur : un éditeur parti (page fermée) emporte sa fenêtre et ses brouillons. */
const states = new WeakMap<object, State>();

function stateOf(editor: object): State {
  let state = states.get(editor);
  if (state === undefined) {
    state = { open: null, drafts: new Map(), listeners: new Set() };
    states.set(editor, state);
  }
  return state;
}

function setOpen(editor: object, open: OpenWindow | null): void {
  const state = stateOf(editor);
  state.open = open;
  for (const listener of [...state.listeners]) listener();
}

/**
 * Ouvre la fenêtre du bloc `blockId` (et ferme celle d'un autre bloc). `returnTo` : où va le focus à
 * la fermeture ; `editor` quand on l'a ouverte depuis le texte (Ctrl+Alt+Entrée), pour y revenir.
 */
export function openBlockWindow(editor: object, blockId: string, returnTo: ReturnTo = "anchor"): void {
  setOpen(editor, { blockId, returnTo });
}

export function closeBlockWindow(editor: object): void {
  if (stateOf(editor).open !== null) setOpen(editor, null);
}

/** Le bouton d'un bloc : ouvre sa fenêtre, ou la ferme si c'est elle qui est ouverte. */
export function toggleBlockWindow(editor: object, blockId: string): void {
  if (stateOf(editor).open?.blockId === blockId) closeBlockWindow(editor);
  else openBlockWindow(editor, blockId);
}

export function openWindowOf(editor: object): OpenWindow | null {
  return stateOf(editor).open;
}

function subscribe(editor: object, listener: () => void): () => void {
  const { listeners } = stateOf(editor);
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** La fenêtre ouverte de l'éditeur, suivie (pour `BlockWindowHost`). */
export function useOpenBlockWindow(editor: object): OpenWindow | null {
  const listen = useCallback((listener: () => void) => subscribe(editor, listener), [editor]);
  return useSyncExternalStore(listen, () => openWindowOf(editor));
}

/**
 * La fenêtre du bloc `blockId` est-elle ouverte ? Suivi par le bouton du bloc (`aria-expanded`) :
 * ne rend le bloc de nouveau que quand la réponse change, pas à chaque fenêtre ouverte ailleurs.
 */
export function useBlockWindow(editor: object, blockId: string): boolean {
  const listen = useCallback((listener: () => void) => subscribe(editor, listener), [editor]);
  return useSyncExternalStore(listen, () => openWindowOf(editor)?.blockId === blockId);
}

const draftKey = (blockId: string, kind: string) => `${kind}\u0000${blockId}`;

/** Le brouillon de la fenêtre `kind` du bloc `blockId`, s'il y en a un. */
export function blockDraft(editor: object, blockId: string, kind: string): string | undefined {
  return stateOf(editor).drafts.get(draftKey(blockId, kind));
}

/** Garde (ou, avec `undefined`, oublie) le brouillon de la fenêtre `kind` du bloc `blockId`. */
export function setBlockDraft(editor: object, blockId: string, kind: string, text: string | undefined): void {
  const { drafts } = stateOf(editor);
  if (text === undefined) drafts.delete(draftKey(blockId, kind));
  else drafts.set(draftKey(blockId, kind), text);
}
