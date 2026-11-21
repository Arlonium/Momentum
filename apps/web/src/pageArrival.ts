// Les arrivées sur une page sans bloc (R292, R1128, R1580, R1589) : un geste mène à une page, et ce
// qui avait le focus part avec la page quittée. Le fil d'Ariane de la page d'arrivée prend alors le
// focus, et la page est montrée en haut (`page/Breadcrumb.tsx`, qui lit ce registre).
//
// Ce module ne dépend de rien : le routeur (`useLinkInterception`, un lien de page ou une mention
// suivis depuis le contenu de la page), la fenêtre de recherche et les rétroliens y notent une
// arrivée sans dépendre du fil d'Ariane.

/** La page qu'un geste vient d'ouvrir, sans bloc ; `null` : aucune. */
let arriving: string | null = null;
/** Les fils d'Ariane montés, prévenus d'une arrivée : celui de la page ouverte la suit sans remonter. */
const listeners = new Set<() => void>();

/**
 * Un geste mène à la page `id`, sans bloc : son fil d'Ariane prendra le focus, à son montage, ou tout
 * de suite s'il est déjà monté (la page ouverte n'est pas remontée, R1104).
 */
export function arriveAt(id: string): void {
  arriving = id;
  for (const listener of listeners) listener();
}

/** L'arrivée notée vise la page `id` ; elle est alors prise (effacée). */
export function takeArrival(id: string): boolean {
  if (arriving !== id) return false;
  arriving = null;
  return true;
}

/** Efface toute arrivée notée : un fil d'Ariane qui se monte sans elle ne la laisse pas traîner. */
export function forgetArrival(): void {
  arriving = null;
}

/** Prévient `listener` de chaque arrivée notée ; rend de quoi cesser. */
export function onArrival(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
