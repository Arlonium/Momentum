// L'arbre des pages, aplati en lignes (`flattenRows`) : c'est l'ordre et la profondeur des
// lignes que la barre latérale affiche, une par page visible.
import type { PageNode } from "@mementum/protocol";
import { expect, test } from "vitest";
import { flattenRows, withoutSubtree } from "./rows";

function page(id: string, parentId: string | null, position: string): PageNode {
  return {
    id,
    spaceId: "space",
    parentId,
    kind: "page",
    title: id.toUpperCase(),
    position,
    level: "full",
    section: "space",
    updatedAt: 0,
    trashedAt: null,
  };
}

/** Les lignes, réduites à ce que le test compare. */
function shape(nodes: PageNode[], collapsed: ReadonlySet<string> = new Set()) {
  return flattenRows(nodes, collapsed).map(({ node, depth, hasChildren, open }) => ({
    id: node.id,
    depth,
    hasChildren,
    open,
  }));
}

test("flatten_orders_by_position_and_skips_collapsed", () => {
  // Données dans le désordre : l'ordre des frères est celui de `position` (ASCII).
  const nodes = [
    page("b", null, "a1"),
    page("b2", "b", "a1"),
    page("a1", "a", "Zz"),
    page("a", null, "a0"),
    page("b1", "b", "a0"),
    page("a1x", "a1", "a0"),
    page("c", null, "a2"),
    page("a2", "a", "a0"),
  ];

  expect(shape(nodes)).toEqual([
    { id: "a", depth: 0, hasChildren: true, open: true },
    // « Zz » < « a0 » : les majuscules passent avant les minuscules en ASCII.
    { id: "a1", depth: 1, hasChildren: true, open: true },
    { id: "a1x", depth: 2, hasChildren: false, open: true },
    { id: "a2", depth: 1, hasChildren: false, open: true },
    { id: "b", depth: 0, hasChildren: true, open: true },
    { id: "b1", depth: 1, hasChildren: false, open: true },
    { id: "b2", depth: 1, hasChildren: false, open: true },
    { id: "c", depth: 0, hasChildren: false, open: true },
  ]);

  // Une page repliée reste, sans ses descendants ; les autres branches ne changent pas.
  expect(shape(nodes, new Set(["a"]))).toEqual([
    { id: "a", depth: 0, hasChildren: true, open: false },
    { id: "b", depth: 0, hasChildren: true, open: true },
    { id: "b1", depth: 1, hasChildren: false, open: true },
    { id: "b2", depth: 1, hasChildren: false, open: true },
    { id: "c", depth: 0, hasChildren: false, open: true },
  ]);
  expect(shape(nodes, new Set(["a1"])).map((row) => row.id)).toEqual([
    "a",
    "a1",
    "a2",
    "b",
    "b1",
    "b2",
    "c",
  ]);

  // Chaque ligne porte la page elle-même, pas une copie.
  expect(flattenRows(nodes, new Set())[0].node).toBe(nodes[3]);
});

test("orphan_parent_goes_to_root", () => {
  // Le parent de « o » n'est pas dans la liste : « o » est à la racine, à son rang, avec
  // ses propres enfants sous lui.
  const nodes = [
    page("r1", null, "a0"),
    page("o", "absent", "a1"),
    page("o1", "o", "a0"),
    page("r2", null, "a2"),
  ];

  expect(shape(nodes)).toEqual([
    { id: "r1", depth: 0, hasChildren: false, open: true },
    { id: "o", depth: 0, hasChildren: true, open: true },
    { id: "o1", depth: 1, hasChildren: false, open: true },
    { id: "r2", depth: 0, hasChildren: false, open: true },
  ]);
});

test("subtree_leaves_with_its_root", () => {
  const nodes = [
    page("a", null, "a0"),
    page("a1", "a", "a0"),
    page("a1x", "a1", "a0"),
    page("b", null, "a1"),
    page("b1", "b", "a0"),
  ];
  // La page et toutes ses descendantes ; le reste garde son ordre et ses objets.
  const kept = withoutSubtree(nodes, "a");
  expect(kept.map((n) => n.id)).toEqual(["b", "b1"]);
  expect(kept[0]).toBe(nodes[3]);
  expect(withoutSubtree(nodes, "a1").map((n) => n.id)).toEqual(["a", "b", "b1"]);
  // Une page absente : rien ne change.
  expect(withoutSubtree(nodes, "absente")).toEqual(nodes);
});
