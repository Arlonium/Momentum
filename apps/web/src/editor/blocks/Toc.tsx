// La table des matières (registre : `toc`, sans prop) : les titres de niveau 1 à 4 de la page, dans
// l'ordre du document, chacun un lien à sa profondeur. Un lien fait défiler la page jusqu'à son titre,
// et y met le curseur et le focus ; il n'est pas suivi (l'adresse ne change pas). La même pour qui
// lit et qui modifie : c'est une navigation, elle n'écrit rien.
//
// Rien sur le chemin de la frappe (R5xx) : les titres sont lus dans le document de l'éditeur
// (`headings.ts`, environ 1 ms pour 10 000 blocs), une fois au montage de la première table de la
// page, puis à chaque changement du document, groupés comme les relectures du serveur (`groupedDelay`
// de `rereads.ts` : 250 ms après le dernier changement, une seconde au plus après le premier). Une
// seule lecture par éditeur, quel que soit le nombre de tables ; une page sans table n'en fait
// aucune : l'éditeur n'est écouté que tant qu'une table est affichée.
//
// Rien ne bouge sous le pointeur ni sous le focus : tant que l'un ou l'autre est dans la table, elle
// garde ce qu'elle montrait (un titre supprimé reste, son lien ne mène plus nulle part), et suit la
// page quand ils en sont sortis. La table supprimée par un autre pendant qu'une de ses entrées a le
// focus, le focus va à l'éditeur, jamais sur `body`.
//
// Le texte des titres vient du document : un nœud de texte, isolé (`dir="auto"`), jamais du HTML.
import { createReactBlockSpec } from "@blocknote/react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type FocusEvent,
  type MouseEvent,
} from "react";
import { blockElement, shorten } from "../../comments/dom";
import { groupedDelay } from "../../rereads";
import type { MementumEditor } from "../debug";
import { focusEditor } from "./BlockWindowHost";
import { readHeadings, type Heading } from "./headings";

/** Ce qu'une entrée montre d'un titre très long (le titre entier reste dans la page). */
const MAX_ENTRY_CHARS = 200;

type TocState = {
  /** Les titres lus ; `null` : pas lus depuis que plus aucune table n'est affichée. */
  headings: readonly Heading[] | null;
  /** Le document lu : un document ProseMirror ne change jamais, un autre est un changement. */
  read: unknown;
  readonly listeners: Set<() => void>;
  /** Cesse d'écouter l'éditeur (la dernière table partie). */
  stop: (() => void) | null;
  timer: ReturnType<typeof setTimeout> | undefined;
  /** Le premier changement pas encore lu ; `null` : aucun. */
  waitingSince: number | null;
};

const states = new WeakMap<object, TocState>();

function stateOf(editor: MementumEditor): TocState {
  let state = states.get(editor);
  if (state === undefined) {
    state = { headings: null, read: null, listeners: new Set(), stop: null, timer: undefined, waitingSince: null };
    states.set(editor, state);
  }
  return state;
}

function sameHeadings(a: readonly Heading[], b: readonly Heading[]): boolean {
  return a.length === b.length && a.every((h, i) => h.id === b[i].id && h.level === b[i].level && h.text === b[i].text);
}

/** Lit les titres si le document a changé depuis la dernière lecture ; prévient les tables. */
function refresh(editor: MementumEditor, state: TocState): void {
  const doc = editor.prosemirrorState.doc;
  if (state.headings !== null && state.read === doc) return;
  const next = readHeadings(editor);
  state.read = doc;
  if (state.headings !== null && sameHeadings(next, state.headings)) return;
  state.headings = next;
  for (const listener of [...state.listeners]) listener();
}

/** Un changement du document : une lecture groupée. */
function changed(editor: MementumEditor, state: TocState): void {
  const now = Date.now();
  state.waitingSince ??= now;
  clearTimeout(state.timer);
  state.timer = setTimeout(() => {
    state.timer = undefined;
    state.waitingSince = null;
    refresh(editor, state);
  }, groupedDelay(state.waitingSince, now));
}

function subscribe(editor: MementumEditor, listener: () => void): () => void {
  const state = stateOf(editor);
  state.listeners.add(listener);
  if (state.stop === null) {
    const off = editor.onChange(() => changed(editor, state));
    state.stop = () => {
      off();
      clearTimeout(state.timer);
      state.timer = undefined;
      state.waitingSince = null;
    };
    // Le document a pu changer entre le rendu et l'abonnement.
    refresh(editor, state);
  }
  return () => {
    state.listeners.delete(listener);
    if (state.listeners.size > 0) return;
    state.stop?.();
    state.stop = null;
    state.headings = null;
    state.read = null;
  };
}

function snapshot(editor: MementumEditor): readonly Heading[] {
  const state = stateOf(editor);
  if (state.headings === null) {
    state.headings = readHeadings(editor);
    state.read = editor.prosemirrorState.doc;
  }
  return state.headings;
}

/** Les titres de la page de `editor`, suivis (groupés). */
function useHeadings(editor: MementumEditor): readonly Heading[] {
  const listen = useCallback((listener: () => void) => subscribe(editor, listener), [editor]);
  return useSyncExternalStore(listen, () => snapshot(editor));
}

/**
 * Va au titre du bloc `id` : le curseur au début du titre, le focus à l'éditeur, la page défilée
 * jusqu'à lui. Un titre parti depuis (la table était tenue sous le pointeur) : rien.
 */
function goToHeading(editor: MementumEditor, id: string): void {
  const root = editor.domElement;
  const element = root ? blockElement(root, id) : null;
  if (element === null || editor.getBlock(id) === undefined) return;
  try {
    editor.setTextCursorPosition(id, "start");
  } catch {
    // Un bloc qu'on ne sait pas désigner : le curseur reste où il était.
  }
  focusEditor(editor);
  // En lecture seule, ProseMirror ne pose pas sa sélection dans la page, et relit celle du document
  // quand l'éditeur prend le focus : on la pose au début du titre, où un lecteur d'écran reprend.
  if (!editor.isEditable) {
    const text = element.querySelector(":scope > .bn-block > .bn-block-content .bn-inline-content");
    if (text !== null) document.getSelection()?.collapse(text, 0);
  }
  element.scrollIntoView({ block: "start" });
}

/** Les modificateurs d'un clic qui ouvre ailleurs (un onglet, une fenêtre) : laissé au navigateur. */
const opensElsewhere = (event: MouseEvent) =>
  event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey;

function TocView({ editor }: { editor: MementumEditor }) {
  const headings = useHeadings(editor);
  // Ce que la table montrait quand le pointeur ou le focus y est entré : gardé tant qu'ils y sont.
  const [held, setHeld] = useState<readonly Heading[] | null>(null);
  const inside = useRef({ pointer: false, focus: false });
  const nav = useRef<HTMLElement>(null);
  const hold = () => setHeld((was) => was ?? headings);
  const release = () => {
    if (!inside.current.pointer && !inside.current.focus) setHeld(null);
  };

  // La table retirée sous le focus (un autre l'a supprimée, ou a recréé son bloc) : React n'appelle
  // pas `onBlur` pour un élément qu'il retire, et le focus tombait sur `body`.
  useEffect(() => {
    return () => {
      if (!inside.current.focus) return;
      setTimeout(() => {
        const active = document.activeElement;
        if (active === null || active === document.body) focusEditor(editor);
      }, 0);
    };
  }, [editor]);

  const shown = held ?? headings;
  const onFocus = () => {
    inside.current.focus = true;
    hold();
  };
  const onBlur = (event: FocusEvent) => {
    if (event.relatedTarget instanceof Node && nav.current?.contains(event.relatedTarget)) return;
    inside.current.focus = false;
    release();
  };
  const follow = (event: MouseEvent, id: string) => {
    if (opensElsewhere(event)) return;
    event.preventDefault();
    goToHeading(editor, id);
  };

  return (
    <nav
      ref={nav}
      className="mm-toc"
      aria-label="Table des matières"
      contentEditable={false}
      onFocus={onFocus}
      onBlur={onBlur}
      onPointerEnter={() => {
        inside.current.pointer = true;
        hold();
      }}
      onPointerLeave={() => {
        inside.current.pointer = false;
        release();
      }}
    >
      {shown.length === 0 ? (
        <p className="mm-toc-empty">Aucun titre dans la page.</p>
      ) : (
        <ol className="mm-toc-list">
          {shown.map((heading) => (
            <li key={heading.id} className={`mm-toc-entry mm-toc-level-${heading.level}`} data-level={heading.level}>
              <a className="mm-toc-link" href={`#${heading.id}`} dir="auto" onClick={(event) => follow(event, heading.id)}>
                {shorten(heading.text, MAX_ENTRY_CHARS)}
              </a>
            </li>
          ))}
        </ol>
      )}
    </nav>
  );
}

export const Toc = createReactBlockSpec(
  { type: "toc", propSchema: {}, content: "none" },
  {
    render: ({ editor }) => <TocView editor={editor as unknown as MementumEditor} />,
  },
);

/**
 * Ctrl+Alt+Entrée sur une table des matières ou un lien de page sélectionné (les flèches les
 * sélectionnent) : le focus va à son premier lien, d'où Tab parcourt les autres et Entrée suit.
 * Dans le texte de l'éditeur, Tab imbrique les blocs : sans cela, aucun chemin au clavier n'y
 * menait pour qui modifie. Rend vrai si le focus y est allé.
 */
export function focusBlockLinks(editor: MementumEditor): boolean {
  const block = editor.getTextCursorPosition().block;
  if (block.type !== "toc" && block.type !== "pageLink") return false;
  const root = editor.domElement;
  // Dans son propre contenu, jamais dans celui d'un bloc enfant.
  const first = root
    ? blockElement(root, block.id)?.querySelector<HTMLElement>(":scope > .bn-block > :not(.bn-block-group) a[href]")
    : null;
  if (first == null) return false;
  first.focus();
  return document.activeElement === first;
}
