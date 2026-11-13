// Où va chaque fil dans le panneau des commentaires, et ce que chacun permet à qui.
//
// L'ordre des fils suit celui des blocs dans le document affiché, en lecture seule comme en
// écriture : `order` donne, dans cet ordre, les blocs commentés qui sont dans la page (lus dans
// l'éditeur, `BlockMarks.tsx`). Un fil dont le bloc n'y est plus est rangé à part, sous « Bloc
// supprimé » : le serveur garde son identifiant de bloc, et le fil se lit toujours. Tant que
// le document n'est pas connu (`order` nul : éditeur pas encore synchronisé), aucun fil n'est
// dit d'un bloc supprimé ; ensuite, seul l'est le fil d'un bloc cherché dans la page et
// introuvable (`absent`). Le bloc d'un fil tout neuf, pas encore cherché, se range après les
// blocs connus, sans changer de liste : son fil ne passe pas sous « Bloc supprimé » le temps
// d'une image.
import type { Comment, Level, Thread } from "@mementum/protocol";
import { canComment, canEdit } from "../share/levels";

export type Arranged = {
  /** Les fils ouverts : ceux des blocs, dans l'ordre des blocs, puis ceux de la page. */
  open: Thread[];
  /** Les fils ouverts dont le bloc n'est plus dans la page. */
  orphans: Thread[];
  /** Les fils résolus, dans le même ordre : blocs, page, puis blocs supprimés. */
  resolved: Thread[];
  /** Le bloc du fil n'est plus dans la page. */
  deleted: (thread: Thread) => boolean;
};

export function arrangeThreads(
  threads: Thread[],
  order: readonly string[] | null,
  /** Les blocs cherchés dans la page et introuvables ; absent : tout bloc hors de `order`. */
  absent?: readonly string[],
): Arranged {
  const rank = new Map(order?.map((id, index) => [id, index]));
  const gone = absent === undefined ? null : new Set(absent);
  const deleted = (thread: Thread) =>
    order !== null &&
    thread.blockId !== null &&
    !rank.has(thread.blockId) &&
    (gone === null || gone.has(thread.blockId));
  /**
   * Rang d'un fil : un bloc par sa place (un bloc pas encore cherché, après les autres), puis la
   * page, puis les blocs supprimés.
   */
  const place = (thread: Thread) => {
    if (thread.blockId === null) return Number.MAX_SAFE_INTEGER - 1;
    if (order === null) return -1;
    if (deleted(thread)) return Number.MAX_SAFE_INTEGER;
    return rank.get(thread.blockId) ?? order.length;
  };
  // `sort` est stable : à rang égal, l'ordre du serveur (le plus ancien d'abord) reste.
  const sorted = [...threads].sort((a, b) => place(a) - place(b));
  const open = sorted.filter((t) => t.resolvedAt === null);
  return {
    open: open.filter((t) => !deleted(t)),
    orphans: open.filter(deleted),
    resolved: sorted.filter((t) => t.resolvedAt !== null),
    deleted,
  };
}

/** Le nombre de fils ouverts, ceux d'un bloc supprimé compris. */
export function openCount(threads: Thread[]): number {
  return threads.filter((t) => t.resolvedAt === null).length;
}

/**
 * Ce que la personne peut faire des fils (tableau des niveaux de R134). Le serveur fait foi :
 * un bouton qu'elle n'a pas le droit d'utiliser n'est pas montré, et un refus venu d'un droit
 * changé entre-temps s'affiche tel quel.
 */
export type CommentRights = {
  level: Level;
  /** Rien ne s'écrit : page à la corbeille, ou onglet d'un autre schéma que le serveur (R35). */
  locked: boolean;
  selfId: string;
};

/** Ouvrir un fil, répondre, réagir : `comment`, hors corbeille. */
export function canWrite(rights: CommentRights): boolean {
  return !rights.locked && canComment(rights.level);
}

/** Modifier un message : `comment`, et en être l'auteur. */
export function canEditMessage(rights: CommentRights, comment: Comment): boolean {
  return canWrite(rights) && comment.author.id === rights.selfId;
}

/** Supprimer un message : en être l'auteur avec `comment`, ou `full`. */
export function canDeleteMessage(rights: CommentRights, comment: Comment): boolean {
  if (rights.locked) return false;
  return (
    (canComment(rights.level) && comment.author.id === rights.selfId) || rights.level === "full"
  );
}

/**
 * Supprimer tout un fil, avec les messages de tous (R352) : `full`, hors corbeille et depuis un
 * onglet du schéma du serveur, qu'on ait ouvert le fil ou non.
 */
export function canDeleteThread(rights: CommentRights): boolean {
  return !rights.locked && rights.level === "full";
}

/** Résoudre ou rouvrir : avoir ouvert le fil avec `comment`, ou `edit`. */
export function canResolve(rights: CommentRights, thread: Thread): boolean {
  if (rights.locked) return false;
  return (
    (canComment(rights.level) && thread.openedBy.id === rights.selfId) || canEdit(rights.level)
  );
}
