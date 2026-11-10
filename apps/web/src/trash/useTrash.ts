// La liste de la corbeille d'un espace, telle que le serveur la donne (`GET
// /api/v1/spaces/{id}/trash`). Elle est lue à l'ouverture de la vue, relue après chacune de
// ses actions (`reload`), et quand une page est jetée ou restaurée ailleurs dans
// l'application (`version`).
//
// Deux relectures peuvent se suivre de près : seule la réponse de la plus récente compte, une
// plus ancienne qui arrive après elle est jetée. Une relecture qui échoue garde la liste déjà
// lue à l'écran, avec le message du serveur, que la relecture suivante qui réussit efface.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState } from "react";
import { getTrash, isSessionLost } from "../api";
import { messageOf } from "../session/forms";

export type TrashList = {
  /** Les pages à la corbeille, la plus récente d'abord ; `null` : pas encore lues. */
  pages: PageNode[] | null;
  /** La dernière lecture a échoué : le message du serveur, en français. */
  error: string | null;
  /** Relit la liste. La promesse est tenue quand cette lecture est finie, même dépassée. */
  reload: () => Promise<void>;
};

export function useTrash(spaceId: string, version: number): TrashList {
  const [pages, setPages] = useState<PageNode[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Le rang de la dernière lecture lancée : la réponse d'une plus ancienne est jetée. */
  const reads = useRef(0);

  const reload = useCallback(async () => {
    const read = ++reads.current;
    try {
      const list = await getTrash(spaceId);
      if (read !== reads.current) return;
      setPages(list);
      setError(null);
    } catch (e) {
      // Session perdue : la connexion est demandée par-dessus ; la liste garde ce qu'elle a.
      if (read === reads.current && !isSessionLost(e)) setError(messageOf(e));
    }
  }, [spaceId]);

  useEffect(() => {
    void reload();
    // La vue fermée, ou une autre lecture demandée : la réponse en route ne compte plus.
    return () => {
      reads.current++;
    };
  }, [reload, version]);

  return { pages, error, reload };
}

const DAY_AND_TIME = { day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" } as const;
const sameYear = new Intl.DateTimeFormat("fr-FR", DAY_AND_TIME);
const otherYear = new Intl.DateTimeFormat("fr-FR", { ...DAY_AND_TIME, year: "numeric" });

/**
 * La date d'une mise à la corbeille (`at`, en millisecondes), en clair : « 15 mars à 14:05 ».
 * Elle porte l'année quand ce n'est pas celle de `now` : « 15 mars 2025 à 14:05 ».
 */
export function trashedOnLabel(at: number, now: number = Date.now()): string {
  const date = new Date(at);
  const format = date.getFullYear() === new Date(now).getFullYear() ? sameYear : otherYear;
  return format.format(date);
}
