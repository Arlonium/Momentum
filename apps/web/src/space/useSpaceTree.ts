// L'arbre des pages d'un espace, tel que la barre latérale le montre : ce que le serveur en
// rend (`GET /spaces/{id}/tree`), et ce que les actions d'ici y changent sans attendre une
// relecture (page créée, renommée, déplacée, jetée).
//
// En deux temps, parce que l'espace à lire dépend de la page ouverte, que l'arbre lui-même
// peut porter : `useSpaceTree` tient l'arbre chargé, `useTreeReader` lit celui de l'espace
// affiché, une fois qu'on sait lequel c'est.
//
// Deux lectures ne se croisent pas : celle d'un autre espace, ou qu'une relecture remplace,
// est abandonnée ; celle qu'un changement fait ici a dépassée (elle date d'avant) est refaite.
//
// Une lecture qui échoue (serveur injoignable) ne retire rien de l'écran : l'arbre chargé
// reste, l'échec est dit, et la lecture repart d'elle-même quand le réseau revient.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { ApiError, getTree, isSessionLost } from "../api";
import { messageOf } from "../session/forms";

/** L'arbre chargé : les pages d'un espace que la personne voit, hors corbeille. */
export type LoadedTree = { spaceId: string; nodes: PageNode[] };

export type SpaceTree = {
  /** L'arbre chargé ; `null` : aucun encore. Ce peut être celui d'un espace qu'on a quitté. */
  loaded: LoadedTree | null;
  /** Le même, à l'instant : pour un rappel qui a attendu le serveur. */
  latest(): LoadedTree | null;
  /**
   * Change les pages de l'arbre chargé (une action d'ici, que le serveur a acceptée). Une
   * lecture commencée avant est périmée : elle sera refaite.
   */
  change(update: (nodes: PageNode[], spaceId: string) => PageNode[]): void;
  /**
   * Le serveur a changé par une action d'ici que l'arbre chargé ne montre pas encore (une
   * page restaurée) : une lecture commencée avant est périmée.
   */
  touch(): void;
  /** Demande une nouvelle lecture de l'arbre. */
  reload(): void;
  /** Pour `useTreeReader` : le compte des changements, celui des relectures, la pose. */
  changes: RefObject<number>;
  reloads: number;
  set(tree: LoadedTree): void;
};

export function useSpaceTree(): SpaceTree {
  const [loaded, setLoaded] = useState<LoadedTree | null>(null);
  const latest = useRef(loaded);
  latest.current = loaded;
  const changes = useRef(0);
  const [reloads, setReloads] = useState(0);

  const change = useCallback((update: (nodes: PageNode[], spaceId: string) => PageNode[]) => {
    changes.current += 1;
    setLoaded(
      (current) => current && { ...current, nodes: update(current.nodes, current.spaceId) },
    );
  }, []);
  const touch = useCallback(() => {
    changes.current += 1;
  }, []);
  const reload = useCallback(() => setReloads((n) => n + 1), []);
  const read = useCallback(() => latest.current, []);

  return useMemo(
    () => ({ loaded, latest: read, change, touch, reload, changes, reloads, set: setLoaded }),
    [loaded, read, change, touch, reload, reloads],
  );
}

type Reading = {
  /** Change quand une lecture perdue avec la session peut être refaite. */
  retries: number;
  /** La lecture a répondu `401 unauthenticated`. */
  onSessionLost: () => void;
  /** L'espace répond `404` : le compte n'y a plus de rôle. */
  onGone: () => void;
};

/**
 * Lit dans `tree` l'arbre de l'espace `spaceId` (`null` : on ne sait pas encore lequel), et le
 * relit quand `tree.reload()` le demande. Rend le message de la dernière lecture si elle a
 * échoué (`null` sinon) : l'arbre chargé, lui, reste tel qu'il était.
 */
export function useTreeReader(
  tree: SpaceTree,
  spaceId: string | null,
  reading: Reading,
): string | null {
  const { changes, reloads, set, reload } = tree;
  const { retries } = reading;
  // Les rappels du dernier rendu : en changer ne relance pas la lecture.
  const callbacks = useRef(reading);
  callbacks.current = reading;
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (spaceId === null) return;
    let cancelled = false;
    const read = async () => {
      for (;;) {
        const before = changes.current;
        const fetched = await getTree(spaceId);
        if (cancelled) return;
        // Une page créée ou renommée ici pendant la lecture : la réponse date d'avant.
        if (before !== changes.current) continue;
        set({ spaceId, nodes: fetched.nodes });
        setError(null);
        return;
      }
    };
    read().catch((e: unknown) => {
      if (cancelled) return;
      if (isSessionLost(e)) callbacks.current.onSessionLost();
      else if (e instanceof ApiError && e.status === 404) callbacks.current.onGone();
      else setError(messageOf(e));
    });
    return () => {
      cancelled = true;
    };
  }, [spaceId, reloads, retries, changes, set]);

  // Une lecture a échoué : elle repart quand le navigateur retrouve le réseau.
  const failed = error !== null;
  useEffect(() => {
    if (!failed) return;
    window.addEventListener("online", reload);
    return () => window.removeEventListener("online", reload);
  }, [failed, reload]);

  return error;
}

/**
 * Une page que le serveur rend, hors corbeille, mais que l'arbre chargé de son espace n'a pas
 * (créée ailleurs depuis) : l'arbre est relu, une fois par page ouverte. `nodes` : les pages
 * de l'espace affiché (`null` : pas encore chargées) ; `found` : la page que le serveur a
 * rendue. Rend `skip(id)` : l'arbre vient d'être relu pour cette page, inutile d'y revenir.
 */
export function useTreeCatchUp(
  tree: SpaceTree,
  nodes: PageNode[] | null,
  activeId: string | null,
  found: PageNode | null,
): (id: string) => void {
  const { reload } = tree;
  const [caughtUp, setCaughtUp] = useState<string | null>(null);
  useEffect(() => setCaughtUp(null), [activeId]);
  useEffect(() => {
    if (found === null || found.trashedAt !== null || nodes === null) return;
    if (caughtUp === found.id || nodes.some((n) => n.id === found.id)) return;
    setCaughtUp(found.id);
    reload();
  }, [found, nodes, caughtUp, reload]);
  return setCaughtUp;
}
