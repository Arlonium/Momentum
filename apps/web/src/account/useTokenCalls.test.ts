// L'activité d'un jeton d'API (lot 5, Task C2) : lue à la demande, page par page ; une lecture qui
// trouve la session perdue reprend au retour du compte ce qui était demandé (R212) ; une lecture qui
// échoue dit pourquoi, et ne se refait pas d'elle-même.
import type { TokenCall, TokenCalls } from "@mementum/protocol";
import { expect, test, vi } from "vitest";
import { ApiError, tokenCalls } from "../api";
import { deferred, renderHook, settle } from "../test/hooks";
import { unauthenticated } from "../test/pages";
import { useTokenCalls } from "./useTokenCalls";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  tokenCalls: vi.fn(),
}));

const call = (id: number): TokenCall => ({
  id,
  at: 1_800_000_000_000 + id,
  surface: "rest",
  tool: "GET /api/v1/pages/{id}",
  spaceId: null,
  pageId: null,
  pageTitle: null,
  outcome: "ok",
  code: null,
  revisionBefore: null,
  revisionAfter: null,
  blocksTouched: null,
});

const page = (ids: number[], more: boolean): TokenCalls => ({
  calls: ids.map(call),
  more,
  busyCount: 0,
  purged: 0,
});

const t1 = { id: "t1", busyCount: 0, callsPurged: 0 };

/** Laisse partir les lectures demandées (elles partent au tour suivant). */
const tick = () => settle(() => new Promise((done) => setTimeout(done, 5)));

test("a_lost_session_reads_what_was_asked_again_when_the_account_comes_back", async () => {
  let login = deferred<void>();
  vi.mocked(tokenCalls)
    .mockReset()
    .mockRejectedValueOnce(unauthenticated())
    .mockResolvedValueOnce(page([9, 8], true))
    .mockRejectedValueOnce(unauthenticated())
    .mockResolvedValueOnce(page([7], false));
  const hook = await renderHook(() => useTokenCalls(t1, null, () => login.promise), undefined);
  await tick();
  // La première lecture trouve la session perdue : rien n'est dit, rien ne repart avant le compte.
  expect(tokenCalls).toHaveBeenCalledTimes(1);
  expect(hook.current).toMatchObject({ calls: null, error: null });
  await settle(() => login.resolve());
  await tick();
  expect(vi.mocked(tokenCalls).mock.calls[1]).toEqual(["t1", { before: undefined, outcome: undefined }]);
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8]);
  expect(hook.current.more).toBe(true);

  // La suite trouve la session perdue : au retour du compte, c'est encore la suite qui est lue.
  login = deferred<void>();
  await settle(() => hook.current.loadMore());
  await tick();
  expect(vi.mocked(tokenCalls).mock.calls[2]).toEqual(["t1", { before: 8, outcome: undefined }]);
  expect(hook.current).toMatchObject({ loadingMore: true, error: null });
  await settle(() => login.resolve());
  await tick();
  expect(vi.mocked(tokenCalls).mock.calls[3]).toEqual(["t1", { before: 8, outcome: undefined }]);
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8, 7]);
  expect(hook.current).toMatchObject({ more: false, loadingMore: false });
});

test("a_failed_read_says_why_and_waits_for_a_gesture", async () => {
  vi.mocked(tokenCalls)
    .mockReset()
    .mockRejectedValueOnce(new ApiError(0, null, "Le serveur Mementum ne répond pas."))
    .mockResolvedValueOnce(page([9, 8], true))
    .mockRejectedValueOnce(new ApiError(500, "internal", "erreur interne du serveur"))
    .mockResolvedValueOnce(page([7], true));
  const hook = await renderHook(() => useTokenCalls(t1, "refused"), undefined);
  await tick();
  expect(hook.current).toMatchObject({ calls: null, error: "Le serveur Mementum ne répond pas." });
  await tick();
  expect(tokenCalls).toHaveBeenCalledTimes(1);
  await settle(() => hook.current.retry());
  await tick();
  expect(vi.mocked(tokenCalls).mock.calls[1]).toEqual(["t1", { before: undefined, outcome: "refused" }]);
  expect(hook.current).toMatchObject({ error: null, more: true });

  // La suite échoue : ce qui est lu reste, le refus est dit, et rien ne repart sans un geste.
  await settle(() => hook.current.loadMore());
  await tick();
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8]);
  expect(hook.current).toMatchObject({ error: "erreur interne du serveur", loadingMore: false, more: true });
  await tick();
  expect(tokenCalls).toHaveBeenCalledTimes(3);
  await settle(() => hook.current.loadMore());
  await tick();
  expect(vi.mocked(tokenCalls).mock.calls[3]).toEqual(["t1", { before: 8, outcome: "refused" }]);
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8, 7]);
  expect(hook.current.error).toBeNull();
});

test("asking_for_more_twice_reads_one_page", async () => {
  const answer = deferred<TokenCalls>();
  vi.mocked(tokenCalls).mockReset().mockResolvedValueOnce(page([9, 8], true)).mockReturnValueOnce(answer.promise);
  const hook = await renderHook(() => useTokenCalls(t1, null), undefined);
  await tick();
  await settle(() => hook.current.loadMore());
  await settle(() => hook.current.loadMore());
  await tick();
  expect(tokenCalls).toHaveBeenCalledTimes(2);
  await settle(() => answer.resolve(page([7, 6], true)));
  await tick();
  expect(tokenCalls).toHaveBeenCalledTimes(2);
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8, 7, 6]);
});

test("retry_after_a_page_reads_nothing", async () => {
  // « Réessayer » ne vaut que pour une première lecture qui a échoué : une page lue, il ne lit ni elle
  // ni la suite (la lecture ne lit la suite que demandée).
  vi.mocked(tokenCalls).mockReset().mockResolvedValueOnce(page([9, 8], true)).mockResolvedValue(page([7], false));
  const hook = await renderHook(() => useTokenCalls(t1, null), undefined);
  await tick();
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8]);
  await settle(() => hook.current.retry());
  await tick();
  expect(tokenCalls).toHaveBeenCalledTimes(1);
  expect(hook.current).toMatchObject({ retrying: false, more: true });
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8]);
});

test("coming_back_to_the_tab_reads_nothing_again", async () => {
  // Rien ne relit l'activité d'elle-même (R1466) : ni le retour sur l'onglet (`visibilitychange`), ni
  // le focus rendu à la fenêtre. Une relecture en tête ferait bouger ce qui est sous le pointeur.
  vi.mocked(tokenCalls).mockReset().mockResolvedValueOnce(page([9, 8], true)).mockResolvedValue(page([7], false));
  const hook = await renderHook(() => useTokenCalls(t1, null), undefined);
  await tick();
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8]);
  try {
    for (const state of ["hidden", "visible"] as const) {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
      await settle(() => {
        document.dispatchEvent(new Event("visibilitychange"));
      });
      await tick();
    }
    await settle(() => {
      window.dispatchEvent(new FocusEvent("blur"));
      window.dispatchEvent(new FocusEvent("focus"));
    });
    await tick();
  } finally {
    // L'état propre au document retiré, celui de jsdom revient.
    delete (document as { visibilityState?: unknown }).visibilityState;
  }
  expect(tokenCalls).toHaveBeenCalledTimes(1);
  expect(hook.current.calls?.map((c) => c.id)).toEqual([9, 8]);
  expect(hook.current.more).toBe(true);
});
