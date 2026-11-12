// Le panneau des commentaires d'une page (conception §4.2, R134), à droite de la page : 320 px,
// par-dessus la page sous 1 100 px de large (`theme.css`). De haut en bas :
// 1. le commentaire d'un bloc qu'on rédige, quand on vient de choisir « Commenter » sur lui ;
// 2. rédiger un commentaire de la page ;
// 3. les fils ouverts, dans l'ordre des blocs dans la page, puis les fils de la page ;
// 4. « Bloc supprimé » : les fils ouverts dont le bloc n'est plus là, toujours lisibles ;
// 5. « Fils résolus (N) », qui montre ou cache les fils résolus.
//
// Rien n'est écrit dans le document : sous « Peut commenter », la personne lit la page sans la
// modifier, et commente. Sous ce niveau, ni zone de rédaction, ni « Répondre », ni réaction
// cliquable : les fils se lisent. À la corbeille, tout se lit et rien ne s'écrit. Un onglet
// d'un autre schéma que le serveur (R35) n'écrit rien non plus, comme pour le menu Partager.
//
// Au clavier : le panneau prend le focus à son ouverture (sur le fil choisi, ou sur la zone du
// bloc à commenter) ; Échap le ferme, sauf depuis une fenêtre de confirmation, qui a le sien.
// C'est la page qui rend le focus où il était (`useCommentsPanel`). Après chaque action, le focus
// ne tombe jamais sur `body` : l'élément qui l'avait (une zone de rédaction refermée, un bouton
// de réaction parti, un fil qui change de liste) le rend à ce qui suit l'action dans le fil, au
// fil lui-même, ou au titre du panneau.
import type { Level } from "@mementum/protocol";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { CloseIcon } from "../icons";
import { arrangeThreads, canWrite, type CommentRights } from "./arrange";
import { cssString } from "./dom";
import { Composer } from "./Composer";
import { createDrafts, DRAFT, type Drafts, type OrphanDraft } from "./drafts";
import { ThreadView } from "./ThreadView";
import { useNow } from "./time";
import type { Comments } from "./useComments";

/**
 * Ce sur quoi le panneau s'ouvre : un fil (une pastille de la marge), ou le commentaire neuf d'un
 * bloc (« Commenter »). `seq` change à chaque demande, même pour le même fil ou le même bloc.
 */
export type PanelTarget =
  | { kind: "thread"; threadId: string; seq: number }
  | { kind: "block"; blockId: string; seq: number };

type Props = {
  id: string;
  comments: Comments;
  /** Les blocs commentés qui sont dans la page, dans l'ordre du document ; `null` : pas encore lu. */
  order: readonly string[] | null;
  /**
   * Les blocs cherchés dans la page et introuvables : leurs fils vont sous « Bloc supprimé ».
   * Absent : tout bloc hors de `order`.
   */
  absent?: readonly string[];
  level: Level;
  trashed: boolean;
  /** L'onglet n'a plus le schéma du serveur (R35) : rien ne s'écrit d'ici. */
  readOnly: boolean;
  /** Le compte de la session. */
  selfId: string;
  target: PanelTarget | null;
  /**
   * Les brouillons de la page, qui survivent au panneau (`useCommentsPanel`). Sans eux, un
   * brouillon ne vit que le temps du panneau.
   */
  drafts?: Drafts;
  /**
   * Le bloc dont on rédige le commentaire neuf ; il le reste quand le panneau va à un fil ou se
   * ferme, jusqu'à ce que le commentaire parte ou soit annulé. Absent : le bloc de `target`.
   */
  composing?: string | null;
  /**
   * Les brouillons dont le fil ou le message a disparu (supprimé entre-temps) : montrés en haut du
   * panneau, avec leur texte, jusqu'à ce qu'on les écarte.
   */
  orphans?: OrphanDraft[];
  onDismissOrphan?: (key: string) => void;
  /** Le début du texte d'un bloc, montré au-dessus de son commentaire neuf. */
  excerpt: (blockId: string) => string | null;
  /** Fait défiler la page jusqu'au bloc. */
  onShowBlock: (blockId: string) => void;
  /** Le commentaire neuf d'un bloc est parti, ou abandonné. */
  onTargetDone: () => void;
  onClose: () => void;
};

export function CommentsPanel({
  id,
  comments,
  order,
  absent,
  level,
  trashed,
  readOnly,
  selfId,
  target,
  drafts: givenDrafts,
  composing: givenComposing,
  orphans = [],
  onDismissOrphan = () => {},
  excerpt,
  onShowBlock,
  onTargetDone,
  onClose,
}: Props) {
  const panelRef = useRef<HTMLElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const titleId = useId();
  const deletedId = useId();
  const resolvedId = useId();
  const [showResolved, setShowResolved] = useState(false);
  const now = useNow();
  const rights: CommentRights = { level, locked: trashed || readOnly, selfId };
  const [ownDrafts] = useState(createDrafts);
  const drafts = givenDrafts ?? ownDrafts;
  const targetBlock = target?.kind === "block" ? target.blockId : null;
  const composing = givenComposing === undefined ? targetBlock : givenComposing;
  const write = canWrite(rights);
  const { threads } = comments;
  const arranged = useMemo(
    () => threads && arrangeThreads(threads, order, absent),
    [threads, order, absent],
  );

  // Le focus, à l'ouverture et à chaque demande : le fil choisi, ou le panneau lui-même. La
  // zone d'un commentaire de bloc le prend d'elle-même. Une demande qui s'achève (le commentaire
  // d'un bloc envoyé ou abandonné) ne le déplace pas : c'est `moveFocus` qui s'en charge.
  const opened = useRef(false);
  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    const first = !opened.current;
    opened.current = true;
    if (target === null && !first) return;
    if (target?.kind === "block") {
      // Le champ du bloc, neuf ou déjà là (un second « Commenter » sur le même bloc).
      const field = panel.querySelector<HTMLTextAreaElement>(".mm-comments-new textarea");
      if (field) {
        field.focus();
        field.setSelectionRange(field.value.length, field.value.length);
        return;
      }
    }
    if (target?.kind === "thread") {
      const chosen = panel.querySelector<HTMLElement>(
        `[data-thread-id=${cssString(target.threadId)}]`,
      );
      if (chosen) {
        chosen.scrollIntoView?.({ block: "nearest" });
        chosen.focus({ preventScroll: true });
        return;
      }
    }
    panel.focus({ preventScroll: true });
  }, [target]);

  /** L'élément du panneau qui a le focus, tant qu'aucun geste ne l'a emmené ailleurs. */
  const focused = useRef<HTMLElement | null>(null);
  /**
   * Où le filet (plus bas) a mis le focus faute de mieux (un fil, le titre), tant que rien
   * d'autre ne l'a pris : une demande plus précise peut l'y reprendre. Jamais un champ où l'on
   * tape : une demande née d'une action passée ne prend pas le focus à qui écrit.
   */
  const caught = useRef<HTMLElement | null>(null);

  /**
   * Le focus a pu tomber (l'élément qui l'avait a quitté la page) : il va, dans le fil
   * `threadId`, à l'élément marqué `data-focus="<within>"`, sinon au fil, sinon au titre du
   * panneau. S'il est encore quelque part, rien ne bouge.
   */
  const [refocus, setRefocus] = useState<{
    threadId: string | null;
    within: string | null;
    seq: number;
  } | null>(null);
  const refocusSeq = useRef(0);
  const moveFocus = useCallback(
    (threadId: string | null, within: string | null) =>
      setRefocus({ threadId, within, seq: ++refocusSeq.current }),
    [],
  );
  useLayoutEffect(() => {
    if (refocus === null) return;
    const panel = panelRef.current;
    if (!panel) return;
    const thread =
      refocus.threadId === null
        ? null
        : panel.querySelector<HTMLElement>(`[data-thread-id=${cssString(refocus.threadId)}]`);
    // Le focus est encore quelque part : rien ne bouge, sauf si c'est le filet ci-dessous qui l'y
    // a mis, faute de mieux : on précise.
    const active = document.activeElement;
    const fallen = active === null || active === document.body || !active.isConnected;
    if (!fallen && active !== caught.current) return;
    const inside =
      refocus.within === null
        ? null
        : thread?.querySelector<HTMLElement>(`[data-focus=${cssString(refocus.within)}]`);
    (inside ?? thread ?? titleRef.current)?.focus();
  }, [refocus]);

  // Filet : un rendu a retiré l'élément du panneau qui avait le focus (un fil qui change de
  // liste est recréé), sans que le focus soit parti ailleurs par un geste (un `blur` l'aurait
  // dit). Si c'était un champ où l'on tapait, et qu'il est revenu (son brouillon le rouvre), le
  // focus y retourne, le curseur à sa place ; sinon, il va au fil, là où il est maintenant, ou
  // au titre du panneau.
  useLayoutEffect(() => {
    const lost = focused.current;
    if (lost === null || lost.isConnected) return;
    focused.current = null;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    const draftKey = lost.getAttribute("data-draft");
    const same =
      draftKey === null
        ? null
        : panelRef.current?.querySelector<HTMLTextAreaElement>(
            `textarea[data-draft=${cssString(draftKey)}]`,
          );
    if (same) {
      const at = lost instanceof HTMLTextAreaElement ? lost.selectionEnd : same.value.length;
      same.focus();
      same.setSelectionRange(at, at);
      // La personne tapait là : ce n'est pas un repli, rien ne le lui reprend.
      caught.current = null;
      return;
    }
    const threadId = lost.closest("[data-thread-id]")?.getAttribute("data-thread-id");
    const thread =
      threadId == null
        ? null
        : panelRef.current?.querySelector<HTMLElement>(`[data-thread-id=${cssString(threadId)}]`);
    const target = thread ?? titleRef.current;
    target?.focus();
    caught.current = target;
  });

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Escape") return;
    // Une fenêtre de confirmation (suppression d'un message) a son propre Échap.
    if ((event.target as Element).closest("dialog")) return;
    // Échap ne détruit jamais un texte tapé : dans un champ qui en a, il sort du champ.
    if (event.target instanceof HTMLTextAreaElement && event.target.value !== "") {
      event.preventDefault();
      event.stopPropagation();
      panelRef.current?.focus();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    onClose();
  };

  const show = (blockId: string | null, deleted: boolean) =>
    blockId !== null && order !== null && !deleted ? () => onShowBlock(blockId) : null;
  const threadView = (thread: NonNullable<typeof threads>[number]) => {
    const deleted = arranged?.deleted(thread) ?? false;
    return (
      <li key={thread.id}>
        <ThreadView
          thread={thread}
          rights={rights}
          now={now}
          deleted={deleted}
          onShowBlock={show(thread.blockId, deleted)}
          comments={comments}
          drafts={drafts}
          onFocusMove={(within) => moveFocus(thread.id, within)}
        />
      </li>
    );
  };

  const quote = composing === null ? null : excerpt(composing);

  return (
    <aside
      ref={panelRef}
      id={id}
      className="mm-comments"
      aria-labelledby={titleId}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onFocus={(event) => {
        focused.current = event.target as HTMLElement;
        if (event.target !== caught.current) caught.current = null;
      }}
      onBlur={(event) => {
        // Le focus part par un geste, hors du panneau : ce n'est pas un élément retiré.
        const next = event.relatedTarget;
        if (!(next instanceof Node) || !event.currentTarget.contains(next)) focused.current = null;
      }}
    >
      <div className="mm-comments-head">
        <h2 ref={titleRef} id={titleId} className="mm-comments-title" tabIndex={-1}>
          Commentaires
        </h2>
        <button
          type="button"
          className="mm-icon-button"
          aria-label="Fermer les commentaires"
          title="Fermer (Échap)"
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </div>
      {orphans.length > 0 && (
        <section className="mm-comments-orphans" aria-label="Textes non envoyés">
          <ul>
            {orphans.map((orphan) => (
              <li key={orphan.key}>
                <p className="mm-comments-orphan-lead">
                  {orphan.kind === "thread"
                    ? "Ce fil a été supprimé. Ton texte :"
                    : "Ce message a été supprimé. Ton texte :"}
                </p>
                <p className="mm-comments-orphan-text mm-isolate" dir="auto">
                  {orphan.text}
                </p>
                <button
                  type="button"
                  className="mm-button mm-button-small"
                  aria-label="Écarter ce texte"
                  onClick={() => onDismissOrphan(orphan.key)}
                >
                  Écarter
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {comments.state === "failed" && (
        <div className="mm-comments-failed">
          <p className="mm-form-error" role="alert">
            {comments.error}
          </p>
          <button type="button" className="mm-button mm-button-small" onClick={comments.reload}>
            Réessayer
          </button>
        </div>
      )}
      {write && composing !== null && (
        <div className="mm-comments-new">
          <p className="mm-comments-quote-label">Sur le bloc :</p>
          {quote && (
            <blockquote className="mm-comments-quote mm-isolate" dir="auto">
              {quote}
            </blockquote>
          )}
          <Composer
            // Un champ par bloc : le même bloc garde le sien, et son brouillon.
            key={composing}
            label="Commenter le bloc"
            submitLabel="Commenter"
            submitName="Commenter le bloc"
            draft={{ store: drafts, key: DRAFT.block(composing) }}
            onSubmit={async (body) => {
              const created = await comments.open(composing, body);
              onTargetDone();
              moveFocus(created.id, null);
            }}
            onCancel={() => {
              onTargetDone();
              moveFocus(null, null);
            }}
          />
        </div>
      )}
      {write && (
        <div className="mm-comments-page">
          <Composer
            label="Commenter la page"
            submitLabel="Commenter"
            draft={{ store: drafts, key: DRAFT.page }}
            onSubmit={async (body) => {
              await comments.open(null, body);
            }}
          />
        </div>
      )}
      {arranged === null ? (
        <p className="mm-comments-empty">Chargement…</p>
      ) : (
        <>
          {arranged.open.length === 0 && arranged.orphans.length === 0 && (
            <p className="mm-comments-empty">Aucun fil ouvert.</p>
          )}
          {arranged.open.length > 0 && (
            <ul className="mm-threads" aria-label="Fils ouverts">
              {arranged.open.map(threadView)}
            </ul>
          )}
          {arranged.orphans.length > 0 && (
            <section className="mm-comments-section" aria-labelledby={deletedId}>
              <h3 id={deletedId} className="mm-comments-subtitle">
                Bloc supprimé
              </h3>
              <ul className="mm-threads" aria-label="Fils d'un bloc supprimé">
                {arranged.orphans.map(threadView)}
              </ul>
            </section>
          )}
          {arranged.resolved.length > 0 && (
            <div className="mm-comments-section">
              <button
                type="button"
                className="mm-link-button"
                aria-expanded={showResolved}
                aria-controls={showResolved ? resolvedId : undefined}
                onClick={() => setShowResolved((was) => !was)}
              >
                Fils résolus ({arranged.resolved.length})
              </button>
              {showResolved && (
                <ul id={resolvedId} className="mm-threads" aria-label="Fils résolus">
                  {arranged.resolved.map(threadView)}
                </ul>
              )}
            </div>
          )}
        </>
      )}
    </aside>
  );
}
