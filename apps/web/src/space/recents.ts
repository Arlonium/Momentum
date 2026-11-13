// Les pages récentes (R255) : gardées dans le navigateur (`localStorage`), par compte et par
// espace, vingt identifiants au plus, la plus récente d'abord, et rien d'autre ; la barre
// latérale montre les cinq premières que l'arbre contient. Rien n'est lu ni écrit sur le
// serveur : un journal « qui a ouvert quoi, quand » n'y est pas tenu. Les identifiants sont
// filtrés par l'arbre, déjà filtré par les droits : une page qu'on ne voit plus n'est pas
// montrée, sans être retirée, et revient avec l'accès.
//
// Sur un navigateur partagé, ceux d'un compte ne restent pas après lui (R297) : la déconnexion
// efface les siens (`forgetRecents`), et l'entrée d'un compte, ceux de tous les autres
// (`forgetOtherRecents`, appelée par `SessionGate` à chaque entrée). Une visite ne s'écrit que
// pour le compte entré dans l'onglet, tant qu'aucun autre onglet n'a dit que la session du
// navigateur n'est plus la sienne : entrer et se déconnecter s'annoncent aux autres onglets
// par une clé posée puis retirée aussitôt, où rien ne reste. La limite : une session qui se
// perd sans déconnexion dans ce navigateur (échue, fermée ailleurs) laisse les identifiants
// du compte jusqu'à l'entrée du compte suivant.
//
// Un stockage refusé ou illisible ne lève rien : la liste est vide.
import type { PageNode } from "@mementum/protocol";
import { useMemo, useSyncExternalStore } from "react";

/** Les identifiants gardés, par compte et par espace. */
export const RECENTS_KEPT = 20;
/** Les pages montrées dans la barre latérale : les premières que l'arbre contient. */
export const RECENTS_SHOWN = 5;

const PREFIX = "mementum.recents.";

/**
 * Le signal entre les onglets du navigateur : posé puis retiré aussitôt, il porte le compte
 * qui vient d'entrer, ou `""` quand celui d'un onglet se déconnecte.
 */
const SIGNAL = "mementum.recents-account";

export function recentsKey(userId: string, spaceId: string): string {
  return `${PREFIX}${userId}.${spaceId}`;
}

/** Le compte entré dans cet onglet ; `null` : aucun. */
let entered: string | null = null;
/**
 * Le compte dont les visites s'écrivent : celui de l'onglet, tant que la session du navigateur
 * est la sienne à ce que l'onglet en sait ; `null` : aucune ne s'écrit.
 */
let writer: string | null = null;

/** Change à chaque visite et à chaque oubli d'ici, ou à chaque écriture d'un autre onglet. */
let version = 0;
const listeners = new Set<() => void>();
let listening = false;

function changed(): void {
  version += 1;
  for (const listener of listeners) listener();
}

/** Un autre onglet du navigateur a écrit des pages récentes, ou annonce un compte. */
function onStorage(event: StorageEvent): void {
  if (event.key === SIGNAL) {
    // Le retrait du signal ne dit rien ; sa pose dit qui a la session, ou que personne.
    if (event.newValue !== null) writer = event.newValue === entered ? entered : null;
    return;
  }
  if (event.key === null || event.key.startsWith(PREFIX)) changed();
}

/** Écoute les autres onglets, une fois pour toute la vie de l'onglet. */
function listen(): void {
  if (listening) return;
  listening = true;
  window.addEventListener("storage", onStorage);
}

function subscribe(listener: () => void): () => void {
  listen();
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Annonce aux autres onglets le compte qui entre (`""` : celui qui part), sans rien garder. */
function announce(account: string): void {
  try {
    localStorage.setItem(SIGNAL, account);
    localStorage.removeItem(SIGNAL);
  } catch {
    // stockage refusé : rien n'y est gardé, il n'y a rien à dire
  }
}

/** Retire du stockage les clés que `doomed` désigne. */
function removeKeys(doomed: (key: string) => boolean): void {
  try {
    const keys: string[] = [];
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key !== null && doomed(key)) keys.push(key);
    }
    for (const key of keys) localStorage.removeItem(key);
  } catch {
    // stockage refusé : rien n'y était gardé
  }
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

/**
 * La page vient d'être ouverte : elle passe en tête des pages récentes du compte. Rien ne
 * s'écrit pour un autre compte que celui de l'onglet, ni après son départ.
 */
export function visit(userId: string, spaceId: string, pageId: string): void {
  if (writer !== userId) return;
  const ids = [pageId, ...readRecents(userId, spaceId).filter((id) => id !== pageId)];
  try {
    localStorage.setItem(recentsKey(userId, spaceId), JSON.stringify(ids.slice(0, RECENTS_KEPT)));
  } catch {
    // stockage refusé : rien n'est gardé
  }
  changed();
}

/**
 * Le compte entre dans l'onglet (chargement, connexion, inscription, session rouverte) : les
 * pages récentes de tous les autres comptes quittent le navigateur, et ses visites s'écrivent.
 */
export function forgetOtherRecents(userId: string): void {
  listen();
  entered = userId;
  writer = userId;
  // Le point final : le compte `u1` garde les siennes, pas celles du compte `u10`.
  const own = `${PREFIX}${userId}.`;
  removeKeys((key) => key.startsWith(PREFIX) && !key.startsWith(own));
  announce(userId);
  changed();
}

/**
 * Le compte se déconnecte : ses pages récentes, dans tous ses espaces, sont effacées, et plus
 * rien ne s'écrit, ici ni dans ses autres onglets.
 */
export function forgetRecents(userId: string): void {
  writer = null;
  const own = `${PREFIX}${userId}.`;
  removeKeys((key) => key.startsWith(own));
  announce("");
  changed();
}

/** Aucune page récente : toujours le même tableau. */
const NONE: PageNode[] = [];

/**
 * Les pages récentes du compte dans l'espace que l'arbre `nodes` contient (`null` : pas encore
 * lu), cinq au plus, la plus récente d'abord. Suit les visites d'ici et des autres onglets.
 */
export function useRecentPages(
  userId: string,
  spaceId: string | null,
  nodes: PageNode[] | null,
): PageNode[] {
  const seen = useSyncExternalStore(subscribe, () => version);
  // `seen` n'est lu que comme dépendance : les identifiants sont relus à chaque visite.
  const ids = useMemo(
    () => (spaceId === null ? [] : readRecents(userId, spaceId)),
    [userId, spaceId, seen],
  );
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
