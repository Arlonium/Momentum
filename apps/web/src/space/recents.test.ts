// Les pages récentes (R255) : gardées dans le navigateur, par compte et par espace, vingt
// identifiants au plus, la plus récente d'abord ; la barre latérale montre les cinq premières
// que l'arbre contient. Elles ne s'écrivent que pour le compte entré dans l'onglet ; la
// déconnexion efface celles du compte, l'entrée d'un compte celles des autres (R297).
import type { PageNode } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { renderHook, settle } from "../test/hooks";
import { page } from "../test/pages";
import {
  forgetOtherRecents,
  forgetRecents,
  readRecents,
  RECENTS_KEPT,
  RECENTS_SHOWN,
  recentsKey,
  useRecentPages,
  visit,
} from "./recents";

/** La clé par laquelle un onglet annonce aux autres le compte qui entre, ou qui part. */
const SIGNAL = "mementum.recents-account";

beforeEach(() => {
  // Le compte `u1` est entré dans l'onglet : ses visites s'écrivent.
  forgetOtherRecents("u1");
});

afterEach(() => {
  vi.restoreAllMocks();
  // Plus personne n'écrit, et rien ne reste pour le test suivant.
  forgetRecents("u1");
  localStorage.clear();
});

/** Ce que le stockage garde pour `u1` dans `s1`, tel quel. */
const stored = () => JSON.parse(localStorage.getItem(recentsKey("u1", "s1")) ?? "null");

test("recents_keep_twenty_ids_most_recent_first_without_repeats", () => {
  expect(RECENTS_KEPT).toBe(20);
  expect(recentsKey("u1", "s1")).toBe("mementum.recents.u1.s1");
  for (let n = 0; n < 25; n += 1) visit("u1", "s1", `p${n}`);
  // Vingt gardés, pas un de plus : les plus récents.
  const kept: string[] = [];
  for (let n = 24; n >= 5; n -= 1) kept.push(`p${n}`);
  expect(stored()).toEqual(kept);
  expect(readRecents("u1", "s1")).toEqual(kept);
  // Revisitée : elle remonte en tête, sans doublon.
  visit("u1", "s1", "p20");
  const expected = ["p20", ...kept.filter((id) => id !== "p20")];
  expect(readRecents("u1", "s1")).toEqual(expected);
  // Rien d'autre que les identifiants n'est gardé.
  expect(stored()).toEqual(expected);
});

test("a_longer_stored_list_is_read_as_twenty", () => {
  // Une liste plus longue (écrite par une autre version, ou à la main) : les vingt premiers.
  const ids = Array.from({ length: 25 }, (_, n) => `p${n}`);
  localStorage.setItem(recentsKey("u1", "s1"), JSON.stringify(ids));
  expect(readRecents("u1", "s1")).toEqual(ids.slice(0, 20));
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
    ({ nodes }: { nodes: PageNode[] | null }) => useRecentPages("u1", "s1", nodes),
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
  const shown = await renderHook(() => useRecentPages("u1", "s1", tree), undefined);
  expect(shown.current).toEqual([]);
  await settle(() => visit("u1", "s1", "a"));
  expect(shown.current.map((node) => node.id)).toEqual(["a"]);
  await settle(() => visit("u1", "s1", "b"));
  expect(shown.current.map((node) => node.id)).toEqual(["b", "a"]);
});

test("a_visit_in_another_tab_shows_here", async () => {
  const tree = ["a", "b"].map((id) => page(id));
  const shown = await renderHook(() => useRecentPages("u1", "s1", tree), undefined);
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
  const other = await renderHook(() => useRecentPages("u2", "s1", tree), undefined);
  expect(other.current).toEqual([]);
  // Témoin : le compte qui l'a ouverte la voit.
  const mine = await renderHook(() => useRecentPages("u1", "s1", tree), undefined);
  expect(mine.current.map((node) => node.id)).toEqual(["a"]);
});

test("logging_out_forgets_the_recents_of_the_account", async () => {
  visit("u1", "s1", "a");
  visit("u1", "s2", "b");
  localStorage.setItem(recentsKey("u10", "s1"), JSON.stringify(["c"]));
  localStorage.setItem(recentsKey("u2", "s1"), JSON.stringify(["d"]));
  localStorage.setItem("mementum.sections.s1", JSON.stringify(["shared"]));
  const tree = [page("a")];
  const shown = await renderHook(() => useRecentPages("u1", "s1", tree), undefined);
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

test("entering_forgets_the_recents_of_every_other_account", async () => {
  visit("u1", "s1", "a");
  visit("u1", "s2", "b");
  localStorage.setItem(recentsKey("u2", "s1"), JSON.stringify(["c"]));
  localStorage.setItem(recentsKey("u10", "s1"), JSON.stringify(["d"]));
  // D'autres formes de clés : laissées.
  localStorage.setItem("mementum.sections.s1", JSON.stringify(["shared"]));
  localStorage.setItem("mementum.space", "s1");
  localStorage.setItem("autre.mementum.recents.u2.s1", "[]");

  await settle(() => forgetOtherRecents("u1"));
  // Les siennes restent ; celles des autres partent, `u10` compris.
  expect(readRecents("u1", "s1")).toEqual(["a"]);
  expect(readRecents("u1", "s2")).toEqual(["b"]);
  expect(localStorage.getItem(recentsKey("u2", "s1"))).toBeNull();
  expect(localStorage.getItem(recentsKey("u10", "s1"))).toBeNull();
  expect(localStorage.getItem("mementum.sections.s1")).toBe('["shared"]');
  expect(localStorage.getItem("mementum.space")).toBe("s1");
  expect(localStorage.getItem("autre.mementum.recents.u2.s1")).toBe("[]");

  // `u2` entre : celles de `u1` partent à leur tour.
  await settle(() => forgetOtherRecents("u2"));
  expect(readRecents("u1", "s1")).toEqual([]);
  expect(readRecents("u1", "s2")).toEqual([]);
});

test("visits_are_written_only_for_the_account_entered_in_the_tab", () => {
  // Un autre compte que celui de l'onglet : rien.
  visit("u2", "s1", "a");
  expect(localStorage.getItem(recentsKey("u2", "s1"))).toBeNull();
  // Celui de l'onglet : écrite.
  visit("u1", "s1", "a");
  expect(stored()).toEqual(["a"]);
  // Déconnecté ici : plus rien.
  forgetRecents("u1");
  visit("u1", "s1", "b");
  expect(localStorage.getItem(recentsKey("u1", "s1"))).toBeNull();
  // Revenu : de nouveau.
  forgetOtherRecents("u1");
  visit("u1", "s1", "b");
  expect(stored()).toEqual(["b"]);
});

test("a_tab_writes_nothing_once_its_account_left_in_another_tab", () => {
  const signal = (newValue: string | null) =>
    window.dispatchEvent(new StorageEvent("storage", { key: SIGNAL, newValue }));
  // Un autre onglet se déconnecte : celui-ci n'écrit plus rien pour son compte.
  signal("");
  visit("u1", "s1", "a");
  expect(localStorage.getItem(recentsKey("u1", "s1"))).toBeNull();
  // Le retrait du signal ne dit rien.
  signal(null);
  visit("u1", "s1", "a");
  expect(localStorage.getItem(recentsKey("u1", "s1"))).toBeNull();
  // Le même compte entre de nouveau ailleurs : celui-ci reprend.
  signal("u1");
  visit("u1", "s1", "a");
  expect(stored()).toEqual(["a"]);
  // Un autre compte entre ailleurs : la session du navigateur n'est plus la sienne.
  signal("u2");
  visit("u1", "s1", "b");
  expect(stored()).toEqual(["a"]);
});

test("entering_and_leaving_are_announced_and_nothing_stays", () => {
  const set = vi.spyOn(Storage.prototype, "setItem");
  forgetOtherRecents("u2");
  expect(set).toHaveBeenCalledWith(SIGNAL, "u2");
  forgetRecents("u2");
  expect(set).toHaveBeenCalledWith(SIGNAL, "");
  // Le signal est retiré aussitôt posé : aucun identifiant ne reste dans le stockage.
  expect(localStorage.getItem(SIGNAL)).toBeNull();
});

test("storage_that_throws_shows_no_recents", async () => {
  const refused = () => {
    throw new DOMException("stockage refusé", "SecurityError");
  };
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(refused);
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(refused);
  vi.spyOn(Storage.prototype, "removeItem").mockImplementation(refused);
  vi.spyOn(Storage.prototype, "key").mockImplementation(refused);
  // Rien ne lève : ni la visite, ni la lecture, ni l'oubli, ni l'entrée.
  expect(() => visit("u1", "s1", "a")).not.toThrow();
  expect(readRecents("u1", "s1")).toEqual([]);
  expect(() => forgetRecents("u1")).not.toThrow();
  expect(() => forgetOtherRecents("u1")).not.toThrow();
  const shown = await renderHook(() => useRecentPages("u1", "s1", [page("a")]), undefined);
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
