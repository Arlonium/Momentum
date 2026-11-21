// La liste des jetons d'API du compte (`GET /api/v1/tokens`), pour l'écran « Jetons d'API ».
//
// Quand la lire : à l'ouverture de l'écran, après chaque geste de l'écran (créer, révoquer, une
// réponse perdue), quand un changement de mot de passe les a révoqués (`tokensChanged.ts`), et au
// retour du compte quand une lecture a trouvé la session perdue (`afterLogin`, R212). Les lectures
// passent par `scheduleRereads` : une demande faite pendant une lecture est servie une fois, à sa fin.
//
// **Le texte d'un jeton n'entre jamais ici** : la liste ne porte que ce que rend le serveur
// (`TokenInfo`, quatre derniers caractères compris) ; le jeton tout juste créé ne vit que dans la
// fenêtre qui le montre (`CreateToken`).
//
// Une lecture qui échoue garde la liste déjà lue et dit pourquoi (le message du serveur) ; elle ne se
// refait pas d'elle-même.
import type { TokenInfo } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isSessionLost, listTokens } from "../api";
import { scheduleRereads, SESSION_LOST, waitForAccount, type Rereads } from "../rereads";
import { messageOf } from "../session/forms";
import { onTokensChanged } from "./tokensChanged";

export type TokensRead = {
  /** Les jetons, dans l'ordre du serveur (les vivants d'abord) ; `null` : pas encore lus. */
  tokens: TokenInfo[] | null;
  /** Le message de la dernière lecture si elle a échoué, tel que le serveur le dit. */
  error: string | null;
  /**
   * Demande une lecture ; la promesse est tenue quand une lecture partie après cette demande a fini
   * (lue, échouée, ou arrêtée sur une session perdue) : un geste qui veut montrer l'état du serveur
   * avant de rendre la main l'attend.
   */
  reread: () => Promise<void>;
};

export function useTokens(afterLogin?: () => Promise<void>): TokensRead {
  const [tokens, setTokens] = useState<TokenInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const options = useRef({ afterLogin });
  options.current = { afterLogin };
  const rereads = useRef<Rereads | null>(null);
  /** Ceux qui attendent une lecture partie après leur demande. */
  const waiting = useRef<(() => void)[]>([]);

  useEffect(() => {
    const schedule = scheduleRereads(async () => {
      // Une demande faite pendant cette lecture attend la suivante.
      const served = waiting.current.splice(0);
      try {
        const read = await listTokens();
        if (schedule.stopped) return;
        setTokens(read);
        setError(null);
      } catch (e) {
        if (schedule.stopped) return;
        // Session perdue : la connexion est demandée par-dessus l'application, et la liste est
        // relue au retour du compte.
        if (isSessionLost(e)) return SESSION_LOST;
        setError(messageOf(e));
      } finally {
        for (const done of served) done();
      }
    }, waitForAccount(options));
    rereads.current = schedule;
    schedule.request(true);
    const off = onTokensChanged(() => schedule.request(true));
    return () => {
      off();
      schedule.stop();
      rereads.current = null;
      for (const done of waiting.current.splice(0)) done();
    };
  }, []);

  const reread = useCallback(
    () =>
      new Promise<void>((done) => {
        const schedule = rereads.current;
        if (schedule === null || schedule.stopped) return done();
        waiting.current.push(done);
        schedule.request(true);
      }),
    [],
  );

  return useMemo(() => ({ tokens, error, reread }), [tokens, error, reread]);
}
