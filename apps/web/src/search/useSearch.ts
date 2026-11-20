// Ce que le serveur trouve dans un espace pour ce qui est tapé (lot 4, Task C1 ; `searchSpace`,
// `POST /api/v1/spaces/{id}/search`).
//
// Les règles :
// - rien n'est demandé pour un champ vide (ou fait d'espaces) ;
// - une requête part `SEARCH_DEBOUNCE_MS` après la dernière frappe. Le serveur ne mène qu'une
//   recherche à la fois par compte, et ne répond jamais avant 50 ms (R910, R932) : deux frappes
//   rapprochées ne font qu'une requête ;
// - dès que ce qui est tapé change, la requête en route est abandonnée (`AbortSignal`) et **sa
//   réponse, qui peut arriver quand même, est jetée**, refus compris. `429 search_busy` est la
//   réponse d'une recherche qu'une plus récente du compte a remplacée : jetée de même, jamais
//   rejouée. Les résultats rendus sont toujours ceux de la recherche du rendu, jamais ceux d'avant ;
// - un refus de la recherche courante est dit tel que le serveur le dit (`error`), et rien ne repart
//   de soi-même : la frappe suivante cherche de nouveau ;
// - la session perdue (`401`) : `api.ts` a demandé la connexion ; la recherche attend le retour du
//   compte (`afterLogin`), puis repart d'elle-même si c'est toujours elle qui est tapée ;
// - la suite (`loadMore`) se demande vingt pages plus loin (`after` : 20, 40, 60, 80), une à la
//   fois, et s'ajoute dessous ; plus rien au-delà de la centième page (R911).
//
// Ce module n'a pas de relectures groupées (`rereads.ts`) : une recherche n'est pas relue quand le
// serveur change, elle est refaite quand la personne tape, et la dernière frappe gagne.
import type { SearchHit } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState } from "react";
import { isSessionLost, searchSpace } from "../api";
import { messageOf } from "../session/forms";

/** Une requête part cette durée après la dernière frappe (ms). */
export const SEARCH_DEBOUNCE_MS = 150;
/** Les pages d'une réponse, au plus : `after` avance d'autant. */
export const ANSWER_PAGES = 20;
/** Les pages d'une recherche, au plus : le serveur refuse `after` à partir de là. */
export const MAX_FOUND_PAGES = 100;

export type Search = {
  /** Les pages trouvées par le serveur pour la recherche du rendu, dans son ordre. */
  hits: readonly SearchHit[];
  /** La première réponse de cette recherche n'est pas arrivée (anti-rebond, requête, compte attendu). */
  pending: boolean;
  /** D'autres pages suivent, et peuvent être demandées (`loadMore`). */
  more: boolean;
  /** La suite est en route. */
  loadingMore: boolean;
  /** L'index n'est pas à jour pour une page que le compte voit : des pages peuvent manquer. */
  indexing: boolean;
  /** Le refus de la recherche courante, tel que le serveur le dit. */
  error: string | null;
  /** Demande la suite ; sans effet s'il n'y en a pas, ou si elle est déjà en route. */
  loadMore(): void;
};

type State = Omit<Search, "loadMore"> & {
  /** L'espace et la recherche auxquels ces résultats répondent. */
  spaceId: string;
  query: string;
};

const fresh = (spaceId: string, query: string): State => ({
  spaceId,
  query,
  hits: [],
  pending: query !== "",
  more: false,
  loadingMore: false,
  indexing: false,
  error: null,
});

export function useSearch(
  spaceId: string,
  query: string,
  options: { afterLogin?: () => Promise<void> } = {},
): Search {
  const wanted = query.trim();
  const [state, setState] = useState(() => fresh(spaceId, wanted));
  // L'attente du compte du dernier rendu : la changer ne refait pas la recherche.
  const latest = useRef(options);
  latest.current = options;
  /** Demande la suite de la recherche en cours ; `null` : aucune recherche. */
  const further = useRef<(() => void) | null>(null);

  useEffect(() => {
    setState(fresh(spaceId, wanted));
    further.current = null;
    if (wanted === "") return;
    const controller = new AbortController();
    /** La recherche n'est plus celle qui est tapée (ou la fenêtre est partie). */
    let stopped = false;
    /** Les réponses reçues : la suite se demande `ANSWER_PAGES` pages plus loin à chacune. */
    let answers = 0;
    /** Une requête est en route, ou attend le compte. */
    let asking = false;
    /** Le serveur a dit qu'une suite existe, à sa dernière réponse. */
    let more = false;

    const ask = async () => {
      const after = answers * ANSWER_PAGES;
      asking = true;
      try {
        const answer = await searchSpace(
          spaceId,
          after === 0 ? { q: wanted } : { q: wanted, after },
          controller.signal,
        );
        // Une réponse en retard, d'une recherche qui n'est plus la courante.
        if (stopped) return;
        asking = false;
        answers += 1;
        more = answer.more && answers * ANSWER_PAGES < MAX_FOUND_PAGES;
        setState((s) => ({
          ...s,
          hits: after === 0 ? answer.results : [...s.hits, ...answer.results],
          more,
          indexing: answer.indexing,
          pending: false,
          loadingMore: false,
          error: null,
        }));
      } catch (e) {
        // Abandonnée, ou refusée après avoir été remplacée (`429 search_busy`) : rien à en dire.
        if (stopped) return;
        const wait = isSessionLost(e) ? latest.current.afterLogin : undefined;
        if (wait !== undefined) {
          // La connexion est demandée par-dessus la fenêtre ; la recherche repart au retour du compte.
          await wait();
          if (!stopped) void ask();
          return;
        }
        asking = false;
        setState((s) => ({
          ...s,
          pending: false,
          loadingMore: false,
          error: isSessionLost(e) ? null : messageOf(e),
        }));
      }
    };

    const timer = setTimeout(() => void ask(), SEARCH_DEBOUNCE_MS);
    further.current = () => {
      // Seulement si le serveur a dit qu'une suite existe (jamais avant la première réponse, ni
      // au-delà de la centième page), et pas deux à la fois.
      if (stopped || asking || !more) return;
      setState((s) => ({ ...s, loadingMore: true, error: null }));
      void ask();
    };
    return () => {
      stopped = true;
      clearTimeout(timer);
      controller.abort();
    };
  }, [spaceId, wanted]);

  const loadMore = useCallback(() => further.current?.(), []);
  // L'état d'une autre recherche (le rendu de la frappe précède l'effet) n'est jamais montré.
  const shown = state.spaceId === spaceId && state.query === wanted ? state : fresh(spaceId, wanted);
  const { hits, pending, more, loadingMore, indexing, error } = shown;
  return { hits, pending, more, loadingMore, indexing, error, loadMore };
}
