// « Favoris » ne bouge pas sous le pointeur (R411, la règle de R296 et R298 pour une liste posée
// au-dessus des arbres). La liste est en haut de la barre latérale : ce qui la fait grandir, ou
// rapetisser, décale toutes les lignes des arbres. Tant que le pointeur est dans la barre, la
// liste montrée garde donc sa taille et son ordre quand c'est une relecture qui la change (un
// favori posé dans un autre onglet du compte, une page partie à la corbeille ou dont l'accès est
// retiré, puis revenue) : une page partie reste à sa place, avec ce qu'on en montrait, une page
// arrivée attend. Les titres suivent. Ce que la personne fait elle-même dans l'onglet depuis le
// début du gel (poser ou retirer un favori, par le menu d'une ligne ou par celui de la page, la
// souris restée sur la barre) se montre tout de suite : c'est son geste. La liste reçue revient
// quand le pointeur sort de la barre.
//
// Le focus ne gèle rien ici : une liste qui grandit au-dessus de l'élément qui a le focus ne
// change pas l'élément qui l'a. Et un lien suivi garde le focus dans Chromium : un gel par le
// focus garderait la liste figée jusqu'à ce qu'on clique ailleurs, y compris au retour sur
// l'onglet, quand arrive un favori posé dans un autre onglet. L'ordre de la liste elle-même, sous
// le focus ou le pointeur, est gelé par `ShortList` (R298).
import type { PageNode } from "@mementum/protocol";
import { useMemo, useState } from "react";

/** Le rang du dernier geste d'ici : 0 sans geste. */
function lastRank(toggles: ReadonlyMap<string, number>): number {
  let last = 0;
  for (const rank of toggles.values()) last = Math.max(last, rank);
  return last;
}

/**
 * La liste montrée quand elle est gelée : `frozen`, ce qu'on montrait quand le gel a commencé,
 * chacune avec son titre à jour (`incoming`), une page que la personne a retirée d'ici en moins ;
 * puis, à la fin, celles qu'elle a ajoutées d'ici.
 */
export function heldPages(
  frozen: readonly PageNode[],
  incoming: PageNode[],
  mine: ReadonlySet<string>,
): PageNode[] {
  const byId = new Map(incoming.map((node) => [node.id, node]));
  const kept = frozen.flatMap((node) => {
    const fresh = byId.get(node.id);
    if (fresh !== undefined) return [fresh];
    // Partie : retirée d'ici, elle part ; sinon elle reste, telle qu'on la montrait.
    return mine.has(node.id) ? [] : [node];
  });
  const shown = new Set(frozen.map((node) => node.id));
  const added = incoming.filter((node) => mine.has(node.id) && !shown.has(node.id));
  return [...kept, ...added];
}

/**
 * La liste `incoming`, gelée tant que `held` est vrai (`heldPages`). `toggles` : le rang du dernier
 * geste d'ici sur chaque page (`useFavorites`) ; ceux d'après le début du gel ne sont pas gelés.
 * `key` (l'espace) : un gel ne passe pas d'un espace à l'autre, celui du nouvel espace part de ce
 * qu'on y voit en arrivant.
 */
export function useHeldPages(
  key: string,
  incoming: PageNode[],
  held: boolean,
  toggles: ReadonlyMap<string, number>,
): PageNode[] {
  const [frozen, setFrozen] = useState<{ key: string; pages: PageNode[]; since: number } | null>(
    null,
  );
  // Le gel commence (ou change d'espace) : ce qu'on montre à cet instant, et le rang du dernier
  // geste d'ici ; il finit avec `held`.
  if (held && (frozen === null || frozen.key !== key)) {
    setFrozen({ key, pages: incoming, since: lastRank(toggles) });
  }
  if (!held && frozen !== null) setFrozen(null);
  const current = held && frozen !== null && frozen.key === key ? frozen : null;
  return useMemo(() => {
    if (current === null) return incoming;
    const mine = new Set([...toggles].filter(([, rank]) => rank > current.since).map(([id]) => id));
    return heldPages(current.pages, incoming, mine);
  }, [current, incoming, toggles]);
}
