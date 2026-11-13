// Les pages récentes (R255) : gardées dans le navigateur (`localStorage`), par compte et par
// espace, vingt identifiants au plus, la plus récente d'abord, et rien d'autre ; la barre
// latérale montre les cinq premières que l'arbre contient. Rien n'est lu ni écrit sur le
// serveur : un journal « qui a ouvert quoi, quand » n'y est pas tenu. Les identifiants sont
// filtrés par l'arbre, déjà filtré par les droits : une page qu'on ne voit plus n'est pas
// montrée, sans être retirée, et revient avec l'accès. La déconnexion efface ceux du compte
// qui part (`forgetRecents`) : sur un navigateur partagé, ils ne restent pas lisibles.
//
// Un stockage refusé ou illisible ne lève rien : la liste est vide.
import type { PageNode } from "@mementum/protocol";
import { useMemo, useSyncExternalStore } from "react";

/** Les identifiants gardés, par compte et par espace. */
export const RECENTS_KEPT = 20;
/** Les pages montrées dans la barre latérale : les premières que l'arbre contient. */
export const RECENTS_SHOWN = 5;

const PREFIX = "mementum.recents.";

export function recentsKey(userId: string, spaceId: string): string {
  return `${PREFIX}${userId}.${spaceId}`;
}

/** Change à chaque visite et à chaque oubli d'ici, ou à chaque écriture d'un autre onglet. */
let version = 0;
const listeners = new Set<() => void>();

function changed(): void {
  version += 1;
  for (const listener of listeners) listener();
}

/** Un autre onglet du navigateur a écrit des pages récentes. */
function onStorage(event: StorageEvent): void {
  if (event.key === null || event.key.startsWith(PREFIX)) changed();
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) window.addEventListener("storage", onStorage);
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}

/**
 * Les pages récentes gardées, la plus récente d'abord ; `null` : le stockage est refusé (rien
 * n'y est gardé). Une valeur illisible n'en donne aucune.
 */
function readStored(userId: string, spaceId: string): string[] | null {
  let raw: string | null;
  try {
    raw = localStorage.getItem(recentsKey(userId, spaceId));
  } catch {
    return null;
  }
  try {
    const stored: unknown = JSON.parse(raw ?? "[]");
    if (!Array.isArray(stored)) return [];
    const ids = stored.filter((id): id is string => typeof id === "string");
    return [...new Set(ids)].slice(0, RECENTS_KEPT);
  } catch {
    // valeur illisible : aucune
    return [];
  }
}

/** Les pages récentes du compte dans l'espace, la plus récente d'abord. */
export function readRecents(userId: string, spaceId: string): string[] {
  return readStored(userId, spaceId) ?? [];
}

/** La page vient d'être ouverte : elle passe en tête des pages récentes du compte. */
export function visit(userId: string, spaceId: string, pageId: string): void {
  const ids = [pageId, ...readRecents(userId, spaceId).filter((id) => id !== pageId)];
  try {
    localStorage.setItem(recentsKey(userId, spaceId), JSON.stringify(ids.slice(0, RECENTS_KEPT)));
  } catch {
    // stockage refusé : rien n'est gardé
  }
  changed();
}

/** Le compte se déconnecte : ses pages récentes, dans tous ses espaces, sont effacées. */
export function forgetRecents(userId: string): void {
  // Le point final : le compte `u1` n'efface pas les clés du compte `u10`.
  const prefix = `${PREFIX}${userId}.`;
  try {
    const keys: string[] = [];
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key?.startsWith(prefix)) keys.push(key);
    }
    for (const key of keys) localStorage.removeItem(key);
  } catch {
    // stockage refusé : rien n'y était gardé
  }
  changed();
}

/** Aucune page récente : toujours le même tableau. */
const NONE: PageNode[] = [];

/**
 * Les pages récentes du compte dans l'espace que l'arbre `nodes` contient (`null` : pas encore
 * lu), cinq au plus, la plus récente d'abord. Suit les visites d'ici et des autres onglets.
 *
 * La page ouverte (`activeId`) est en tête dès le rendu qui l'ouvre, avant que sa visite ne
 * s'écrive (l'effet d'`App` la suit) : la liste grandit dans le même rendu que l'arbre montre
 * la page et fait défiler la barre jusqu'à elle, et rien ne la déplace après. Stockage refusé :
 * rien n'est gardé, et rien n'est montré, pas même elle.
 */
export function useRecentPages(
  userId: string,
  spaceId: string | null,
  nodes: PageNode[] | null,
  activeId: string | null,
): PageNode[] {
  const seen = useSyncExternalStore(subscribe, () => version);
  // `seen` n'est lu que comme dépendance : les identifiants sont relus à chaque visite.
  const ids = useMemo(() => {
    if (spaceId === null) return [];
    const stored = readStored(userId, spaceId);
    if (stored === null || activeId === null) return stored ?? [];
    return [activeId, ...stored.filter((id) => id !== activeId)];
  }, [userId, spaceId, activeId, seen]);
  // Indexées quand l'arbre change, pas à chaque page ouverte.
  const byId = useMemo(
    () => (nodes === null ? null : new Map(nodes.map((node) => [node.id, node]))),
    [nodes],
  );
  return useMemo(() => {
    if (byId === null || ids.length === 0) return NONE;
    const pages: PageNode[] = [];
    for (const id of ids) {
      const node = byId.get(id);
      if (node === undefined) continue;
      pages.push(node);
      if (pages.length === RECENTS_SHOWN) break;
    }
    return pages.length === 0 ? NONE : pages;
  }, [ids, byId]);
}
