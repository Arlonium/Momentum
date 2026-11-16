// La formule en ligne (registre : contenu en ligne `math`, prop `latex`), dans le texte d'un bloc.
// Pour qui peut modifier, elle est un bouton, « Modifier la formule », qui ouvre sa fenêtre ; « Équation
// en ligne » du menu « / » en pose une vide au curseur et ouvre la sienne ; Ctrl+Alt+Entrée ouvre
// celle de la formule sélectionnée.
//
// Une formule en ligne n'a pas d'id : sa fenêtre la tient par sa vue (`InlineMathTarget`, que la vue
// tient à jour, et qui sait sa place dans le document au moment d'écrire). Elle est rendue hors du
// document, par `InlineMathHost`, à côté de la vue de l'éditeur, comme celle d'un bloc : la vue de
// la formule recréée ou supprimée par un autre ne la ferme pas, le focus reste dans son champ, et
// la frappe ne part jamais dans le document. Son brouillon vit dans l'hôte, tant que l'éditeur et
// la formule sont là : fermer la page le perd, et la fenêtre le dit (R424).
//
// La formule s'affiche dans la ligne ; plus large que la ligne, elle défile en dedans (elle était
// coupée sans rien dire).
import { createReactInlineContentSpec } from "@blocknote/react";
import { NodeSelection, TextSelection } from "prosemirror-state";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import type { MementumEditor } from "../debug";
import { BlockPopover } from "./BlockPopover";
import { Formula } from "./Formula";
import { LatexField } from "./LatexField";
import { textProp } from "./props";
import { useEditable } from "./useEditable";

/** La vue d'une formule en ligne, telle que sa fenêtre la tient. */
export type InlineMathTarget = {
  /** Faux dès que la vue est démontée (la formule supprimée ou recréée). */
  alive: boolean;
  element: HTMLElement | null;
  getPos: () => number | undefined;
  latex: () => string;
  apply: (latex: string) => void;
  /** Prévient la fenêtre qui la tient quand la vue part (posé par la fenêtre). */
  onGone: (() => void) | null;
};

/** Les vues de formule en ligne de chaque éditeur, pour retrouver celle qu'on vient de poser. */
const targetsOf = new WeakMap<object, Set<InlineMathTarget>>();

function targets(editor: object): Set<InlineMathTarget> {
  let set = targetsOf.get(editor);
  if (set === undefined) {
    set = new Set();
    targetsOf.set(editor, set);
  }
  return set;
}

/** La vue de la formule en ligne qui est à la place `pos` du document de `editor`. */
export function inlineMathAt(editor: object, pos: number): InlineMathTarget | undefined {
  for (const target of targets(editor)) if (target.alive && target.getPos() === pos) return target;
  return undefined;
}

/** La fenêtre ouverte : la vue tenue, ou la place d'une formule qu'on vient de poser, pas encore montée. */
type OpenInline = {
  /** Une fenêtre neuve à chaque ouverture. */
  readonly id: number;
  readonly target: InlineMathTarget | null;
  readonly pos: number;
  readonly returnTo: "anchor" | "editor";
};

type InlineState = { open: OpenInline | null; readonly listeners: Set<() => void> };
const statesOf = new WeakMap<object, InlineState>();
let openings = 0;

function stateOf(editor: object): InlineState {
  let state = statesOf.get(editor);
  if (state === undefined) {
    state = { open: null, listeners: new Set() };
    statesOf.set(editor, state);
  }
  return state;
}

function setOpen(editor: object, open: OpenInline | null): void {
  const state = stateOf(editor);
  state.open = open;
  for (const listener of [...state.listeners]) listener();
}

/**
 * Ouvre la fenêtre d'une formule en ligne : celle d'une vue (un clic), ou celle de la formule à la
 * place `pos` (posée à l'instant, dont la vue n'est pas encore montée). `returnTo` : où va le focus
 * à la fermeture.
 */
export function openInlineMath(
  editor: object,
  opening: { target: InlineMathTarget } | { pos: number },
  returnTo: "anchor" | "editor",
): void {
  openings += 1;
  if ("target" in opening) {
    setOpen(editor, { id: openings, target: opening.target, pos: opening.target.getPos() ?? -1, returnTo });
  } else setOpen(editor, { id: openings, target: null, pos: opening.pos, returnTo });
}

export function closeInlineMath(editor: object): void {
  if (stateOf(editor).open !== null) setOpen(editor, null);
}

function useOpenInlineMath(editor: object): OpenInline | null {
  const listen = useCallback(
    (listener: () => void) => {
      const { listeners } = stateOf(editor);
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    [editor],
  );
  return useSyncExternalStore(listen, () => stateOf(editor).open);
}

/**
 * Ctrl+Alt+Entrée sur une formule en ligne sélectionnée (les flèches la sélectionnent) : ouvre sa
 * fenêtre, le focus reviendra au texte. Rend vrai si elle s'est ouverte.
 */
export function openInlineMathAtSelection(editor: MementumEditor): boolean {
  if (!editor.isEditable) return false;
  const { selection } = editor.prosemirrorState;
  if (!(selection instanceof NodeSelection) || selection.node.type.name !== "math") return false;
  const target = inlineMathAt(editor, selection.from);
  openInlineMath(editor, target !== undefined ? { target } : { pos: selection.from }, "editor");
  return true;
}

type ViewProps = {
  latex: string;
  editor: MementumEditor;
  getPos: () => number | undefined;
  update: (latex: string) => void;
};

function InlineMathView({ latex, editor, getPos, update }: ViewProps) {
  const editable = useEditable(editor);
  // La vue telle que sa fenêtre la tient : un seul objet pour la vie de la vue, tenu à jour.
  const target = useRef<InlineMathTarget>({
    alive: true,
    element: null,
    getPos,
    latex: () => latex,
    apply: update,
    onGone: null,
  }).current;
  target.getPos = getPos;
  target.latex = () => latex;
  target.apply = update;
  useLayoutEffect(() => {
    target.alive = true;
    targets(editor).add(target);
    return () => {
      target.alive = false;
      targets(editor).delete(target);
      // La vue part après le changement du document qui l'a supprimée : la fenêtre l'apprend ici.
      target.onGone?.();
    };
  }, [editor, target]);
  const shown =
    latex.trim() === "" ? <span className="mm-placeholder">formule</span> : <Formula latex={latex} display={false} />;
  const keep = (element: HTMLElement | null) => {
    if (element !== null) target.element = element;
  };
  if (!editable) {
    return (
      <span ref={keep} className="mm-math">
        {shown}
      </span>
    );
  }
  return (
    <button
      ref={keep}
      type="button"
      className="mm-math mm-math-button"
      aria-label="Modifier la formule"
      aria-haspopup="dialog"
      onClick={(event) => {
        event.currentTarget.focus();
        openInlineMath(editor, { target }, "anchor");
      }}
    >
      {shown}
    </button>
  );
}

export const MathInline = createReactInlineContentSpec(
  { type: "math", propSchema: { latex: { default: "" } }, content: "none" },
  {
    render: ({ inlineContent, editor, getPos, updateInlineContent }) => (
      <InlineMathView
        latex={textProp(inlineContent.props, "latex")}
        editor={editor as unknown as MementumEditor}
        getPos={() => (typeof getPos === "function" ? getPos() : undefined)}
        update={(latex) => updateInlineContent({ type: "math", props: { latex } })}
      />
    ),
  },
);

/** Ce que dit la fenêtre d'une formule en ligne : où vit son brouillon. */
export const INLINE_DRAFT_NOTE = "Ce texte n'est gardé que tant que la page est ouverte : la fermer le perd.";

/** Ce que dit la fenêtre d'une formule partie. */
export const INLINE_GONE = "Cette formule n'est plus dans la page : quelqu'un l'a supprimée ou changée. Ton texte reste ici, à copier.";

/** Combien d'images on attend, au plus, la vue d'une formule qu'on vient de poser ou dont la vue est partie. */
const MAX_WAIT_FRAMES = 60;

/** Où est une formule en ligne : l'id de son bloc et son rang parmi les formules en ligne du bloc. */
type Place = { blockId: string; rank: number };

/** La place de la formule en ligne qui est à la place `pos` du document de `editor`. */
function placeAt(editor: MementumEditor, pos: number | undefined): Place | null {
  if (pos === undefined) return null;
  try {
    const at = editor.prosemirrorState.doc.resolve(pos);
    for (let depth = at.depth; depth >= 0; depth -= 1) {
      const node = at.node(depth);
      if (node.type.name !== "blockContainer") continue;
      let rank = 0;
      at.parent.forEach((child, offset) => {
        if (offset < at.parentOffset && child.type.name === "math") rank += 1;
      });
      return { blockId: String(node.attrs.id), rank };
    }
  } catch {
    // La place ne tient plus dans le document.
  }
  return null;
}

/**
 * La vue vivante de la formule qui a la place `place` et la formule `latex` : celle dont la vue est
 * partie quand un autre a déplacé son bloc (supprimé et recréé au même id, R220). Une autre formule
 * qui aurait pris son rang (la sienne supprimée) n'a pas la même formule : elle n'est pas prise.
 */
function sameFormula(editor: MementumEditor, place: Place | null, latex: string): InlineMathTarget | undefined {
  if (place === null) return undefined;
  for (const target of targets(editor)) {
    if (!target.alive || target.latex() !== latex) continue;
    const there = placeAt(editor, target.getPos());
    if (there !== null && there.blockId === place.blockId && there.rank === place.rank) return target;
  }
  return undefined;
}

/** Les brouillons d'un éditeur : par vue de formule, et, pour une formule pas encore arrivée, par place. */
type Drafts = { byTarget: WeakMap<InlineMathTarget, string>; early: Map<number, string> };

/** La fenêtre ouverte d'une formule en ligne de `editor`, s'il y en a une. */
export function InlineMathHost({ editor }: { editor: MementumEditor }) {
  const open = useOpenInlineMath(editor);
  // Les brouillons vivent tant que cet hôte (la vue de l'éditeur) vit, fenêtre fermée comprise.
  const drafts = useRef<Drafts>({ byTarget: new WeakMap(), early: new Map() }).current;
  if (open === null) return null;
  return <OpenInlineMath key={open.id} editor={editor} open={open} drafts={drafts} />;
}

function OpenInlineMath({ editor, open, drafts }: { editor: MementumEditor; open: OpenInline; drafts: Drafts }) {
  const [target, setTarget] = useState<InlineMathTarget | null>(open.target);
  const [gaveUp, setGaveUp] = useState(false);
  const [version, refresh] = useState(0);
  const editable = useEditable(editor);

  const alive = target !== null && target.alive;
  // La dernière formule vue, et sa place : de quoi la retrouver quand sa vue part.
  const seen = useRef("");
  const place = useRef<Place | null>(null);
  if (alive) {
    seen.current = target.latex();
    place.current = placeAt(editor, target.getPos()) ?? place.current;
  }

  // Une formule qu'on vient de poser : sa vue arrive au rendu suivant, on la cherche à sa place. Une
  // formule dont la vue est partie : on la cherche par sa place et sa formule. Au bout de l'attente,
  // la fenêtre la dit partie ; elle la cherche encore à chaque changement du document (une annulation).
  useEffect(() => {
    if (alive) return;
    let frames = 0;
    let handle = 0;
    const look = () => {
      const found = target === null ? inlineMathAt(editor, open.pos) : sameFormula(editor, place.current, seen.current);
      if (found !== undefined) {
        const typed = target === null ? drafts.early.get(open.pos) : drafts.byTarget.get(target);
        if (typed !== undefined) drafts.byTarget.set(found, typed);
        if (target === null) drafts.early.delete(open.pos);
        setGaveUp(false);
        setTarget(found);
        return;
      }
      frames += 1;
      if (gaveUp) return;
      if (frames < MAX_WAIT_FRAMES) handle = window.setTimeout(look, 16);
      else setGaveUp(true);
    };
    look();
    return () => window.clearTimeout(handle);
  }, [editor, open.pos, target, alive, gaveUp, drafts, version]);

  // La formule peut changer ou partir : relue à chaque changement du document, et quand sa vue part.
  useEffect(() => editor.onChange(() => refresh((n) => n + 1)), [editor]);
  useEffect(() => {
    if (target === null) return;
    target.onGone = () => refresh((n) => n + 1);
    return () => {
      if (target.onGone !== null) target.onGone = null;
    };
  }, [target]);

  const draft = target !== null ? drafts.byTarget.get(target) : drafts.early.get(open.pos);
  const searching = !alive && !gaveUp;
  const writable = alive && editable;

  const focusBack = () => {
    const anchor = open.returnTo === "anchor" && alive ? target.element : null;
    anchor?.focus();
    if (anchor !== null && document.activeElement === anchor) return;
    // Le curseur juste après la formule, où la frappe reprend.
    if (alive) {
      const pos = target.getPos();
      if (pos !== undefined) {
        try {
          editor.transact((tr) => tr.setSelection(TextSelection.create(tr.doc, pos + 1)));
        } catch {
          // La place ne tient plus : le curseur reste où il était.
        }
      }
    }
    editor.focus();
    const dom = editor.domElement;
    if (dom !== undefined && !dom.contains(document.activeElement)) dom.focus({ preventScroll: true });
  };
  const close = (how: "escape" | "outside" | "done") => {
    closeInlineMath(editor);
    if (how !== "outside") {
      focusBack();
      return;
    }
    setTimeout(() => {
      const active = document.activeElement;
      if (active === null || active === document.body) focusBack();
    }, 0);
  };
  const apply = () => {
    if (!writable) return;
    if (draft !== undefined) {
      target.apply(draft);
      drafts.byTarget.delete(target);
    }
    close("done");
  };
  return (
    <BlockPopover label="Formule en ligne" anchor={() => (alive ? target.element : null)} onClose={close}>
      {!alive && gaveUp && (
        <p className="mm-block-window-gone" role="status">
          {INLINE_GONE}
        </p>
      )}
      <LatexField
        value={draft ?? seen.current}
        display={false}
        readOnly={!searching && !writable}
        onApply={apply}
        onChange={(value) => {
          if (target !== null) drafts.byTarget.set(target, value);
          else drafts.early.set(open.pos, value);
          refresh((n) => n + 1);
        }}
        note={INLINE_DRAFT_NOTE}
      />
    </BlockPopover>
  );
}
