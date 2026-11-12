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
// Une relecture qui rend le même arbre (les mêmes pages, les mêmes champs, le même ordre) ne
// change rien : l'arbre chargé reste le même objet, et la barre latérale n'est pas rendue de
// nouveau. Un menu ouvert, un glisser en cours ou le focus n'en sont pas dérangés.
//
// Une lecture qui échoue (serveur injoignable) ne retire rien de l'écran : l'arbre chargé
// reste, l'échec est dit, et la lecture repart d'elle-même quand le réseau revient.
//
// Ce que d'autres changent dans l'espace arrive par le canal de l'espace (`useSpaceEvents`,
// Ruling R133) : quand la version qu'il pousse dépasse celle de l'arbre affiché (`knownLsn`,
// la version que le serveur a donnée avec l'arbre, R141), l'arbre est relu. Le canal tombé,
// l'arbre est relu par repli :
// - quand l'onglet redevient visible ou reprend le focus, au plus une fois toutes les cinq
//   secondes (Ruling R97), canal ou non ;
// - toutes les dix secondes tant que l'onglet est visible (Ruling R128), seulement quand le
//   canal est tombé (`live` faux) : sans lui, à deux sur un espace, sans changer de fenêtre,
//   rien d'autre ne déclencherait la relecture. Le minuteur repart de la dernière lecture
//   partie, quelle qu'en soit la raison, et ne lit ni pour un onglet caché, ni par-dessus une
//   lecture en route. Il reprend dès que le canal tombe.
// Le compte est relu au retour sur l'onglet (`onReturn`, Ruling R126), pas par le minuteur :
// son rôle dans l'espace a pu changer, et la session a pu être rouverte sous un autre compte
// dans un autre onglet. Il a son propre compte de temps, cinq secondes aussi : une lecture de
// l'arbre par le minuteur ne l'en prive pas au retour qui la suit. Une nouvelle version de
// l'espace le relit aussi (`rereadAccount`, Ruling R150) : le nom de l'espace et les rôles ne
// sont pas dans l'arbre. Avec la même limite ; un appel qu'elle retient est rejoué à son
// échéance, pour que le dernier changement finisse toujours par arriver.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { ApiError, getTree, isSessionLost } from "../api";
import { messageOf } from "../session/forms";

/** L'arbre chargé : les pages d'un espace que la personne voit, hors corbeille. */
export type LoadedTree = { spaceId: string; nodes: PageNode[] };

/** Les mêmes pages, avec les mêmes champs, dans le même ordre ? */
export function sameNodes(a: PageNode[], b: PageNode[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((node, index) => {
    const other = b[index];
    const fields = Object.keys(node) as (keyof PageNode)[];
    return (
      fields.length === Object.keys(other).length &&
      fields.every((field) => node[field] === other[field])
    );
  });
}

/** Au retour sur l'onglet, l'arbre n'est relu que si sa dernière lecture a au moins cet âge. */
export const REFRESH_INTERVAL_MS = 5_000;

/**
 * Quand le canal de l'espace est tombé, et tant que l'onglet est visible, l'arbre est relu
 * quand sa dernière lecture a cet âge (Ruling R128) : ce que d'autres créent, renomment ou
 * déplacent arrive en dix secondes au plus, sans changer de fenêtre. Un repli : canal ouvert,
 * le serveur le pousse.
 */
export const POLL_INTERVAL_MS = 10_000;

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
  /**
   * La version de l'espace que le serveur a donnée avec l'arbre affiché de `spaceId` (R141) ;
   * `null` : pas d'arbre lu pour cet espace. Le canal de l'espace la compare à ses trames.
   */
  knownLsn(spaceId: string): number | null;
  /**
   * Relit le compte (noms des espaces, rôles) après une nouvelle version de l'espace, par le
   * chemin du retour sur l'onglet et avec sa limite (R150).
   */
  rereadAccount(): void;
  /** Pour `useTreeReader` : le compte des changements, celui des relectures, la pose. */
  changes: RefObject<number>;
  reloads: number;
  set(tree: LoadedTree): void;
  lsn: RefObject<{ spaceId: string; lsn: number } | null>;
  account: RefObject<() => void>;
};

export function useSpaceTree(): SpaceTree {
  const [loaded, setLoaded] = useState<LoadedTree | null>(null);
  const latest = useRef(loaded);
  latest.current = loaded;
  const changes = useRef(0);
  const [reloads, setReloads] = useState(0);
  const lsn = useRef<{ spaceId: string; lsn: number } | null>(null);
  const account = useRef<() => void>(() => {});

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
  const knownLsn = useCallback(
    (spaceId: string) => (lsn.current?.spaceId === spaceId ? lsn.current.lsn : null),
    [],
  );
  const rereadAccount = useCallback(() => account.current(), []);

  return useMemo(
    () => ({
      loaded,
      latest: read,
      change,
      touch,
      reload,
      knownLsn,
      rereadAccount,
      changes,
      reloads,
      set: setLoaded,
      lsn,
      account,
    }),
    [loaded, read, change, touch, reload, knownLsn, rereadAccount, reloads],
  );
}

type Reading = {
  /** Change quand une lecture perdue avec la session peut être refaite. */
  retries: number;
  /** La lecture a répondu `401 unauthenticated`. */
  onSessionLost: () => void;
  /** L'espace répond `404` : le compte n'y a plus de rôle. */
  onGone: () => void;
  /** De retour sur l'onglet, l'arbre est relu : ce qui se relit avec lui (le compte). */
  onReturn: () => void;
  /** Le canal de l'espace est ouvert : il pousse les changements, le minuteur se tait. */
  live?: boolean;
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
  const { changes, reloads, set, reload, latest, lsn, account } = tree;
  const { retries } = reading;
  const live = reading.live === true;
  // Les rappels du dernier rendu : en changer ne relance pas la lecture.
  const callbacks = useRef(reading);
  callbacks.current = reading;
  const [error, setError] = useState<string | null>(null);
  /** Quand la dernière lecture est partie (millisecondes Unix). */
  const readAt = useRef(0);
  /** Le rang de la lecture en route ; 0 : aucune. */
  const pending = useRef(0);
  const ranks = useRef(0);

  useEffect(() => {
    if (spaceId === null) return;
    let cancelled = false;
    readAt.current = Date.now();
    const rank = ++ranks.current;
    pending.current = rank;
    /** Cette lecture est finie ou abandonnée : si c'est toujours elle, plus rien n'est en route. */
    const settled = () => {
      if (pending.current === rank) pending.current = 0;
    };
    const read = async () => {
      for (;;) {
        const before = changes.current;
        const fetched = await getTree(spaceId);
        if (cancelled) return;
        // Une page créée ou renommée ici pendant la lecture : la réponse date d'avant.
        if (before !== changes.current) continue;
        // Le même arbre que celui qui est chargé : il reste, tel quel.
        const loaded = latest();
        // La version suit même quand l'arbre ne change pas : un changement qu'on ne voit pas
        // (une page privée d'un autre) ne doit pas faire relire à chaque trame.
        const previous = lsn.current;
        lsn.current = {
          spaceId,
          lsn: previous?.spaceId === spaceId ? Math.max(previous.lsn, fetched.lsn) : fetched.lsn,
        };
        const unchanged =
          loaded !== null && loaded.spaceId === spaceId && sameNodes(loaded.nodes, fetched.nodes);
        if (!unchanged) set({ spaceId, nodes: fetched.nodes });
        setError(null);
        return;
      }
    };
    read()
      .catch((e: unknown) => {
        if (cancelled) return;
        if (isSessionLost(e)) callbacks.current.onSessionLost();
        else if (e instanceof ApiError && e.status === 404) callbacks.current.onGone();
        else setError(messageOf(e));
      })
      .finally(settled);
    return () => {
      cancelled = true;
      settled();
    };
  }, [spaceId, reloads, retries, changes, set, latest, lsn]);

  // Une lecture a échoué : elle repart quand le navigateur retrouve le réseau.
  const failed = error !== null;
  useEffect(() => {
    if (!failed) return;
    window.addEventListener("online", reload);
    return () => window.removeEventListener("online", reload);
  }, [failed, reload]);

  // De retour sur l'onglet : ce que d'autres ont créé, renommé ou déplacé entre-temps (R97),
  // et le compte (R126).
  const known = spaceId !== null;
  /** Quand le compte a été relu pour la dernière fois ; d'abord au chargement de l'application. */
  const returnedAt = useRef(Date.now());
  // Une nouvelle version de l'espace (R150) : le compte est relu avec la même limite, et un
  // appel retenu est rejoué à son échéance. Un seul rejeu en attente, quel que soit le nombre
  // d'appels qu'il remplace.
  useEffect(() => {
    let replay: ReturnType<typeof setTimeout> | undefined;
    const request = () => {
      clearTimeout(replay);
      replay = undefined;
      const wait = returnedAt.current + REFRESH_INTERVAL_MS - Date.now();
      if (wait > 0) {
        replay = setTimeout(request, wait);
        return;
      }
      returnedAt.current = Date.now();
      callbacks.current.onReturn();
    };
    account.current = request;
    return () => {
      clearTimeout(replay);
      account.current = () => {};
    };
  }, [account]);
  useEffect(() => {
    if (!known) return;
    const refresh = () => {
      if (document.visibilityState !== "visible") return;
      const now = Date.now();
      // Deux signaux du même retour (visible, puis focus) ne font qu'une lecture. Et pas de
      // lecture coup sur coup avec celle du minuteur, ou d'une action.
      if (now - readAt.current >= REFRESH_INTERVAL_MS) {
        readAt.current = now;
        reload();
      }
      // Le compte a son propre compte de temps : le minuteur relit l'arbre sans lui.
      if (now - returnedAt.current >= REFRESH_INTERVAL_MS) {
        returnedAt.current = now;
        callbacks.current.onReturn();
      }
    };
    document.addEventListener("visibilitychange", refresh);
    window.addEventListener("focus", refresh);
    return () => {
      document.removeEventListener("visibilitychange", refresh);
      window.removeEventListener("focus", refresh);
    };
  }, [known, reload]);

  // Canal tombé, tant que l'onglet est visible : dix secondes après la dernière lecture
  // partie (R128).
  useEffect(() => {
    if (!known || live) return;
    let timer: ReturnType<typeof setTimeout>;
    const arm = () => {
      const age = Date.now() - readAt.current;
      // Une lecture plus ancienne que cela n'a pas été refaite (onglet caché, lecture en
      // route) : on y revient dans dix secondes.
      timer = setTimeout(tick, age < POLL_INTERVAL_MS ? POLL_INTERVAL_MS - age : POLL_INTERVAL_MS);
    };
    const tick = () => {
      const due = Date.now() - readAt.current >= POLL_INTERVAL_MS;
      if (due && pending.current === 0 && document.visibilityState === "visible") {
        readAt.current = Date.now();
        reload();
      }
      arm();
    };
    arm();
    return () => clearTimeout(timer);
  }, [known, live, reload]);

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
