// Le code de la fenêtre « Historique » et de l'aperçu, chargé à la demande (R135) : rien de lui sur
// le chemin de l'ouverture d'une page. Un module à part pour qu'un test voie qui le demande (un
// lecteur qui ouvre le menu « … » ne le demande pas : il n'y a pas « Historique »).

/** Charge le module de la fenêtre « Historique » (le navigateur ne le charge qu'une fois). */
export function loadHistory() {
  return import("./HistoryDialog");
}
