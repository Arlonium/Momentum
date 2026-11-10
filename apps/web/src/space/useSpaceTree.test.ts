// L'arbre de l'espace affiché : sa lecture, ses relectures, et ce que les actions d'ici y
// changent sans l'attendre.
import type { PageNode, Tree } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { ApiError, getTree } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { page, unauthenticated, unreachable } from "../test/pages";
import { useSpaceTree, useTreeCatchUp, useTreeReader } from "./useSpaceTree";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getTree: vi.fn(),
}));

/** Les lectures de l'arbre, dans l'ordre : le test tient ou rompt chacune. */
let reads: { spaceId: string; answer: Deferred<Tree> }[] = [];
const answer = (index: number, spaceId: string, nodes: PageNode[]) =>
  settle(() => reads[index].answer.resolve({ spaceId, nodes }));

const onSessionLost = vi.fn();
const onGone = vi.fn();
const onError = vi.fn();

beforeEach(() => {
  reads = [];
  for (const mock of [onSessionLost, onGone, onError]) mock.mockClear();
  vi.mocked(getTree).mockReset();
  vi.mocked(getTree).mockImplementation((spaceId) => {
    const answer = deferred<Tree>();
    reads.push({ spaceId, answer });
    return answer.promise;
  });
});

type Props = { spaceId: string | null; retries?: number };
function hook({ spaceId, retries = 0 }: Props) {
  const tree = useSpaceTree();
  useTreeReader(tree, spaceId, { retries, onSessionLost, onGone, onError });
  return tree;
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
  const fresh = [page("a"), page("b")];
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
  // Serveur injoignable : son message.
  await settle(() => reads[2].answer.reject(unreachable()));
  expect(onError).toHaveBeenCalledWith("Le serveur Mementum ne répond pas.");
  expect(tree.current.loaded).toBeNull();
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
