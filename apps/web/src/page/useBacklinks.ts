// Les rétroliens d'une page (lot 4, Task C3) : les pages qui mènent à elle, telles que le serveur
// les rend (`GET /api/v1/pages/{id}/backlinks`), pour « Pages qui mènent ici » sous le texte.
//
// Quand les lire. **Jamais sur le chemin de l'ouverture de la page** : la réponse prend au moins
// 150 ms chez le serveur (son plancher, R1022). La première lecture attend la première synchro de la
// session de la page, et aucun rendu ne l'attend. Rien ne pousse un changement de rétroliens (un
// lien posé dans une autre page n'avance ni le canal de l'espace ni une trame de la page) : ils sont
// relus
// - à la première synchro de chaque session de la page (un droit qui change en ouvre une neuve) ;
// - au retour de la connexion de synchro (que le serveur ferme quand les droits de la page changent) ;
// - au retour sur l'onglet, si la dernière lecture demandée a au moins `BACKLINKS_RETURN_MS`, et
//   si la synchro est là (hors ligne, c'est son retour qui relira) ;
// - au retour du compte, quand une lecture a trouvé la session perdue (`afterLogin`).
// Jamais à la frappe : rien ici n'écoute le document, et ce que la session annonce sans changer
// d'état ne fait rien relire.
//
// Une lecture qui échoue garde la liste déjà lue, dit pourquoi (le message du serveur), et ne se
// refait pas d'elle-même. Un `404` dit que la page n'est plus vue : la page est relue (`onRefused`),
// et c'est elle qui le dira à l'écran. Tout est lié à la page : rien ne passe d'une page à l'autre.
import type { Backlink } from "@mementum/protocol";
import { useEffect, useMemo, useRef, useState } from "react";
import { ApiError, getBacklinks, isSessionLost } from "../api";
import type { CollabSession } from "../editor/collab";
import { scheduleRereads, SESSION_LOST, waitForAccount } from "../rereads";
import { messageOf } from "../session/forms";

/** Au retour sur l'onglet, les rétroliens ne sont relus que si leur dernière lecture a cet âge (ms). */
export const BACKLINKS_RETURN_MS = 5_000;

export type BacklinksRead = {
  /** Les pages qui mènent ici, dans l'ordre du serveur ; `null` : pas encore lues. */
  pages: Backlink[] | null;
  /** Le message de la dernière lecture si elle a échoué, tel que le serveur le dit. */
  error: string | null;
};

/** Ce que le hook lit de la session de synchro de la page. */
export type BacklinksSession = Pick<CollabSession, "synced" | "status" | "subscribe">;

type Options = {
  /** Le serveur dit que la page n'est plus vue (`404`) : à relire. */
  onRefused?: (error: ApiError) => void;
  /** Tenue quand le compte revient, après une session perdue (`useAfterLogin`). */
  afterLogin?: () => Promise<void>;
};

type Loaded = BacklinksRead & { pageId: string };

export function useBacklinks(
  pageId: string,
  session: BacklinksSession | null,
  options: Options = {},
): BacklinksRead {
  const [loaded, setLoaded] = useState<Loaded>(() => ({ pageId, pages: null, error: null }));
  // Les rappels du dernier rendu : en changer ne relance rien.
  const callbacks = useRef(options);
  callbacks.current = options;
  /** Demande une lecture : tout de suite (`true`), ou groupée. */
  const request = useRef<(immediate: boolean) => void>(() => {});
  /** Quand la dernière lecture a été demandée ; `null` : aucune encore, pour cette page. */
  const askedAt = useRef<number | null>(null);

  useEffect(() => {
    const rereads = scheduleRereads(async () => {
      try {
        const { pages } = await getBacklinks(pageId);
        if (rereads.stopped) return;
        setLoaded({ pageId, pages, error: null });
      } catch (e) {
        if (rereads.stopped) return;
        // Session perdue : la connexion est demandée par-dessus la page (`api.ts`), et la liste
        // est relue au retour du compte.
        if (isSessionLost(e)) return SESSION_LOST;
        setLoaded((current) => ({
          pageId,
          pages: current.pageId === pageId ? current.pages : null,
          error: messageOf(e),
        }));
        if (e instanceof ApiError && e.status === 404) callbacks.current.onRefused?.(e);
      }
    }, waitForAccount(callbacks));
    request.current = (immediate) => {
      askedAt.current = Date.now();
      rereads.request(immediate);
    };
    return () => {
      rereads.stop();
      request.current = () => {};
      askedAt.current = null;
    };
  }, [pageId]);

  useEffect(() => {
    if (session === null) return;
    let synced = session.synced;
    let status = session.status;
    if (synced) request.current(true);
    const offStatus = session.subscribe(() => {
      const wasSynced = synced;
      const was = status;
      synced = session.synced;
      status = session.status;
      // La première synchro de cette session : la première lecture, ou celle d'une session neuve.
      if (!wasSynced && synced) request.current(true);
      // La connexion revient : un droit a pu changer pendant la coupure.
      else if (synced && status === "connected" && was !== "connected") request.current(false);
    });
    // Le retour sur l'onglet : rien d'autre ne dit qu'un lien a été posé ailleurs.
    const comeBack = () => {
      if (document.visibilityState !== "visible") return;
      // Rien n'a encore été lu (la première synchro n'est pas faite), ou la synchro est coupée.
      if (askedAt.current === null || !session.synced || session.status !== "connected") return;
      // Deux signaux du même retour (visible, puis focus) ne font qu'une lecture.
      if (Date.now() - askedAt.current < BACKLINKS_RETURN_MS) return;
      request.current(true);
    };
    document.addEventListener("visibilitychange", comeBack);
    window.addEventListener("focus", comeBack);
    return () => {
      offStatus();
      document.removeEventListener("visibilitychange", comeBack);
      window.removeEventListener("focus", comeBack);
    };
  }, [session, pageId]);

  const current = loaded.pageId === pageId ? loaded : null;
  const pages = current?.pages ?? null;
  const error = current?.error ?? null;
  return useMemo(() => ({ pages, error }), [pages, error]);
}
