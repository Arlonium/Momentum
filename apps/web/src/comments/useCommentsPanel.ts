// Le câblage du panneau des commentaires d'une page ouverte : le bouton de la barre du haut,
// les pastilles de la marge et le panneau lui-même, autour des fils que lit `useComments`.
//
// - Le panneau s'ouvre sur rien de précis (le bouton), sur un fil (une pastille), ou sur le
//   commentaire neuf d'un bloc (« Commenter ») ; en se fermant, il rend le focus à ce qui
//   l'avait avant qu'il s'ouvre, ou au bouton.
// - Les pastilles suivent les blocs des fils, et les fils ouverts de chacun ; elles disent en
//   retour quels blocs commentés sont dans la page, et dans quel ordre.
// - Un fil mène à son bloc : la page défile jusqu'à lui, qui est mis en évidence un instant.
// - Les brouillons vivent ici, pour la page ouverte, hors du panneau qui se démonte ; le bloc dont
//   on rédige le commentaire neuf aussi : une pastille, un second « Commenter » ou le panneau
//   fermé puis rouvert les retrouvent (`drafts.ts`).
import { useCallback, useId, useMemo, useRef, useState, type RefObject } from "react";
import { openCount } from "./arrange";
import type { PanelTarget } from "./CommentsPanel";
import { blockElement, blockText, shorten } from "./dom";
import { createDrafts, type Drafts } from "./drafts";
import type { Comments } from "./useComments";

/** Le début du texte d'un bloc, au-dessus de son commentaire neuf : au plus tant de caractères. */
const EXCERPT_CHARS = 120;

export type CommentsPanelWiring = {
  /** Le bouton « Commentaires » de la barre du haut. */
  button: {
    buttonRef: RefObject<HTMLButtonElement | null>;
    /** Les fils ouverts ; `null` : pas encore lus. */
    count: number | null;
    expanded: boolean;
    panelId: string;
    onClick: () => void;
  };
  /** Ce que les pastilles de la marge suivent, et ce qu'elles font. */
  marks: {
    blockIds: string[];
    marks: Map<string, string[]>;
    onOrder: (order: string[] | null) => void;
    onOpenThread: (threadId: string) => void;
    highlight: { blockId: string; seq: number } | null;
  };
  /** Le panneau, s'il est ouvert : ses props de câblage. */
  panel: {
    id: string;
    order: string[] | null;
    target: PanelTarget | null;
    drafts: Drafts;
    composing: string | null;
    excerpt: (blockId: string) => string | null;
    onShowBlock: (blockId: string) => void;
    onTargetDone: () => void;
    onClose: () => void;
  } | null;
  /** « Commenter » sur un bloc : le panneau s'ouvre sur son commentaire neuf. Stable. */
  commentBlock: (blockId: string) => void;
};

export function useCommentsPanel(
  comments: Comments,
  /** Autour de l'éditeur : les blocs de la page y sont cherchés par leur identifiant. */
  frame: RefObject<HTMLElement | null>,
): CommentsPanelWiring {
  const panelId = useId();
  const buttonRef = useRef<HTMLButtonElement>(null);
  /** Le panneau, et ce sur quoi il s'est ouvert ; `null` : fermé. */
  const [panel, setPanel] = useState<{ target: PanelTarget | null } | null>(null);
  /** Ce qui avait le focus avant que le panneau s'ouvre : il le retrouve à la fermeture. */
  const returnFocus = useRef<HTMLElement | null>(null);
  /** Numérote les demandes faites au panneau (un fil, un bloc à commenter, un bloc à montrer). */
  const requests = useRef(0);
  const open = panel !== null;
  const [drafts] = useState(createDrafts);
  /** Le bloc dont on rédige le commentaire neuf, jusqu'à ce qu'il parte ou soit annulé. */
  const [composing, setComposing] = useState<string | null>(null);

  const openPanel = (target: PanelTarget | null) => {
    if (!open) {
      const active = document.activeElement;
      returnFocus.current = active instanceof HTMLElement ? active : null;
    }
    setPanel({ target });
  };
  const closePanel = () => {
    const back = returnFocus.current;
    returnFocus.current = null;
    setPanel(null);
    const panelElement = document.getElementById(panelId);
    if (back !== null && back.isConnected && !panelElement?.contains(back)) back.focus();
    else buttonRef.current?.focus();
  };
  const openOnThread = (threadId: string) =>
    openPanel({ kind: "thread", threadId, seq: ++requests.current });
  const commentBlockNow = useRef((_: string) => {});
  commentBlockNow.current = (blockId: string) => {
    setComposing(blockId);
    openPanel({ kind: "block", blockId, seq: ++requests.current });
  };
  const commentBlock = useCallback((blockId: string) => commentBlockNow.current(blockId), []);

  const { threads } = comments;
  const blockIds = useMemo(
    () => [...new Set(threads?.flatMap((t) => (t.blockId === null ? [] : [t.blockId])))],
    [threads],
  );
  const marks = useMemo(() => {
    const opened = new Map<string, string[]>();
    for (const t of threads ?? []) {
      if (t.blockId === null || t.resolvedAt !== null) continue;
      opened.set(t.blockId, [...(opened.get(t.blockId) ?? []), t.id]);
    }
    return opened;
  }, [threads]);
  /** Les blocs commentés qui sont dans la page, dans l'ordre ; `null` : pas encore lus. */
  const [order, setOrder] = useState<string[] | null>(null);
  const [highlight, setHighlight] = useState<{ blockId: string; seq: number } | null>(null);

  // Les blocs de la page sont cherchés à la demande (un clic, pas une frappe).
  const find = (blockId: string) =>
    frame.current === null ? null : blockElement(frame.current, blockId);
  const excerpt = (blockId: string) => {
    const element = find(blockId);
    const text = element === null ? "" : blockText(element);
    return text === "" ? null : shorten(text, EXCERPT_CHARS);
  };
  const showBlock = (blockId: string) => {
    find(blockId)?.scrollIntoView({ block: "center", behavior: "smooth" });
    setHighlight({ blockId, seq: ++requests.current });
  };

  return {
    button: {
      buttonRef,
      count: threads === null ? null : openCount(threads),
      expanded: open,
      panelId,
      onClick: () => (open ? closePanel() : openPanel(null)),
    },
    marks: { blockIds, marks, onOrder: setOrder, onOpenThread: openOnThread, highlight },
    panel: panel && {
      id: panelId,
      order,
      target: panel.target,
      drafts,
      composing,
      excerpt,
      onShowBlock: showBlock,
      onTargetDone: () => {
        setComposing(null);
        setPanel((current) => current && { target: null });
      },
      onClose: closePanel,
    },
    commentBlock,
  };
}
