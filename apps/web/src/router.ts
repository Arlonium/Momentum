// Routeur minimal (History API) : `/` (accueil) et `/p/:id` (une page). Les liens internes
// restent de vrais `<a href>` (nouvel onglet, copie du lien) ; un clic simple est
// intercepté pour changer de page sans recharger l'application.
import { useEffect, useSyncExternalStore } from "react";

export type Route = { name: "home" } | { name: "page"; id: string } | { name: "notFound" };

const CHANGE = "mementum:navigate";

export function parseRoute(pathname: string): Route {
  if (pathname === "/" || pathname === "") return { name: "home" };
  const match = /^\/p\/([^/]+)\/?$/.exec(pathname);
  if (match) {
    try {
      return { name: "page", id: decodeURIComponent(match[1]) };
    } catch {
      return { name: "notFound" };
    }
  }
  return { name: "notFound" };
}

export function pagePath(id: string): string {
  return `/p/${encodeURIComponent(id)}`;
}

export function navigate(path: string): void {
  if (path === location.pathname + location.search) return;
  history.pushState(null, "", path);
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
      const url = new URL(anchor.href);
      if (url.origin !== location.origin || parseRoute(url.pathname).name === "notFound") return;
      event.preventDefault();
      navigate(url.pathname + url.search);
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, []);
}
