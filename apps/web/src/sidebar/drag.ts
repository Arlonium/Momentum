// Déplacer une page dans l'arbre d'une section : à la souris (glisser-déposer) ou au clavier
// (« Déplacer vers le haut / le bas »). Ce module dit où une page irait, et refuse ce que les
// droits interdisent ; le serveur fait foi, et refuse le reste.
//
// Ce que le serveur exige (`PATCH /api/v1/pages/{id}`) : changer de place parmi les mêmes
// sœurs demande de pouvoir modifier la page (`edit`) ; changer de page parente demande l'accès
// complet à la page (`full`) et de pouvoir modifier la nouvelle page parente.
//
// Un déplacement reste dans sa section. Une sous-page passée à la racine ne garde que l'accès
// de qui la déplace : elle devient privée. Dans « Privé », elle reste donc dans sa section ;
// dans « Espace » ou « Partagé », elle en changerait, et disparaîtrait pour les autres :
// c'est refusé ici. Changer une page de section passera par le menu Partager.
import { canEdit } from "../share/levels";
import type { Row } from "./rows";

/**
 * Où ranger la page : sous `parentId` (`null` : à la racine de sa section), après `afterId`
 * (`null` : en tête de ses sœurs).
 */
export type Drop = { parentId: string | null; afterId: string | null };

/** La zone visée sur une ligne : avant elle, dedans (sous-page), ou après elle. */
export type Zone = "before" | "inside" | "after";

/** Une ligne, et sa page parente dans l'arbre (`null` : à la racine). */
type Placed = { row: Row; parent: string | null };

/** La page parente de chaque ligne, d'après la profondeur des lignes qui la précèdent. */
function place(rows: Row[]): Placed[] {
  /** La dernière ligne vue à chaque profondeur : les ancêtres de la ligne en cours. */
  const ancestors: string[] = [];
  return rows.map((row) => {
    ancestors.length = row.depth;
    const parent = row.depth === 0 ? null : ancestors[row.depth - 1];
    ancestors[row.depth] = row.node.id;
    return { row, parent };
  });
}

/** Les pages sous `parent`, dans l'ordre. */
function childrenOf(placed: Placed[], parent: string | null): string[] {
  return placed.filter((p) => p.parent === parent).map((p) => p.row.node.id);
}

/** Le dépôt est-il permis, pour la page de `from` ? Il doit aussi changer quelque chose. */
function allowed(placed: Placed[], from: Placed, drop: Drop): boolean {
  const { node } = from.row;
  // Même changer de place demande de pouvoir modifier la page.
  if (!canEdit(node.level)) return false;
  const siblings = childrenOf(placed, from.parent);
  const before = siblings[siblings.indexOf(node.id) - 1] ?? null;
  if (drop.parentId === from.parent) return drop.afterId !== before;
  if (node.level !== "full") return false;
  // À la racine : seule une page privée reste dans sa section.
  if (drop.parentId === null) return node.section === "private";
  const parent = placed.find((p) => p.row.node.id === drop.parentId);
  return parent !== undefined && canEdit(parent.row.node.level);
}

/**
 * Où irait la page `dragged`, lâchée sur la zone `zone` de la ligne de rang `overIndex` ?
 * `rows` : toutes les lignes de l'arbre de sa section, dépliées. `null` : dépôt refusé (sur
 * elle-même ou sous une de ses descendantes, au-dessus de son niveau, hors de sa section), ou
 * qui ne changerait rien.
 */
export function dropTarget(rows: Row[], dragged: string, overIndex: number, zone: Zone): Drop | null {
  const placed = place(rows);
  const from = placed.find((p) => p.row.node.id === dragged);
  const over = placed[overIndex];
  if (from === undefined || over === undefined) return null;
  // Sur elle-même, ou dans son propre sous-arbre : on remonte de la ligne visée.
  const parents = new Map(placed.map((p) => [p.row.node.id, p.parent]));
  for (let id: string | null = over.row.node.id; id !== null; id = parents.get(id) ?? null) {
    if (id === dragged) return null;
  }
  const target = over.row.node.id;
  let drop: Drop;
  if (zone === "inside") {
    // Dernière sous-page : après celle qui l'est aujourd'hui (sans compter la page déplacée).
    const children = childrenOf(placed, target).filter((id) => id !== dragged);
    drop = { parentId: target, afterId: children[children.length - 1] ?? null };
  } else {
    const siblings = childrenOf(placed, over.parent).filter((id) => id !== dragged);
    const before = siblings[siblings.indexOf(target) - 1] ?? null;
    drop = { parentId: over.parent, afterId: zone === "after" ? target : before };
  }
  return allowed(placed, from, drop) ? drop : null;
}

/**
 * Les zones qu'un pointeur à `offset` px du haut d'une ligne haute de `height` px peut viser,
 * la première d'abord : le quart haut vise « avant », le quart bas « après », le milieu
 * « dedans ». La seconde est celle qu'on essaie quand la première est refusée. Sous une page
 * dépliée, la ligne suivante est sa première sous-page : son bord bas vise « dedans ».
 */
export function zonesAt(offset: number, height: number, row: Row): Zone[] {
  const edge = height / 4;
  if (offset < edge) return ["before", "inside"];
  if (offset >= height - edge && !(row.open && row.hasChildren)) return ["after", "inside"];
  return offset < height / 2 ? ["inside", "before"] : ["inside", "after"];
}

/**
 * Où irait la page `id`, déplacée d'un cran parmi ses sœurs ? `null` : elle est déjà au bout,
 * ou son niveau ne permet pas de la déplacer.
 */
export function stepTarget(rows: Row[], id: string, direction: "up" | "down"): Drop | null {
  const placed = place(rows);
  const from = placed.find((p) => p.row.node.id === id);
  if (from === undefined) return null;
  const siblings = childrenOf(placed, from.parent);
  const at = siblings.indexOf(id);
  // Vers le haut : avant la sœur qui précède, donc après celle d'encore avant.
  const after = direction === "up" ? siblings[at - 2] ?? null : siblings[at + 1];
  if (direction === "up" ? at === 0 : after === undefined) return null;
  const drop = { parentId: from.parent, afterId: after ?? null };
  return allowed(placed, from, drop) ? drop : null;
}
