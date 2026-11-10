// Le cache arrière-avant du navigateur (bfcache) garde parfois un document entier, et
// « Précédent » le rend tel quel, sans aucun appel au serveur. Après une déconnexion, ce
// serait l'application connectée : arbre des pages, nom, adresse. Un document rendu ainsi
// se recharge donc, et `SessionGate` relit la session.

/**
 * Appelle `reload` quand `target` est rendu par le cache arrière-avant (`pageshow` avec
 * `persisted`). Rend la fonction qui retire l'écouteur.
 */
export function reloadWhenRestored(target: Window, reload: () => void): () => void {
  const onPageShow = (event: PageTransitionEvent) => {
    if (event.persisted) reload();
  };
  target.addEventListener("pageshow", onPageShow);
  return () => target.removeEventListener("pageshow", onPageShow);
}
