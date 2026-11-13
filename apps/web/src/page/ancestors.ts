// Les pages parentes d'une page, pour le fil d'Ariane de la barre du haut (R256). Elles se
// lisent dans l'arbre déjà chargé, que le serveur a filtré par les droits du compte : aucune
// requête de plus, et aucun titre qu'il ne voit pas. Une page dont la page parente n'est pas
// dans l'arbre (le compte ne la voit pas : le serveur rend alors `parentId` à `null`, ou un
// identifiant qu'il ne rend pas) est la plus haute de son fil.
import type { PageNode } from "@mementum/protocol";

/**
 * Les pages parentes de la page `id` que l'arbre `nodes` contient, de la plus haute à la page
 * parente. Une page hors de l'arbre (à la corbeille, arbre pas encore lu) n'en a aucune. Une
 * chaîne qui boucle (des parents en rond, que le serveur ne rend jamais) s'arrête à la
 * première page revue.
 */
export function ancestorsOf(nodes: PageNode[], id: string): PageNode[] {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const start = byId.get(id);
  if (start === undefined) return [];
  const seen = new Set([id]);
  const chain: PageNode[] = [];
  let parent = start.parentId === null ? undefined : byId.get(start.parentId);
  while (parent !== undefined && !seen.has(parent.id)) {
    seen.add(parent.id);
    chain.push(parent);
    parent = parent.parentId === null ? undefined : byId.get(parent.parentId);
  }
  return chain.reverse();
}
