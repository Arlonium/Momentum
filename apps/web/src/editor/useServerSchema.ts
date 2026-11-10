// La version de schéma du serveur, vue de l'application. Elle est lue au chargement, puis
// relue avant chaque connexion de synchronisation (R35, `schemaGate.ts`) : si elle diffère de
// celle de ce client, tout passe en lecture seule, pour de bon.
import { useCallback, useEffect, useRef, useState } from "react";
import { getInfo } from "../api";
import { messageOf } from "../session/forms";
import type { SchemaCheck } from "./schemaGate";
import { CLIENT_SCHEMA_VERSION } from "./version";

export type ServerSchema = {
  /** La première lecture est faite. */
  loaded: boolean;
  /** La première lecture a échoué : le message, en français. */
  error: string | null;
  /** Le serveur n'a pas le schéma de ce client : plus aucune écriture (bandeau). */
  stale: boolean;
  /** Le même, à l'instant : pour un rappel qui ne se rend pas de nouveau. */
  isStale(): boolean;
  /** Relit la version du serveur ; `false` met l'application en lecture seule. */
  check: SchemaCheck;
};

export function useServerSchema(): ServerSchema {
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const staleRef = useRef(false);

  const markStale = useCallback(() => {
    staleRef.current = true;
    setStale(true);
  }, []);

  const check = useCallback(async () => {
    const info = await getInfo();
    if (info.schemaVersion === CLIENT_SCHEMA_VERSION && !staleRef.current) return true;
    markStale();
    return false;
  }, [markStale]);

  useEffect(() => {
    let cancelled = false;
    getInfo()
      .then((info) => {
        if (cancelled) return;
        if (info.schemaVersion !== CLIENT_SCHEMA_VERSION) markStale();
        setLoaded(true);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(messageOf(e));
      });
    return () => {
      cancelled = true;
    };
  }, [markStale]);

  const isStale = useCallback(() => staleRef.current, []);
  return { loaded, error, stale, isStale, check };
}
