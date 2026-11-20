// Routeur minimal (History API) : `/` (le dernier espace ouvert, sinon le premier),
// `/s/:spaceId` (un espace), `/s/:spaceId/trash` (sa corbeille), `/s/:spaceId/settings` (ses
// réglages), `/p/:id` (une page : l'espace affiché est le sien ; `/p/:id#b=<bloc>` : la page, à
// ce bloc, pour un résultat de recherche ou un rétrolien), `/login` et `/signup`
// (connexion et inscription, montrées par `SessionGate` sans session), `/invite` (un lien
// d'invitation, `/invite#<jeton>`, avec ou sans session). Les liens internes restent de vrais
// `<a href>` (nouvel onglet, copie du lien) ; un clic simple est intercepté pour changer de
// page sans recharger l'application.
import { useEffect, useSyncExternalStore } from "react";

export type Route =
  | { name: "home" }
  | { name: "login" }
  | { name: "signup" }
  | { name: "invite" }
  | { name: "page"; id: string }
  | { name: "space"; spaceId: string }
  | { name: "trash"; spaceId: string }
  | { name: "spaceSettings"; spaceId: string }
  | { name: "notFound" };

const CHANGE = "mementum:navigate";

/** Le segment d'adresse décodé, ou `null` s'il est mal formé. */
function segment(raw: string): string | null {
  try {
    return decodeURIComponent(raw);
  } catch {
    return null;
  }
}

export function parseRoute(pathname: string): Route {
  if (pathname === "/" || pathname === "") return { name: "home" };
  if (/^\/login\/?$/.test(pathname)) return { name: "login" };
  if (/^\/signup\/?$/.test(pathname)) return { name: "signup" };
  if (/^\/invite\/?$/.test(pathname)) return { name: "invite" };
  const page = /^\/p\/([^/]+)\/?$/.exec(pathname);
  if (page) {
    const id = segment(page[1]);
    return id === null ? { name: "notFound" } : { name: "page", id };
  }
  const space = /^\/s\/([^/]+)(?:\/(settings|trash))?\/?$/.exec(pathname);
  if (space) {
    const spaceId = segment(space[1]);
    if (spaceId === null) return { name: "notFound" };
    if (space[2] === "settings") return { name: "spaceSettings", spaceId };
    return space[2] === "trash" ? { name: "trash", spaceId } : { name: "space", spaceId };
  }
  return { name: "notFound" };
}

export function pagePath(id: string): string {
  return `/p/${encodeURIComponent(id)}`;
}

export function spacePath(spaceId: string): string {
  return `/s/${encodeURIComponent(spaceId)}`;
}

export function trashPath(spaceId: string): string {
  return `/s/${encodeURIComponent(spaceId)}/trash`;
}

export function spaceSettingsPath(spaceId: string): string {
  return `/s/${encodeURIComponent(spaceId)}/settings`;
}

/** Ce qui précède, dans le fragment d'une adresse de page, l'identifiant du bloc visé. */
const BLOCK_PREFIX = "#b=";

/**
 * L'adresse de la page `pageId`, à son bloc `blockId` (`/p/<page>#b=<bloc>`) ; sans bloc (`null`),
 * celle de la page. L'identifiant d'un bloc est un texte écrit par un client, pas forcément un
 * UUID : il est toujours échappé. C'est ici, et nulle part ailleurs, qu'un bloc entre dans une
 * adresse (un résultat de recherche, un rétrolien).
 */
export function blockPath(pageId: string, blockId: string | null): string {
  return `${pagePath(pageId)}${blockFragment(blockId)}`;
}

/**
 * Le fragment qui vise le bloc `blockId` dans la page où l'on est (`#b=<bloc>`), pour un lien de la
 * page vers un de ses blocs (la table des matières) ; vide sans bloc.
 */
export function blockFragment(blockId: string | null): string {
  return blockId === null || blockId === "" ? "" : `${BLOCK_PREFIX}${encodeURIComponent(blockId)}`;
}

/**
 * Le plus long fragment lu (caractères, `#` compris). Un identifiant de bloc fait cent octets au
 * plus, trois cents caractères une fois échappé : au-delà d'un Kio, ce n'est pas un bloc.
 */
const MAX_FRAGMENT_CHARS = 1_024;

/**
 * Le bloc que vise le fragment `hash` (`location.hash`, `#` compris) : `null` s'il n'en vise aucun
 * (pas de fragment, un fragment d'une autre forme ou trop long, un bloc vide ou mal échappé).
 */
export function parseBlockTarget(hash: string): string | null {
  if (hash.length > MAX_FRAGMENT_CHARS || !hash.startsWith(BLOCK_PREFIX)) return null;
  const blockId = segment(hash.slice(BLOCK_PREFIX.length));
  return blockId === null || blockId === "" ? null : blockId;
}

/**
 * Le bloc visé par l'adresse. Un objet neuf à chaque fois que l'adresse vise un bloc : une
 * navigation vers un bloc, la même refaite comprise (`navigate`). Qui le suit le fait une fois par
 * objet.
 */
export type BlockTarget = { readonly pageId: string; readonly blockId: string };

/** La visée de l'adresse courante, gardée tant que l'adresse vise le même bloc de la même page. */
let aimed: BlockTarget | null = null;
/** L'adresse (chemin et fragment) dont `aimed` a été lu : elle n'est décodée qu'une fois. */
let aimedAt: string | null = null;

function blockTarget(): BlockTarget | null {
  // Le magasin est relu à chaque rendu de qui le suit : rien n'est décodé tant que l'adresse tient.
  const address = `${location.pathname}${location.hash}`;
  if (address === aimedAt) return aimed;
  aimedAt = address;
  const route = parseRoute(location.pathname);
  const blockId = route.name === "page" ? parseBlockTarget(location.hash) : null;
  if (route.name !== "page" || blockId === null) aimed = null;
  else if (aimed === null || aimed.pageId !== route.id || aimed.blockId !== blockId) {
    aimed = { pageId: route.id, blockId };
  }
  return aimed;
}

/**
 * Va à `path` ; `replace` : à la place de l'entrée courante de l'historique. Le fragment compte :
 * un autre bloc de la page ouverte est une autre adresse. Déjà à cette adresse, rien n'entre dans
 * l'historique ; si elle vise un bloc, il est visé de nouveau (la personne a pu faire défiler la
 * page depuis), sinon rien ne se passe.
 */
export function navigate(path: string, { replace = false }: { replace?: boolean } = {}): void {
  if (path === location.pathname + location.search + location.hash) {
    const again = blockTarget();
    if (again === null) return;
    aimed = { ...again };
  } else if (replace) history.replaceState(null, "", path);
  else history.pushState(null, "", path);
  window.dispatchEvent(new Event(CHANGE));
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener("popstate", onChange);
  window.addEventListener(CHANGE, onChange);
  return () => {
    window.removeEventListener("popstate", onChange);
    window.removeEventListener(CHANGE, onChange);
  };
}

/** Le chemin courant ; le composant se rend de nouveau à chaque navigation. */
export function usePathname(): string {
  return useSyncExternalStore(subscribe, () => location.pathname);
}

function subscribeToFragment(onChange: () => void): () => void {
  const stop = subscribe(onChange);
  // Un fragment changé à la main dans la barre d'adresse, ou par un lien que rien n'intercepte.
  window.addEventListener("hashchange", onChange);
  return () => {
    stop();
    window.removeEventListener("hashchange", onChange);
  };
}

/**
 * Le bloc de la page `pageId` que vise l'adresse (`#b=<bloc>`), ou `null`. Suit les navigations, le
 * retour en arrière et un fragment changé à la main ; ce qui ne lit que le chemin (`usePathname`)
 * n'est pas rendu de nouveau quand seul le bloc change : la page ouverte n'est pas remontée.
 */
export function useBlockTarget(pageId: string): BlockTarget | null {
  const target = useSyncExternalStore(subscribeToFragment, blockTarget);
  return target !== null && target.pageId === pageId ? target : null;
}

/** Intercepte les clics simples sur les liens internes de la page. */
export function useLinkInterception(): void {
  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      if (event.defaultPrevented || event.button !== 0) return;
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      const anchor = (event.target as Element | null)?.closest?.("a[href]");
      if (!(anchor instanceof HTMLAnchorElement) || anchor.target || anchor.hasAttribute("download")) {
        return;
      }
      // Un lien dans du texte modifiable appartient à l'éditeur (curseur, barre de lien) ;
      // seul un îlot non modifiable, comme le cadre d'un `pageLink`, navigue.
      if (anchor.closest("[contenteditable]")?.getAttribute("contenteditable") === "true") return;
      const url = new URL(anchor.href);
      if (url.origin !== location.origin) return;
      // Un lien d'invitation porte son jeton dans le fragment : il se suit en chargeant la page,
      // qui lit le jeton. Le fragment d'une page (le bloc visé, `#b=`) est gardé.
      const target = parseRoute(url.pathname).name;
      if (target === "notFound" || target === "invite") return;
      event.preventDefault();
      navigate(url.pathname + url.search + url.hash);
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, []);
}
