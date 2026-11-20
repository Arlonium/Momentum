// Les rétroliens d'une page, côté données (lot 4, Task C3) : quand ils sont lus (après la première
// synchro, jamais avant), quand ils sont relus (retour sur l'onglet, retour de la connexion, retour
// du compte ; jamais parce que la session dit autre chose, donc jamais à la frappe), et ce que
// devient la liste quand une lecture échoue.
import type { Backlinks } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getBacklinks } from "../api";
import type { SyncStatus } from "../editor/collab";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { notFound, unauthenticated, unreachable } from "../test/pages";
import { BACKLINKS_RETURN_MS, useBacklinks } from "./useBacklinks";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getBacklinks: vi.fn(),
}));

/** Une session de synchro réduite à ce que le hook lit : synchro et état. */
function fakeSession(start: { synced?: boolean; status?: SyncStatus } = {}) {
  const listeners = new Set<() => void>();
  const session = {
    synced: start.synced ?? true,
    status: start.status ?? ("connected" as SyncStatus),
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    set(next: { synced?: boolean; status?: SyncStatus }) {
      Object.assign(session, next);
      for (const listener of [...listeners]) listener();
    },
  };
  return session;
}

type Session = ReturnType<typeof fakeSession>;

/** Les lectures, dans l'ordre : le test tient ou rompt chacune. */
let reads: Deferred<Backlinks>[] = [];
const onRefused = vi.fn();
/** Les attentes du compte posées par le hook. */
let logins: Deferred<void>[] = [];
const afterLogin = () => {
  const wait = deferred<void>();
  logins.push(wait);
  return wait.promise;
};

beforeEach(() => {
  vi.useFakeTimers();
  reads = [];
  logins = [];
  onRefused.mockClear();
  vi.mocked(getBacklinks).mockReset();
  vi.mocked(getBacklinks).mockImplementation(() => {
    const answer = deferred<Backlinks>();
    reads.push(answer);
    return answer.promise;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

type Props = { pageId: string; session: Session | null };
const hook = ({ pageId, session }: Props) => useBacklinks(pageId, session, { onRefused, afterLogin });
const advance = (ms: number) => settle(() => vi.advanceTimersByTime(ms));
const link = (pageId: string, excerpt = "") => ({ pageId, blockId: null, excerpt });
/** L'onglet redevient visible, ou reprend le focus. */
const comeBack = (signal: "visibilitychange" | "focus" = "visibilitychange") =>
  settle(() => {
    if (signal === "focus") window.dispatchEvent(new Event("focus"));
    else document.dispatchEvent(new Event("visibilitychange"));
  });

/** Le hook monté sur une session déjà synchronisée, sa première lecture tenue. */
async function loaded(pages = [link("a", "vers ici")]) {
  const session = fakeSession();
  const rendered = await renderHook(hook, { pageId: "p", session });
  await advance(0);
  expect(reads).toHaveLength(1);
  await settle(() => reads[0].resolve({ pages }));
  return { session, rendered };
}

test("backlinks_are_read_after_the_first_sync_only", async () => {
  // Sans session, puis avec une session qui n'a pas fini sa première synchro : rien ne part, quel
  // que soit le temps passé (la lecture prend au moins 150 ms chez le serveur, R1022).
  const session = fakeSession({ synced: false, status: "connecting" });
  const rendered = await renderHook(hook, { pageId: "p", session: null });
  await advance(60_000);
  await rendered.rerender({ pageId: "p", session });
  await advance(60_000);
  session.set({ status: "connected" });
  await advance(60_000);
  // Le retour sur l'onglet non plus : rien n'a encore été lu.
  await comeBack();
  await advance(0);
  expect(getBacklinks).not.toHaveBeenCalled();
  expect(rendered.current.pages).toBeNull();

  session.set({ synced: true });
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(1);
  expect(getBacklinks).toHaveBeenCalledWith("p");
  await settle(() => reads[0].resolve({ pages: [link("a")] }));
  expect(rendered.current.pages).toEqual([link("a")]);
});

test("a_session_already_synced_is_read_at_once", async () => {
  const { rendered } = await loaded();
  expect(rendered.current.pages).toEqual([link("a", "vers ici")]);
  expect(rendered.current.error).toBeNull();
});

test("nothing_is_read_again_when_the_session_says_nothing_new", async () => {
  // Ce que la session annonce pendant la frappe (présents, état inchangé) ne fait rien relire.
  const { session } = await loaded();
  for (let i = 0; i < 20; i++) session.set({});
  await advance(60_000);
  expect(getBacklinks).toHaveBeenCalledTimes(1);
});

test("the_list_is_read_again_when_the_connection_returns", async () => {
  const { session, rendered } = await loaded();
  session.set({ status: "disconnected" });
  await advance(10_000);
  expect(getBacklinks).toHaveBeenCalledTimes(1);
  session.set({ status: "connected" });
  await advance(1_000);
  expect(getBacklinks).toHaveBeenCalledTimes(2);
  await settle(() => reads[1].resolve({ pages: [link("a"), link("b")] }));
  expect(rendered.current.pages).toEqual([link("a"), link("b")]);
});

test("a_new_session_of_the_page_reads_again_at_its_first_sync", async () => {
  // La page passe en lecture seule : une session neuve, dont la première synchro relit.
  const { rendered } = await loaded();
  const next = fakeSession({ synced: false, status: "connecting" });
  await rendered.rerender({ pageId: "p", session: next });
  await advance(10_000);
  expect(getBacklinks).toHaveBeenCalledTimes(1);
  next.set({ synced: true, status: "connected" });
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(2);
});

test("the_list_is_read_again_on_the_way_back_to_the_tab_five_seconds_apart", async () => {
  await loaded();
  // Juste après une lecture : le retour ne relit pas.
  await comeBack();
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(1);

  await advance(BACKLINKS_RETURN_MS);
  // Deux signaux du même retour (visible, puis focus) : une seule lecture.
  await comeBack("visibilitychange");
  await comeBack("focus");
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(2);
  await settle(() => reads[1].resolve({ pages: [] }));
  await comeBack();
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(2);
});

test("a_hidden_tab_reads_nothing", async () => {
  await loaded();
  await advance(BACKLINKS_RETURN_MS);
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  await comeBack();
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(1);
  visibility.mockRestore();
});

test("a_tab_back_while_the_sync_is_cut_waits_for_the_connection", async () => {
  // Hors ligne, la lecture échouerait : c'est le retour de la connexion qui relit.
  const { session } = await loaded();
  session.set({ status: "disconnected" });
  await advance(BACKLINKS_RETURN_MS);
  await comeBack();
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(1);
});

test("a_refusal_is_said_and_the_page_is_read_again", async () => {
  // `404` : la page n'est plus vue. Le message du serveur est dit, la page relue (c'est elle qui
  // dira « Tu n'as plus accès à cette page »), et la liste déjà lue reste.
  const { session, rendered } = await loaded();
  session.set({ status: "disconnected" });
  session.set({ status: "connected" });
  await advance(1_000);
  await settle(() => reads[1].reject(notFound()));
  expect(rendered.current.error).toBe("page introuvable");
  expect(rendered.current.pages).toEqual([link("a", "vers ici")]);
  expect(onRefused).toHaveBeenCalledTimes(1);
  // Elle ne se refait pas d'elle-même.
  await advance(60_000);
  expect(getBacklinks).toHaveBeenCalledTimes(2);
});

test("a_read_that_fails_without_a_refusal_does_not_read_the_page_again", async () => {
  const session = fakeSession();
  const rendered = await renderHook(hook, { pageId: "p", session });
  await advance(0);
  await settle(() => reads[0].reject(unreachable()));
  expect(rendered.current.error).toBe("Le serveur Mementum ne répond pas.");
  expect(rendered.current.pages).toBeNull();
  expect(onRefused).not.toHaveBeenCalled();
  // La lecture suivante qui aboutit retire le message.
  session.set({ status: "disconnected" });
  session.set({ status: "connected" });
  await advance(1_000);
  await settle(() => reads[1].resolve({ pages: [] }));
  expect(rendered.current.error).toBeNull();
  expect(rendered.current.pages).toEqual([]);
});

test("a_lost_session_reads_again_after_login", async () => {
  const session = fakeSession();
  const rendered = await renderHook(hook, { pageId: "p", session });
  await advance(0);
  await settle(() => reads[0].reject(unauthenticated()));
  // Rien n'est dit (la connexion est demandée par-dessus la page), rien ne repart avant le compte.
  expect(rendered.current.error).toBeNull();
  expect(onRefused).not.toHaveBeenCalled();
  expect(logins).toHaveLength(1);
  await advance(60_000);
  await comeBack();
  await advance(0);
  expect(getBacklinks).toHaveBeenCalledTimes(1);

  await settle(() => logins[0].resolve());
  expect(getBacklinks).toHaveBeenCalledTimes(2);
  await settle(() => reads[1].resolve({ pages: [link("a")] }));
  expect(rendered.current.pages).toEqual([link("a")]);
});

test("nothing_passes_from_a_page_to_the_next", async () => {
  const { session, rendered } = await loaded();
  session.set({ status: "disconnected" });
  session.set({ status: "connected" });
  await advance(1_000);
  expect(getBacklinks).toHaveBeenCalledTimes(2);
  // La page change pendant la lecture : sa réponse n'est pas montrée sous la page suivante.
  const next = fakeSession({ synced: false, status: "connecting" });
  await rendered.rerender({ pageId: "q", session: next });
  expect(rendered.current.pages).toBeNull();
  await settle(() => reads[1].resolve({ pages: [link("z")] }));
  expect(rendered.current.pages).toBeNull();
});

test("nothing_is_read_after_the_page_is_left", async () => {
  const { session, rendered } = await loaded();
  await rendered.unmount();
  session.set({ status: "disconnected" });
  session.set({ status: "connected" });
  await advance(BACKLINKS_RETURN_MS);
  await comeBack();
  await advance(60_000);
  expect(getBacklinks).toHaveBeenCalledTimes(1);
});
