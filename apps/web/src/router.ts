// Routeur minimal (History API) : `/` (accueil), `/p/:id` (une page), `/login` et `/signup`
// (connexion et inscription, montrées par `SessionGate` sans session). Les liens internes
// restent de vrais `<a href>` (nouvel onglet, copie du lien) ; un clic simple est
// intercepté pour changer de page sans recharger l'application.
import { useEffect, useSyncExternalStore } from "react";

export type Route =
  | { name: "home" }
  | { name: "login" }
  | { name: "signup" }
  | { name: "page"; id: string }
  | { name: "notFound" };

const CHANGE = "mementum:navigate";

export function parseRoute(pathname: string): Route {
  if (pathname === "/" || pathname === "") return { name: "home" };
  if (/^\/login\/?$/.test(pathname)) return { name: "login" };
  if (/^\/signup\/?$/.test(pathname)) return { name: "signup" };
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
      if (url.origin !== location.origin || parseRoute(url.pathname).name === "notFound") return;
      event.preventDefault();
      navigate(url.pathname + url.search);
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, []);
}
