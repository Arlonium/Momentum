// Les pages récentes (R255) : gardées dans le navigateur, par compte et par espace, vingt
// identifiants au plus, la plus récente d'abord ; la barre latérale montre les cinq premières
// que l'arbre contient ; la déconnexion les efface.
import type { PageNode } from "@mementum/protocol";
import { afterEach, expect, test, vi } from "vitest";
import { renderHook, settle } from "../test/hooks";
import { page } from "../test/pages";
import {
  forgetRecents,
  readRecents,
  RECENTS_KEPT,
  RECENTS_SHOWN,
  recentsKey,
  useRecentPages,
  visit,
} from "./recents";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

test("recents_keep_twenty_ids_most_recent_first_without_repeats", () => {
  expect(RECENTS_KEPT).toBe(20);
  expect(recentsKey("u1", "s1")).toBe("mementum.recents.u1.s1");
  for (let n = 0; n < 25; n += 1) visit("u1", "s1", `p${n}`);
  // Revisitée : elle remonte en tête, sans doublon.
  visit("u1", "s1", "p20");
  const expected = ["p20", "p24", "p23", "p22", "p21"];
  for (let n = 19; n >= 5; n -= 1) expected.push(`p${n}`);
  expect(readRecents("u1", "s1")).toEqual(expected);
  // Rien d'autre que les identifiants n'est gardé.
  expect(JSON.parse(localStorage.getItem("mementum.recents.u1.s1")!)).toEqual(expected);
});

test("recents_are_kept_per_space", () => {
  visit("u1", "s1", "a");
  visit("u1", "s2", "b");
  expect(readRecents("u1", "s1")).toEqual(["a"]);
  expect(readRecents("u1", "s2")).toEqual(["b"]);
});

test("recents_show_only_pages_of_the_tree", async () => {
  expect(RECENTS_SHOWN).toBe(5);
  for (const id of ["w", "y", "x", "a", "b", "c", "z"]) visit("u1", "s1", id);
  // `z` n'est pas (ou plus) dans l'arbre : ni montrée, ni retirée du stockage.
  const tree = ["a", "b", "c", "w", "x", "y"].map((id) => page(id));
  const shown = await renderHook(
    ({ nodes }: { nodes: PageNode[] | null }) => useRecentPages("u1", "s1", nodes, null),
    { nodes: tree },
  );
  expect(shown.current.map((node) => node.id)).toEqual(["c", "b", "a", "x", "y"]);
  expect(readRecents("u1", "s1")).toContain("z");

  // L'accès revient : elle aussi.
  await shown.rerender({ nodes: [...tree, page("z")] });
  expect(shown.current.map((node) => node.id)).toEqual(["z", "c", "b", "a", "x"]);

  // Sans arbre (pas encore lu) : rien.
  await shown.rerender({ nodes: null });
  expect(shown.current).toEqual([]);
});

test("a_visit_shows_at_once_in_the_list", async () => {
  const tree = ["a", "b"].map((id) => page(id));
  const shown = await renderHook(() => useRecentPages("u1", "s1", tree, null), undefined);
  expect(shown.current).toEqual([]);
  await settle(() => visit("u1", "s1", "a"));
  expect(shown.current.map((node) => node.id)).toEqual(["a"]);
  await settle(() => visit("u1", "s1", "b"));
  expect(shown.current.map((node) => node.id)).toEqual(["b", "a"]);
});

test("the_open_page_is_first_from_the_render_that_opens_it", async () => {
  // La visite s'écrit après le rendu qui ouvre la page (effet d'`App`) ; la liste, elle, la
  // montre en tête dès ce rendu : elle grandit dans le même rendu que l'arbre montre la page,
  // et la ligne de l'arbre que la barre fait défiler pour la montrer ne bouge plus après.
  visit("u1", "s1", "a");
  const tree = ["a", "b"].map((id) => page(id));
  const shown = await renderHook(
    ({ activeId }: { activeId: string | null }) => useRecentPages("u1", "s1", tree, activeId),
    { activeId: "b" },
  );
  expect(shown.current.map((node) => node.id)).toEqual(["b", "a"]);
  // Rien n'est écrit pour autant : c'est la visite qui l'écrit.
  expect(readRecents("u1", "s1")).toEqual(["a"]);
  // Déjà en tête, ou hors de l'arbre : la liste gardée, telle quelle.
  await shown.rerender({ activeId: "a" });
  expect(shown.current.map((node) => node.id)).toEqual(["a"]);
  await shown.rerender({ activeId: "z" });
  expect(shown.current.map((node) => node.id)).toEqual(["a"]);
});

test("a_visit_in_another_tab_shows_here", async () => {
  const tree = ["a", "b"].map((id) => page(id));
  const shown = await renderHook(() => useRecentPages("u1", "s1", tree, null), undefined);
  // Un autre onglet du navigateur écrit dans le stockage : celui-ci l'apprend par `storage`.
  localStorage.setItem(recentsKey("u1", "s1"), JSON.stringify(["b"]));
  await settle(() =>
    window.dispatchEvent(new StorageEvent("storage", { key: recentsKey("u1", "s1") })),
  );
  expect(shown.current.map((node) => node.id)).toEqual(["b"]);
});

test("recents_of_another_account_are_not_read", async () => {
  visit("u1", "s1", "a");
  expect(readRecents("u2", "s1")).toEqual([]);
  const tree = [page("a")];
  const other = await renderHook(() => useRecentPages("u2", "s1", tree, null), undefined);
  expect(other.current).toEqual([]);
  // Témoin : le compte qui l'a ouverte la voit.
  const mine = await renderHook(() => useRecentPages("u1", "s1", tree, null), undefined);
  expect(mine.current.map((node) => node.id)).toEqual(["a"]);
});

test("logging_out_forgets_the_recents_of_the_account", async () => {
  visit("u1", "s1", "a");
  visit("u1", "s2", "b");
  visit("u10", "s1", "c");
  visit("u2", "s1", "d");
  localStorage.setItem("mementum.sections.s1", JSON.stringify(["shared"]));
  const tree = [page("a")];
  const shown = await renderHook(() => useRecentPages("u1", "s1", tree, null), undefined);
  expect(shown.current.map((node) => node.id)).toEqual(["a"]);

  await settle(() => forgetRecents("u1"));
  // Toutes les siennes, dans tous ses espaces, et la liste montrée le suit.
  expect(readRecents("u1", "s1")).toEqual([]);
  expect(readRecents("u1", "s2")).toEqual([]);
  expect(localStorage.getItem("mementum.recents.u1.s1")).toBeNull();
  expect(localStorage.getItem("mementum.recents.u1.s2")).toBeNull();
  expect(shown.current).toEqual([]);
  // Celles d'un autre compte restent, même quand son identifiant commence comme le sien ; et le
  // reste du stockage n'est pas touché.
  expect(readRecents("u10", "s1")).toEqual(["c"]);
  expect(readRecents("u2", "s1")).toEqual(["d"]);
  expect(localStorage.getItem("mementum.sections.s1")).toBe('["shared"]');
});

test("storage_that_throws_shows_no_recents", async () => {
  const refused = () => {
    throw new DOMException("stockage refusé", "SecurityError");
  };
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(refused);
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(refused);
  vi.spyOn(Storage.prototype, "removeItem").mockImplementation(refused);
  vi.spyOn(Storage.prototype, "key").mockImplementation(refused);
  // Rien ne lève : ni la visite, ni la lecture, ni l'oubli.
  expect(() => visit("u1", "s1", "a")).not.toThrow();
  expect(readRecents("u1", "s1")).toEqual([]);
  expect(() => forgetRecents("u1")).not.toThrow();
  // Pas même la page ouverte : rien n'est gardé, rien n'est montré.
  const tree = [page("a")];
  const shown = await renderHook(() => useRecentPages("u1", "s1", tree, "a"), undefined);
  expect(shown.current).toEqual([]);
});

test("unreadable_recents_are_an_empty_list", () => {
  const key = recentsKey("u1", "s1");
  localStorage.setItem(key, "pas du JSON");
  expect(readRecents("u1", "s1")).toEqual([]);
  localStorage.setItem(key, JSON.stringify({ a: 1 }));
  expect(readRecents("u1", "s1")).toEqual([]);
  // Ce qui n'est pas un identifiant est écarté ; le reste, gardé dans l'ordre.
  localStorage.setItem(key, JSON.stringify(["a", 3, null, "b", "a"]));
  expect(readRecents("u1", "s1")).toEqual(["a", "b"]);
  // Une visite repart de ce qui se lit.
  visit("u1", "s1", "c");
  expect(readRecents("u1", "s1")).toEqual(["c", "a", "b"]);
});
