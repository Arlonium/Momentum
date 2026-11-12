// Des fils de commentaires pour les tests unitaires.
import type { Author, Comment, Thread } from "@mementum/protocol";

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
