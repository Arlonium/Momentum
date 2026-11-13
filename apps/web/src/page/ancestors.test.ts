// Les pages parentes d'une page, telles que le fil d'Ariane les montre : celles de l'arbre du
// compte, de la plus haute qu'il voit à la page parente (R256).
import { expect, test } from "vitest";
import { page } from "../test/pages";
import { ancestorsOf } from "./ancestors";

const ids = (nodes: { id: string }[]) => nodes.map((node) => node.id);

test("a_page_at_the_root_has_no_ancestors", () => {
  const nodes = [page("a"), page("b")];
  expect(ancestorsOf(nodes, "a")).toEqual([]);
});

test("ancestors_go_from_the_highest_page_to_the_parent", () => {
  const nodes = [
    page("d", { parentId: "c" }),
    page("a"),
    page("c", { parentId: "b" }),
    page("b", { parentId: "a" }),
  ];
  expect(ids(ancestorsOf(nodes, "d"))).toEqual(["a", "b", "c"]);
  expect(ids(ancestorsOf(nodes, "b"))).toEqual(["a"]);
});

test("a_parent_outside_the_tree_starts_the_trail_at_the_page", () => {
  // Le serveur ne rend pas une page parente que le compte ne voit pas : la chaîne s'arrête à
  // la première qui manque, sans rien supposer de ce qu'il y a au-dessus.
  const nodes = [page("c", { parentId: "hidden" }), page("d", { parentId: "c" })];
  expect(ids(ancestorsOf(nodes, "c"))).toEqual([]);
  expect(ids(ancestorsOf(nodes, "d"))).toEqual(["c"]);
});

test("a_page_outside_the_tree_has_no_ancestors", () => {
  // À la corbeille, ou arbre pas encore lu : la page ne montre que son titre.
  expect(ancestorsOf([page("a")], "z")).toEqual([]);
});

test("a_chain_that_loops_stops", () => {
  // Des parents en rond, que le serveur ne rend jamais : la montée s'arrête, sans boucler, et
  // la page ne se compte pas parmi ses propres pages parentes.
  const nodes = [
    page("a", { parentId: "c" }),
    page("b", { parentId: "a" }),
    page("c", { parentId: "b" }),
  ];
  expect(ids(ancestorsOf(nodes, "c"))).toEqual(["a", "b"]);
  expect(ids(ancestorsOf([page("s", { parentId: "s" })], "s"))).toEqual([]);
});
