// Les brouillons des commentaires d'une page ouverte : ce qu'on a tapé et pas encore envoyé, par
// cible (le commentaire de la page, celui d'un bloc, la réponse à un fil, la modification d'un
// message). Ils vivent là où le panneau est câblé (`useCommentsPanel`), hors des composants qui
// se démontent : fermer puis rouvrir le panneau, cliquer une pastille, relancer « Commenter » les
// retrouve. Un brouillon part quand son texte est envoyé, ou explicitement annulé. En mémoire
// seulement : rien dans le stockage du navigateur.
//
// Un brouillon dont la cible n'existe plus après une relecture des fils (le fil, ou le message
// qu'on modifiait, a été supprimé entre-temps) n'est pas jeté en silence : il sort des brouillons
// et devient « orphelin », que le panneau montre, avec son texte, jusqu'à ce qu'on l'écarte.
import type { Thread } from "@mementum/protocol";

/** Un brouillon dont la cible a disparu : le fil d'une réponse, ou le message d'une modification. */
export type OrphanDraft = { key: string; kind: "thread" | "message"; text: string };

export type Drafts = {
  /** Le texte du brouillon ; `""` s'il n'y en a pas. */
  get(key: string): string;
  /** Garde le texte ; un texte vide retire le brouillon. */
  set(key: string, text: string): void;
  drop(key: string): void;
  has(key: string): boolean;
  /**
   * Après une relecture des fils : les brouillons dont le fil ou le message n'est plus dans
   * `threads` deviennent orphelins. Rend vrai si la liste des orphelins a changé.
   */
  settle(threads: Thread[]): boolean;
  /** Les orphelins, dans l'ordre où ils le sont devenus. */
  orphans(): OrphanDraft[];
  /** Écarte un orphelin : son texte part pour de bon. */
  dismiss(key: string): void;
};

const REPLY = "reply:";
const EDIT = "edit:";

/** Les cibles d'un brouillon. */
export const DRAFT = {
  page: "page",
  block: (blockId: string) => `block:${blockId}`,
  reply: (threadId: string) => `${REPLY}${threadId}`,
  edit: (commentId: string) => `${EDIT}${commentId}`,
};

export function createDrafts(): Drafts {
  const texts = new Map<string, string>();
  const orphaned: OrphanDraft[] = [];
  return {
    get: (key) => texts.get(key) ?? "",
    set: (key, text) => {
      if (text === "") texts.delete(key);
      else texts.set(key, text);
    },
    drop: (key) => {
      texts.delete(key);
    },
    has: (key) => texts.has(key),
    settle: (threads) => {
      const threadIds = new Set(threads.map((t) => t.id));
      const commentIds = new Set(threads.flatMap((t) => t.comments.map((c) => c.id)));
      let changed = false;
      for (const [key, text] of [...texts]) {
        const gone = key.startsWith(REPLY)
          ? !threadIds.has(key.slice(REPLY.length)) && "thread"
          : key.startsWith(EDIT)
            ? !commentIds.has(key.slice(EDIT.length)) && "message"
            : false;
        if (gone === false) continue;
        texts.delete(key);
        orphaned.push({ key, kind: gone, text });
        changed = true;
      }
      return changed;
    },
    orphans: () => [...orphaned],
    dismiss: (key) => {
      const at = orphaned.findIndex((orphan) => orphan.key === key);
      if (at !== -1) orphaned.splice(at, 1);
    },
  };
}
