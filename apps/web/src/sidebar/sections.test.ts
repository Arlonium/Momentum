// Les sections de la barre latérale (« Espace », « Partagé », « Privé ») : la répartition
// des pages, les sections montrées à chacun, et celles où l'on crée une page.
import type { PageNode, Section } from "@mementum/protocol";
import { afterEach, expect, test } from "vitest";
import {
  bySection,
  canCreateIn,
  readCollapsedSections,
  visibleSections,
  writeCollapsedSections,
} from "./sections";

afterEach(() => localStorage.clear());

function page(id: string, parentId: string | null, section: Section): PageNode {
  return {
    id,
    spaceId: "space",
    parentId,
    kind: "page",
    title: id.toUpperCase(),
    position: "a0",
    level: "full",
    section,
    updatedAt: 0,
    trashedAt: null,
  };
}

/** Les ids de chaque section, dans l'ordre rendu. */
function ids(groups: Record<Section, PageNode[]>): Record<Section, string[]> {
  return {
    space: groups.space.map((n) => n.id),
    shared: groups.shared.map((n) => n.id),
    private: groups.private.map((n) => n.id),
  };
}

test("groups_top_nodes_by_section", () => {
  const nodes = [
    page("e1", null, "space"),
    page("s1", null, "shared"),
    page("p1", null, "private"),
    page("e2", null, "space"),
    page("e1a", "e1", "space"),
    page("s1a", "s1", "shared"),
    page("s1a1", "s1a", "shared"),
    // Une sous-page suit la section de sa plus haute page visible, quoi qu'elle dise
    // elle-même (une réponse plus ancienne) : un sous-arbre n'est jamais coupé en deux.
    page("e2a", "e2", "private"),
    page("e2a1", "e2a", "shared"),
    // Son parent n'est pas dans la liste : c'est une plus haute page, dans sa propre section.
    page("o", "absent", "shared"),
    page("o1", "o", "shared"),
  ];

  const groups = bySection(nodes);
  // Dans chaque section, l'ordre reçu est gardé.
  expect(ids(groups)).toEqual({
    space: ["e1", "e2", "e1a", "e2a", "e2a1"],
    shared: ["s1", "s1a", "s1a1", "o", "o1"],
    private: ["p1"],
  });
  // Chaque section porte les pages elles-mêmes, pas des copies.
  expect(groups.space[0]).toBe(nodes[0]);
  expect(groups.private[0]).toBe(nodes[2]);

  expect(bySection([])).toEqual({ space: [], shared: [], private: [] });
});

test("parent_cycle_does_not_hang", () => {
  // Le serveur ne rend jamais de cycle ; s'il en venait un, le calcul se termine.
  const groups = bySection([page("a", "b", "space"), page("b", "a", "private")]);
  expect(groups.space.length + groups.shared.length + groups.private.length).toBe(2);
});

test("guest_has_only_the_shared_section", () => {
  const empty = bySection([]);
  // Un invité ne voit que « Partagé », même vide.
  expect(visibleSections("guest", empty)).toEqual(["shared"]);
  expect(visibleSections("guest", bySection([page("s", null, "shared")]))).toEqual(["shared"]);
});

test("empty_shared_section_is_hidden", () => {
  // Pour un membre : « Espace » et « Privé » toujours, « Partagé » quand elle a des pages.
  for (const role of ["owner", "admin", "member"] as const) {
    expect(visibleSections(role, bySection([]))).toEqual(["space", "private"]);
    expect(visibleSections(role, bySection([page("s", null, "shared")]))).toEqual([
      "space",
      "shared",
      "private",
    ]);
  }
});

test("pages_are_created_in_space_and_private_only", () => {
  expect(canCreateIn("space")).toBe(true);
  expect(canCreateIn("private")).toBe(true);
  // « Partagé » ne se choisit pas : une page y arrive quand on la partage.
  expect(canCreateIn("shared")).toBe(false);
});

test("collapsed_sections_are_kept_per_space", () => {
  expect(readCollapsedSections("perso")).toEqual([]);
  writeCollapsedSections("perso", ["private", "space"]);
  // Relues dans l'ordre de la barre latérale, et pour cet espace seulement.
  expect(readCollapsedSections("perso")).toEqual(["space", "private"]);
  expect(readCollapsedSections("pro")).toEqual([]);
  // Tout déplié : plus rien n'est gardé.
  writeCollapsedSections("perso", []);
  expect(readCollapsedSections("perso")).toEqual([]);
  expect(localStorage.length).toBe(0);
  // Une valeur illisible, ou qui n'est pas une liste de sections : tout est déplié.
  localStorage.setItem("mementum.sections.perso", "{pas du JSON");
  expect(readCollapsedSections("perso")).toEqual([]);
  localStorage.setItem("mementum.sections.perso", JSON.stringify(["shared", "autre", 3]));
  expect(readCollapsedSections("perso")).toEqual(["shared"]);
});
