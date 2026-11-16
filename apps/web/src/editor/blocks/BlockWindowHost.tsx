// La fenêtre de réglage du bloc riche ouvert (`blockWindows.ts`), rendue hors du document, à côté de
// la vue de l'éditeur (`EditorView`), jamais dans la vue du bloc.
//
// Elle tient le bloc par son id, et le relit à chaque changement du document, tant qu'elle est
// ouverte (fermée, elle ne relit rien) : un autre qui recrée le bloc (déplacement, R223) ne la ferme
// pas, son élément est retrouvé à sa nouvelle place ; un autre qui le supprime ou le change en
// autre chose ne la ferme pas non plus : elle le dit, garde ce qu'on y a tapé, et n'écrit plus rien
// (le bloc revenu, une annulation, elle reprend). Le focus reste donc où l'on tape, et la frappe qui
// suit ne part jamais dans le document.
//
// À la fermeture, le focus revient au bouton du bloc (retrouvé par l'id du bloc, à sa place du
// moment), ou à l'éditeur si on l'a ouverte depuis le texte (Ctrl+Alt+Entrée, le menu « / ») ou si
// le bouton n'est plus là ; il est vérifié, jamais laissé sur `body` (R470). Un clic dehors laisse
// le focus où le clic l'a mis, sauf s'il tombe sur `body`.
//
// La fenêtre est celle de la page (`blockWindows.ts`, R471) : une session recréée apporte un
// éditeur neuf, et la fenêtre passe à lui, avec ce qu'on y a tapé. L'éditeur n'est plus modifiable
// (une baisse de droits, la corbeille) : elle le dit, et n'écrit plus rien (R472).
//
// `BLOCK_WINDOWS` dit quelles sortes de blocs ont une fenêtre, et laquelle : la Task C2 y ajoute
// l'équation. Ctrl+Alt+Entrée (Cmd+Option+Entrée) ouvre celle du bloc où est le curseur.
import { useEffect, useState, type ComponentType } from "react";
import type { ShortcutEvent } from "../../comments/shortcut";
import { blockElement } from "../../comments/dom";
import type { MementumEditor } from "../debug";
import { BookmarkWindow } from "./Bookmark";
import { CalloutWindow } from "./Callout";
import {
  blockDraft,
  closeBlockWindow,
  openBlockWindow,
  useOpenBlockWindow,
  type BlockWindowProps,
  type EditorBlock,
  type OpenWindow,
} from "./blockWindows";
import { BlockPopover } from "./BlockPopover";
import { useEditable } from "./useEditable";

export type BlockWindowSpec = {
  /** Le nom de la fenêtre (lecteurs d'écran). */
  label: string;
  /** Ce qu'elle dit quand son bloc n'est plus là. */
  gone: string;
  /** Son contenu. */
  Window: ComponentType<BlockWindowProps>;
};

/** Les blocs qui ont une fenêtre de réglage, par type de bloc. */
export const BLOCK_WINDOWS: Readonly<Record<string, BlockWindowSpec>> = {
  callout: {
    label: "Icône et couleur du callout",
    gone: "Ce callout n'est plus dans la page : quelqu'un l'a supprimé ou changé.",
    Window: CalloutWindow,
  },
  bookmark: {
    label: "Adresse du signet",
    gone: "Ce signet n'est plus dans la page : quelqu'un l'a supprimé ou changé. Ton texte reste ici, à copier.",
    Window: BookmarkWindow,
  },
};

function windowOf(type: string | undefined): BlockWindowSpec | null {
  return type !== undefined && Object.hasOwn(BLOCK_WINDOWS, type) ? BLOCK_WINDOWS[type] : null;
}

/** Ce que dit la fenêtre quand l'éditeur ne peut pas écrire. */
export const READ_ONLY_NOTICE = "Tu ne peux pas modifier cette page pour l'instant : ton texte reste ici, à copier.";

/**
 * Ctrl+Alt+Entrée (Cmd+Option+Entrée sur Mac) : régler le bloc riche où est le curseur. La touche
 * se reconnaît à `key` ; sous une fenêtre modale, le reste de la page est inerte (comme Ctrl+Alt+M,
 * `comments/shortcut.ts`). Pas de garde contre AltGr (R475) : Entrée ne tape aucun caractère avec
 * lui, et une disposition à AltGr (l'AZERTY sous Windows) pourrait dire AltGr pour Ctrl+Alt.
 */
export function isBlockWindowShortcut(event: ShortcutEvent): boolean {
  if (event.key !== "Enter" || !event.altKey || !(event.ctrlKey || event.metaKey)) return false;
  return globalThis.document?.querySelector("dialog[open]") == null;
}

/**
 * Ouvre la fenêtre du bloc riche où est le curseur de `editor` (un callout où l'on tape, un signet
 * sélectionné), s'il en a une ; rend vrai si elle s'est ouverte. L'éditeur en lecture seule ne
 * l'ouvre que si un texte tapé avant la perte du droit l'y attend, à copier (R472). Le focus
 * reviendra à l'éditeur.
 */
export function openWindowAtCursor(editor: MementumEditor): boolean {
  const block = editor.getTextCursorPosition().block;
  if (windowOf(block.type) === null) return false;
  if (!editor.isEditable && blockDraft(editor, block.id, block.type) === undefined) return false;
  openBlockWindow(editor, block, "editor");
  return true;
}

/** L'élément affiché du bloc `blockId`, s'il l'est. */
function shownBlock(editor: MementumEditor, blockId: string): HTMLElement | null {
  const root = editor.domElement;
  return root ? blockElement(root, blockId) : null;
}

/**
 * Le bouton du bloc `blockId` qui ouvre sa fenêtre (`data-block-window-anchor`), dans son propre
 * contenu, jamais dans celui d'un bloc enfant ; `null` s'il n'en a pas (lecteur, bloc changé en
 * autre chose) ou n'est pas affiché. Seul lui reçoit le focus : l'élément du bloc ne le prend pas.
 */
function anchorOf(editor: MementumEditor, blockId: string): HTMLElement | null {
  return (
    shownBlock(editor, blockId)?.querySelector<HTMLElement>(
      ":scope > .bn-block > :not(.bn-block-group) [data-block-window-anchor]",
    ) ?? null
  );
}

/** Où poser la fenêtre : près du bouton du bloc, sinon près du bloc. */
function placeOf(editor: MementumEditor, blockId: string): HTMLElement | null {
  return anchorOf(editor, blockId) ?? shownBlock(editor, blockId);
}

/**
 * Donne le focus à l'éditeur. ProseMirror ne le prend pas pour un éditeur en lecture seule
 * (`view.focus()` ne focalise que s'il est modifiable) : on le donne alors à son élément, qui garde
 * son `tabindex`, plutôt que de le laisser tomber sur `body` (R470).
 */
function focusEditor(editor: MementumEditor): void {
  editor.focus();
  const dom = editor.domElement;
  if (dom !== undefined && !dom.contains(document.activeElement)) dom.focus({ preventScroll: true });
}



/** La fenêtre ouverte de `editor`, s'il y en a une. */
export function BlockWindowHost({ editor }: { editor: MementumEditor }) {
  const open = useOpenBlockWindow(editor);
  if (open === null) return null;
  return <OpenBlockWindow key={open.blockId} editor={editor} open={open} />;
}

function OpenBlockWindow({ editor, open }: { editor: MementumEditor; open: OpenWindow }) {
  // La sorte de la fenêtre est celle du bloc à l'ouverture : changé en autre chose, il n'est plus le
  // sien.
  const { blockId, kind, returnTo } = open;
  const spec = windowOf(kind);
  const read = (): EditorBlock | null => {
    const block = editor.getBlock(blockId);
    return block !== undefined && block.type === kind ? block : null;
  };
  const [block, setBlock] = useState(read);
  const editable = useEditable(editor);

  useEffect(() => {
    if (spec === null) {
      closeBlockWindow(editor);
      return;
    }
    setBlock(read());
    return editor.onChange(() => setBlock(read()));
    // `read` ne lit que l'éditeur, l'id et la sorte : l'éditeur peut changer (une session recréée),
    // la fenêtre relit alors le bloc dans le nouveau.
  }, [editor, blockId, spec]);

  if (spec === null) return null;

  // Le bouton n'existe que pour qui peut modifier (`useEditable`), ou a un texte tapé à copier :
  // sans lui, l'éditeur. Ouverte par le bouton, la fenêtre ne sait pas où était le curseur de
  // l'éditeur (peut-être loin) : si le bloc est encore là (changé en autre chose), il va à sa fin, où
  // la frappe reprend ; sinon, là où il était (R470).
  const focusBack = () => {
    const anchor = returnTo === "anchor" ? anchorOf(editor, blockId) : null;
    anchor?.focus();
    if (anchor !== null && document.activeElement === anchor) return;
    if (returnTo === "anchor" && editor.getBlock(blockId) !== undefined) {
      try {
        editor.setTextCursorPosition(blockId, "end");
      } catch {
        // Un bloc qu'on ne sait pas désigner : le curseur reste où il était.
      }
    }
    focusEditor(editor);
  };
  const close = (how: "escape" | "outside" | "done") => {
    closeBlockWindow(editor);
    if (how !== "outside") {
      focusBack();
      return;
    }
    // Le clic met le focus où il veut ; s'il ne le met nulle part, il revient au bloc.
    setTimeout(() => {
      const active = document.activeElement;
      if (active === null || active === document.body) focusBack();
    }, 0);
  };

  const { Window } = spec;
  return (
    <BlockPopover label={spec.label} anchor={() => placeOf(editor, blockId)} onClose={close}>
      {(!editable || block === null) && (
        <p className="mm-block-window-gone" role="status">
          {editable ? spec.gone : READ_ONLY_NOTICE}
        </p>
      )}
      <Window editor={editor} blockId={blockId} block={block} editable={editable} close={close} />
    </BlockPopover>
  );
}
