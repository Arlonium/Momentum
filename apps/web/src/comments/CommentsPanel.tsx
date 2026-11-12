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
// C'est la page qui rend le focus où il était (`PageView`).
import type { Level } from "@mementum/protocol";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { CloseIcon } from "../icons";
import { arrangeThreads, canWrite, type CommentRights } from "./arrange";
import { Composer } from "./Composer";
import { ThreadView } from "./ThreadView";
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
  level: Level;
  trashed: boolean;
  /** L'onglet n'a plus le schéma du serveur (R35) : rien ne s'écrit d'ici. */
  readOnly: boolean;
  /** Le compte de la session. */
  selfId: string;
  target: PanelTarget | null;
  /** Le début du texte d'un bloc, montré au-dessus de son commentaire neuf. */
  excerpt: (blockId: string) => string | null;
  /** Fait défiler la page jusqu'au bloc. */
  onShowBlock: (blockId: string) => void;
  /** Le commentaire neuf d'un bloc est parti, ou abandonné. */
  onTargetDone: () => void;
  onClose: () => void;
};

/** « il y a… » se remet à jour à ce rythme, tant que le panneau est ouvert. */
const CLOCK_MS = 30_000;

function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => clearInterval(timer);
  }, []);
  return now;
}

/** Une valeur d'attribut entre guillemets dans un sélecteur CSS. */
export function cssString(value: string): string {
  return `"${value.replace(/["\\]/g, "\\$&").replace(/[\u0000-\u001f\u007f]/g, (c) => `\\${c.charCodeAt(0).toString(16)} `)}"`;
}

export function CommentsPanel({
  id,
  comments,
  order,
  level,
  trashed,
  readOnly,
  selfId,
  target,
  excerpt,
  onShowBlock,
  onTargetDone,
  onClose,
}: Props) {
  const panelRef = useRef<HTMLElement>(null);
  const titleId = useId();
  const deletedId = useId();
  const resolvedId = useId();
  const [showResolved, setShowResolved] = useState(false);
  const now = useNow();
  const rights: CommentRights = { level, locked: trashed || readOnly, selfId };
  const write = canWrite(rights);
  const { threads } = comments;
  const arranged = useMemo(() => threads && arrangeThreads(threads, order), [threads, order]);

  // Le focus, à l'ouverture et à chaque demande : le fil choisi, ou le panneau lui-même. La
  // zone d'un commentaire de bloc le prend d'elle-même.
  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    if (target?.kind === "block" && panel.querySelector("textarea")) return;
    if (target?.kind === "thread") {
      const chosen = panel.querySelector<HTMLElement>(`[data-thread-id=${cssString(target.threadId)}]`);
      if (chosen) {
        chosen.scrollIntoView?.({ block: "nearest" });
        chosen.focus({ preventScroll: true });
        return;
      }
    }
    panel.focus({ preventScroll: true });
  }, [target]);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Escape") return;
    // Une fenêtre de confirmation (suppression d'un message) a son propre Échap.
    if ((event.target as Element).closest("dialog")) return;
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
        />
      </li>
    );
  };

  const quote = target?.kind === "block" ? excerpt(target.blockId) : null;

  return (
    <aside
      ref={panelRef}
      id={id}
      className="mm-comments"
      aria-labelledby={titleId}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div className="mm-comments-head">
        <h2 id={titleId} className="mm-comments-title">
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
      {write && target?.kind === "block" && (
        <div className="mm-comments-new">
          <p className="mm-comments-quote-label">Sur le bloc :</p>
          {quote && (
            <blockquote className="mm-comments-quote mm-isolate" dir="auto">
              {quote}
            </blockquote>
          )}
          <Composer
            key={target.seq}
            label="Commenter le bloc"
            submitLabel="Commenter"
            submitName="Commenter le bloc"
            autoFocus
            onSubmit={async (body) => {
              await comments.open(target.blockId, body);
              onTargetDone();
            }}
            onCancel={onTargetDone}
          />
        </div>
      )}
      {write && (
        <div className="mm-comments-page">
          <Composer
            label="Commenter la page"
            submitLabel="Commenter"
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
