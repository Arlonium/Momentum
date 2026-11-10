// La page ouverte : celle de l'arbre chargé, sinon celle que le serveur rend.
import type { PageNode } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { getPage } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { notFound, page, unauthenticated, unreachable } from "../test/pages";
import { useOpenPage } from "./useOpenPage";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getPage: vi.fn(),
}));

/** Les appels à `getPage`, dans l'ordre : le test tient ou rompt chacun. */
let asked: { id: string; answer: Deferred<PageNode> }[] = [];

beforeEach(() => {
  asked = [];
  vi.mocked(getPage).mockReset();
  vi.mocked(getPage).mockImplementation((id) => {
    const answer = deferred<PageNode>();
    asked.push({ id, answer });
    return answer.promise;
  });
});

type Props = { activeId: string | null; inTree?: PageNode; retries?: number };
const sessionLost = vi.fn();
const hook = ({ activeId, inTree, retries = 0 }: Props) =>
  useOpenPage(activeId, inTree, { retries, onSessionLost: sessionLost });

test("page_of_the_loaded_tree_is_shown_without_asking", async () => {
  const x = page("x");
  const open = await renderHook(hook, { activeId: "x", inTree: x });
  expect(open.current.node).toBe(x);
  expect(open.current.resolving).toBe(false);
  expect(open.current.looked).toBeNull();
  expect(getPage).not.toHaveBeenCalled();
  // Aucune page ouverte : rien non plus.
  await open.rerender({ activeId: null });
  expect(open.current.node).toBeUndefined();
  expect(open.current.resolving).toBe(false);
  expect(getPage).not.toHaveBeenCalled();
});

test("page_outside_the_tree_is_asked_to_the_server", async () => {
  const open = await renderHook(hook, { activeId: "x" });
  // En attendant sa réponse, on ne sait pas quel espace montrer.
  expect(asked.map((a) => a.id)).toEqual(["x"]);
  expect(open.current.looked).toEqual({ id: "x", state: "loading" });
  expect(open.current.node).toBeUndefined();
  expect(open.current.resolving).toBe(true);

  const x = page("x", { spaceId: "pro", trashedAt: 12 });
  await settle(() => asked[0].answer.resolve(x));
  expect(open.current.node).toBe(x);
  expect(open.current.resolving).toBe(false);
  expect(open.current.looked).toEqual({ id: "x", state: "found", node: x });

  // L'arbre de son espace arrive, et la porte : c'est celle de l'arbre qui est montrée.
  const fromTree = page("x", { spaceId: "pro", title: "Renommée" });
  await open.rerender({ activeId: "x", inTree: fromTree });
  expect(open.current.node).toBe(fromTree);
  expect(asked).toHaveLength(1);
});

test("missing_page_and_unreadable_page_get_the_same_answer", async () => {
  const open = await renderHook(hook, { activeId: "x" });
  await settle(() => asked[0].answer.reject(notFound()));
  expect(open.current.looked).toEqual({ id: "x", state: "missing" });
  expect(open.current.node).toBeUndefined();
  // On sait à quoi s'en tenir : l'arbre de l'espace affiché peut être lu.
  expect(open.current.resolving).toBe(false);
});

test("server_failure_says_so", async () => {
  const open = await renderHook(hook, { activeId: "x" });
  await settle(() => asked[0].answer.reject(unreachable()));
  expect(open.current.looked).toEqual({
    id: "x",
    state: "failed",
    message: "Le serveur Mementum ne répond pas.",
  });
  expect(open.current.resolving).toBe(false);
});

test("lost_session_asks_again_when_the_account_is_back", async () => {
  sessionLost.mockClear();
  const open = await renderHook(hook, { activeId: "x" });
  await settle(() => asked[0].answer.reject(unauthenticated()));
  // La connexion est demandée ailleurs : ici, la page attend toujours.
  expect(sessionLost).toHaveBeenCalledTimes(1);
  expect(open.current.looked).toEqual({ id: "x", state: "loading" });
  // Le compte revient : la lecture repart.
  await open.rerender({ activeId: "x", retries: 1 });
  expect(asked).toHaveLength(2);
  const x = page("x");
  await settle(() => asked[1].answer.resolve(x));
  expect(open.current.node).toBe(x);
});

test("answer_for_a_page_left_meanwhile_is_dropped", async () => {
  const open = await renderHook(hook, { activeId: "x" });
  await open.rerender({ activeId: "y" });
  expect(asked.map((a) => a.id)).toEqual(["x", "y"]);
  // La réponse pour « x » arrive après qu'on l'a quittée : elle ne s'affiche pas.
  await settle(() => asked[0].answer.resolve(page("x")));
  expect(open.current.looked).toEqual({ id: "y", state: "loading" });
  expect(open.current.node).toBeUndefined();
  const y = page("y");
  await settle(() => asked[1].answer.resolve(y));
  expect(open.current.node).toBe(y);
});

test("page_that_leaves_the_tree_stays_on_screen_while_it_is_asked", async () => {
  const x = page("x");
  const open = await renderHook(hook, { activeId: "x", inTree: x });
  // Jetée ici : on sait déjà ce qu'elle est devenue.
  const trashed = page("x", { trashedAt: 5 });
  await settle(() => open.current.show(trashed));
  await open.rerender({ activeId: "x" });
  expect(open.current.node).toBe(trashed);
  expect(open.current.resolving).toBe(false);
  // Le serveur est relu, et confirme.
  expect(asked.map((a) => a.id)).toEqual(["x"]);
  const confirmed = page("x", { trashedAt: 6 });
  await settle(() => asked[0].answer.resolve(confirmed));
  expect(open.current.node).toBe(confirmed);
});

test("update_replaces_only_the_page_the_server_was_asked_for", async () => {
  const open = await renderHook(hook, { activeId: "x" });
  const x = page("x", { trashedAt: 5 });
  await settle(() => asked[0].answer.resolve(x));
  // Une autre page a changé : rien.
  await settle(() => open.current.update(page("y", { title: "Autre" })));
  expect(open.current.node).toBe(x);
  // Celle-ci (restaurée) : la voici.
  const restored = page("x");
  await settle(() => open.current.update(restored));
  expect(open.current.node).toBe(restored);
});

test("page_that_leaves_the_tree_stays_when_the_server_fails", async () => {
  const x = page("x");
  const open = await renderHook(hook, { activeId: "x", inTree: x });
  // Une relecture de l'arbre ne la porte plus. Elle reste à l'écran, telle qu'on la
  // connaissait, le temps que le serveur dise ce qu'elle est devenue.
  await open.rerender({ activeId: "x" });
  expect(open.current.node).toBe(x);
  expect(open.current.resolving).toBe(false);
  expect(asked.map((a) => a.id)).toEqual(["x"]);
  // Le serveur ne répond pas : elle reste, avec ce qu'on y a tapé.
  await settle(() => asked[0].answer.reject(unreachable()));
  expect(open.current.node).toBe(x);
  expect(open.current.looked).toEqual({ id: "x", state: "found", node: x });
  // Redemandée : le serveur répond qu'elle n'existe plus (ou qu'on ne la voit plus), et
  // alors elle s'en va.
  await settle(() => open.current.ask());
  expect(asked).toHaveLength(2);
  expect(open.current.node).toBe(x);
  await settle(() => asked[1].answer.reject(notFound()));
  expect(open.current.node).toBeUndefined();
  expect(open.current.looked).toEqual({ id: "x", state: "missing" });
});

test("ask_reads_again_only_a_page_outside_the_tree", async () => {
  const x = page("x");
  const open = await renderHook(hook, { activeId: "x", inTree: x });
  // Dans l'arbre : c'est lui qui la tient à jour.
  await settle(() => open.current.ask());
  expect(asked).toEqual([]);
});
