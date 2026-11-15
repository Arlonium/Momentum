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
// moment), ou à l'éditeur si on l'a ouverte depuis le texte (Ctrl+Alt+Entrée) ou si le bouton n'est
// plus là. Un clic dehors laisse le focus où le clic l'a mis, sauf s'il tombe sur `body`.
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

/**
 * Ctrl+Alt+Entrée (Cmd+Option+Entrée sur Mac) : régler le bloc riche où est le curseur. La touche
 * se reconnaît à `key` ; tapée avec AltGr (Ctrl+Alt sous Windows), elle n'est jamais prise ; sous
 * une fenêtre modale, le reste de la page est inerte (comme Ctrl+Alt+M, `comments/shortcut.ts`).
 */
export function isBlockWindowShortcut(event: ShortcutEvent): boolean {
  if (event.key !== "Enter" || !event.altKey || !(event.ctrlKey || event.metaKey)) return false;
  if (event.getModifierState("AltGraph")) return false;
  return globalThis.document?.querySelector("dialog[open]") == null;
}

/**
 * Ouvre la fenêtre du bloc riche où est le curseur de `editor` (un callout où l'on tape, un signet
 * sélectionné), s'il en a une et si l'éditeur est modifiable ; rend vrai si elle s'est ouverte. Le
 * focus reviendra à l'éditeur.
 */
export function openWindowAtCursor(editor: MementumEditor): boolean {
  if (!editor.isEditable) return false;
  const block = editor.getTextCursorPosition().block;
  if (windowOf(block.type) === null) return false;
  openBlockWindow(editor, block.id, "editor");
  return true;
}

/**
 * Le bouton du bloc `blockId` qui ouvre sa fenêtre (`data-block-window-anchor`), dans son propre
 * contenu (pas dans celui d'un bloc enfant) ; à défaut, l'élément du bloc ; `null` s'il n'est pas
 * affiché.
 */
function anchorOf(editor: MementumEditor, blockId: string): HTMLElement | null {
  const root = editor.domElement;
  const element = root ? blockElement(root, blockId) : null;
  if (element === null) return null;
  return (
    element.querySelector<HTMLElement>(":scope > .bn-block > :not(.bn-block-group) [data-block-window-anchor]") ??
    element
  );
}

/** La fenêtre ouverte de `editor`, s'il y en a une. */
export function BlockWindowHost({ editor }: { editor: MementumEditor }) {
  const open = useOpenBlockWindow(editor);
  if (open === null) return null;
  return <OpenBlockWindow key={open.blockId} editor={editor} open={open} />;
}

function OpenBlockWindow({ editor, open }: { editor: MementumEditor; open: OpenWindow }) {
  const { blockId, returnTo } = open;
  // La sorte de la fenêtre est celle du bloc à l'ouverture : changé en autre chose, il n'est plus le
  // sien.
  const [kind] = useState(() => editor.getBlock(blockId)?.type);
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
    // `read` ne lit que l'éditeur, l'id et la sorte, fixes pour cette fenêtre.
  }, [editor, blockId, spec]);

  if (spec === null) return null;

  const focusBack = () => {
    const anchor = returnTo === "anchor" && editable ? anchorOf(editor, blockId) : null;
    if (anchor !== null && anchor.isConnected) anchor.focus();
    else editor.focus();
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
    <BlockPopover label={spec.label} anchor={() => anchorOf(editor, blockId)} onClose={close}>
      {block === null && (
        <p className="mm-block-window-gone" role="status">
          {spec.gone}
        </p>
      )}
      <Window editor={editor} blockId={blockId} block={block} editable={editable} close={close} />
    </BlockPopover>
  );
}
