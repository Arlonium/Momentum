// Déplacer une page dans l'arbre d'une section : où un dépôt la range (`dropTarget`), ce
// qu'il refuse, la zone visée d'après la place du pointeur sur une ligne (`zonesAt`), et le
// déplacement d'un cran au clavier (`stepTarget`).
import type { Level, PageNode, Role, Section } from "@mementum/protocol";
import { expect, test } from "vitest";
import * as drag from "./drag";
import { stepTarget, zonesAt, type Zone } from "./drag";
import { flattenRows, ROW_HEIGHT, type Row } from "./rows";

// Sauf mention contraire, la personne est membre de l'espace.
const dropTarget = (rows: Row[], dragged: string, over: number, zone: Zone, role: Role = "member") =>
  drag.dropTarget(rows, dragged, over, zone, role);
const nestTarget = (rows: Row[], id: string, role: Role = "member") =>
  drag.nestTarget(rows, id, role);
const unnestTarget = (rows: Row[], id: string, role: Role = "member") =>
  drag.unnestTarget(rows, id, role);

function page(
  id: string,
  parentId: string | null,
  position: string,
  { level = "full", section = "space" }: { level?: Level; section?: Section } = {},
): PageNode {
  return {
    id,
    spaceId: "space",
    parentId,
    kind: "page",
    title: id.toUpperCase(),
    position,
    level,
    section,
    updatedAt: 0,
    trashedAt: null,
  };
}

/**
 * L'arbre des tests, déplié :
 *
 *     a
 *       a1
 *         a1x
 *       a2
 *       a3
 *     b
 *     c
 *       c1
 */
function tree(overrides: Partial<Record<string, Partial<PageNode>>> = {}): Row[] {
  const nodes = [
    page("a", null, "a0"),
    page("a1", "a", "a0"),
    page("a1x", "a1", "a0"),
    page("a2", "a", "a1"),
    page("a3", "a", "a2"),
    page("b", null, "a1"),
    page("c", null, "a2"),
    page("c1", "c", "a0"),
  ].map((node) => ({ ...node, ...overrides[node.id] }));
  return flattenRows(nodes, new Set());
}

/** Le rang de la ligne de `id`. */
const at = (rows: Row[], id: string) => rows.findIndex((row) => row.node.id === id);

test("drop_inside_makes_last_child", () => {
  const rows = tree();
  // Dans une page qui a des sous-pages : après la dernière.
  expect(dropTarget(rows, "b", at(rows, "a"), "inside")).toEqual({ parentId: "a", afterId: "a3" });
  expect(dropTarget(rows, "b", at(rows, "c"), "inside")).toEqual({ parentId: "c", afterId: "c1" });
  // Dans une page qui n'en a pas : la première.
  expect(dropTarget(rows, "a2", at(rows, "b"), "inside")).toEqual({ parentId: "b", afterId: null });
  // Dans sa propre page parente : elle passe en dernier, après ses sœurs.
  expect(dropTarget(rows, "a1", at(rows, "a"), "inside")).toEqual({ parentId: "a", afterId: "a3" });
  // Plus profond : sous une sous-page.
  expect(dropTarget(rows, "c", at(rows, "a1"), "inside")).toEqual({ parentId: "a1", afterId: "a1x" });
});

test("drop_before_and_after_pick_the_right_sibling", () => {
  const rows = tree();
  // Après une ligne : sa sœur suivante.
  expect(dropTarget(rows, "c", at(rows, "a"), "after")).toEqual({ parentId: null, afterId: "a" });
  expect(dropTarget(rows, "b", at(rows, "a2"), "after")).toEqual({ parentId: "a", afterId: "a2" });
  // Avant une ligne : après la sœur qui la précède ; avant la première, en tête.
  expect(dropTarget(rows, "c", at(rows, "b"), "before")).toEqual({ parentId: null, afterId: "a" });
  expect(dropTarget(rows, "c", at(rows, "a"), "before")).toEqual({ parentId: null, afterId: null });
  expect(dropTarget(rows, "b", at(rows, "a1"), "before")).toEqual({ parentId: "a", afterId: null });
  expect(dropTarget(rows, "b", at(rows, "a3"), "before")).toEqual({ parentId: "a", afterId: "a2" });
  // La page déplacée ne compte pas parmi ses sœurs : avant « a3 », « a1 » va après « a2 » ;
  // avant « a2 », « a3 » va après « a1 ».
  expect(dropTarget(rows, "a1", at(rows, "a3"), "before")).toEqual({ parentId: "a", afterId: "a2" });
  expect(dropTarget(rows, "a3", at(rows, "a2"), "before")).toEqual({ parentId: "a", afterId: "a1" });
  // Après une sous-page : sœur de cette sous-page, pas de sa page parente.
  expect(dropTarget(rows, "b", at(rows, "a1x"), "after")).toEqual({ parentId: "a1", afterId: "a1x" });
});

test("drop_on_own_descendant_is_refused", () => {
  const rows = tree();
  for (const zone of ["before", "inside", "after"] as const) {
    // Sur elle-même.
    expect(dropTarget(rows, "a", at(rows, "a"), zone)).toBeNull();
    // Sous une de ses sous-pages, à toute profondeur.
    expect(dropTarget(rows, "a", at(rows, "a1"), zone)).toBeNull();
    expect(dropTarget(rows, "a", at(rows, "a1x"), zone)).toBeNull();
    expect(dropTarget(rows, "a", at(rows, "a3"), zone)).toBeNull();
    expect(dropTarget(rows, "a1", at(rows, "a1x"), zone)).toBeNull();
  }
  // Une page inconnue, ou une ligne qui n'existe pas.
  expect(dropTarget(rows, "absente", at(rows, "a"), "inside")).toBeNull();
  expect(dropTarget(rows, "a", rows.length, "inside")).toBeNull();
  expect(dropTarget(rows, "a", -1, "inside")).toBeNull();
});

test("reparent_without_full_is_refused", () => {
  // « a2 » : niveau `edit`. Changer de place parmi ses sœurs lui est permis, pas changer de
  // page parente.
  const rows = tree({ a2: { level: "edit" } });
  expect(dropTarget(rows, "a2", at(rows, "a1"), "before")).toEqual({ parentId: "a", afterId: null });
  expect(dropTarget(rows, "a2", at(rows, "a3"), "after")).toEqual({ parentId: "a", afterId: "a3" });
  expect(dropTarget(rows, "a2", at(rows, "a"), "inside")).toEqual({ parentId: "a", afterId: "a3" });
  expect(dropTarget(rows, "a2", at(rows, "b"), "inside")).toBeNull();
  expect(dropTarget(rows, "a2", at(rows, "b"), "after")).toBeNull();
  expect(dropTarget(rows, "a2", at(rows, "a1"), "inside")).toBeNull();
  expect(dropTarget(rows, "a2", at(rows, "a1x"), "after")).toBeNull();

  // Sous `edit` (lire, commenter), rien ne se déplace.
  for (const level of ["read", "comment"] as const) {
    const locked = tree({ a2: { level } });
    expect(dropTarget(locked, "a2", at(locked, "a1"), "before")).toBeNull();
    expect(dropTarget(locked, "a2", at(locked, "b"), "inside")).toBeNull();
  }
});

test("new_parent_must_be_editable", () => {
  // Créer une sous-page demande de pouvoir modifier la page parente : on ne dépose rien dans
  // une page qu'on ne fait que lire, ni sous elle parmi ses sous-pages.
  const rows = tree({ c: { level: "read" }, c1: { level: "read" } });
  expect(dropTarget(rows, "b", at(rows, "c"), "inside")).toBeNull();
  expect(dropTarget(rows, "b", at(rows, "c1"), "after")).toBeNull();
  // À côté d'elle, à la racine, rien ne s'y oppose.
  expect(dropTarget(rows, "a", at(rows, "c"), "after")).toEqual({ parentId: null, afterId: "c" });
});

test("drop_that_changes_nothing_is_refused", () => {
  const rows = tree();
  // Déjà juste après « a1 », juste avant « a3 », et « a3 » déjà dernière de « a ».
  expect(dropTarget(rows, "a2", at(rows, "a1"), "after")).toBeNull();
  expect(dropTarget(rows, "a2", at(rows, "a3"), "before")).toBeNull();
  expect(dropTarget(rows, "a3", at(rows, "a"), "inside")).toBeNull();
  // Déjà en tête.
  expect(dropTarget(rows, "a1", at(rows, "a2"), "before")).toBeNull();
  expect(dropTarget(rows, "a", at(rows, "b"), "before")).toBeNull();
});

/** L'arbre des tests, tout entier dans la section `section`. */
function treeIn(section: Section, overrides: Partial<Record<string, Partial<PageNode>>> = {}): Row[] {
  const ids = ["a", "a1", "a1x", "a2", "a3", "b", "c", "c1"];
  return tree(Object.fromEntries(ids.map((id) => [id, { section, ...overrides[id] }])));
}

test("subpage_leaves_for_the_root_of_the_space_section", () => {
  // Une sous-page passée à la racine garde son accès général (R94) : une page de « Espace »
  // y reste, visible des autres membres.
  const rows = treeIn("space");
  for (const role of ["owner", "admin", "member"] as const) {
    expect(dropTarget(rows, "a1", at(rows, "b"), "before", role)).toEqual({ parentId: null, afterId: "a" });
    expect(dropTarget(rows, "a1", at(rows, "a"), "before", role)).toEqual({ parentId: null, afterId: null });
    expect(dropTarget(rows, "a1x", at(rows, "c"), "after", role)).toEqual({ parentId: null, afterId: "c" });
  }
  // Le serveur demande d'être membre de l'espace pour ranger une page à sa racine.
  expect(dropTarget(rows, "a1", at(rows, "b"), "before", "guest")).toBeNull();
  // Et l'accès complet à la page : la modifier ne suffit pas.
  const edit = treeIn("space", { a1: { level: "edit" } });
  expect(dropTarget(edit, "a1", at(edit, "b"), "before")).toBeNull();
});

test("subpage_leaves_for_the_root_of_the_private_section", () => {
  const rows = treeIn("private");
  expect(dropTarget(rows, "a1", at(rows, "b"), "before")).toEqual({ parentId: null, afterId: "a" });
});

test("subpage_does_not_leave_for_the_root_of_the_shared_section", () => {
  // Dans « Partagé », les accès nommés sur la page parente partiraient avec elle : la page
  // changerait de section, et disparaîtrait pour les autres. Quel que soit le rôle.
  const rows = treeIn("shared");
  for (const role of ["owner", "admin", "member", "guest"] as const) {
    expect(dropTarget(rows, "a1", at(rows, "b"), "before", role)).toBeNull();
    expect(dropTarget(rows, "a1", at(rows, "a"), "before", role)).toBeNull();
  }
  // Une page déjà à la racine y change de place.
  expect(dropTarget(rows, "c", at(rows, "a"), "before")).toEqual({ parentId: null, afterId: null });
  // Sous une autre page de la section, rien ne s'y oppose.
  expect(dropTarget(rows, "a1", at(rows, "b"), "inside")).toEqual({ parentId: "b", afterId: null });
});

test("pointer_height_on_a_row_picks_the_zone", () => {
  const rows = tree();
  const leaf = rows[at(rows, "b")];
  const openParent = rows[at(rows, "a")];
  // Le quart haut : avant ; le quart bas : après ; entre les deux : dedans. La zone qui suit
  // est celle qu'on essaie si la première est refusée.
  expect(zonesAt(0, ROW_HEIGHT, leaf)).toEqual(["before", "inside"]);
  expect(zonesAt(7, ROW_HEIGHT, leaf)).toEqual(["before", "inside"]);
  expect(zonesAt(8, ROW_HEIGHT, leaf)).toEqual(["inside", "before"]);
  expect(zonesAt(14, ROW_HEIGHT, leaf)).toEqual(["inside", "before"]);
  expect(zonesAt(15, ROW_HEIGHT, leaf)).toEqual(["inside", "after"]);
  expect(zonesAt(22, ROW_HEIGHT, leaf)).toEqual(["inside", "after"]);
  expect(zonesAt(23, ROW_HEIGHT, leaf)).toEqual(["after", "inside"]);
  expect(zonesAt(29, ROW_HEIGHT, leaf)).toEqual(["after", "inside"]);
  // Sous une page dépliée, la ligne suivante est sa première sous-page : son bord bas vise
  // l'intérieur, pas « après elle », qui rangerait la page après tout son sous-arbre.
  expect(zonesAt(29, ROW_HEIGHT, openParent)).toEqual(["inside", "after"]);
  // Repliée, c'est une ligne comme une autre.
  const closed = flattenRows(
    rows.map((row) => row.node),
    new Set(["a"]),
  );
  expect(zonesAt(29, ROW_HEIGHT, closed[at(closed, "a")])).toEqual(["after", "inside"]);
});

test("step_moves_one_place_among_siblings", () => {
  const rows = tree();
  // Vers le haut : avant la sœur qui précède ; vers le bas : après celle qui suit.
  expect(stepTarget(rows, "a2", "up")).toEqual({ parentId: "a", afterId: null });
  expect(stepTarget(rows, "a3", "up")).toEqual({ parentId: "a", afterId: "a1" });
  expect(stepTarget(rows, "a1", "down")).toEqual({ parentId: "a", afterId: "a2" });
  expect(stepTarget(rows, "a", "down")).toEqual({ parentId: null, afterId: "b" });
  expect(stepTarget(rows, "c", "up")).toEqual({ parentId: null, afterId: "a" });
  // Aux extrémités, et pour une page seule sous sa page parente : rien.
  expect(stepTarget(rows, "a1", "up")).toBeNull();
  expect(stepTarget(rows, "a3", "down")).toBeNull();
  expect(stepTarget(rows, "a", "up")).toBeNull();
  expect(stepTarget(rows, "c", "down")).toBeNull();
  expect(stepTarget(rows, "c1", "up")).toBeNull();
  expect(stepTarget(rows, "c1", "down")).toBeNull();
  expect(stepTarget(rows, "absente", "up")).toBeNull();
  // Changer de place demande de pouvoir modifier la page (`edit` suffit).
  expect(stepTarget(tree({ a2: { level: "edit" } }), "a2", "up")).toEqual({ parentId: "a", afterId: null });
  expect(stepTarget(tree({ a2: { level: "read" } }), "a2", "up")).toBeNull();
});

test("nest_makes_the_page_the_last_child_of_its_previous_sibling", () => {
  const rows = tree();
  // « b » suit « a », qui a déjà trois sous-pages : elle devient la quatrième.
  expect(nestTarget(rows, "b")).toEqual({ parentId: "a", afterId: "a3" });
  // « c » suit « b », qui n'en a pas : elle devient la première.
  expect(nestTarget(rows, "c")).toEqual({ parentId: "b", afterId: null });
  // Plus profond : « a2 » sous « a1 », après « a1x ».
  expect(nestTarget(rows, "a2")).toEqual({ parentId: "a1", afterId: "a1x" });
  // C'est le dépôt « dedans » de la souris, sur la sœur précédente.
  expect(nestTarget(rows, "b")).toEqual(dropTarget(rows, "b", at(rows, "a"), "inside"));
  // Sans sœur avant elle, ou inconnue : rien.
  expect(nestTarget(rows, "a")).toBeNull();
  expect(nestTarget(rows, "a1")).toBeNull();
  expect(nestTarget(rows, "c1")).toBeNull();
  expect(nestTarget(rows, "absente")).toBeNull();
});

test("unnest_makes_the_page_the_next_sibling_of_its_parent", () => {
  const rows = tree();
  // « a1x », sous « a1 » : elle passe sous « a », juste après « a1 ».
  expect(unnestTarget(rows, "a1x")).toEqual({ parentId: "a", afterId: "a1" });
  // C'est le dépôt « après » de la souris, sur la page parente.
  expect(unnestTarget(rows, "a1x")).toEqual(dropTarget(rows, "a1x", at(rows, "a1"), "after"));
  // Une page à la racine n'a pas de page parente d'où sortir.
  expect(unnestTarget(rows, "a")).toBeNull();
  expect(unnestTarget(rows, "absente")).toBeNull();
  // Jusqu'à la racine : dans « Espace » (pour un membre) et dans « Privé », où la page reste
  // dans sa section ; pas dans « Partagé ».
  expect(unnestTarget(rows, "a2")).toEqual({ parentId: null, afterId: "a" });
  expect(unnestTarget(rows, "a2", "guest")).toBeNull();
  expect(unnestTarget(treeIn("private"), "a2")).toEqual({ parentId: null, afterId: "a" });
  expect(unnestTarget(treeIn("shared"), "a2")).toBeNull();
});

test("nest_and_unnest_follow_the_rules_of_a_drop", () => {
  // Changer de page parente demande l'accès complet à la page.
  const edit = tree({ b: { level: "edit" }, a1x: { level: "edit" } });
  expect(nestTarget(edit, "b")).toBeNull();
  expect(unnestTarget(edit, "a1x")).toBeNull();
  // Et de pouvoir modifier la nouvelle page parente.
  const locked = tree({ a: { level: "read" }, a1: { level: "read" }, a1x: { level: "full" } });
  expect(nestTarget(locked, "b")).toBeNull();
  expect(unnestTarget(locked, "a1x")).toBeNull();
});
