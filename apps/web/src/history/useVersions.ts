// Les versions d'une page (R135), telles que le serveur les rend
// (`GET /api/v1/pages/{id}/versions`) : la plus récente d'abord, cent au plus, chacune avec sa
// date, sa raison et les comptes qui ont modifié la page depuis la version d'avant.
//
// Quand les lire. Le hook ne vit que dans la fenêtre « Historique » : la liste est lue quand elle
// s'ouvre, jamais avec la page (rien sur le chemin de l'ouverture, budgets du §5.10). Tant qu'elle
// est ouverte, le serveur envoie la trame `{"type":"versions"}` sur la connexion de synchro de la
// page à chaque version coupée (R185) : la liste est relue par l'API. Une trame peut se perdre
// pendant une coupure : elle l'est aussi quand la connexion revient, et à la première synchro de
// chaque session (une page qui passe en lecture seule en ouvre une neuve).
//
// Les relectures sont celles des commentaires (`rereads.ts`) : une rafale de trames n'en fait
// qu'une, `VERSIONS_DEBOUNCE_MS` après la dernière et au plus tard `VERSIONS_MAX_WAIT_MS` après
// la première. Une relecture demandée pendant une lecture ne l'annule pas : elle est faite une
// fois, à sa fin. Une lecture qui échoue garde ce qui était affiché, dit pourquoi, et ne se
// refait pas d'elle-même : la prochaine trame, le retour de la connexion ou « Réessayer » la
// refont. Tout est lié à la page, et plus rien n'est appelé après le démontage.
import type { Version } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersions, isSessionLost } from "../api";
import type { CollabSession } from "../editor/collab";
import { REREAD_DEBOUNCE_MS, REREAD_MAX_WAIT_MS, scheduleRereads } from "../rereads";
import { messageOf } from "../session/forms";

/** Plusieurs trames qui se suivent ne font qu'une relecture, cette durée après la dernière. */
export const VERSIONS_DEBOUNCE_MS = REREAD_DEBOUNCE_MS;
/** Une relecture part au plus tard cette durée après la première trame qu'elle doit traiter. */
export const VERSIONS_MAX_WAIT_MS = REREAD_MAX_WAIT_MS;

/** `loading` : rien n'est encore lu ; `failed` : la dernière lecture a échoué. */
export type VersionsState = "loading" | "ready" | "failed";

export type Versions = {
  /** Les versions de la page, la plus récente d'abord ; `null` : pas encore lues. */
  versions: Version[] | null;
  state: VersionsState;
  /** Le message de la dernière lecture si elle a échoué, tel que le serveur le dit. */
  error: string | null;
  /** Relit la liste tout de suite (« Réessayer », version introuvable, restauration refusée). */
  reload(): void;
};

/** Ce que le hook lit de la session de synchro de la page. */
export type VersionsSession = Pick<CollabSession, "synced" | "status" | "subscribe" | "onPageEvent">;

type Loaded = {
  pageId: string;
  versions: Version[] | null;
  state: VersionsState;
  error: string | null;
};

export function useVersions(pageId: string, session: VersionsSession | null): Versions {
  const [loaded, setLoaded] = useState<Loaded>(() => ({
    pageId,
    versions: null,
    state: "loading",
    error: null,
  }));
  /** Demande une relecture : tout de suite (`true`), ou groupée avec les trames qui suivent. */
  const request = useRef<(immediate: boolean) => void>(() => {});

  useEffect(() => {
    // Groupées, jamais annulées en route, rien après l'arrêt (`rereads.ts`).
    const rereads = scheduleRereads(async () => {
      try {
        const versions = await getVersions(pageId);
        if (rereads.stopped) return;
        setLoaded({ pageId, versions, state: "ready", error: null });
      } catch (e) {
        if (rereads.stopped) return;
        // Session perdue : la connexion est demandée par-dessus la page (`api.ts`), et la
        // connexion de synchro qui revient ensuite fait relire.
        if (!isSessionLost(e)) {
          setLoaded((current) => ({
            pageId,
            versions: current.pageId === pageId ? current.versions : null,
            state: "failed",
            error: messageOf(e),
          }));
        }
      }
    });
    request.current = rereads.request;
    // La fenêtre vient de s'ouvrir : la liste est lue tout de suite, sans attendre la synchro.
    rereads.request(true);

    return () => {
      rereads.stop();
      request.current = () => {};
    };
  }, [pageId]);

  // La session de synchro : ses trames, sa première synchro, ses retours.
  useEffect(() => {
    if (session === null) return;
    let synced = session.synced;
    let status = session.status;
    // Une session déjà synchronisée : celle de l'ouverture (cette demande rejoint la première
    // lecture), ou une session neuve, qui a pu manquer une trame pendant que l'ancienne se fermait.
    if (synced) request.current(true);
    const offFrames = session.onPageEvent((event) => {
      if (event === "versions") request.current(false);
    });
    const offStatus = session.subscribe(() => {
      const wasSynced = synced;
      const was = status;
      synced = session.synced;
      status = session.status;
      if (!wasSynced && synced) request.current(true);
      // La connexion revient : une trame a pu partir pendant la coupure.
      else if (synced && status === "connected" && was !== "connected") request.current(false);
    });
    return () => {
      offFrames();
      offStatus();
    };
  }, [session, pageId]);

  const reload = useCallback(() => request.current(true), []);

  const current = loaded.pageId === pageId ? loaded : null;
  const versions = current?.versions ?? null;
  const state = current?.state ?? "loading";
  const error = current?.error ?? null;
  return useMemo(() => ({ versions, state, error, reload }), [versions, state, error, reload]);
}
