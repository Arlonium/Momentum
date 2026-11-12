// Un fil de commentaires dans le panneau : où il est attaché (un bloc, qu'un bouton fait voir
// dans la page ; la page ; un bloc supprimé), ses messages (auteur, « il y a… », « modifié »),
// leurs réactions, puis « Répondre » et « Résoudre » (ou « Rouvrir »).
//
// Chaque bouton suit le tableau des niveaux de R134 (`arrange.ts`) : celui que la personne ne
// peut pas utiliser n'est pas montré. Si le serveur refuse quand même (un droit a changé
// entre-temps), son message s'affiche tel quel dans le fil, et le panneau se relit
// (`useComments`).
//
// Tout ce qui vient d'autres comptes (le corps d'un message, un nom, l'emoji d'une réaction) est
// affiché comme du texte, jamais comme du HTML ni dans un attribut `style`, chacun dans son
// élément isolé (`dir="auto"`, `unicode-bidi: isolate` : un caractère de format Unicode n'en
// sort pas). Les six réactions proposées sont des constantes exactes ; une réaction qu'un autre
// client a posée s'affiche telle quelle, et se pose ou se retire comme les autres.
import type { Comment, Thread } from "@mementum/protocol";
import { useState, type MouseEvent, type ReactNode } from "react";
import { isSessionLost } from "../api";
import { ReactIcon } from "../icons";
import { inkOn } from "../page/avatarInk";
import { messageOf } from "../session/forms";
import { colorOf } from "../session/session";
import { Confirm } from "../ui/Confirm";
import {
  canDeleteMessage,
  canEditMessage,
  canResolve,
  canWrite,
  type CommentRights,
} from "./arrange";
import { Composer } from "./Composer";
import { isolate } from "./dom";
import { DRAFT, type Drafts } from "./drafts";
import { fullDate, relativeTime } from "./time";
import type { Comments } from "./useComments";

/**
 * Les réactions proposées, telles quelles : pour le serveur, deux chaînes différentes sont deux
 * réactions (❤️ porte son sélecteur de variante, U+FE0F).
 */
export const REACTIONS = ["👍", "❤️", "😄", "🎉", "👀", "✅"] as const;

type Props = {
  thread: Thread;
  rights: CommentRights;
  /** L'instant présent (millisecondes Unix), pour « il y a… ». */
  now: number;
  /** Le bloc du fil est dans la page : y mener. `null` : fil de la page, ou bloc supprimé. */
  onShowBlock: (() => void) | null;
  /** Le bloc du fil n'est plus dans la page. */
  deleted: boolean;
  comments: Pick<Comments, "reply" | "edit" | "remove" | "resolve" | "react">;
  /** Les brouillons de la page : une réponse ou une modification en cours se retrouvent. */
  drafts: Drafts;
  /**
   * Après une action, l'élément qui avait le focus a pu quitter la page : le panneau le rend à
   * l'élément du fil marqué `data-focus="<within>"`, sinon au fil, sinon à son titre (le fil
   * a quitté la liste affichée). Rien ne change si le focus est encore quelque part.
   */
  onFocusMove: (within: string | null) => void;
};

/** Les éléments d'un fil où le focus revient après une action (`data-focus`). */
export const FOCUS = {
  reply: "reply",
  edit: (commentId: string) => `edit:${commentId}`,
  react: (commentId: string) => `react:${commentId}`,
};

export function ThreadView({
  thread,
  rights,
  now,
  onShowBlock,
  deleted,
  comments,
  drafts,
  onFocusMove,
}: Props) {
  // Une réponse ou une modification qui a un brouillon reprend où elle en était.
  const [replying, setReplying] = useState(() => drafts.has(DRAFT.reply(thread.id)));
  const [editing, setEditing] = useState<string | null>(
    () => thread.comments.find((c) => drafts.has(DRAFT.edit(c.id)))?.id ?? null,
  );
  /** Le message dont on confirme la suppression. */
  const [deleting, setDeleting] = useState<Comment | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const resolved = thread.resolvedAt !== null;
  const write = canWrite(rights);
  const resolvable = canResolve(rights, thread);

  /**
   * Une action du fil : son refus s'affiche tel quel (sauf une session perdue) ; ensuite, le
   * focus revient à `within` s'il est tombé (l'élément qui l'avait a pu partir).
   */
  const run = async (action: () => Promise<unknown>, within: string | null) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      if (!isSessionLost(e)) setError(messageOf(e));
    } finally {
      setBusy(false);
      onFocusMove(within);
    }
  };

  // Un clic dans le fil, hors de ses commandes et d'un texte qu'on sélectionne, mène au bloc.
  const onClick = (event: MouseEvent<HTMLElement>) => {
    if (onShowBlock === null) return;
    const target = event.target as Element;
    if (target.closest("button, a, textarea, input, form, [role='group'], dialog")) return;
    if (!(window.getSelection()?.isCollapsed ?? true)) return;
    onShowBlock();
  };

  return (
    <article
      className={`mm-thread${resolved ? " is-resolved" : ""}`}
      data-thread-id={thread.id}
      tabIndex={-1}
      aria-label={`Fil ouvert par ${isolate(thread.openedBy.name)}`}
      onClick={onClick}
    >
      <div className="mm-thread-context">
        {thread.blockId === null ? (
          <span>Sur la page</span>
        ) : deleted ? (
          <span className="mm-tag">Bloc supprimé</span>
        ) : onShowBlock !== null ? (
          <button type="button" className="mm-link-button" onClick={onShowBlock}>
            Aller au bloc
          </button>
        ) : (
          <span>Sur un bloc</span>
        )}
        {resolved && thread.resolvedBy && (
          <span className="mm-thread-resolved">
            Résolu par{" "}
            <span className="mm-isolate" dir="auto">
              {thread.resolvedBy.name}
            </span>
            , {relativeTime(thread.resolvedAt!, now)}
          </span>
        )}
      </div>
      <ul className="mm-thread-messages" aria-label="Messages">
        {thread.comments.map((comment) => (
          <li key={comment.id} className="mm-comment">
            <MessageHead comment={comment} now={now} />
            {editing === comment.id ? (
              <Composer
                label="Modifier le message"
                submitLabel="Enregistrer"
                submitName="Enregistrer le message"
                initial={comment.body}
                draft={{ store: drafts, key: DRAFT.edit(comment.id) }}
                autoFocus
                clearOnSent={false}
                onSubmit={async (body) => {
                  await comments.edit(thread.id, comment.id, body);
                  setEditing(null);
                  onFocusMove(FOCUS.edit(comment.id));
                }}
                onCancel={() => {
                  setEditing(null);
                  onFocusMove(FOCUS.edit(comment.id));
                }}
              />
            ) : (
              <p className="mm-comment-body mm-isolate" dir="auto">
                {comment.body}
              </p>
            )}
            <Reactions
              comment={comment}
              selfId={rights.selfId}
              canReact={write}
              onToggle={(emoji, on) =>
                void run(
                  () => comments.react(thread.id, comment.id, emoji, on),
                  FOCUS.react(comment.id),
                )
              }
              onPicked={() => onFocusMove(FOCUS.react(comment.id))}
            >
              {editing !== comment.id &&
                (canEditMessage(rights, comment) || canDeleteMessage(rights, comment)) && (
                  <div className="mm-comment-actions">
                    {canEditMessage(rights, comment) && (
                      <button
                        type="button"
                        className="mm-button mm-button-quiet"
                        aria-label="Modifier ce message"
                        data-focus={FOCUS.edit(comment.id)}
                        onClick={() => setEditing(comment.id)}
                      >
                        Modifier
                      </button>
                    )}
                    {canDeleteMessage(rights, comment) && (
                      <button
                        type="button"
                        className="mm-button mm-button-quiet"
                        aria-label="Supprimer ce message"
                        onClick={() => setDeleting(comment)}
                      >
                        Supprimer
                      </button>
                    )}
                  </div>
                )}
            </Reactions>
          </li>
        ))}
      </ul>
      {error && (
        <p className="mm-form-error" role="alert">
          {error}
        </p>
      )}
      {((write && !replying) || resolvable) && (
        <div className="mm-thread-actions">
          {write && !replying && (
            <button
              type="button"
              className="mm-button mm-button-small"
              data-focus={FOCUS.reply}
              onClick={() => setReplying(true)}
            >
              Répondre
            </button>
          )}
          {resolvable && (
            <button
              type="button"
              className="mm-button mm-button-small"
              aria-disabled={busy || undefined}
              // Le fil quitte la liste où il était : le focus va au fil, là où il arrive, ou au
              // titre du panneau.
              onClick={() => void run(() => comments.resolve(thread.id, !resolved), null)}
            >
              {resolved ? "Rouvrir" : "Résoudre"}
            </button>
          )}
        </div>
      )}
      {replying && write && (
        <Composer
          label="Répondre au fil"
          submitLabel="Envoyer"
          submitName="Envoyer la réponse"
          draft={{ store: drafts, key: DRAFT.reply(thread.id) }}
          autoFocus
          onSubmit={async (body) => {
            await comments.reply(thread.id, body);
            setReplying(false);
            onFocusMove(FOCUS.reply);
          }}
          onCancel={() => {
            setReplying(false);
            onFocusMove(FOCUS.reply);
          }}
        />
      )}
      {deleting && (
        <Confirm
          // Espace insécable : le « ? » ne passe pas seul à la ligne.
          title={"Supprimer ce message ?"}
          confirmLabel="Supprimer"
          destructive
          busy={busy}
          // Le message parti, le focus va au fil, ou au titre du panneau si le fil est parti avec.
          onConfirm={() =>
            void run(() => comments.remove(thread.id, deleting.id), null).then(() =>
              setDeleting(null),
            )
          }
          onCancel={() => setDeleting(null)}
        >
          {thread.comments.length === 1
            ? "Le message part pour tout le monde, et le fil avec lui."
            : "Le message et ses réactions partent pour tout le monde."}
        </Confirm>
      )}
    </article>
  );
}

/** L'auteur d'un message, son âge et s'il a été modifié. */
function MessageHead({ comment, now }: { comment: Comment; now: number }) {
  const color = colorOf(comment.author.id);
  // L'initiale : la première lettre ou le premier chiffre du nom, pas un caractère de format.
  const initial = comment.author.name.match(/[\p{L}\p{N}]/u)?.[0]?.toLocaleUpperCase() ?? "?";
  return (
    <div className="mm-comment-head">
      <span
        className="mm-comment-avatar"
        aria-hidden="true"
        style={{ backgroundColor: color, color: inkOn(color) }}
      >
        {initial}
      </span>
      <span className="mm-comment-author mm-isolate" dir="auto">
        {comment.author.name}
      </span>
      <time
        className="mm-comment-time"
        dateTime={new Date(comment.createdAt).toISOString()}
        title={fullDate(comment.createdAt)}
      >
        {relativeTime(comment.createdAt, now)}
      </time>
      {comment.editedAt !== null && (
        <span className="mm-comment-edited" title={`Modifié ${fullDate(comment.editedAt)}`}>
          modifié
        </span>
      )}
    </div>
  );
}

type ReactionsProps = {
  comment: Comment;
  selfId: string;
  /** Les réactions se posent et se retirent d'ici ; sinon, elles se lisent seulement. */
  canReact: boolean;
  onToggle: (emoji: string, on: boolean) => void;
  /** Une réaction a été choisie dans le choix des six, qui se ferme. */
  onPicked: () => void;
  /** Les actions du message (« Modifier », « Supprimer »), sur la même ligne. */
  children?: ReactNode;
};

/**
 * Le pied d'un message : ses réactions, ses actions à droite, et dessous, pour qui peut réagir,
 * le choix des six.
 */
function Reactions({ comment, selfId, canReact, onToggle, onPicked, children }: ReactionsProps) {
  const [picking, setPicking] = useState(false);
  const shown = comment.reactions.filter((reaction) => reaction.users.length > 0);
  const hasActions = children !== undefined && children !== null && children !== false;
  if (shown.length === 0 && !canReact && !hasActions) return null;
  const mineOf = (emoji: string) =>
    comment.reactions.some((r) => r.emoji === emoji && r.users.some((u) => u.id === selfId));
  return (
    <div className="mm-comment-foot">
      <div className="mm-comment-foot-row">
        {(shown.length > 0 || canReact) && (
          <ul className="mm-reactions" aria-label="Réactions">
            {shown.map((reaction) => {
              const mine = reaction.users.some((user) => user.id === selfId);
              const count = reaction.users.length;
              // Les noms en infobulle : un attribut de texte, jamais de style.
              const names = reaction.users.map((user) => isolate(user.name)).join(", ");
              const content = (
                <>
                  <span className="mm-reaction-emoji mm-isolate" dir="auto">
                    {reaction.emoji}
                  </span>
                  <span className="mm-reaction-count">{count}</span>
                </>
              );
              return (
                <li key={reaction.emoji}>
                  {canReact ? (
                    <button
                      type="button"
                      className="mm-reaction"
                      aria-pressed={mine}
                      aria-label={`${isolate(reaction.emoji)} ${count}${mine ? ", dont toi" : ""}`}
                      title={names}
                      onClick={() => onToggle(reaction.emoji, !mine)}
                    >
                      {content}
                    </button>
                  ) : (
                    <span className="mm-reaction" title={names}>
                      {content}
                    </span>
                  )}
                </li>
              );
            })}
            {canReact && (
              <li>
                <button
                  type="button"
                  className="mm-reaction mm-reaction-add"
                  aria-label="Ajouter une réaction"
                  data-focus={FOCUS.react(comment.id)}
                  aria-expanded={picking}
                  onClick={() => setPicking((was) => !was)}
                >
                  <ReactIcon />
                </button>
              </li>
            )}
          </ul>
        )}
        {children}
      </div>
      {canReact && picking && (
        <div className="mm-reaction-picker" role="group" aria-label="Choisir une réaction">
          {REACTIONS.map((emoji) => {
            const mine = mineOf(emoji);
            return (
              <button
                key={emoji}
                type="button"
                className="mm-reaction"
                aria-label={`Réagir avec ${emoji}`}
                aria-pressed={mine}
                onClick={() => {
                  setPicking(false);
                  onPicked();
                  onToggle(emoji, !mine);
                }}
              >
                {emoji}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
