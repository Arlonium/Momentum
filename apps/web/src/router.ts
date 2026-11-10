// Routeur minimal (History API) : `/` (le dernier espace ouvert, sinon le premier),
// `/s/:spaceId` (un espace), `/s/:spaceId/settings` (ses réglages), `/p/:id` (une page :
// l'espace affiché est le sien), `/login` et `/signup` (connexion et inscription, montrées par
// `SessionGate` sans session), `/invite` (un lien d'invitation, `/invite#<jeton>`, avec ou
// sans session). Les liens internes restent de vrais `<a href>` (nouvel onglet, copie du
// lien) ; un clic simple est intercepté pour changer de page sans recharger l'application.
import { useEffect, useSyncExternalStore } from "react";

export type Route =
  | { name: "home" }
  | { name: "login" }
  | { name: "signup" }
  | { name: "invite" }
  | { name: "page"; id: string }
  | { name: "space"; spaceId: string }
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
  const space = /^\/s\/([^/]+)(?:\/(settings))?\/?$/.exec(pathname);
  if (space) {
    const spaceId = segment(space[1]);
    if (spaceId === null) return { name: "notFound" };
    return space[2] === "settings" ? { name: "spaceSettings", spaceId } : { name: "space", spaceId };
  }
  return { name: "notFound" };
}

export function pagePath(id: string): string {
  return `/p/${encodeURIComponent(id)}`;
}

export function spacePath(spaceId: string): string {
  return `/s/${encodeURIComponent(spaceId)}`;
}

export function spaceSettingsPath(spaceId: string): string {
  return `/s/${encodeURIComponent(spaceId)}/settings`;
}

/** Va à `path` ; `replace` : à la place de l'entrée courante de l'historique. */
export function navigate(path: string, { replace = false }: { replace?: boolean } = {}): void {
  if (path === location.pathname + location.search) return;
  if (replace) history.replaceState(null, "", path);
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
      // Un lien d'invitation porte son jeton dans le fragment, que `navigate` ne garde pas :
      // il se suit en chargeant la page, qui lit le jeton.
      const target = parseRoute(url.pathname).name;
      if (target === "notFound" || target === "invite") return;
      event.preventDefault();
      navigate(url.pathname + url.search);
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, []);
}
