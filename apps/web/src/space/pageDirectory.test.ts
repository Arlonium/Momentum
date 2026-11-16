// Les pages que le compte voit, pour l'éditeur (Task C3, R251) : le titre à jour d'une page que
// l'arbre contient, rien pour une autre ; la recherche d'un sélecteur, sans accents ni casse.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { renderHook } from "../test/hooks";
import { page } from "../test/pages";
import { findPages, isPageId, pageTitle, publishPages, usePageTitle, usePublishedTree } from "./pageDirectory";

const A = "0190f5a0-0000-7000-8000-00000000000a";
const B = "0190f5a0-0000-7000-8000-00000000000b";
const C = "0190f5a0-0000-7000-8000-00000000000c";

afterEach(() => publishPages(null, null));

/** Publie l'arbre comme `App`, les rendus qui suivent sous `act`. */
const publish = (...args: Parameters<typeof publishPages>) => act(async () => publishPages(...args));

test("a_published_page_has_its_current_title_and_any_other_none", async () => {
  await publish("space", [page(A, { title: "Plan" }), page(B, { title: "Notes" })]);
  expect(pageTitle(A)).toBe("Plan");
  expect(pageTitle(C)).toBeNull();

  const shown = await renderHook((id: string) => usePageTitle(id), A);
  expect(shown.current).toBe("Plan");
  // L'arbre relu : le titre suit.
  await publish("space", [page(A, { title: "Plan 2027" }), page(B, { title: "Notes" })]);
  await shown.rerender(A);
  expect(shown.current).toBe("Plan 2027");
  // La page quitte l'arbre (accès retiré, corbeille) : plus de titre.
  await publish("space", [page(B, { title: "Notes" })]);
  await shown.rerender(A);
  expect(shown.current).toBeNull();
});

test("a_trashed_page_is_neither_named_nor_offered", () => {
  publishPages("space", [page(A, { title: "Plan", trashedAt: 1 }), page(B, { title: "Planning" })]);
  expect(pageTitle(A)).toBeNull();
  expect(findPages("plan", 8).map((p) => p.id)).toEqual([B]);
});

test("leaving_the_space_forgets_every_page", async () => {
  await publish("space", [page(A, { title: "Plan" })]);
  const shown = await renderHook((id: string) => usePageTitle(id), A);
  expect(shown.current).toBe("Plan");
  // Un autre espace, encore sans arbre, puis aucun : rien de l'espace quitté ne reste.
  await publish("other", null);
  await shown.rerender(A);
  expect(shown.current).toBeNull();
  expect(findPages("", 8)).toEqual([]);
  await publish("space", [page(A, { title: "Plan" })]);
  await publish(null, null);
  expect(pageTitle(A)).toBeNull();
  expect(findPages("", 8)).toEqual([]);
});

test("find_pages_ignores_accents_and_case_and_puts_titles_that_start_with_the_query_first", () => {
  publishPages("space", [
    page("1", { title: "Compte rendu de réunion" }),
    page("2", { title: "Réunion d'équipe" }),
    page("3", { title: "Autre chose" }),
    page("4", { title: "RÊVE" }),
    page("5", { title: "reunion budget" }),
  ]);
  const titles = (query: string, limit = 8) => findPages(query, limit).map((p) => p.title);
  // Les titres qui commencent par la requête d'abord, puis ceux qui la contiennent, chacun dans
  // l'ordre de l'arbre.
  expect(titles("REU")).toEqual(["Réunion d'équipe", "reunion budget", "Compte rendu de réunion"]);
  expect(titles("ré")).toEqual(["Réunion d'équipe", "RÊVE", "reunion budget", "Compte rendu de réunion", "Autre chose"]);
  expect(titles("  équipe ")).toEqual(["Réunion d'équipe"]);
  expect(titles("introuvable")).toEqual([]);
  // Une requête vide : les pages dans l'ordre de l'arbre ; au plus `limit`.
  expect(titles("", 2)).toEqual(["Compte rendu de réunion", "Réunion d'équipe"]);
  expect(titles("e", 3)).toHaveLength(3);
  // Ce que rend la recherche : l'id et le titre, rien d'autre.
  expect(findPages("autre", 8)).toEqual([{ id: "3", title: "Autre chose" }]);
});

test("a_title_hook_renders_again_only_when_its_own_title_changes", async () => {
  await publish("space", [page(A, { title: "Plan" }), page(B, { title: "Notes" })]);
  let renders = 0;
  const shown = await renderHook((id: string) => {
    renders += 1;
    return usePageTitle(id);
  }, A);
  const before = renders;
  // Une autre page renommée : rien à rendre ici.
  await publish("space", [page(A, { title: "Plan" }), page(B, { title: "Notes 2" })]);
  await new Promise((done) => setTimeout(done, 0));
  expect(renders).toBe(before);
  // Témoin : la sienne renommée, elle est rendue de nouveau.
  await publish("space", [page(A, { title: "Plan 2" }), page(B, { title: "Notes 2" })]);
  await new Promise((done) => setTimeout(done, 0));
  expect(renders).toBeGreaterThan(before);
  expect(shown.current).toBe("Plan 2");
});

test("only_a_uuid_names_a_page", () => {
  expect(isPageId(A)).toBe(true);
  expect(isPageId(A.toUpperCase())).toBe(true);
  for (const id of ["", "plan", `${A} `, `../${A}`, `${A}x`, "javascript:alert(1)", "0190f5a0000070008000000000000000a"]) {
    expect(isPageId(id), id).toBe(false);
  }
});

test("the_application_publishes_its_tree_and_takes_it_back_when_it_goes", async () => {
  type Shown = { spaceId: string | null; nodes: ReturnType<typeof page>[] | null };
  const app = await renderHook(({ spaceId, nodes }: Shown) => usePublishedTree(spaceId, nodes), {
    spaceId: "space",
    nodes: [page(A, { title: "Plan" })],
  });
  expect(pageTitle(A)).toBe("Plan");
  // L'arbre relu, puis un autre espace dont l'arbre n'est pas encore lu.
  await app.rerender({ spaceId: "space", nodes: [page(A, { title: "Plan 2" })] });
  expect(pageTitle(A)).toBe("Plan 2");
  await app.rerender({ spaceId: "other", nodes: null });
  expect(pageTitle(A)).toBeNull();
  await app.rerender({ spaceId: "space", nodes: [page(A, { title: "Plan 2" })] });
  expect(pageTitle(A)).toBe("Plan 2");
  // L'application démontée (session fermée, autre compte) : rien ne reste au suivant.
  await app.unmount();
  expect(pageTitle(A)).toBeNull();
  expect(findPages("", 8)).toEqual([]);
});
