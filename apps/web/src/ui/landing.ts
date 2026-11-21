// Le titre d'un écran qui prend la place d'une page (« Tu n'as plus accès à cette page », « Page
// introuvable », « Espace introuvable »…) : là où va le focus quand ce qui l'avait est parti avec la
// page (R1579, R1591, R1592 ; recette du lot 4, E5).
//
// - `giveBack` (`search/focusBack.ts`) l'y rend quand la fenêtre de recherche se ferme et que son
//   opener a quitté le document, si un tel titre est monté.
// - Sinon (la fenêtre s'est fermée avant que l'écran soit monté : l'ordre des événements n'est pas
//   garanti), `awaitLanding` note une arrivée en attente, deux secondes au plus : le premier titre
//   d'écran qui se monte la prend, si le focus est alors sur `body` (`useLanding`). Un écran de
//   passage (« Chargement… ») ne la prend pas.

import { useEffect, type RefObject } from "react";

/** L'attribut du titre d'un écran qui remplace la page : focalisable par le script seulement. */
export const LANDING = "data-focus-landing";

/** Combien de temps une arrivée attend le titre d'un écran (ms). */
export const LANDING_WAIT_MS = 2_000;

/** L'heure (`Date.now`) jusqu'à laquelle un titre d'écran monté prend le focus ; 0 : aucune attente. */
let awaitedUntil = 0;

/** Le focus n'a trouvé où revenir : le premier titre d'écran monté bientôt le prendra. */
export function awaitLanding(): void {
  awaitedUntil = Date.now() + LANDING_WAIT_MS;
}

/** Le titre d'écran le plus récent du document, s'il y en a un. */
export function landingTitle(): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[${LANDING}]`);
}

/**
 * Le titre `title` d'un écran qui remplace la page prend, à son montage, une arrivée en attente
 * (`awaitLanding`) si le focus est sur `body`. `enabled` faux : un écran de passage, qui la laisse.
 */
export function useLanding(title: RefObject<HTMLElement | null>, enabled: boolean): void {
  useEffect(() => {
    if (!enabled || awaitedUntil === 0) return;
    const awaited = Date.now() <= awaitedUntil;
    awaitedUntil = 0;
    const now = document.activeElement;
    if (awaited && (now === null || now === document.body)) title.current?.focus({ preventScroll: true });
  }, [title, enabled]);
}
