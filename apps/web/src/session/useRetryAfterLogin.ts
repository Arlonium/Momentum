// Une lecture (l'arbre d'un espace, une page) qui tombe sur une session perdue ne dit rien :
// la connexion est demandée par-dessus l'application (`SessionGate`). Mais elle n'a rien lu.
// Elle se signale ici (`sessionLost`), et `retries` change quand le compte revient : les
// lectures qui en dépendent sont refaites.
import type { Me } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState } from "react";

export type RetryAfterLogin = {
  /** Change quand une lecture perdue peut être refaite : à mettre dans ses dépendances. */
  retries: number;
  /** Une lecture a répondu `401 unauthenticated`. Garde son identité. */
  sessionLost: () => void;
};

export function useRetryAfterLogin(me: Me): RetryAfterLogin {
  const lost = useRef(false);
  const [retries, setRetries] = useState(0);
  // `me` change à la reconnexion (et quand le compte est relu) : s'il y a eu une lecture
  // perdue, elle repart.
  useEffect(() => {
    if (!lost.current) return;
    lost.current = false;
    setRetries((n) => n + 1);
  }, [me]);
  const sessionLost = useCallback(() => {
    lost.current = true;
  }, []);
  return { retries, sessionLost };
}
