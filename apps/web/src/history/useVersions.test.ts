// Les versions d'une page, côté données (R135) : la liste est lue quand la fenêtre « Historique »
// s'ouvre, et relue, groupée, à chaque trame `versions` de la connexion de synchro, au retour de
// la connexion et à la première synchro d'une session neuve. Une relecture n'en annule jamais une
// autre en route ; une lecture qui échoue garde ce qui était affiché.
import type { Version } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, getVersions } from "../api";
import type { PageEvent, SyncStatus } from "../editor/collab";
import { author } from "../test/comments";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { unauthenticated, unreachable } from "../test/pages";
import { useVersions, VERSIONS_DEBOUNCE_MS, VERSIONS_MAX_WAIT_MS } from "./useVersions";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getVersions: vi.fn(),
}));

/** Une session de synchro réduite à ce que le hook lit : synchro, état, trames de page. */
function fakeSession(start: { synced?: boolean; status?: SyncStatus } = {}) {
  const listeners = new Set<() => void>();
  const events = new Set<(event: PageEvent) => void>();
  const session = {
    synced: start.synced ?? true,
    status: start.status ?? ("connected" as SyncStatus),
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    onPageEvent(listener: (event: PageEvent) => void) {
      events.add(listener);
      return () => events.delete(listener);
    },
    /** Le serveur envoie une trame de page. */
    frame(event: PageEvent = "versions") {
      for (const listener of [...events]) listener(event);
    },
    set(next: { synced?: boolean; status?: SyncStatus }) {
      Object.assign(session, next);
      for (const listener of [...listeners]) listener();
    },
  };
  return session;
}

type Session = ReturnType<typeof fakeSession>;

const version = (id: string, overrides: Partial<Version> = {}): Version => ({
  id,
  createdAt: 1_000,
  reason: "periodic",
  authors: [author("alice")],
  ...overrides,
});

/** Les lectures de la liste, dans l'ordre : le test tient ou rompt chacune. */
let reads: Deferred<Version[]>[] = [];

beforeEach(() => {
  vi.useFakeTimers();
  reads = [];
  vi.mocked(getVersions).mockReset();
  vi.mocked(getVersions).mockImplementation(() => {
    const answer = deferred<Version[]>();
    reads.push(answer);
    return answer.promise;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

type Props = { pageId: string; session: Session | null };
const hook = ({ pageId, session }: Props) => useVersions(pageId, session);
const advance = (ms: number) => settle(() => vi.advanceTimersByTime(ms));

/** Le hook monté sur une session déjà synchronisée, sa première lecture tenue. */
async function loaded(versions: Version[] = [version("v1")]) {
  const session = fakeSession();
  const rendered = await renderHook(hook, { pageId: "p", session });
  await advance(0);
  expect(reads).toHaveLength(1);
  await settle(() => reads[0].resolve(versions));
  return { session, rendered };
}

test("the_list_is_read_as_soon_as_the_window_opens", async () => {
  // La fenêtre s'ouvre sur une page dont la session n'a pas encore fini sa synchro : la liste
  // vient de l'API, elle n'attend pas l'éditeur.
  const session = fakeSession({ synced: false, status: "connecting" });
  const rendered = await renderHook(hook, { pageId: "p", session });
  expect(rendered.current.versions).toBeNull();
  expect(rendered.current.state).toBe("loading");
  await advance(0);
  expect(reads).toHaveLength(1);
  expect(vi.mocked(getVersions)).toHaveBeenLastCalledWith("p");
  await settle(() => reads[0].resolve([version("v2"), version("v1")]));
  expect(rendered.current.versions?.map((v) => v.id)).toEqual(["v2", "v1"]);
  expect(rendered.current.state).toBe("ready");
  // Ni sans session.
  const alone = await renderHook(hook, { pageId: "q", session: null });
  await advance(0);
  expect(reads).toHaveLength(2);
  expect(vi.mocked(getVersions)).toHaveBeenLastCalledWith("q");
  await settle(() => reads[1].resolve([]));
  expect(alone.current.versions).toEqual([]);
});

test("one_read_when_the_window_opens_on_a_synced_session", async () => {
  await loaded();
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
});

test("versions_are_reread_on_a_versions_frame", async () => {
  const { session, rendered } = await loaded([version("v1")]);
  await settle(() => session.frame("versions"));
  // Pas tout de suite : une rafale ne fait qu'une relecture.
  expect(reads).toHaveLength(1);
  await advance(VERSIONS_DEBOUNCE_MS);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([version("v2"), version("v1")]));
  expect(rendered.current.versions?.map((v) => v.id)).toEqual(["v2", "v1"]);

  // Les commentaires ont leur propre trame : rien à relire ici.
  await settle(() => session.frame("comments"));
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("a_burst_of_frames_makes_one_reread_within_the_max_wait", async () => {
  const { session } = await loaded();
  for (let i = 0; i < 3; i += 1) {
    await settle(() => session.frame());
    await advance(VERSIONS_DEBOUNCE_MS - 50);
  }
  expect(reads).toHaveLength(1);
  await advance(50);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([]));
  // Une trame toutes les 200 ms : l'anti-rebond seul ne relirait jamais.
  for (let elapsed = 0; elapsed < VERSIONS_MAX_WAIT_MS; elapsed += 200) {
    await settle(() => session.frame());
    await advance(200);
  }
  expect(reads).toHaveLength(3);
});

test("a_reread_asked_during_a_read_waits_for_it_and_is_done_once", async () => {
  const { session, rendered } = await loaded();
  await settle(() => session.frame());
  await advance(VERSIONS_DEBOUNCE_MS);
  expect(reads).toHaveLength(2);
  // Deux trames pendant la lecture : elle n'est pas jetée, et une seule relecture suit.
  await settle(() => session.frame());
  await advance(VERSIONS_DEBOUNCE_MS);
  await settle(() => rendered.current.reload());
  await advance(VERSIONS_MAX_WAIT_MS);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([version("v2"), version("v1")]));
  // Sa réponse s'affiche : elle n'a pas été annulée.
  expect(rendered.current.versions?.map((v) => v.id)).toEqual(["v2", "v1"]);
  await advance(0);
  expect(reads).toHaveLength(3);
  await settle(() => reads[2].resolve([version("v3"), version("v2"), version("v1")]));
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(3);
  expect(rendered.current.versions?.map((v) => v.id)).toEqual(["v3", "v2", "v1"]);
});

test("a_failed_read_keeps_what_was_shown_and_does_not_loop", async () => {
  const { session, rendered } = await loaded([version("v1")]);
  await settle(() => session.frame());
  await advance(VERSIONS_DEBOUNCE_MS);
  await settle(() => reads[1].reject(unreachable()));
  expect(rendered.current.state).toBe("failed");
  expect(rendered.current.error).toBe("Le serveur Mementum ne répond pas.");
  expect(rendered.current.versions?.map((v) => v.id)).toEqual(["v1"]);
  await advance(60_000);
  expect(reads).toHaveLength(2);
  // « Réessayer » relit, et une lecture réussie efface l'échec.
  await settle(() => rendered.current.reload());
  await advance(0);
  expect(reads).toHaveLength(3);
  await settle(() => reads[2].resolve([version("v1")]));
  expect(rendered.current.state).toBe("ready");
  expect(rendered.current.error).toBeNull();
});

test("versions_are_reread_when_the_connection_comes_back", async () => {
  const { session } = await loaded();
  // Une trame a pu partir pendant la coupure.
  await settle(() => session.set({ status: "disconnected" }));
  await settle(() => session.set({ status: "connecting" }));
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
  await settle(() => session.set({ status: "connected" }));
  await advance(VERSIONS_MAX_WAIT_MS);
  expect(reads).toHaveLength(2);
});

test("a_new_session_of_the_same_page_rereads_once_synced", async () => {
  const { session: old, rendered } = await loaded();
  // La page passe en lecture seule : une session neuve, qui a pu manquer une trame.
  const next = fakeSession({ synced: false, status: "connecting" });
  await rendered.rerender({ pageId: "p", session: next });
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
  await settle(() => next.set({ status: "connected", synced: true }));
  await advance(VERSIONS_MAX_WAIT_MS);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([version("v1")]));
  // L'ancienne session ne dit plus rien au hook.
  await settle(() => old.frame("versions"));
  await settle(() => old.set({ status: "disconnected" }));
  await settle(() => old.set({ status: "connected" }));
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
  await settle(() => next.frame("versions"));
  await advance(VERSIONS_MAX_WAIT_MS);
  expect(reads).toHaveLength(3);
});

/**
 * Le compte de la session, perdu puis revenu : `afterLogin()` est tenue au retour du compte
 * (`useAfterLogin`). `waiting` compte ceux qui l'attendent encore.
 */
function account() {
  const waiting: (() => void)[] = [];
  return {
    afterLogin: () => new Promise<void>((resume) => waiting.push(resume)),
    back: () => settle(() => waiting.splice(0).forEach((resume) => resume())),
    get waiting() {
      return waiting.length;
    },
  };
}

type WithLogin = Props & { afterLogin: () => Promise<void> };
const withLogin = ({ pageId, session, afterLogin }: WithLogin) =>
  useVersions(pageId, session, { afterLogin });

test("the_list_is_read_again_once_the_account_is_back", async () => {
  // La sonde du relecteur : la session est échue, la connexion de synchro reste ouverte (rien ne
  // la fait revenir), et la liste répond `401` à l'ouverture de la fenêtre.
  const login = account();
  const session = fakeSession();
  const rendered = await renderHook(withLogin, { pageId: "p", session, afterLogin: login.afterLogin });
  await advance(0);
  await settle(() => reads[0].reject(unauthenticated()));
  await advance(60_000);
  expect(
    `reads: ${reads.length} state: ${rendered.current.state} versions: ${rendered.current.versions}`,
  ).toBe("reads: 1 state: loading versions: null");
  // Le compte revient : la liste est relue, une fois.
  await login.back();
  await advance(0);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([version("v1")]));
  expect(rendered.current.state).toBe("ready");
  expect(rendered.current.versions?.map((v) => v.id)).toEqual(["v1"]);
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("frames_during_the_wait_for_the_account_are_served_by_one_read", async () => {
  const login = account();
  const session = fakeSession();
  const rendered = await renderHook(withLogin, { pageId: "p", session, afterLogin: login.afterLogin });
  await advance(0);
  await settle(() => reads[0].reject(unauthenticated()));
  // Des trames pendant l'attente : rien ne part, elles répondraient `401` aussi.
  await settle(() => session.frame());
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  await settle(() => rendered.current.reload());
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
  // Le compte revient : une seule lecture les sert toutes.
  await login.back();
  await advance(0);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([version("v1")]));
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("the_wait_for_the_account_does_not_outlive_the_unmount_or_the_page", async () => {
  const login = account();
  const session = fakeSession();
  const rendered = await renderHook(withLogin, { pageId: "p", session, afterLogin: login.afterLogin });
  await advance(0);
  await settle(() => reads[0].reject(unauthenticated()));
  // Une autre page : sa liste est lue ; celle de la première ne le sera plus.
  await rendered.rerender({ pageId: "q", session, afterLogin: login.afterLogin });
  await advance(0);
  expect(vi.mocked(getVersions).mock.calls.map(([id]) => id)).toEqual(["p", "q"]);
  await settle(() => reads[1].reject(unauthenticated()));
  // La fenêtre se ferme pendant l'attente : le compte revenu, rien n'est relu.
  await rendered.unmount();
  await login.back();
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(vi.mocked(getVersions).mock.calls.map(([id]) => id)).toEqual(["p", "q"]);
});

test("nothing_is_read_after_unmount", async () => {
  const { session, rendered } = await loaded();
  await settle(() => session.frame());
  await advance(VERSIONS_DEBOUNCE_MS);
  expect(reads).toHaveLength(2);
  await rendered.unmount();
  await settle(() => session.frame());
  await settle(() => reads[1].resolve([version("v2")]));
  await advance(VERSIONS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("a_refused_read_says_so_and_tells_the_page", async () => {
  // Le niveau a baissé sous « Peut modifier » pendant que la fenêtre est ouverte : le serveur
  // refuse la liste. Le message est rendu tel quel, et la page est à relire.
  const onRefused = vi.fn();
  const session = fakeSession();
  const rendered = await renderHook(
    ({ pageId, session }: Props) => useVersions(pageId, session, { onRefused }),
    { pageId: "p", session },
  );
  await advance(0);
  await settle(() => reads[0].resolve([version("v1")]));
  expect(rendered.current.refused).toBe(false);
  await settle(() => session.frame());
  await advance(VERSIONS_DEBOUNCE_MS);
  const refusal = new ApiError(
    403,
    "forbidden",
    "niveau insuffisant sur cette page : il faut « Peut modifier », tu as « Peut lire »",
  );
  await settle(() => reads[1].reject(refusal));
  expect(rendered.current.state).toBe("failed");
  expect(rendered.current.refused).toBe(true);
  expect(rendered.current.error).toBe(refusal.message);
  expect(onRefused).toHaveBeenCalledExactlyOnceWith(refusal);
  // Un autre échec (le serveur ne répond pas) n'est pas un refus, et ne fait rien relire.
  await settle(() => session.frame());
  await advance(VERSIONS_DEBOUNCE_MS);
  await settle(() => reads[2].reject(unreachable()));
  expect(rendered.current.refused).toBe(false);
  expect(onRefused).toHaveBeenCalledTimes(1);
});
