// Les messages de `[[` « Créer la page » (Task C4, R619 revu, R631) : une création refusée, en
// attente du compte, ou faite sans que sa mention ait pu être posée. Choisir « Créer la page » retire
// du texte `[[` et le titre tapé : **un message nomme toujours ce titre** quand la mention n'est pas
// posée.
//
// Ils vivent dans ce module, pas dans l'éditeur ni dans ses menus : la réponse du serveur peut
// arriver après que la session de la page a été recréée, ou qu'une autre page a été ouverte, et le
// message doit encore se dire. C'est la page qui les montre (`MentionNotices`, rendu par `PageView`),
// hors du document, au bas de la fenêtre. Ils restent jusqu'à ce qu'on les ferme : par le bouton, ou
// par Échap **quand le focus est dans le message** ; le focus revient alors au texte (`focusBack`),
// jamais à `body`. Un Échap tapé ailleurs (fermer un menu, un panneau, la fenêtre de connexion) ne les
// ferme pas : le titre tapé ne serait plus nommé nulle part (R632). Rien d'autre que React et une
// icône n'est chargé ici : la page le rend sans l'éditeur.
import { useCallback, useRef, useSyncExternalStore } from "react";
import { CloseIcon } from "../icons";

/**
 * Ce par quoi est rompue l'attente du compte d'une création quand sa page est quittée (`PageView`) :
 * rien ne sera créé, et le message d'attente doit cesser de le promettre.
 */
export const PAGE_LEFT = Symbol("page quittée");

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

/**
 * Remplace le message `id` par `text` (à sa place), ou le retire (`null`). Rend faux, sans rien
 * faire, s'il a été fermé entre-temps : à l'appelant de dire autrement ce qu'il avait à dire.
 */
export function replaceNotice(id: number, text: string | null): boolean {
  if (!notices.some((notice) => notice.id === id)) return false;
  set(text === null ? notices.filter((n) => n.id !== id) : notices.map((n) => (n.id === id ? { id, text } : n)));
  return true;
}

/**
 * L'issue d'une demande : `text` prend la place du message `id` qui la faisait attendre (`null` :
 * il n'y en avait pas), ou vient en message neuf si celui-là a été fermé ; `null` : plus rien à dire.
 */
export function settleNotice(id: number | null, text: string | null): void {
  const replaced = id !== null && replaceNotice(id, text);
  if (!replaced && text !== null) postNotice(text);
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
  if (shown.length === 0) return null;
  return (
    // Échap n'est écouté que là : tapé ailleurs, il appartient à ce qu'il ferme.
    <div
      ref={box}
      className="mm-mention-notice"
      role="alert"
      onKeyDown={(event) => {
        if (event.key === "Escape") close();
      }}
    >
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
