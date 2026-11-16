// Les messages de `[[` « Créer la page » (Task C4, R619 revu, R631) : une création refusée, en
// attente du compte, ou faite sans que sa mention ait pu être posée. Choisir « Créer la page » retire
// du texte `[[` et le titre tapé : **un message nomme toujours ce titre** quand la mention n'est pas
// posée.
//
// Ils vivent dans ce module, pas dans l'éditeur ni dans ses menus : la réponse du serveur peut
// arriver après que la session de la page a été recréée, ou qu'une autre page a été ouverte, et le
// message doit encore se dire. C'est la page qui les montre (`MentionNotices`, rendu par `PageView`),
// hors du document, au bas de la fenêtre. Ils restent jusqu'à ce qu'on les ferme : le bouton, ou
// Échap, où que soit le focus ; s'il était dans le message, il revient au texte (`focusBack`), jamais
// à `body`. Rien d'autre que React et une icône n'est chargé ici : la page le rend sans l'éditeur.
import { useCallback, useEffect, useRef, useSyncExternalStore } from "react";
import { CloseIcon } from "../icons";

type Notice = { readonly id: number; readonly text: string };

let notices: readonly Notice[] = [];
let posted = 0;
const listeners = new Set<() => void>();

function set(next: readonly Notice[]): void {
  notices = next;
  for (const listener of [...listeners]) listener();
}

/** Dit `text`, à la suite des messages déjà là ; rend de quoi le remplacer ou le retirer. */
export function postNotice(text: string): number {
  posted += 1;
  set([...notices, { id: posted, text }]);
  return posted;
}

/** Remplace le message `id` par `text` (à sa place), ou le retire (`null`) ; sans effet s'il est déjà fermé. */
export function replaceNotice(id: number, text: string | null): void {
  if (!notices.some((notice) => notice.id === id)) return;
  set(text === null ? notices.filter((n) => n.id !== id) : notices.map((n) => (n.id === id ? { id, text } : n)));
}

/** Ferme tous les messages. */
export function clearNotices(): void {
  if (notices.length > 0) set([]);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Les messages en cours, s'il y en a. `focusBack` : où rendre le focus quand il était dans le
 * message qu'on ferme (le texte de la page).
 */
export function MentionNotices({ focusBack }: { focusBack: () => void }) {
  const shown = useSyncExternalStore(subscribe, () => notices);
  const box = useRef<HTMLDivElement>(null);
  // Une seule fermeture, pour le bouton comme pour Échap.
  const close = useCallback(() => {
    const inside = box.current?.contains(document.activeElement) ?? false;
    clearNotices();
    // Le bouton part avec le message : le focus revient au texte, pas à `body`.
    if (inside) focusBack();
  }, [focusBack]);
  const open = shown.length > 0;
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, close]);
  if (!open) return null;
  return (
    <div ref={box} className="mm-mention-notice" role="alert">
      <div className="mm-mention-notice-texts">
        {shown.map((notice) => (
          <p key={notice.id} dir="auto">
            {notice.text}
          </p>
        ))}
      </div>
      <button type="button" className="mm-icon-button" aria-label="Fermer le message" onClick={close}>
        <CloseIcon />
      </button>
    </div>
  );
}
