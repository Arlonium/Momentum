// La fenêtre de réglage d'un bloc riche (l'icône et la couleur d'un callout, l'adresse d'un signet ;
// la formule d'une équation, Task C2) : une seule à la fois par page, tenue par **l'id du bloc**,
// jamais par l'élément affiché du bloc ni par une position dans le document.
//
// Pourquoi : un bloc affiché se recrée sans que personne ici n'y touche. Un autre qui le déplace le
// supprime et le récrit au même id (R223), un autre qui change son type recrée son élément de
// contenu, une version restaurée récrit la page. Une fenêtre rendue dans la vue du bloc partait avec
// elle : elle se fermait sous les doigts, le focus tombait, et la frappe qui suivait partait dans le
// document. Tenue ici, par id, elle est rendue hors du document (`BlockWindowHost`), qui relit le
// bloc par son id à chaque changement.
//
// **Rangée par page, pas par éditeur** (R471). Une session de synchro recréée (une baisse de droits,
// la corbeille, un schéma changé, le droit qui revient) apporte un éditeur neuf : la fenêtre ouverte
// et ce qu'on y a tapé passent à lui. La vue de l'éditeur d'une page dit à quelle page il appartient
// (`useBlockWindowPage`) ; quand plus aucun éditeur de la page n'est affiché (la page quittée), sa
// fenêtre et ses brouillons partent. Un éditeur que rien ne rattache à une page (un aperçu, un test)
// a la sienne, qui part avec lui.
//
// Les brouillons (ce qu'on tape dans une fenêtre, pas encore écrit dans le document) vivent ici,
// par id de bloc et par sorte de fenêtre, tant que la page est ouverte : fermer la fenêtre (Échap,
// un clic dehors), ou perdre le droit d'écrire, ne détruit jamais un texte tapé. Rien ne va dans le
// stockage du navigateur.
import { useCallback, useEffect, useSyncExternalStore } from "react";
import type { MementumEditor } from "../debug";

/** Un bloc de l'éditeur, tel que BlockNote le lit. */
export type EditorBlock = MementumEditor["document"][number];

/**
 * Ce que reçoit le contenu de la fenêtre d'une sorte de bloc (`BLOCK_WINDOWS`, `BlockWindowHost`).
 * `block` : le bloc, relu par son id à chaque changement du document, s'il existe encore et qu'il
 * est toujours de la sorte de la fenêtre ; sinon `null`. Rien ne s'écrit quand `block` est `null`
 * ou que l'éditeur n'est pas modifiable (`editable`) : la fenêtre reste, avec ce qu'on y a tapé.
 * `close("done")` : le réglage est fait, la fenêtre se ferme, le focus revient.
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

/**
 * La fenêtre ouverte : le bloc, par son id, sa sorte à l'ouverture (changé en autre chose, il n'est
 * plus celui de la fenêtre), et où rendre le focus à la fermeture.
 */
export type OpenWindow = { readonly blockId: string; readonly kind: string; readonly returnTo: ReturnTo };

type State = {
  open: OpenWindow | null;
  readonly drafts: Map<string, string>;
  readonly listeners: Set<() => void>;
};

const newState = (): State => ({ open: null, drafts: new Map(), listeners: new Set() });

/** La page de chaque éditeur qu'une vue y a rattaché (`useBlockWindowPage`). */
const pageOfEditor = new WeakMap<object, string>();
/** Par page : la fenêtre, les brouillons, et combien de vues d'éditeur de la page sont affichées. */
const pages = new Map<string, State & { views: number }>();
/** Pour un éditeur rattaché à aucune page : elle part avec lui. */
const loose = new WeakMap<object, State>();

function stateOf(editor: object): State {
  const pageId = pageOfEditor.get(editor);
  if (pageId !== undefined) {
    let state = pages.get(pageId);
    if (state === undefined) {
      state = { ...newState(), views: 0 };
      pages.set(pageId, state);
    }
    return state;
  }
  let state = loose.get(editor);
  if (state === undefined) {
    state = newState();
    loose.set(editor, state);
  }
  return state;
}

/**
 * Rattache `editor` à la page `pageId` (rien sans page) : sa fenêtre et ses brouillons sont ceux de
 * la page. Appelé par la vue de l'éditeur, pendant son rendu (avant ses enfants, qui lisent la
 * fenêtre), et tenu tant qu'elle est affichée ; la dernière vue de la page partie, la fenêtre et les
 * brouillons partent aussi, après la tâche en cours (une session remplacée en monte une autre dans
 * le même rendu).
 */
export function useBlockWindowPage(editor: object, pageId: string | undefined): void {
  if (pageId !== undefined) pageOfEditor.set(editor, pageId);
  useEffect(() => {
    if (pageId === undefined) return;
    const held = stateOf(editor) as State & { views: number };
    held.views += 1;
    return () => {
      held.views -= 1;
      setTimeout(() => {
        if (held.views === 0 && pages.get(pageId) === held) pages.delete(pageId);
      }, 0);
    };
  }, [editor, pageId]);
}

/**
 * L'objet propre à la page de `editor` (ou à l'éditeur seul, s'il n'est rattaché à aucune page), qui
 * vit tant qu'une vue d'éditeur de la page est affichée. Une autre fenêtre de la page y range son
 * état (le sélecteur du lien de page, `PageLink.tsx`), qui passe ainsi, comme celle d'un bloc, à
 * l'éditeur neuf d'une session recréée.
 */
export function pageScope(editor: object): object {
  return stateOf(editor);
}

function setOpen(editor: object, open: OpenWindow | null): void {
  const state = stateOf(editor);
  state.open = open;
  for (const listener of [...state.listeners]) listener();
}

/**
 * Ouvre la fenêtre du bloc `block` (et ferme celle d'un autre bloc). `returnTo` : où va le focus à
 * la fermeture ; `editor` quand on l'a ouverte depuis le texte (Ctrl+Alt+Entrée, le menu « / »),
 * pour que la frappe y reprenne.
 */
export function openBlockWindow(
  editor: object,
  block: { readonly id: string; readonly type: string },
  returnTo: ReturnTo = "anchor",
): void {
  setOpen(editor, { blockId: block.id, kind: block.type, returnTo });
}

export function closeBlockWindow(editor: object): void {
  if (stateOf(editor).open !== null) setOpen(editor, null);
}

/** Le bouton d'un bloc : ouvre sa fenêtre, ou la ferme si c'est elle qui est ouverte. */
export function toggleBlockWindow(editor: object, block: { readonly id: string; readonly type: string }): void {
  if (stateOf(editor).open?.blockId === block.id) closeBlockWindow(editor);
  else openBlockWindow(editor, block);
}

export function openWindowOf(editor: object): OpenWindow | null {
  return stateOf(editor).open;
}

function subscribe(editor: object, listener: () => void): () => void {
  const { listeners } = stateOf(editor);
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * `listener` à chaque fenêtre ouverte ou fermée, et à chaque brouillon né ou parti, dans la page de
 * `editor`, jusqu'à l'appel de la fonction rendue. Pour une vue de bloc sans React (`richView.ts`,
 * R626), qui relit alors ce qui la concerne (`openWindowOf`, `blockDraft`), comme les crochets plus
 * bas pour une vue rendue par React.
 */
export function watchBlockWindows(editor: object, listener: () => void): () => void {
  return subscribe(editor, listener);
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

/**
 * Garde (ou, avec `undefined`, oublie) le brouillon de la fenêtre `kind` du bloc `blockId`. Les vues
 * ne sont prévenues que quand un brouillon naît ou part (`useHasBlockDraft`), pas à chaque frappe.
 */
export function setBlockDraft(editor: object, blockId: string, kind: string, text: string | undefined): void {
  const state = stateOf(editor);
  const key = draftKey(blockId, kind);
  const had = state.drafts.has(key);
  if (text === undefined) state.drafts.delete(key);
  else state.drafts.set(key, text);
  if (had !== state.drafts.has(key)) for (const listener of [...state.listeners]) listener();
}

/**
 * Le bloc `blockId` a-t-il un brouillon de sa fenêtre `kind` ? Suivi par le bouton du bloc : pour un
 * lecteur qui a tapé un texte avant de perdre le droit d'écrire, le bouton reste, qui rouvre la
 * fenêtre en lecture seule, le texte à copier (R472).
 */
export function useHasBlockDraft(editor: object, blockId: string, kind: string): boolean {
  const listen = useCallback((listener: () => void) => subscribe(editor, listener), [editor]);
  return useSyncExternalStore(listen, () => blockDraft(editor, blockId, kind) !== undefined);
}
