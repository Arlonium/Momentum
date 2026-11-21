// Aller à un bloc de la page affichée : le curseur au début de son texte, le focus à l'éditeur, le
// bloc amené à l'écran. Pour un lien de la table des matières (`blocks/Toc.tsx`, d'où vient ce code)
// et pour une adresse qui vise un bloc (`/p/<page>#b=<bloc>`, lot 4 : un résultat de recherche, un
// rétrolien ; `useArrival`). C'est une navigation : rien n'est écrit dans le document, et la même
// chose vaut pour qui lit et pour qui modifie. Les replis fermés qui contiennent le bloc s'ouvrent,
// et le bloc atteint est surligné un instant (recette du lot 4, C5 et correction 11).
//
// L'identifiant d'un bloc vient d'ailleurs (une adresse, un serveur, un autre client) : un texte
// quelconque. Il n'est comparé qu'à l'attribut `data-id` des blocs affichés (`blockElement`, qui
// l'échappe), et un bloc introuvable ne fait rien : ni erreur, ni message, ni défilement.
import { useCallback, useEffect, useRef, useSyncExternalStore } from "react";
import { blockElement, OWN_CONTENT } from "../comments/dom";
import { lightBlock } from "./arrival";
import { focusEditor } from "./blocks/BlockWindowHost";
import type { MementumEditor } from "./debug";

type How = {
  /** Où le bloc est amené dans la fenêtre (`scrollIntoView`) : au milieu par défaut. */
  block?: ScrollLogicalPosition;
  /** Donner le focus à l'éditeur (par défaut) ; sinon le bloc est seulement montré. */
  focus?: boolean;
};

/**
 * Ouvre les replis fermés qui contiennent le bloc `element` (C5 de la recette du lot 4) : la frappe
 * n'irait jamais dans un texte caché. BlockNote 0.57 tient l'état d'un repli dans le navigateur
 * (`localStorage`, `toggle-<id>`) et dans `aria-expanded` de son bouton, jamais dans le document
 * (`createToggleFrame`) : ouvert ici par son bouton, par son propre gestionnaire, rien n'est écrit, et
 * le repli reste ouvert comme si la personne l'avait ouvert. Le repli du bloc lui-même n'est pas
 * touché : son texte est montré, ses enfants restent comme ils étaient. À relire à chaque montée de
 * BlockNote (`.bn-toggle-frame`, `.bn-toggle-button`, `aria-expanded`).
 */
function openFolds(root: Element, element: Element): void {
  for (
    let frame = element.parentElement?.closest(".bn-toggle-frame") ?? null;
    frame !== null && root.contains(frame);
    frame = frame.parentElement?.closest(".bn-toggle-frame") ?? null
  ) {
    const button = frame.querySelector<HTMLElement>(":scope > .bn-toggle-button");
    if (button?.getAttribute("aria-expanded") === "false") button.click();
  }
}

/**
 * Va au bloc `id` de `editor`. Rend faux, sans rien faire, si la vue n'est pas montée ou si le bloc
 * n'y est pas (parti, d'une autre page, jamais écrit). Un bloc dans des replis fermés les ouvre
 * (`openFolds`) ; le bloc atteint est surligné un instant (`arrival.ts`).
 */
export function goToBlock(editor: MementumEditor, id: string, { block = "center", focus = true }: How = {}): boolean {
  const root = editor.domElement;
  const element = root?.isConnected ? blockElement(root, id) : null;
  // Une seule recherche du bloc dans la page (R641), et seulement s'il est affiché.
  if (element === null || editor.getBlock(id) === undefined) return false;
  openFolds(root!, element);
  try {
    editor.setTextCursorPosition(id, "start");
  } catch {
    // Un bloc qu'on ne sait pas désigner (sans texte) : le curseur reste où il était.
  }
  if (focus) {
    focusEditor(editor);
    // En lecture seule, ProseMirror ne pose pas sa sélection dans la page, et relit celle du document
    // quand l'éditeur prend le focus : on la pose au début du bloc, où un lecteur d'écran reprend.
    if (!editor.isEditable) {
      const text = element.querySelector(":scope > .bn-block > .bn-block-content .bn-inline-content");
      if (text !== null) document.getSelection()?.collapse(text, 0);
    }
  }
  // Le contenu propre du bloc, pas le bloc entier, qui porte aussi ses sous-blocs : centré sur son
  // sous-arbre, un bloc dont les sous-blocs dépassent la fenêtre n'y aurait pas son texte.
  (element.querySelector(OWN_CONTENT) ?? element).scrollIntoView({ block });
  lightBlock(editor, id);
  return true;
}

/** Un bloc visé : suivi une fois par objet (`BlockTarget` du routeur en est un). */
export type Arrival = { readonly blockId: string };

/** Ce que l'arrivée lit de la session de synchro de la page (`CollabSession`). */
export type Synced = { readonly synced: boolean; subscribe(listener: () => void): () => void };

/**
 * La personne écrit ailleurs que dans cet éditeur (un champ, un autre texte modifiable), ou une
 * fenêtre est ouverte par-dessus la page : une arrivée tardive ne lui prend pas le focus.
 */
function busyElsewhere(editor: MementumEditor): boolean {
  if (document.querySelector("dialog[open]") !== null) return true;
  const active = document.activeElement;
  if (active === null || active === document.body || editor.domElement?.contains(active)) return false;
  return (
    active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement ||
    active instanceof HTMLSelectElement ||
    // Un texte modifiable, ou un menu ouvert (celui d'une ligne de la barre latérale est posé dans
    // `body`, marqué `data-sidebar-menu`) : le focus parti, Échap et les flèches n'y iraient plus.
    active.closest('[contenteditable="true"], [role="menu"], [data-sidebar-menu]') !== null
  );
}

/**
 * Suit la visée `target` dans `editor` : va à son bloc dès que la session de la page a fait sa
 * première synchro (jamais sur un document encore vide) et que la vue de l'éditeur est montée
 * (y-prosemirror y rend le document en la créant), puis le dit par `onDone`, bloc trouvé ou non. Qui
 * donne la visée la retire alors : une session recréée, donc un éditeur neuf, ne ramène pas au bloc.
 *
 * Donnée à un éditeur déjà monté et prêt, la visée est suivie dans le rendu du geste qui l'a posée,
 * et prend le focus (la fenêtre de recherche vient de le rendre à ce qui l'avait). Suivie plus tard
 * (la page s'ouvrait : la synchro, la vue ou le code de l'éditeur manquaient), elle montre le bloc,
 * et ne prend pas le focus à qui écrit ailleurs entre-temps (`busyElsewhere`).
 */
export function useArrival<T extends Arrival>(
  editor: MementumEditor,
  session: Synced,
  target: T | null,
  onDone: ((target: T) => void) | undefined,
): void {
  const subscribe = useCallback((onChange: () => void) => session.subscribe(onChange), [session]);
  const ready = useSyncExternalStore(subscribe, () => session.synced);
  const done = useRef(onDone);
  done.current = onDone;
  /** La visée en cours, et si elle a dû attendre la page. */
  const pending = useRef<{ target: T | null; waited: boolean }>({ target: null, waited: false });
  /** L'éditeur de la page était déjà là quand la visée est arrivée. */
  const mounted = useRef(false);
  useEffect(() => {
    // N'est « dans le geste » qu'une visée reçue par un éditeur déjà monté : celle qui est là à son
    // premier passage a attendu son code, même si tout est prêt (la synchro faite avant lui), et
    // la personne a pu se mettre à écrire dans le titre.
    if (pending.current.target !== target) pending.current = { target, waited: !mounted.current };
    mounted.current = true;
    if (target === null) return;
    const state = pending.current;
    const arrive = () => {
      goToBlock(editor, target.blockId, { focus: !state.waited || !busyElsewhere(editor) });
      done.current?.(target);
    };
    if (!ready) {
      state.waited = true;
      return;
    }
    if (editor.domElement?.isConnected) {
      arrive();
      return;
    }
    state.waited = true;
    return editor.onMount(arrive);
  }, [editor, ready, target]);
}
