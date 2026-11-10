// Ce qu'une page ouverte relit avant chaque connexion de synchro. Le serveur ne pousse rien
// quand les droits d'une page changent : il ferme ses connexions de synchro, et le client
// revient. C'est donc à ce moment que l'onglet apprend ce que la page est devenue pour lui,
// avec la relecture du schéma (Ruling R35, `editor/schemaGate.ts`) :
//
// - `404` : la page n'est plus là pour cette personne (supprimée, ou plus partagée avec elle :
//   la même réponse). Aucune connexion, et l'application le dit (`onGone`) ;
// - niveau tombé sous `edit`, page passée à la corbeille, ou schéma d'un autre serveur : une
//   session qui écrit ne se connecte pas. L'application, prévenue (`onPage`), la remplace par
//   une session en lecture seule, sur un document neuf (jamais l'ancien, qui porte des
//   éditions locales) ;
// - `401` : la connexion est demandée par-dessus la page (`api.ts`, `SessionGate`), qui reste
//   montée. La vérification attend le retour du compte, puis relit : la synchro reprend, et ce
//   qui a été tapé entre-temps part au serveur ;
// - serveur injoignable : la connexion échoue, et y-websocket réessaiera.
//
// Sans cela, le serveur servirait en lecture seule une connexion ouverte pour écrire, et
// ignorerait en silence tout ce que tape l'onglet.
//
// Deux lectures de la page peuvent être en route (une connexion de synchro, et le menu
// Partager qui vient de perdre l'accès complet) : une réponse plus ancienne qu'une autre déjà
// retenue ne dit plus rien. C'est le rang de la demande qui compte, et seulement parmi les
// lectures qui ont répondu : si la plus récente échoue, l'autre reste ce qu'on sait de mieux.
// Une fois la page quittée, plus aucune réponse ne compte.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { ApiError, getPage, isSessionLost } from "../api";
import type { ConnectionCheck, SchemaCheck } from "../editor/schemaGate";
import { canEdit } from "../share/levels";

type Inputs = {
  /** La page ouverte. */
  id: string;
  /** La session de synchro écrit : elle ne se connecte que si le serveur le permet encore. */
  writable: boolean;
  /** Relit la version de schéma du serveur ; `false` met l'application en lecture seule. */
  checkSchema: SchemaCheck;
  /** Ce que le serveur dit de la page, à chaque relecture. */
  onPage: (node: PageNode) => void;
  /** La page n'est plus là pour cette personne. */
  onGone: () => void;
  /** Tenue quand le compte revient, après une session perdue (`useAfterLogin`). */
  afterLogin: () => Promise<void>;
};

export type PageGate = {
  /** À donner à la session de synchro : relit le schéma et la page avant chaque connexion. */
  check: ConnectionCheck;
  /**
   * Relit la page hors d'une connexion : ce qu'elle est devenue (`onPage`, `onGone`). Un
   * serveur injoignable ne dit rien : la prochaine connexion de synchro la relira.
   */
  reread: () => Promise<void>;
};

export function usePageGate(inputs: Inputs): PageGate {
  const { id, writable } = inputs;
  // Les rappels du dernier rendu : en changer ne recrée pas la session de synchro.
  const latest = useRef(inputs);
  latest.current = inputs;

  /** Le rang de la dernière lecture demandée. */
  const asked = useRef(0);
  /** La réponse retenue (`null` : la page n'est plus là), et le rang de sa lecture. */
  const kept = useRef<{ rank: number; node: PageNode | null }>({ rank: 0, node: null });
  const unmounted = useRef(false);
  useEffect(() => {
    unmounted.current = false;
    return () => {
      unmounted.current = true;
    };
  }, []);

  /**
   * Retient la réponse de la lecture de rang `rank` et la dit à l'application, sauf si une
   * lecture demandée après elle a déjà répondu. Rend ce qu'on sait de la page.
   */
  const keep = useCallback((rank: number, node: PageNode | null): PageNode | null => {
    if (unmounted.current) return null;
    if (rank < kept.current.rank) return kept.current.node;
    kept.current = { rank, node };
    if (node === null) latest.current.onGone();
    else latest.current.onPage(node);
    return node;
  }, []);

  /** Ce que le serveur dit de la page, le compte revenu s'il le faut ; `null` : plus là. */
  const read = useCallback(async (): Promise<PageNode | null> => {
    const rank = ++asked.current;
    for (;;) {
      try {
        return keep(rank, await getPage(id));
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) return keep(rank, null);
        if (!isSessionLost(error)) throw error;
        await latest.current.afterLogin();
      }
    }
  }, [id, keep]);

  const check = useCallback(async () => {
    // Le schéma ne compte que pour qui écrit : une session en lecture seule ne fait que
    // recevoir. Les deux relectures partent ensemble : l'ouverture d'une page n'attend pas
    // l'une après l'autre.
    const [sameSchema, node] = await Promise.all([
      writable ? latest.current.checkSchema() : true,
      read(),
    ]);
    // `node` : ce qu'on sait de plus récent de la page, que ce soit ou non la réponse de cette
    // lecture. La connexion se décide d'après ce que l'application sait.
    if (node === null) return false;
    return !writable || (sameSchema && canEdit(node.level) && node.trashedAt === null);
  }, [writable, read]);

  const reread = useCallback(async () => {
    try {
      await read();
    } catch {
      // serveur injoignable : rien à dire ici
    }
  }, [read]);

  return useMemo(() => ({ check, reread }), [check, reread]);
}
