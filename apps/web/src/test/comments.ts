// Des fils de commentaires pour les tests unitaires.
import type { Author, Comment, Thread } from "@mementum/protocol";
import { useMemo, useRef, useState } from "react";
import type { Comments } from "../comments/useComments";

export const author = (id: string, name = id.toUpperCase()): Author => ({ id, name });

/** Un message d'Alice, sans réaction ni modification. */
export const message = (id: string, overrides: Partial<Comment> = {}): Comment => ({
  id,
  author: author("alice"),
  body: `texte ${id}`,
  createdAt: 1_000,
  editedAt: null,
  reactions: [],
  ...overrides,
});

/** Un fil ouvert de la page (sans bloc), ouvert par Alice, avec un message. */
export const thread = (id: string, overrides: Partial<Thread> = {}): Thread => ({
  id,
  blockId: null,
  openedBy: author("alice"),
  createdAt: 1_000,
  resolvedAt: null,
  resolvedBy: null,
  comments: [message(`${id}-1`)],
  ...overrides,
});

/**
 * Des fils gardés en mémoire, que chaque écriture change avant de répondre, comme le fait
 * `useComments` (le résultat d'une écriture s'affiche aussitôt) : de quoi voir ce que devient
 * l'écran après une action. `selfId` : le compte qui écrit.
 */
export function useMemoryComments(initial: Thread[], selfId = "alice"): Comments {
  const [threads, setThreads] = useState(initial);
  const ids = useRef(0);
  return useMemo(() => {
    const mapComment = (threadId: string, commentId: string, change: (c: Comment) => Comment) =>
      setThreads((all) =>
        all.map((t) =>
          t.id === threadId
            ? { ...t, comments: t.comments.map((c) => (c.id === commentId ? change(c) : c)) }
            : t,
        ),
      );
    return {
      threads,
      state: "ready",
      error: null,
      open: async (blockId, body) => {
        const n = ++ids.current;
        const created = thread(`neuf-${n}`, {
          blockId,
          openedBy: author(selfId),
          comments: [message(`neuf-${n}-1`, { author: author(selfId), body })],
        });
        setThreads((all) => [...all, created]);
        return created;
      },
      reply: async (threadId, body) => {
        const created = message(`réponse-${++ids.current}`, { author: author(selfId), body });
        setThreads((all) =>
          all.map((t) => (t.id === threadId ? { ...t, comments: [...t.comments, created] } : t)),
        );
        return created;
      },
      edit: async (threadId, commentId, body) => {
        mapComment(threadId, commentId, (c) => ({ ...c, body, editedAt: 2_000 }));
        return message(commentId, { body });
      },
      remove: async (threadId, commentId) => {
        setThreads((all) =>
          all.flatMap((t) => {
            if (t.id !== threadId) return [t];
            const comments = t.comments.filter((c) => c.id !== commentId);
            return comments.length === 0 ? [] : [{ ...t, comments }];
          }),
        );
      },
      removeThread: async (threadId) => {
        setThreads((all) => all.filter((t) => t.id !== threadId));
      },
      resolve: async (threadId, resolved) => {
        setThreads((all) =>
          all.map((t) =>
            t.id === threadId
              ? { ...t, resolvedAt: resolved ? 2_000 : null, resolvedBy: resolved ? author(selfId) : null }
              : t,
          ),
        );
        return thread(threadId);
      },
      react: async (threadId, commentId, emoji, on) => {
        mapComment(threadId, commentId, (c) => {
          const others = c.reactions.filter((r) => r.emoji !== emoji);
          const users = (c.reactions.find((r) => r.emoji === emoji)?.users ?? []).filter(
            (u) => u.id !== selfId,
          );
          const next = on ? [...users, author(selfId)] : users;
          return {
            ...c,
            reactions: next.length === 0 ? others : [...others, { emoji, users: next }],
          };
        });
        return message(commentId);
      },
      reload: () => {},
    } satisfies Comments;
  }, [threads, selfId]);
}
