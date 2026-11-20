// Le code de la fenêtre de recherche, chargé à la demande (lot 4, Task C1) : rien de lui sur le
// chemin de l'ouverture de l'application ni d'une page. Un module à part pour qu'un test voie qui
// le demande, et quand (`App.test.tsx` : pas avant que la recherche soit ouverte, ou que l'entrée
// de la barre latérale soit survolée ou prenne le focus).

/** Charge le module de la fenêtre de recherche (le navigateur ne le charge qu'une fois). */
export function loadSearch() {
  return import("./SearchDialog");
}
