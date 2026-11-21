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
// Les comptes des appels refusés par le débit et des lignes purgées sont ceux du jeton, pas d'une
// issue : ils partent de ceux de la liste (`TokenInfo`), sont remplacés par ceux de chaque réponse,
// et restent d'une issue à l'autre, pendant la relecture comprise (R1476). La phrase qui les dit,
// au-dessus du filtre, ne disparaît donc pas quand on change d'issue : le filtre ne bouge pas.
import type { TokenCall, TokenCallOutcome } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isSessionLost, tokenCalls } from "../api";
import { scheduleRereads, SESSION_LOST, waitForAccount } from "../rereads";
import { messageOf } from "../session/forms";

/** Le jeton dont on lit l'activité, et ses comptes tels que la liste les a lus. */
export type ActivityToken = {
  id: string;
  busyCount: number;
  callsPurged: number;
};

export type TokenCallsRead = {
  /** Les appels lus, le plus récent d'abord ; `null` : la première page n'est pas encore lue. */
  calls: TokenCall[] | null;
  /** Des appels plus anciens restent à lire. */
  more: boolean;
  /** Les appels refusés par le débit, comptés sans ligne (R1244), d'après la dernière réponse. */
  busyCount: number;
  /** Les lignes que la purge a retirées (R1244), d'après la dernière réponse. */
  purged: number;
  /** Le message de la dernière lecture si elle a échoué, tel que le serveur le dit. */
  error: string | null;
  /** La suite est demandée et pas encore lue. */
  loadingMore: boolean;
  /** Un « Réessayer » a demandé la première page, pas encore lue. */
  retrying: boolean;
  /** Demande la page qui suit la dernière ligne lue. */
  loadMore(): void;
  /** Refait une première lecture qui a échoué ; sans effet une fois une page lue. */
  retry(): void;
};

type Loaded = {
  /** Le jeton et l'issue que ces lignes concernent. */
  key: string;
  calls: TokenCall[] | null;
  more: boolean;
  error: string | null;
  loadingMore: boolean;
  retrying: boolean;
};

/** Les comptes d'un jeton, quelle que soit l'issue lue. */
type Counts = { tokenId: string; busyCount: number; purged: number };

const keyOf = (tokenId: string, outcome: TokenCallOutcome | null) => `${tokenId}/${outcome ?? "all"}`;

const fresh = (key: string): Loaded => ({
  key,
  calls: null,
  more: false,
  error: null,
  loadingMore: false,
  retrying: false,
});

const countsOf = (token: ActivityToken): Counts => ({
  tokenId: token.id,
  busyCount: token.busyCount,
  purged: token.callsPurged,
});

export function useTokenCalls(
  token: ActivityToken,
  outcome: TokenCallOutcome | null,
  afterLogin?: () => Promise<void>,
): TokenCallsRead {
  const tokenId = token.id;
  const key = keyOf(tokenId, outcome);
  const [loaded, setLoaded] = useState<Loaded>(() => fresh(key));
  const [counts, setCounts] = useState<Counts>(() => countsOf(token));
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
        setCounts({ tokenId, busyCount: page.busyCount, purged: page.purged });
        show({
          ...shown,
          calls: shown.calls === null ? page.calls : [...shown.calls, ...page.calls],
          more: page.more,
          error: null,
          loadingMore: false,
          retrying: false,
        });
      } catch (e) {
        if (schedule.stopped) return;
        // Session perdue : la connexion est demandée par-dessus l'application ; la même lecture
        // repart au retour du compte, la suite toujours demandée.
        if (isSessionLost(e)) return SESSION_LOST;
        wantMore = false;
        show({ ...shown, error: messageOf(e), loadingMore: false, retrying: false });
      }
    }, waitForAccount(options));
    actions.current = {
      loadMore: () => {
        // Défense en profondeur : la lecture ne lit de toute façon qu'une page par demande.
        if (schedule.stopped || shown.calls === null || !shown.more || wantMore) return;
        wantMore = true;
        show({ ...shown, loadingMore: true });
        schedule.request(true);
      },
      retry: () => {
        if (schedule.stopped || shown.retrying) return;
        // Une page déjà lue : la lecture ne lit rien (elle ne lit la suite que demandée).
        if (shown.calls === null) show({ ...shown, retrying: true });
        schedule.request(true);
      },
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
  const { calls, more, error, loadingMore, retrying } = current;
  // Un autre jeton (le composant n'en change pas, mais le hook le permet) repart de ses comptes.
  const { busyCount, purged } = counts.tokenId === tokenId ? counts : countsOf(token);
  return useMemo(
    () => ({ calls, more, busyCount, purged, error, loadingMore, retrying, loadMore, retry }),
    [calls, more, busyCount, purged, error, loadingMore, retrying, loadMore, retry],
  );
}
