// Les favoris du compte dans l'espace affiché (R254) : lus après la lecture de l'arbre, jamais
// avant (rien sur le chemin de l'ouverture), relus après chaque lecture de l'arbre et après chaque
// favori posé ou retiré d'ici ; un favori se montre tout de suite, et revient en arrière si le
// serveur le refuse.
import type { Favorites } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, getFavorites, setFavorite } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { notFound, page, unauthenticated, unreachable } from "../test/pages";
import { favoritePages, useFavorites, type FavoritesOptions } from "./useFavorites";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getFavorites: vi.fn(),
  setFavorite: vi.fn(),
}));

/** Les lectures de la liste, dans l'ordre : le test tient ou rompt chacune. */
let reads: { spaceId: string; answer: Deferred<Favorites> }[] = [];
/** Les favoris posés (`true`) ou retirés, dans l'ordre où ils partent. */
let writes: { pageId: string; on: boolean; answer: Deferred<void> }[] = [];

/** Les écouteurs de la fin des lectures de l'arbre (`SpaceTree.onRead`). */
const treeListeners = new Set<(spaceId: string) => void>();
const onTreeRead = (listener: (spaceId: string) => void) => {
  treeListeners.add(listener);
  return () => {
    treeListeners.delete(listener);
  };
};
/** Une lecture de l'arbre de `spaceId` a abouti. */
const treeRead = (spaceId: string) =>
  settle(() => {
    for (const listener of [...treeListeners]) listener(spaceId);
  });

beforeEach(() => {
  vi.useFakeTimers();
  reads = [];
  writes = [];
  treeListeners.clear();
  vi.mocked(getFavorites).mockReset();
  vi.mocked(getFavorites).mockImplementation((spaceId) => {
    const answer = deferred<Favorites>();
    reads.push({ spaceId, answer });
    return answer.promise;
  });
  vi.mocked(setFavorite).mockReset();
  vi.mocked(setFavorite).mockImplementation((pageId, on) => {
    const answer = deferred<void>();
    writes.push({ pageId, on, answer });
    return answer.promise;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

type Props = { spaceId: string | null; options?: FavoritesOptions };
const hook = ({ spaceId, options }: Props) => useFavorites(spaceId, onTreeRead, options ?? {});
const advance = (ms: number) => settle(() => vi.advanceTimersByTime(ms));
const answer = (index: number, pages: string[]) =>
  settle(() => reads[index].answer.resolve({ pages }));

/** Le hook monté sur `perso`, l'arbre lu, et la première lecture tenue. */
async function loaded(pages: string[], options?: FavoritesOptions) {
  const rendered = await renderHook(hook, { spaceId: "perso", options });
  await treeRead("perso");
  await advance(0);
  await answer(reads.length - 1, pages);
  return rendered;
}

test("favorites_are_read_after_the_tree_never_before", async () => {
  // L'espace n'est pas encore connu : rien n'est lu.
  const rendered = await renderHook(hook, { spaceId: null });
  await advance(60_000);
  expect(reads).toEqual([]);
  // Il l'est, l'arbre pas encore : toujours rien, si longtemps qu'on attende. Une lecture de
  // l'arbre d'un autre espace n'y change rien.
  await rendered.rerender({ spaceId: "perso" });
  await advance(60_000);
  await treeRead("pro");
  await advance(60_000);
  expect(reads).toEqual([]);
  expect(rendered.current.pages).toEqual([]);
  expect(rendered.current.loaded).toBe(false);
  // L'arbre de l'espace est là : la liste est lue, une fois.
  await treeRead("perso");
  await advance(0);
  expect(reads.map((r) => r.spaceId)).toEqual(["perso"]);
  await answer(0, ["a", "b"]);
  expect(rendered.current.pages).toEqual(["a", "b"]);
  expect(rendered.current.loaded).toBe(true);
  await advance(60_000);
  expect(reads).toHaveLength(1);
});

test("favorites_are_reread_after_every_tree_read", async () => {
  // Un favori posé dans un autre onglet du compte n'avance pas la version de l'espace : l'arbre
  // relu au retour sur l'onglet est le même, de même version. Sa lecture suffit à relire la liste.
  const rendered = await loaded(["a"]);
  await treeRead("perso");
  await advance(0);
  expect(reads).toHaveLength(2);
  await answer(1, ["a", "c"]);
  expect(rendered.current.pages).toEqual(["a", "c"]);
  await treeRead("perso");
  await advance(0);
  expect(reads).toHaveLength(3);
  await answer(2, ["c"]);
  expect(rendered.current.pages).toEqual(["c"]);
});

test("favorites_are_read_again_once_the_account_is_back", async () => {
  // La première lecture trouve la session perdue : rien ne repart d'elle-même, puis la liste est
  // relue au retour du compte, sans autre demande (ni lecture de l'arbre, ni favori).
  const waiting: (() => void)[] = [];
  const afterLogin = () => new Promise<void>((resume) => waiting.push(resume));
  const rendered = await renderHook(hook, { spaceId: "perso", options: { afterLogin } });
  await treeRead("perso");
  await advance(0);
  await settle(() => reads[0].answer.reject(unauthenticated()));
  await advance(60_000);
  expect(reads).toHaveLength(1);
  expect(rendered.current.pages).toEqual([]);
  expect(rendered.current.error).toBeNull();
  await settle(() => waiting.splice(0).forEach((resume) => resume()));
  await advance(0);
  expect(reads).toHaveLength(2);
  await answer(1, ["a"]);
  expect(rendered.current.pages).toEqual(["a"]);
  await advance(60_000);
  expect(reads).toHaveLength(2);
});

test("a_toggle_shows_at_once_and_is_put_back_if_refused", async () => {
  const onRefused = vi.fn();
  const rendered = await loaded(["a"], { onRefused });
  // Posé : la page est là tout de suite, à la fin, avant toute réponse du serveur.
  let done = false;
  await settle(() => void rendered.current.toggle("b").then(() => (done = true)));
  expect(writes.map((w) => [w.pageId, w.on])).toEqual([["b", true]]);
  expect(rendered.current.pages).toEqual(["a", "b"]);
  // Refusé (la page n'est plus visible) : elle repart, et le refus se dit tel quel.
  await settle(() => writes[0].answer.reject(notFound()));
  expect(done).toBe(true);
  expect(rendered.current.pages).toEqual(["a"]);
  expect(rendered.current.error).toBe("page introuvable");
  expect(onRefused).toHaveBeenCalledTimes(1);
  expect(onRefused.mock.calls[0][0]).toBeInstanceOf(ApiError);
  expect(onRefused.mock.calls[0].slice(1)).toEqual(["b", true]);
  // Retiré : la page part tout de suite ; refusé, elle revient à sa place, et le message de
  // l'essai d'avant part avec le nouvel essai.
  await settle(() => void rendered.current.toggle("a"));
  expect(rendered.current.error).toBeNull();
  expect(writes.map((w) => [w.pageId, w.on])).toEqual([
    ["b", true],
    ["a", false],
  ]);
  expect(rendered.current.pages).toEqual([]);
  await settle(() => writes[1].answer.reject(new ApiError(500, "internal", "erreur interne")));
  expect(rendered.current.pages).toEqual(["a"]);
  expect(rendered.current.error).toBe("erreur interne");
  expect(onRefused.mock.calls[1].slice(1)).toEqual(["a", false]);
  // Accepté : la page reste, et la liste est relue.
  const before = reads.length;
  await settle(() => void rendered.current.toggle("b"));
  await settle(() => writes[2].answer.resolve());
  expect(rendered.current.pages).toEqual(["a", "b"]);
  await advance(0);
  expect(reads).toHaveLength(before + 1);
  await answer(before, ["a", "b"]);
  expect(rendered.current.pages).toEqual(["a", "b"]);
  expect(rendered.current.error).toBeNull();
});

test("a_reread_asked_during_a_read_is_done_once_after_it", async () => {
  const rendered = await loaded(["a"]);
  await treeRead("perso");
  await advance(0);
  expect(reads).toHaveLength(2);
  // Trois lectures de l'arbre pendant que la liste est relue : aucune ne l'annule, et elles ne
  // font qu'une lecture de plus, à sa fin.
  await treeRead("perso");
  await treeRead("perso");
  await treeRead("perso");
  await advance(60_000);
  expect(reads).toHaveLength(2);
  await answer(1, ["a", "b"]);
  expect(rendered.current.pages).toEqual(["a", "b"]);
  await advance(0);
  expect(reads).toHaveLength(3);
  await answer(2, ["b"]);
  await advance(60_000);
  expect(reads).toHaveLength(3);
  expect(rendered.current.pages).toEqual(["b"]);
});

test("favorites_of_the_previous_space_are_not_shown", async () => {
  const rendered = await loaded(["a"]);
  await treeRead("perso");
  await advance(0);
  // Un autre espace : plus rien de « perso » n'est montré, avant même sa lecture.
  await rendered.rerender({ spaceId: "pro" });
  expect(rendered.current.pages).toEqual([]);
  expect(rendered.current.loaded).toBe(false);
  // La réponse de « perso », arrivée après, ne se montre pas ; la liste de « pro » attend son arbre.
  await answer(1, ["a", "b"]);
  expect(rendered.current.pages).toEqual([]);
  await advance(60_000);
  expect(reads).toHaveLength(2);
  await treeRead("perso");
  await advance(0);
  expect(reads).toHaveLength(2);
  await treeRead("pro");
  await advance(0);
  expect(reads.map((r) => r.spaceId)).toEqual(["perso", "perso", "pro"]);
  await answer(2, ["x"]);
  expect(rendered.current.pages).toEqual(["x"]);
  expect(rendered.current.loaded).toBe(true);
  // Un favori posé dans « pro », dont la réponse arrive une fois revenu dans « perso » : il ne se
  // montre pas dans « perso », dont la liste repart de rien.
  await settle(() => void rendered.current.toggle("y"));
  expect(rendered.current.pages).toEqual(["x", "y"]);
  await rendered.rerender({ spaceId: "perso" });
  expect(rendered.current.pages).toEqual([]);
  await settle(() => writes[0].answer.resolve());
  expect(rendered.current.pages).toEqual([]);
  await advance(60_000);
  expect(reads).toHaveLength(3);
});

test("favorites_show_only_pages_of_the_tree", () => {
  // Les identifiants viennent du serveur, déjà triés par les droits ; entre deux lectures, une
  // page partie de l'arbre (à la corbeille, accès retiré) n'est plus montrée, et revient avec lui.
  const [a, b, c] = [page("a"), page("b"), page("c")];
  expect(favoritePages(["b", "x", "a"], [a, b, c])).toEqual([b, a]);
  expect(favoritePages(["x"], [a, b, c])).toEqual([]);
  expect(favoritePages(["a"], null)).toEqual([]);
  expect(favoritePages([], [a])).toEqual([]);
});

test("toggles_leave_one_after_the_other_in_the_order_given", async () => {
  // Posé puis retiré aussitôt (un réseau lent) : le retrait ne part qu'une fois la pose répondue,
  // pour que le serveur les reçoive dans cet ordre.
  const rendered = await loaded([]);
  await settle(() => void rendered.current.toggle("b"));
  await settle(() => void rendered.current.toggle("b"));
  expect(rendered.current.pages).toEqual([]);
  expect(writes.map((w) => [w.pageId, w.on])).toEqual([["b", true]]);
  await settle(() => writes[0].answer.resolve());
  expect(writes.map((w) => [w.pageId, w.on])).toEqual([
    ["b", true],
    ["b", false],
  ]);
  // La pose a répondu, le retrait pas encore : on voit toujours le dernier geste.
  expect(rendered.current.pages).toEqual([]);
  await settle(() => writes[1].answer.resolve());
  expect(rendered.current.pages).toEqual([]);
});

test("a_read_from_before_a_toggle_does_not_take_it_back", async () => {
  // Une lecture partie avant le favori répond après lui, sans lui : la page reste, et la liste
  // est relue une fois.
  const rendered = await loaded(["a"]);
  await treeRead("perso");
  await advance(0);
  expect(reads).toHaveLength(2);
  await settle(() => void rendered.current.toggle("b"));
  await settle(() => writes[0].answer.resolve());
  await answer(1, ["a"]);
  expect(rendered.current.pages).toEqual(["a", "b"]);
  await advance(0);
  expect(reads).toHaveLength(3);
  await answer(2, ["a", "b"]);
  expect(rendered.current.pages).toEqual(["a", "b"]);
  // Une lecture partie après le favori fait foi : la page a pu partir à la corbeille entre-temps.
  await treeRead("perso");
  await advance(0);
  await answer(3, ["a"]);
  expect(rendered.current.pages).toEqual(["a"]);
});

test("a_toggle_refused_by_a_lost_session_is_put_back_without_a_message", async () => {
  // La connexion est demandée par-dessus l'application : ce n'est pas un refus.
  const onRefused = vi.fn();
  const rendered = await loaded(["a"], { onRefused });
  await settle(() => void rendered.current.toggle("b"));
  await settle(() => writes[0].answer.reject(unauthenticated()));
  expect(rendered.current.pages).toEqual(["a"]);
  expect(rendered.current.error).toBeNull();
  expect(onRefused).not.toHaveBeenCalled();
  // Un serveur injoignable, lui, se dit.
  await settle(() => void rendered.current.toggle("b"));
  await settle(() => writes[1].answer.reject(unreachable()));
  expect(rendered.current.error).toBe("Le serveur Mementum ne répond pas.");
  expect(onRefused).toHaveBeenCalledTimes(1);
});

test("a_failed_read_keeps_what_was_shown", async () => {
  const rendered = await loaded(["a"]);
  await treeRead("perso");
  await advance(0);
  await settle(() => reads[1].answer.reject(unreachable()));
  expect(rendered.current.pages).toEqual(["a"]);
  // Rien ne se refait de soi-même : c'est la prochaine lecture de l'arbre qui la refera.
  await advance(60_000);
  expect(reads).toHaveLength(2);
});

test("nothing_is_read_or_sent_after_unmount", async () => {
  const rendered = await loaded(["a"]);
  const toggled = rendered.current.toggle;
  await settle(() => void toggled("b"));
  await rendered.unmount();
  await treeRead("perso");
  await settle(() => writes[0].answer.resolve());
  await advance(60_000);
  expect(reads).toHaveLength(1);
  // Un favori demandé après le démontage ne part pas.
  await settle(() => void toggled("c"));
  expect(writes).toHaveLength(1);
});

test("every_gesture_made_here_is_ranked_for_the_sidebar", async () => {
  // La barre latérale ne gèle pas ce que la personne fait elle-même (R411), d'où que vienne le
  // geste (menu d'une ligne, menu de la page) : le hook dit le rang du dernier geste de chaque page.
  const rendered = await loaded(["a"]);
  expect([...rendered.current.toggles]).toEqual([]);
  await settle(() => void rendered.current.toggle("b"));
  await settle(() => void rendered.current.toggle("a"));
  const first = rendered.current.toggles;
  expect([...first.keys()]).toEqual(["b", "a"]);
  expect(first.get("a")!).toBeGreaterThan(first.get("b")!);
  // Un geste refusé compte aussi : c'est un geste d'ici, que la barre ne doit pas geler.
  await settle(() => writes[0].answer.reject(notFound()));
  await settle(() => writes[1].answer.resolve());
  await settle(() => void rendered.current.toggle("b"));
  expect(rendered.current.toggles.get("b")!).toBeGreaterThan(first.get("a")!);
  // Un autre espace repart de rien.
  await rendered.rerender({ spaceId: "pro" });
  expect([...rendered.current.toggles]).toEqual([]);
});

test("an_undone_gesture_is_ranked_again_for_the_sidebar", async () => {
  // Un geste défait (refusé, ou session perdue) change la liste montrée, d'ici : la barre ne doit
  // pas le geler, même si le geste lui-même date d'avant le gel (M3). Son rang monte.
  const rendered = await loaded(["a"]);
  await settle(() => void rendered.current.toggle("b"));
  const made = rendered.current.toggles.get("b")!;
  await settle(() => void rendered.current.toggle("c"));
  const later = rendered.current.toggles.get("c")!;
  await settle(() => writes[0].answer.reject(notFound()));
  expect(rendered.current.toggles.get("b")!).toBeGreaterThan(later);
  await settle(() => writes[1].answer.reject(unauthenticated()));
  expect(rendered.current.toggles.get("c")!).toBeGreaterThan(rendered.current.toggles.get("b")!);
  // Un geste accepté ne change pas la liste montrée : son rang reste.
  await settle(() => void rendered.current.toggle("d"));
  const accepted = rendered.current.toggles.get("d")!;
  await settle(() => writes[2].answer.resolve());
  expect(rendered.current.toggles.get("d")).toBe(accepted);
  expect(made).toBeLessThan(later);
});
