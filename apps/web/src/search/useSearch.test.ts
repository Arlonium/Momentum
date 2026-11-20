// `useSearch` (lot 4, Task C1) : ce que le serveur trouve pour ce qui est tapé. Une requête part
// 150 ms après la dernière frappe ; celle d'avant est abandonnée, et sa réponse, arrivée quand
// même, est jetée, `429 search_busy` compris (la réponse d'une recherche dépassée, R910) ; la suite
// d'une recherche se demande vingt pages plus loin ; une session perdue attend le retour du compte.
//
// Le serveur est `searchSpace`, que le test fait répondre quand il veut ; le temps est celui des
// minuteurs simulés : aucun test n'attend une durée.
import type { SearchHit, SearchRequest, SearchResults } from "@mementum/protocol";
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { unauthenticated, unreachable } from "../test/pages";
import { SEARCH_DEBOUNCE_MS, useSearch } from "./useSearch";

/** Une recherche reçue par le serveur du test. */
type Asked = {
  spaceId: string;
  body: SearchRequest;
  signal: AbortSignal | undefined;
  answer: Deferred<SearchResults>;
};

const asked: Asked[] = [];

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  searchSpace: (spaceId: string, body: SearchRequest, signal?: AbortSignal) => {
    const answer = deferred<SearchResults>();
    asked.push({ spaceId, body, signal, answer });
    return answer.promise;
  },
}));

beforeEach(() => {
  asked.length = 0;
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
});

afterEach(() => {
  vi.useRealTimers();
});

const hit = (pageId: string, overrides: Partial<SearchHit> = {}): SearchHit => ({
  pageId,
  title: pageId.toUpperCase(),
  path: [],
  blockId: `bloc-${pageId}`,
  excerpt: `le texte de ${pageId}`,
  marks: [],
  ...overrides,
});

const found = (ids: string[], overrides: Partial<SearchResults> = {}): SearchResults => ({
  results: ids.map((id) => hit(id)),
  more: false,
  indexing: false,
  ...overrides,
});

const busy = () => new ApiError(429, "search_busy", "une recherche plus récente a pris la place de celle-ci");

type Props = { spaceId: string; query: string; afterLogin?: () => Promise<void> };
const search = ({ spaceId, query, afterLogin }: Props) => useSearch(spaceId, query, { afterLogin });

/** Le temps passe, sous `act` : les minuteurs échus tournent, React suit. */
const pass = (ms: number) =>
  act(async () => {
    vi.advanceTimersByTime(ms);
  });

const ids = (hits: readonly SearchHit[]) => hits.map((h) => h.pageId);

test("nothing_is_asked_for_an_empty_query", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "" });
  await pass(10 * SEARCH_DEBOUNCE_MS);
  await hook.rerender({ spaceId: "s1", query: "   " });
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked).toEqual([]);
  expect(hook.current).toMatchObject({ hits: [], pending: false, more: false, indexing: false, error: null });
});

test("the_server_is_asked_once_150ms_after_the_last_keystroke", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "c" });
  expect(hook.current.pending).toBe(true);
  await pass(SEARCH_DEBOUNCE_MS - 1);
  await hook.rerender({ spaceId: "s1", query: "ch" });
  await pass(SEARCH_DEBOUNCE_MS - 1);
  await hook.rerender({ spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS - 1);
  // Chaque frappe a repoussé l'envoi : rien n'est parti.
  expect(asked).toEqual([]);
  await pass(1);
  expect(asked).toHaveLength(1);
  // L'espace, et la recherche telle que tapée, dans le corps ; pas d'`after` à la première réponse.
  expect(asked[0].spaceId).toBe("s1");
  expect(asked[0].body).toEqual({ q: "chat" });
  await settle(() => asked[0].answer.resolve(found(["a", "b"], { indexing: true })));
  expect(ids(hook.current.hits)).toEqual(["a", "b"]);
  expect(hook.current).toMatchObject({ pending: false, indexing: true, more: false, error: null });
  // Des espaces de bord en plus ne sont pas une autre recherche.
  await hook.rerender({ spaceId: "s1", query: "chat " });
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked).toHaveLength(1);
  expect(ids(hook.current.hits)).toEqual(["a", "b"]);
});

test("a_late_answer_of_an_older_query_is_dropped", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS);
  const [older] = asked;
  await hook.rerender({ spaceId: "s1", query: "chaton" });
  // La requête en route est abandonnée dès la frappe.
  expect(older.signal?.aborted).toBe(true);
  // Sa réponse arrive quand même (le serveur va au bout), avant comme après le départ de la suivante.
  await settle(() => older.answer.resolve(found(["vieux"])));
  expect(hook.current.hits).toEqual([]);
  expect(hook.current.pending).toBe(true);
  await pass(SEARCH_DEBOUNCE_MS);
  expect(asked).toHaveLength(2);
  expect(hook.current.hits).toEqual([]);
  await settle(() => asked[1].answer.resolve(found(["neuf"])));
  expect(ids(hook.current.hits)).toEqual(["neuf"]);
  expect(asked[1].signal?.aborted).toBe(false);
});

test("the_results_of_another_query_are_never_shown_for_the_current_one", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[0].answer.resolve(found(["a"], { more: true, indexing: true })));
  expect(ids(hook.current.hits)).toEqual(["a"]);
  // Dès le rendu de la frappe suivante, plus rien de la recherche d'avant.
  await hook.rerender({ spaceId: "s1", query: "chien" });
  expect(hook.current).toMatchObject({ hits: [], pending: true, more: false, indexing: false, error: null });
  // Un autre espace, la même recherche : une autre recherche.
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[1].answer.resolve(found(["b"])));
  await hook.rerender({ spaceId: "s2", query: "chien" });
  expect(hook.current).toMatchObject({ hits: [], pending: true });
  await pass(SEARCH_DEBOUNCE_MS);
  expect(asked[2].spaceId).toBe("s2");
});

test("search_busy_is_not_shown_for_a_replaced_query", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS);
  await hook.rerender({ spaceId: "s1", query: "chaton" });
  await pass(SEARCH_DEBOUNCE_MS);
  // Le serveur refuse celle qui attendait, qu'une plus récente du compte a remplacée.
  await settle(() => asked[0].answer.reject(busy()));
  expect(hook.current.error).toBeNull();
  expect(hook.current.pending).toBe(true);
  // Et elle n'est jamais rejouée.
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked.map((a) => a.body.q)).toEqual(["chat", "chaton"]);
  await settle(() => asked[1].answer.resolve(found(["a"])));
  expect(ids(hook.current.hits)).toEqual(["a"]);
  expect(hook.current.error).toBeNull();
});

test("a_refusal_of_the_current_query_is_said_as_the_server_says_it", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() =>
    asked[0].answer.reject(new ApiError(422, "invalid_query", "Ta recherche compte plus de 8 mots.")),
  );
  expect(hook.current).toMatchObject({ hits: [], pending: false, error: "Ta recherche compte plus de 8 mots." });
  // Rien ne repart de soi-même ; la frappe suivante efface le refus et cherche de nouveau.
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked).toHaveLength(1);
  await hook.rerender({ spaceId: "s1", query: "chaton" });
  expect(hook.current).toMatchObject({ error: null, pending: true });
  // Le serveur muet : le message d'une lecture.
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[1].answer.reject(unreachable()));
  expect(hook.current.error).toBe("Le serveur Mementum ne répond pas.");
  // Refusée pour de bon (une autre fenêtre du compte cherche) : dit tel quel aussi, sans rejeu.
  await hook.rerender({ spaceId: "s1", query: "chatons" });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[2].answer.reject(busy()));
  expect(hook.current.error).toBe("une recherche plus récente a pris la place de celle-ci");
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked).toHaveLength(3);
});

test("more_results_are_asked_twenty_pages_further_and_added_below", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[0].answer.resolve(found(["a", "b"], { more: true })));
  expect(hook.current.more).toBe(true);
  await settle(() => hook.current.loadMore());
  expect(hook.current.loadingMore).toBe(true);
  // Un second clic pendant la demande n'en fait pas une seconde.
  await settle(() => hook.current.loadMore());
  expect(asked.map((a) => a.body)).toEqual([{ q: "chat" }, { q: "chat", after: 20 }]);
  await settle(() => asked[1].answer.resolve(found(["c"], { more: true })));
  expect(ids(hook.current.hits)).toEqual(["a", "b", "c"]);
  expect(hook.current).toMatchObject({ more: true, loadingMore: false, pending: false });
  // Jusqu'à la centième page : 40, 60, 80, puis plus rien n'est demandé, quoi que dise la réponse.
  for (const after of [40, 60, 80]) {
    await settle(() => hook.current.loadMore());
    expect(asked.at(-1)!.body).toEqual({ q: "chat", after });
    await settle(() => asked.at(-1)!.answer.resolve(found([`p${after}`], { more: true })));
  }
  expect(hook.current.more).toBe(false);
  await settle(() => hook.current.loadMore());
  expect(asked).toHaveLength(5);
});

test("a_failed_request_for_more_keeps_what_is_shown_and_can_be_asked_again", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[0].answer.resolve(found(["a"], { more: true })));
  await settle(() => hook.current.loadMore());
  await settle(() => asked[1].answer.reject(unreachable()));
  expect(ids(hook.current.hits)).toEqual(["a"]);
  expect(hook.current).toMatchObject({ more: true, loadingMore: false, error: "Le serveur Mementum ne répond pas." });
  // Redemandée : la même suite, pas la suivante.
  await settle(() => hook.current.loadMore());
  expect(asked[2].body).toEqual({ q: "chat", after: 20 });
  expect(hook.current.error).toBeNull();
  await settle(() => asked[2].answer.resolve(found(["b"])));
  expect(ids(hook.current.hits)).toEqual(["a", "b"]);
  // Une frappe abandonne aussi la suite en route.
  await hook.rerender({ spaceId: "s1", query: "chats" });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[3].answer.resolve(found(["c"], { more: true })));
  await settle(() => hook.current.loadMore());
  await hook.rerender({ spaceId: "s1", query: "chatte" });
  expect(asked[4].signal?.aborted).toBe(true);
  await settle(() => asked[4].answer.resolve(found(["tard"])));
  expect(hook.current.hits).toEqual([]);
});

test("a_lost_session_searches_again_when_the_account_returns", async () => {
  const login = deferred<void>();
  const afterLogin = vi.fn(() => login.promise);
  const hook = await renderHook(search, { spaceId: "s1", query: "chat", afterLogin });
  await pass(SEARCH_DEBOUNCE_MS);
  // `api.ts` a demandé la connexion (`onSessionLost`) ; ici, rien n'est dit, et la recherche attend.
  await settle(() => asked[0].answer.reject(unauthenticated()));
  expect(afterLogin).toHaveBeenCalledTimes(1);
  expect(hook.current).toMatchObject({ hits: [], pending: true, error: null });
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked).toHaveLength(1);
  // Le compte revient : la même recherche repart, sans attendre une frappe.
  await settle(() => login.resolve());
  expect(asked).toHaveLength(2);
  expect(asked[1].body).toEqual({ q: "chat" });
  await settle(() => asked[1].answer.resolve(found(["a"])));
  expect(ids(hook.current.hits)).toEqual(["a"]);
  expect(hook.current.pending).toBe(false);
});

test("the_account_returning_does_not_search_again_what_is_no_longer_typed", async () => {
  const login = deferred<void>();
  const hook = await renderHook(search, { spaceId: "s1", query: "chat", afterLogin: () => login.promise });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[0].answer.reject(unauthenticated()));
  // Le champ est vidé pendant que la connexion est demandée.
  await hook.rerender({ spaceId: "s1", query: "", afterLogin: () => login.promise });
  await settle(() => login.resolve());
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked).toHaveLength(1);
  // Sans attente du compte donnée, une session perdue ne laisse rien en suspens.
  const bare = await renderHook(search, { spaceId: "s1", query: "chien" });
  await pass(SEARCH_DEBOUNCE_MS);
  await settle(() => asked[1].answer.reject(unauthenticated()));
  expect(bare.current).toMatchObject({ pending: false, error: null });
});

test("leaving_abandons_the_request_and_drops_its_answer", async () => {
  const hook = await renderHook(search, { spaceId: "s1", query: "chat" });
  await pass(SEARCH_DEBOUNCE_MS);
  await hook.unmount();
  expect(asked[0].signal?.aborted).toBe(true);
  // Démonté avant l'envoi : rien ne part.
  const early = await renderHook(search, { spaceId: "s1", query: "chien" });
  await early.unmount();
  await pass(10 * SEARCH_DEBOUNCE_MS);
  expect(asked).toHaveLength(1);
});
