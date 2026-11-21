// L'activité d'un jeton d'API (`GET /api/v1/tokens/{id}/calls`, lot 5, Task C2) : ses appels, le plus
// récent d'abord, cinquante par lecture.
//
// Quand la lire : seulement quand on la demande (l'activité dépliée sous la ligne du jeton), jamais
// avec la liste des jetons ; puis la suite, une page à la fois (« Afficher plus » : `before`, le rang de
// la dernière ligne lue), ajoutée sous ce qui est montré. Une autre issue (le filtre) repart de zéro.
// Rien ne la relit d'elle-même : replier l'activité puis la déplier la relit.
//
// Les lectures passent par `scheduleRereads` : une demande faite pendant une lecture est servie à sa
// fin, sans en faire partir une seconde. Une lecture qui trouve la session perdue (`401`) attend le
// retour du compte (`afterLogin`, R212), puis reprend ce qui était demandé (la première page, ou la
// suite). Une lecture qui échoue garde ce qui est montré et dit pourquoi, tel que le serveur le dit ;
// elle ne se refait pas d'elle-même (« Réessayer », ou « Afficher plus » de nouveau).
//
// Les comptes des appels refusés par le débit et des lignes purgées sont ceux de la première lecture :
// la suite ne les change pas, une phrase de plus au-dessus de la liste ferait bouger ce qui est sous
// le pointeur.
import type { TokenCall, TokenCallOutcome } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isSessionLost, tokenCalls } from "../api";
import { scheduleRereads, SESSION_LOST, waitForAccount } from "../rereads";
import { messageOf } from "../session/forms";

export type TokenCallsRead = {
  /** Les appels lus, le plus récent d'abord ; `null` : la première page n'est pas encore lue. */
  calls: TokenCall[] | null;
  /** Des appels plus anciens restent à lire. */
  more: boolean;
  /** Les appels refusés par le débit, comptés sans ligne (R1244). */
  busyCount: number;
  /** Les lignes que la purge a retirées (R1244). */
  purged: number;
  /** Le message de la dernière lecture si elle a échoué, tel que le serveur le dit. */
  error: string | null;
  /** La suite est demandée et pas encore lue. */
  loadingMore: boolean;
  /** Demande la page qui suit la dernière ligne lue. */
  loadMore(): void;
  /** Refait une première lecture qui a échoué. */
  retry(): void;
};

type Loaded = {
  /** Le jeton et l'issue que ces lignes concernent. */
  key: string;
  calls: TokenCall[] | null;
  more: boolean;
  busyCount: number;
  purged: number;
  error: string | null;
  loadingMore: boolean;
};

const keyOf = (tokenId: string, outcome: TokenCallOutcome | null) => `${tokenId}/${outcome ?? "all"}`;

const fresh = (key: string): Loaded => ({
  key,
  calls: null,
  more: false,
  busyCount: 0,
  purged: 0,
  error: null,
  loadingMore: false,
});

export function useTokenCalls(
  tokenId: string,
  outcome: TokenCallOutcome | null,
  afterLogin?: () => Promise<void>,
): TokenCallsRead {
  const key = keyOf(tokenId, outcome);
  const [loaded, setLoaded] = useState<Loaded>(() => fresh(key));
  const options = useRef({ afterLogin });
  options.current = { afterLogin };
  /** Demande la suite ; refait une première lecture. Remplacés à chaque jeton ou issue. */
  const actions = useRef({ loadMore: () => {}, retry: () => {} });

  useEffect(() => {
    /** Ce qui est montré pour ce jeton et cette issue : la lecture en part, jamais d'un rendu. */
    let shown = fresh(key);
    /** La suite est demandée. */
    let wantMore = false;
    const show = (next: Loaded) => {
      shown = next;
      setLoaded(next);
    };
    const schedule = scheduleRereads(async () => {
      // La première page, ou la suite si elle est demandée ; sinon rien.
      const last = shown.calls?.at(-1);
      if (shown.calls !== null && (!wantMore || last === undefined)) return;
      const before = shown.calls === null ? undefined : last?.id;
      try {
        const page = await tokenCalls(tokenId, {
          before,
          outcome: outcome ?? undefined,
        });
        if (schedule.stopped) return;
        wantMore = false;
        show(
          shown.calls === null
            ? {
                key,
                calls: page.calls,
                more: page.more,
                busyCount: page.busyCount,
                purged: page.purged,
                error: null,
                loadingMore: false,
              }
            : { ...shown, calls: [...shown.calls, ...page.calls], more: page.more, error: null, loadingMore: false },
        );
      } catch (e) {
        if (schedule.stopped) return;
        // Session perdue : la connexion est demandée par-dessus l'application ; la même lecture
        // repart au retour du compte, la suite toujours demandée.
        if (isSessionLost(e)) return SESSION_LOST;
        wantMore = false;
        show({ ...shown, error: messageOf(e), loadingMore: false });
      }
    }, waitForAccount(options));
    actions.current = {
      loadMore: () => {
        if (schedule.stopped || shown.calls === null || !shown.more || wantMore) return;
        wantMore = true;
        show({ ...shown, loadingMore: true });
        schedule.request(true);
      },
      retry: () => schedule.request(true),
    };
    schedule.request(true);
    return () => {
      schedule.stop();
      actions.current = { loadMore: () => {}, retry: () => {} };
    };
  }, [key, tokenId, outcome]);

  const loadMore = useCallback(() => actions.current.loadMore(), []);
  const retry = useCallback(() => actions.current.retry(), []);
  const current = loaded.key === key ? loaded : fresh(key);
  const { calls, more, busyCount, purged, error, loadingMore } = current;
  return useMemo(
    () => ({ calls, more, busyCount, purged, error, loadingMore, loadMore, retry }),
    [calls, more, busyCount, purged, error, loadingMore, loadMore, retry],
  );
}
