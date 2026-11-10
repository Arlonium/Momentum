// L'arbre des pages, aplati en lignes : la barre latérale n'en met dans le document que
// celles de la fenêtre visible (Ruling R43). Chaque page affichée ajoutait environ 0,26 ms
// à l'ouverture d'une page, parce que les recalculs de styles suivent le nombre de nœuds du
// document.
import type { PageNode } from "@mementum/protocol";

/** Une ligne de l'arbre : une page visible, à sa profondeur (0 : la racine). */
export type Row = { node: PageNode; depth: number; hasChildren: boolean; open: boolean };

/** Hauteur d'une ligne de l'arbre (px, §4.3 : éléments de 30 à 32 px). */
export const ROW_HEIGHT = 30;

/** Lignes rendues au-delà de la fenêtre, de chaque côté. */
export const OVERSCAN = 10;

/** Enfants de chaque page (`null` : la racine), dans l'ordre de `position`. */
function childrenByParent(nodes: PageNode[]): Map<string | null, PageNode[]> {
  const ids = new Set(nodes.map((n) => n.id));
  const map = new Map<string | null, PageNode[]>();
  for (const node of nodes) {
    // Un parent absent de l'arbre : la page reste visible, à la racine.
    const parent = node.parentId !== null && ids.has(node.parentId) ? node.parentId : null;
    const list = map.get(parent) ?? [];
    list.push(node);
    map.set(parent, list);
  }
  // Index fractionnaires comparés unité par unité (ASCII).
  for (const list of map.values()) {
    list.sort((a, b) => (a.position < b.position ? -1 : a.position > b.position ? 1 : 0));
  }
  return map;
}

/**
 * Les lignes de l'arbre, de haut en bas : chaque page, puis ses enfants dans l'ordre de
 * `position`, sauf sous une page repliée (`collapsed`), qui garde sa ligne.
 */
export function flattenRows(nodes: PageNode[], collapsed: ReadonlySet<string>): Row[] {
  const children = childrenByParent(nodes);
  const rows: Row[] = [];
  // Parcours en profondeur avec une pile : un arbre très profond ne déborde pas la pile
  // d'appels. Les frères y sont empilés du dernier au premier.
  const pending: { node: PageNode; depth: number }[] = [];
  const push = (list: PageNode[] = [], depth: number) => {
    for (let i = list.length - 1; i >= 0; i--) pending.push({ node: list[i], depth });
  };
  push(children.get(null), 0);
  for (let next = pending.pop(); next; next = pending.pop()) {
    const { node, depth } = next;
    const kids = children.get(node.id);
    const open = !collapsed.has(node.id);
    rows.push({ node, depth, hasChildren: kids !== undefined, open });
    if (open) push(kids, depth + 1);
  }
  return rows;
}
