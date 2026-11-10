// La version de schéma du serveur, vue de l'application. Elle est lue au chargement, puis
// relue avant chaque connexion de synchronisation (R35, `schemaGate.ts`) : si elle diffère de
// celle de ce client, tout passe en lecture seule, pour de bon. Une première lecture qui
// échoue se refait à la demande (`retry`), sans recharger l'application.
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
  /** Refait la première lecture, qui a échoué. */
  retry(): void;
};

export function useServerSchema(): ServerSchema {
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const staleRef = useRef(false);
  /** Compte les premières lectures refaites à la demande. */
  const [retries, setRetries] = useState(0);

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
    setError(null);
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
  }, [markStale, retries]);

  const isStale = useCallback(() => staleRef.current, []);
  const retry = useCallback(() => setRetries((n) => n + 1), []);
  return { loaded, error, stale, isStale, check, retry };
}
