// Ce qui change les jetons d'API du compte hors de leur écran : un changement de mot de passe les
// révoque tous (R1221), depuis le menu du compte, pendant que l'écran des jetons peut être ouvert
// derrière la fenêtre. L'écran relit alors sa liste (`useTokens`). Un module à lui, petit : le menu du
// compte l'importe sans tirer l'écran des jetons, chargé à la demande.

const listeners = new Set<() => void>();

/** Les jetons du compte ont pu changer : qui les montre les relit. */
export function announceTokensChanged(): void {
  for (const listener of [...listeners]) listener();
}

/** Appelle `listener` à chaque annonce ; rend la fonction qui le retire. */
export function onTokensChanged(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
