// L'arbre de l'espace affiché : sa lecture, ses relectures, et ce que les actions d'ici y
// changent sans l'attendre.
import type { PageNode, Tree } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, getTree } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { page, unauthenticated, unreachable } from "../test/pages";
import {
  POLL_INTERVAL_MS,
  REFRESH_INTERVAL_MS,
  sameNodes,
  useSpaceTree,
  useTreeCatchUp,
  useTreeReader,
} from "./useSpaceTree";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getTree: vi.fn(),
}));

/** Les lectures de l'arbre, dans l'ordre : le test tient ou rompt chacune. */
let reads: { spaceId: string; answer: Deferred<Tree> }[] = [];
const answer = (index: number, spaceId: string, nodes: PageNode[], lsn = 0) =>
  settle(() => reads[index].answer.resolve({ spaceId, lsn, nodes }));

const onSessionLost = vi.fn();
const onGone = vi.fn();
const onReturn = vi.fn();

beforeEach(() => {
  reads = [];
  for (const mock of [onSessionLost, onGone, onReturn]) mock.mockClear();
  vi.mocked(getTree).mockReset();
  vi.mocked(getTree).mockImplementation((spaceId) => {
    const answer = deferred<Tree>();
    reads.push({ spaceId, answer });
    return answer.promise;
  });
});

type Props = { spaceId: string | null; retries?: number; live?: boolean };
function hook({ spaceId, retries = 0, live }: Props) {
  const tree = useSpaceTree();
  const readError = useTreeReader(tree, spaceId, {
    retries,
    live,
    onSessionLost,
    onGone,
    onReturn,
  });
  return { ...tree, readError };
}

test("tree_of_the_space_is_read_once_known", async () => {
  // On ne sait pas encore quel espace montrer : rien n'est lu.
  const tree = await renderHook(hook, { spaceId: null });
  expect(reads).toEqual([]);
  expect(tree.current.loaded).toBeNull();

  await tree.rerender({ spaceId: "perso" });
  expect(reads.map((r) => r.spaceId)).toEqual(["perso"]);
  const a = page("a");
  await answer(0, "perso", [a]);
  expect(tree.current.loaded).toEqual({ spaceId: "perso", nodes: [a] });
  expect(tree.current.latest()).toBe(tree.current.loaded);
  // Rendu de nouveau sans rien de neuf : pas d'autre lecture.
  await tree.rerender({ spaceId: "perso" });
  expect(reads).toHaveLength(1);
});

test("read_of_a_space_left_meanwhile_is_dropped", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  await tree.rerender({ spaceId: "pro" });
  expect(reads.map((r) => r.spaceId)).toEqual(["perso", "pro"]);
  // La réponse de « perso » arrive après qu'on l'a quitté : elle ne s'affiche pas.
  await answer(0, "perso", [page("a")]);
  expect(tree.current.loaded).toBeNull();
  const b = page("b", { spaceId: "pro" });
  await answer(1, "pro", [b]);
  expect(tree.current.loaded).toEqual({ spaceId: "pro", nodes: [b] });
});

test("reload_reads_again_and_replaces_the_tree", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", [page("a"), page("b")]);
  await settle(() => tree.current.reload());
  expect(reads).toHaveLength(2);
  // Ce que le serveur rend remplace tout : une page partie ailleurs s'en va.
  const a = page("a");
  await answer(1, "perso", [a]);
  expect(tree.current.loaded?.nodes).toEqual([a]);
});

test("reread_that_changes_nothing_keeps_the_loaded_tree", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  const nodes = [page("a"), page("b", { parentId: "a" })];
  await answer(0, "perso", nodes);
  const loaded = tree.current.loaded;
  // Les mêmes pages, les mêmes champs, le même ordre, dans d'autres objets (une autre réponse
  // du serveur) : l'arbre chargé reste le même objet, et rien de ce qui en dépend n'est rendu
  // de nouveau (un menu ouvert, un glisser en cours, le focus).
  const rereads = async (answerNodes: PageNode[]) => {
    await settle(() => tree.current.reload());
    await answer(reads.length - 1, "perso", answerNodes);
  };
  await rereads(nodes.map((node) => ({ ...node })));
  expect(tree.current.loaded).toBe(loaded);
  expect(tree.current.latest()).toBe(loaded);

  // Dès que quelque chose diffère, ce que le serveur rend remplace l'arbre : un champ…
  const renamed = [page("a", { title: "Renommée ailleurs" }), page("b", { parentId: "a" })];
  await rereads(renamed);
  expect(tree.current.loaded?.nodes).toBe(renamed);
  // … l'ordre…
  const reordered = [renamed[1], renamed[0]];
  await rereads(reordered);
  expect(tree.current.loaded?.nodes).toBe(reordered);
  // … une page de plus, ou de moins.
  const more = [...reordered, page("c")];
  await rereads(more);
  expect(tree.current.loaded?.nodes).toBe(more);
  const fewer = more.slice(0, 2);
  await rereads(fewer);
  expect(tree.current.loaded?.nodes).toBe(fewer);
});

test("reread_that_changes_nothing_still_clears_the_read_error", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  const a = page("a");
  await answer(0, "perso", [a]);
  const loaded = tree.current.loaded;
  await settle(() => tree.current.reload());
  await settle(() => reads[1].answer.reject(unreachable()));
  expect(tree.current.readError).toBe("Le serveur Mementum ne répond pas.");
  // Le serveur revient, et rend le même arbre : le message part, l'arbre reste le même objet.
  await settle(() => tree.current.reload());
  await answer(2, "perso", [{ ...a }]);
  expect(tree.current.readError).toBeNull();
  expect(tree.current.loaded).toBe(loaded);
});

test("same_nodes_compares_pages_fields_and_order", () => {
  const a = page("a");
  const b = page("b", { parentId: "a" });
  expect(sameNodes([], [])).toBe(true);
  expect(sameNodes([a, b], [{ ...a }, { ...b }])).toBe(true);
  expect(sameNodes([a, b], [b, a])).toBe(false);
  expect(sameNodes([a, b], [a])).toBe(false);
  expect(sameNodes([a], [a, b])).toBe(false);
  // Chaque champ d'une page compte : ce qu'on y voit, où elle est, ce qu'on peut en faire.
  const changes: Partial<PageNode>[] = [
    { title: "Autre" },
    { position: "a1" },
    { parentId: "z" },
    { level: "read" },
    { section: "private" },
    { updatedAt: 1 },
    { trashedAt: 1 },
    { spaceId: "ailleurs" },
  ];
  for (const change of changes) {
    expect(sameNodes([a], [{ ...a, ...change }]), JSON.stringify(change)).toBe(false);
  }
});

test("read_overtaken_by_a_local_change_is_made_again", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  const a = page("a");
  await answer(0, "perso", [a]);
  await settle(() => tree.current.reload());
  // Pendant la relecture, une page est créée ici.
  const b = page("b");
  await settle(() => tree.current.change((nodes) => [...nodes, b]));
  expect(tree.current.loaded?.nodes).toEqual([a, b]);
  // La réponse, partie avant, n'a pas cette page : elle est jetée, et la lecture refaite.
  await answer(1, "perso", [a]);
  expect(tree.current.loaded?.nodes).toEqual([a, b]);
  expect(reads).toHaveLength(3);
  // La lecture refaite rend la page telle que le serveur l'a (une autre date) : c'est elle qui
  // est montrée. (Identique à l'arbre chargé, elle le laisserait tel quel.)
  const fresh = [page("a"), page("b", { updatedAt: 5 })];
  await answer(2, "perso", fresh);
  expect(tree.current.loaded?.nodes).toBe(fresh);

  // `touch` (le serveur a changé sans que l'arbre le montre) périme aussi une lecture.
  await settle(() => tree.current.reload());
  await settle(() => tree.current.touch());
  await answer(3, "perso", [a]);
  expect(tree.current.loaded?.nodes).toBe(fresh);
  expect(reads).toHaveLength(5);
});

test("change_applies_to_the_loaded_tree_only", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  // Rien de chargé : rien à changer.
  await settle(() => tree.current.change((nodes) => [...nodes, page("x")]));
  expect(tree.current.loaded).toBeNull();
  // La lecture en cours, périmée par ce changement, est refaite.
  await answer(0, "perso", []);
  expect(reads).toHaveLength(2);
  const a = page("a");
  await answer(1, "perso", [a]);
  // La fonction reçoit les pages et l'espace de l'arbre chargé.
  const seen: string[] = [];
  await settle(() =>
    tree.current.change((nodes, spaceId) => {
      seen.push(spaceId);
      return nodes.filter((n) => n.id !== "a");
    }),
  );
  expect(seen).toEqual(["perso"]);
  expect(tree.current.loaded).toEqual({ spaceId: "perso", nodes: [] });
});

test("failed_read_tells_why", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  // Plus de rôle dans l'espace : `404`, comme un espace qui n'existe pas.
  await settle(() => reads[0].answer.reject(new ApiError(404, "not_found", "espace introuvable")));
  expect(onGone).toHaveBeenCalledTimes(1);
  // Session perdue : la lecture repart quand le compte revient.
  await settle(() => tree.current.reload());
  await settle(() => reads[1].answer.reject(unauthenticated()));
  expect(onSessionLost).toHaveBeenCalledTimes(1);
  await tree.rerender({ spaceId: "perso", retries: 1 });
  expect(reads).toHaveLength(3);
  // Serveur injoignable : son message. Ni l'espace parti ni la session perdue n'en sont un.
  expect(tree.current.readError).toBeNull();
  await settle(() => reads[2].answer.reject(unreachable()));
  expect(tree.current.readError).toBe("Le serveur Mementum ne répond pas.");
  expect(tree.current.loaded).toBeNull();
});

test("failed_reload_keeps_the_tree_and_says_so", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  const a = page("a");
  await answer(0, "perso", [a]);
  expect(tree.current.readError).toBeNull();
  // Relecture, serveur injoignable : l'arbre affiché reste, et l'échec est dit.
  await settle(() => tree.current.reload());
  await settle(() => reads[1].answer.reject(unreachable()));
  expect(tree.current.loaded).toEqual({ spaceId: "perso", nodes: [a] });
  expect(tree.current.readError).toBe("Le serveur Mementum ne répond pas.");
  // La relecture suivante réussit : le message part.
  await settle(() => tree.current.reload());
  const fresh = [page("a"), page("b")];
  await answer(2, "perso", fresh);
  expect(tree.current.loaded?.nodes).toBe(fresh);
  expect(tree.current.readError).toBeNull();
});

test("failed_read_is_made_again_when_the_network_is_back", async () => {
  const tree = await renderHook(hook, { spaceId: "perso" });
  await settle(() => reads[0].answer.reject(unreachable()));
  expect(tree.current.readError).not.toBeNull();
  // Le navigateur retrouve le réseau : la lecture repart d'elle-même.
  await settle(() => window.dispatchEvent(new Event("online")));
  expect(reads).toHaveLength(2);
  await answer(1, "perso", [page("a")]);
  expect(tree.current.readError).toBeNull();
  // Sans échec en attente, le retour du réseau ne relit rien.
  await settle(() => window.dispatchEvent(new Event("online")));
  expect(reads).toHaveLength(2);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

test("tree_is_read_again_when_the_tab_comes_back", async () => {
  // Seule l'horloge est fausse : les promesses suivent leur cours.
  vi.useFakeTimers({ toFake: ["Date"], now: 1_000_000 });
  let visibility: DocumentVisibilityState = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  const focus = () => settle(() => window.dispatchEvent(new Event("focus")));
  const visible = (state: DocumentVisibilityState) =>
    settle(() => {
      visibility = state;
      document.dispatchEvent(new Event("visibilitychange"));
    });

  const tree = await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", [page("a")]);
  expect(REFRESH_INTERVAL_MS).toBe(5_000);

  // L'onglet reprend le focus juste après une lecture : elle est assez fraîche.
  vi.setSystemTime(1_000_000 + REFRESH_INTERVAL_MS - 1);
  await focus();
  expect(reads).toHaveLength(1);
  // Cinq secondes après : l'arbre est relu, et ce que d'autres ont changé arrive.
  vi.setSystemTime(1_000_000 + REFRESH_INTERVAL_MS);
  await focus();
  expect(reads).toHaveLength(2);
  const fresh = [page("a"), page("b")];
  await answer(1, "perso", fresh);
  expect(tree.current.loaded?.nodes).toBe(fresh);
  // Au plus une fois toutes les cinq secondes, quel que soit le signal.
  await focus();
  await visible("visible");
  expect(reads).toHaveLength(2);

  // L'onglet passe à l'arrière-plan : rien. Il redevient visible : relu.
  vi.setSystemTime(1_000_000 + 3 * REFRESH_INTERVAL_MS);
  await visible("hidden");
  expect(reads).toHaveLength(2);
  await visible("visible");
  expect(reads).toHaveLength(3);
  await answer(2, "perso", fresh);

  // Une relecture demandée par une action compte aussi : le retour sur l'onglet, juste
  // après, n'en refait pas une.
  vi.setSystemTime(1_000_000 + 5 * REFRESH_INTERVAL_MS);
  await settle(() => tree.current.reload());
  expect(reads).toHaveLength(4);
  await focus();
  expect(reads).toHaveLength(4);
});

test("tab_coming_back_reads_nothing_before_a_space_is_known", async () => {
  vi.useFakeTimers({ toFake: ["Date"], now: 1_000_000 });
  await renderHook(hook, { spaceId: null });
  vi.setSystemTime(2_000_000);
  await settle(() => window.dispatchEvent(new Event("focus")));
  expect(reads).toEqual([]);
  expect(onReturn).not.toHaveBeenCalled();
});

test("tab_coming_back_is_told_with_the_tree_read_and_no_more_often", async () => {
  // Ce qui se relit avec l'arbre au retour sur l'onglet (le compte, Ruling R126) : au même
  // moment, au plus une fois toutes les cinq secondes.
  vi.useFakeTimers({ toFake: ["Date"], now: 1_000_000 });
  const focus = () => settle(() => window.dispatchEvent(new Event("focus")));
  const tree = await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", [page("a")]);
  // La première lecture de l'arbre n'est pas un retour.
  expect(onReturn).not.toHaveBeenCalled();

  // Trop tôt : ni l'arbre ni le reste.
  vi.setSystemTime(1_000_000 + REFRESH_INTERVAL_MS - 1);
  await focus();
  expect(onReturn).not.toHaveBeenCalled();
  // Cinq secondes après : les deux, une fois, quel que soit le nombre de signaux.
  vi.setSystemTime(1_000_000 + REFRESH_INTERVAL_MS);
  await focus();
  await settle(() => document.dispatchEvent(new Event("visibilitychange")));
  expect(reads).toHaveLength(2);
  expect(onReturn).toHaveBeenCalledTimes(1);
  await answer(1, "perso", [page("a")]);

  // Une relecture de l'arbre demandée par une action, ou par le minuteur (R128), n'est pas un
  // retour sur l'onglet. Le retour qui la suit de près ne relit pas l'arbre une seconde fois,
  // mais il relit le compte, qui a son propre compte de temps : le minuteur, qui relit l'arbre
  // seul, ne doit pas l'en priver.
  vi.setSystemTime(1_000_000 + 3 * REFRESH_INTERVAL_MS);
  await settle(() => tree.current.reload());
  expect(reads).toHaveLength(3);
  expect(onReturn).toHaveBeenCalledTimes(1);
  await focus();
  expect(reads).toHaveLength(3);
  expect(onReturn).toHaveBeenCalledTimes(2);
  // Un autre retour dans les cinq secondes : rien.
  vi.setSystemTime(1_000_000 + 4 * REFRESH_INTERVAL_MS - 1);
  await focus();
  expect(reads).toHaveLength(3);
  expect(onReturn).toHaveBeenCalledTimes(2);
  // Cinq secondes plus tard : de nouveau les deux.
  vi.setSystemTime(1_000_000 + 4 * REFRESH_INTERVAL_MS);
  await focus();
  expect(reads).toHaveLength(4);
  expect(onReturn).toHaveBeenCalledTimes(3);
});

/**
 * Horloge et minuteurs simulés, et la visibilité de l'onglet : le minuteur de relecture (R128)
 * avance à la demande. Les promesses suivent leur cours.
 */
function simulated() {
  vi.useFakeTimers({ toFake: ["Date", "setTimeout", "clearTimeout"], now: 1_000_000 });
  let visibility: DocumentVisibilityState = "visible";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visibility);
  return {
    pass: (ms: number) => settle(() => vi.advanceTimersByTime(ms)),
    focus: () => settle(() => window.dispatchEvent(new Event("focus"))),
    show: (state: DocumentVisibilityState) =>
      settle(() => {
        visibility = state;
        document.dispatchEvent(new Event("visibilitychange"));
      }),
  };
}

test("tree_is_read_again_every_ten_seconds_while_the_tab_is_visible", async () => {
  const clock = simulated();
  const tree = await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", [page("a")]);
  expect(POLL_INTERVAL_MS).toBe(10_000);

  // Sans retour sur l'onglet ni action : dix secondes après la dernière lecture, pas avant.
  await clock.pass(POLL_INTERVAL_MS - 1);
  expect(reads).toHaveLength(1);
  await clock.pass(1);
  expect(reads).toHaveLength(2);
  // Ce que quelqu'un d'autre a créé entre-temps arrive.
  const fresh = [page("a"), page("b")];
  await answer(1, "perso", fresh);
  expect(tree.current.loaded?.nodes).toBe(fresh);
  // Et ainsi de suite.
  await clock.pass(POLL_INTERVAL_MS);
  expect(reads).toHaveLength(3);
  await answer(
    2,
    "perso",
    fresh.map((node) => ({ ...node })),
  );
  // Rien n'a changé : l'arbre chargé reste le même objet.
  expect(tree.current.loaded?.nodes).toBe(fresh);
  // Le minuteur ne relit que l'arbre : le compte reste relu au retour sur l'onglet (R126).
  expect(onReturn).not.toHaveBeenCalled();
});

test("hidden_tab_reads_nothing_until_it_comes_back", async () => {
  const clock = simulated();
  await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", []);
  await clock.show("hidden");
  await clock.pass(5 * POLL_INTERVAL_MS);
  expect(reads).toHaveLength(1);
  // De retour : relu tout de suite (R97), puis dix secondes après cette lecture.
  await clock.show("visible");
  expect(reads).toHaveLength(2);
  await answer(1, "perso", []);
  await clock.pass(POLL_INTERVAL_MS - 1);
  expect(reads).toHaveLength(2);
  await clock.pass(1);
  expect(reads).toHaveLength(3);
});

test("timer_starts_again_from_the_last_read_whatever_asked_for_it", async () => {
  const clock = simulated();
  const tree = await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", []);
  // Retour sur l'onglet à 7 s : l'arbre est relu. Le minuteur, qui tombait à 10 s, ne relit
  // pas coup sur coup : il repart de cette lecture, et tombe à 17 s.
  await clock.pass(7_000);
  await clock.focus();
  expect(reads).toHaveLength(2);
  await answer(1, "perso", []);
  await clock.pass(3_000);
  expect(reads).toHaveLength(2);
  await clock.pass(6_999);
  expect(reads).toHaveLength(2);
  await clock.pass(1);
  expect(reads).toHaveLength(3);
  await answer(2, "perso", []);
  // De même après une relecture demandée par une action, à 21 s : la suivante tombe à 31 s.
  await clock.pass(4_000);
  await settle(() => tree.current.reload());
  expect(reads).toHaveLength(4);
  await answer(3, "perso", []);
  await clock.pass(9_999);
  expect(reads).toHaveLength(4);
  await clock.pass(1);
  expect(reads).toHaveLength(5);
  // Et le retour sur l'onglet juste après une lecture du minuteur ne la double pas.
  await answer(4, "perso", []);
  await clock.pass(1_000);
  await clock.focus();
  expect(reads).toHaveLength(5);
});

test("timer_does_not_read_over_a_read_in_flight", async () => {
  const clock = simulated();
  await renderHook(hook, { spaceId: "perso" });
  // La première lecture n'a pas encore répondu (serveur lent) : pas d'autre par-dessus.
  await clock.pass(3 * POLL_INTERVAL_MS);
  expect(reads).toHaveLength(1);
  await answer(0, "perso", []);
  // Elle a répondu : dix secondes plus tard au plus, l'arbre est relu.
  await clock.pass(POLL_INTERVAL_MS);
  expect(reads).toHaveLength(2);
});

test("timer_stops_without_a_space_and_with_the_view", async () => {
  const clock = simulated();
  const tree = await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", []);
  // Plus d'espace connu : plus de minuteur.
  await tree.rerender({ spaceId: null });
  expect(vi.getTimerCount()).toBe(0);
  await clock.pass(3 * POLL_INTERVAL_MS);
  expect(reads).toHaveLength(1);
  // De nouveau un espace : lu, puis relu dix secondes après.
  await tree.rerender({ spaceId: "pro" });
  expect(reads).toHaveLength(2);
  await answer(1, "pro", []);
  await clock.pass(POLL_INTERVAL_MS);
  expect(reads).toHaveLength(3);
  await answer(2, "pro", []);
  // La vue démontée : plus rien n'est relu, et aucun minuteur ne reste.
  await tree.unmount();
  expect(vi.getTimerCount()).toBe(0);
  await clock.pass(3 * POLL_INTERVAL_MS);
  expect(reads).toHaveLength(3);
});

type CatchUpProps = { nodes: PageNode[] | null; activeId: string | null; found: PageNode | null };
function catchUpHook({ nodes, activeId, found }: CatchUpProps) {
  const tree = useSpaceTree();
  const skip = useTreeCatchUp(tree, nodes, activeId, found);
  return { reloads: tree.reloads, skip };
}

test("page_the_server_shows_but_the_tree_lacks_reloads_the_tree_once", async () => {
  const x = page("x");
  const a = page("a");
  const hook = await renderHook(catchUpHook, { nodes: null, activeId: "x", found: x });
  // L'arbre n'est pas encore chargé : on ne sait pas s'il la porte.
  expect(hook.current.reloads).toBe(0);
  // Chargé, sans elle (créée ailleurs depuis) : il est relu.
  await hook.rerender({ nodes: [a], activeId: "x", found: x });
  expect(hook.current.reloads).toBe(1);
  // Relu, toujours sans elle : pas une seconde fois.
  await hook.rerender({ nodes: [page("a")], activeId: "x", found: x });
  expect(hook.current.reloads).toBe(1);
  // Une page à la corbeille n'est pas dans l'arbre : rien à relire.
  await hook.rerender({ nodes: [a], activeId: "y", found: page("y", { trashedAt: 3 }) });
  expect(hook.current.reloads).toBe(1);
  // Revenue sur la première page : le compte repart.
  await hook.rerender({ nodes: [a], activeId: "x", found: x });
  expect(hook.current.reloads).toBe(2);
  // L'arbre la porte : rien.
  await hook.rerender({ nodes: [a, page("z")], activeId: "z", found: page("z") });
  expect(hook.current.reloads).toBe(2);
});

test("catch_up_is_skipped_for_a_page_just_reloaded_for", async () => {
  const a = page("a");
  const x = page("x", { trashedAt: 4 });
  const hook = await renderHook(catchUpHook, { nodes: [a], activeId: "x", found: x });
  // Restaurée ici : l'arbre est déjà relu pour elle.
  await settle(() => hook.current.skip("x"));
  await hook.rerender({ nodes: [a], activeId: "x", found: page("x") });
  expect(hook.current.reloads).toBe(0);
});

test("tree_remembers_the_version_it_was_read_at", async () => {
  // C'est avec elle que le canal de l'espace compare ses trames (R141), même quand une
  // relecture rend les mêmes pages : l'arbre chargé reste alors le même objet.
  const tree = await renderHook(hook, { spaceId: "perso" });
  expect(tree.current.knownLsn("perso")).toBeNull();
  const a = [page("a")];
  await answer(0, "perso", a, 7);
  expect(tree.current.knownLsn("perso")).toBe(7);
  // Celle d'un autre espace n'est pas celle de l'arbre affiché.
  expect(tree.current.knownLsn("autre")).toBeNull();
  await settle(() => tree.current.reload());
  await answer(1, "perso", [{ ...a[0] }], 9);
  expect(tree.current.loaded?.nodes).toBe(a);
  expect(tree.current.knownLsn("perso")).toBe(9);
});

test("polling_stops_while_the_channel_is_live_and_resumes_when_it_falls", async () => {
  const clock = simulated();
  const tree = await renderHook(hook, { spaceId: "perso", live: true });
  await answer(0, "perso", [page("a")]);
  await clock.pass(3 * POLL_INTERVAL_MS);
  expect(reads).toHaveLength(1);
  // Le canal tombe : le minuteur reprend, depuis la dernière lecture (déjà ancienne).
  await tree.rerender({ spaceId: "perso", live: false });
  await clock.pass(POLL_INTERVAL_MS);
  expect(reads).toHaveLength(2);
  await answer(1, "perso", [page("a")]);
  // Il revient : le minuteur se tait de nouveau.
  await tree.rerender({ spaceId: "perso", live: true });
  await clock.pass(3 * POLL_INTERVAL_MS);
  expect(reads).toHaveLength(2);
  // Le retour sur l'onglet reste une raison de relire.
  await clock.focus();
  expect(reads).toHaveLength(3);
});

test("account_is_read_again_after_a_change_with_the_same_limit_and_replayed", async () => {
  // Un espace renommé, un rôle changé par quelqu'un d'autre (R150) : le compte est relu par
  // le chemin du retour sur l'onglet, au plus une fois toutes les cinq secondes ; un appel
  // retenu par la limite est rejoué à son échéance, une seule fois pour tous ceux qu'elle a
  // retenus.
  const clock = simulated();
  const tree = await renderHook(hook, { spaceId: "perso", live: true });
  await answer(0, "perso", [page("a")]);
  // Le compte vient d'être lu, au chargement de l'application : trop tôt.
  await clock.pass(1_000);
  await settle(() => tree.current.channelChanged(1, false));
  await settle(() => tree.current.channelChanged(1, false));
  expect(onReturn).not.toHaveBeenCalled();
  await clock.pass(REFRESH_INTERVAL_MS - 1_000 - 1);
  expect(onReturn).not.toHaveBeenCalled();
  await clock.pass(1);
  expect(onReturn).toHaveBeenCalledTimes(1);
  // Hors limite : tout de suite, et la limite repart.
  await clock.pass(REFRESH_INTERVAL_MS);
  await settle(() => tree.current.channelChanged(1, false));
  expect(onReturn).toHaveBeenCalledTimes(2);
  await clock.pass(100);
  await settle(() => tree.current.channelChanged(1, false));
  expect(onReturn).toHaveBeenCalledTimes(2);
  await clock.pass(REFRESH_INTERVAL_MS - 100);
  expect(onReturn).toHaveBeenCalledTimes(3);
  await clock.pass(10 * REFRESH_INTERVAL_MS);
  expect(onReturn).toHaveBeenCalledTimes(3);
});

test("account_replay_is_cancelled_when_the_hook_unmounts", async () => {
  const clock = simulated();
  const tree = await renderHook(hook, { spaceId: "perso", live: true });
  await answer(0, "perso", [page("a")]);
  await settle(() => tree.current.channelChanged(1, false));
  await tree.unmount();
  await clock.pass(2 * REFRESH_INTERVAL_MS);
  expect(onReturn).not.toHaveBeenCalled();
});

test("known_version_follows_the_last_read_even_when_it_goes_down", async () => {
  // Une base restaurée sous un serveur relancé : l'arbre n'est plus à jour de ce qu'on croyait
  // (R156), la version connue est celle que la dernière lecture a rendue.
  const tree = await renderHook(hook, { spaceId: "perso" });
  await answer(0, "perso", [page("a")], 9);
  await settle(() => tree.current.reload());
  await answer(1, "perso", [page("a")], 4);
  expect(tree.current.knownLsn("perso")).toBe(4);
});
