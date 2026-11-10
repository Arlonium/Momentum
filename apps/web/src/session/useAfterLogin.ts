// Ce qu'une session perdue a refusé (`401 unauthenticated` : un titre tapé, une connexion de
// synchro) n'est pas perdu : la connexion est demandée par-dessus l'application
// (`SessionGate`), qui reste montée, et l'envoi attend ici le retour du compte pour repartir.
// `useRetryAfterLogin` fait de même pour une lecture, qui se refait en entier.
//
// Si c'est un autre compte qui se connecte, l'application est démontée, puis l'onglet rechargé
// (`SessionGate`, Ruling R115) : ce hook est démonté avec elle, et ceux qui attendaient
// n'envoient jamais rien sous son nom.
import type { Me } from "@mementum/protocol";
import { useCallback, useEffect, useRef } from "react";

/**
 * Rend `afterLogin()` : une promesse tenue quand le compte revient, jamais une fois le hook
 * démonté. La fonction garde son identité.
 */
export function useAfterLogin(me: Me): () => Promise<void> {
  const waiting = useRef<(() => void)[]>([]);
  // `me` change à la reconnexion (et quand le compte est relu, ce qui demande une session) :
  // ceux qui attendaient repartent.
  useEffect(() => {
    for (const resume of waiting.current.splice(0)) resume();
  }, [me]);
  useEffect(
    () => () => {
      waiting.current = [];
    },
    [],
  );
  return useCallback(() => new Promise<void>((resume) => waiting.current.push(resume)), []);
}
