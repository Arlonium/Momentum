// Les fils de commentaires d'une page, côté données (R134) : quand ils sont lus et relus (trame
// `comments` de la connexion de synchro, retour de la connexion, écritures d'ici), et ce que
// devient ce qu'ils montrent quand une lecture échoue, qu'une écriture passe ou qu'elle est
// refusée.
import type { Thread } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import {
  ApiError,
  deleteComment,
  editComment,
  getComments,
  openThread,
  replyToThread,
  resolveThread,
  setReaction,
} from "../api";
import type { PageEvent, SyncStatus } from "../editor/collab";
import { author, message, thread } from "../test/comments";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { unauthenticated, unreachable } from "../test/pages";
import { COMMENTS_DEBOUNCE_MS, COMMENTS_MAX_WAIT_MS, useComments } from "./useComments";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getComments: vi.fn(),
  openThread: vi.fn(),
  replyToThread: vi.fn(),
  resolveThread: vi.fn(),
  editComment: vi.fn(),
  deleteComment: vi.fn(),
  setReaction: vi.fn(),
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
    frame(event: PageEvent = "comments") {
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

/** Les lectures des fils, dans l'ordre : le test tient ou rompt chacune. */
let reads: Deferred<Thread[]>[] = [];
const onRefused = vi.fn();

beforeEach(() => {
  vi.useFakeTimers();
  reads = [];
  onRefused.mockClear();
  for (const mock of [openThread, replyToThread, resolveThread, editComment, deleteComment, setReaction]) {
    vi.mocked(mock).mockReset();
  }
  vi.mocked(getComments).mockReset();
  vi.mocked(getComments).mockImplementation(() => {
    const answer = deferred<Thread[]>();
    reads.push(answer);
    return answer.promise;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

type Props = { pageId: string; session: Session | null };
const hook = ({ pageId, session }: Props) => useComments(pageId, session, { onRefused });
const advance = (ms: number) => settle(() => vi.advanceTimersByTime(ms));

/** Le hook monté sur une session déjà synchronisée, sa première lecture tenue. */
async function loaded(threads: Thread[] = [thread("t1")]) {
  const session = fakeSession();
  const rendered = await renderHook(hook, { pageId: "p", session });
  await advance(0);
  expect(reads).toHaveLength(1);
  await settle(() => reads[0].resolve(threads));
  return { session, rendered };
}

test("threads_are_reread_on_a_page_frame", async () => {
  const { session, rendered } = await loaded([thread("t1")]);
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1"]);
  expect(rendered.current.state).toBe("ready");

  await settle(() => session.frame("comments"));
  // Pas tout de suite : une rafale ne fait qu'une relecture.
  expect(reads).toHaveLength(1);
  await advance(COMMENTS_DEBOUNCE_MS);
  expect(reads).toHaveLength(2);
  expect(vi.mocked(getComments)).toHaveBeenLastCalledWith("p");
  await settle(() => reads[1].resolve([thread("t1"), thread("t2")]));
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1", "t2"]);

  // L'historique a sa propre trame : rien à relire ici.
  await settle(() => session.frame("versions"));
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("nothing_is_read_before_the_first_sync", async () => {
  const session = fakeSession({ synced: false, status: "connecting" });
  const rendered = await renderHook(hook, { pageId: "p", session });
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  // L'ouverture de la page ne partage rien avec la lecture des fils.
  expect(reads).toHaveLength(0);
  expect(rendered.current.threads).toBeNull();
  expect(rendered.current.state).toBe("loading");
  await settle(() => session.set({ status: "connected" }));
  await advance(0);
  expect(reads).toHaveLength(0);
  await settle(() => session.set({ synced: true }));
  await advance(0);
  expect(reads).toHaveLength(1);
});

test("a_burst_of_frames_makes_one_reread_after_the_last", async () => {
  const { session } = await loaded();
  for (let i = 0; i < 3; i += 1) {
    await settle(() => session.frame());
    await advance(COMMENTS_DEBOUNCE_MS - 50);
  }
  expect(reads).toHaveLength(1);
  await advance(50);
  expect(reads).toHaveLength(2);
});

test("a_steady_stream_of_frames_rereads_within_the_max_wait", async () => {
  const { session } = await loaded();
  // Une trame toutes les 200 ms : l'anti-rebond seul ne relirait jamais.
  for (let elapsed = 0; elapsed < COMMENTS_MAX_WAIT_MS; elapsed += 200) {
    await settle(() => session.frame());
    await advance(200);
  }
  expect(reads).toHaveLength(2);
});

test("a_reread_asked_during_a_read_waits_for_it_and_is_done_once", async () => {
  const { session } = await loaded();
  await settle(() => session.frame());
  await advance(COMMENTS_DEBOUNCE_MS);
  expect(reads).toHaveLength(2);
  // Deux trames pendant la lecture : elle n'est pas jetée, et une seule relecture suit.
  await settle(() => session.frame());
  await advance(COMMENTS_DEBOUNCE_MS);
  await settle(() => session.frame());
  await advance(COMMENTS_MAX_WAIT_MS);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([thread("t1")]));
  await advance(0);
  expect(reads).toHaveLength(3);
  await settle(() => reads[2].resolve([thread("t1")]));
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(3);
});

test("a_failed_read_keeps_what_was_shown_and_does_not_loop", async () => {
  const { session, rendered } = await loaded([thread("t1")]);
  await settle(() => session.frame());
  await advance(COMMENTS_DEBOUNCE_MS);
  await settle(() => reads[1].reject(unreachable()));
  expect(rendered.current.state).toBe("failed");
  expect(rendered.current.error).toBe("Le serveur Mementum ne répond pas.");
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1"]);
  await advance(60_000);
  expect(reads).toHaveLength(2);
  // « Réessayer » relit, et une lecture réussie efface l'échec.
  await settle(() => rendered.current.reload());
  await advance(0);
  expect(reads).toHaveLength(3);
  await settle(() => reads[2].resolve([thread("t1")]));
  expect(rendered.current.state).toBe("ready");
  expect(rendered.current.error).toBeNull();
});

test("threads_are_reread_when_the_connection_comes_back", async () => {
  const { session } = await loaded();
  // Une trame a pu partir pendant la coupure.
  await settle(() => session.set({ status: "disconnected" }));
  await settle(() => session.set({ status: "connecting" }));
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
  await settle(() => session.set({ status: "connected" }));
  await advance(COMMENTS_MAX_WAIT_MS);
  expect(reads).toHaveLength(2);
});

test("a_new_session_of_the_same_page_rereads_once_synced", async () => {
  const { session: old, rendered } = await loaded();
  // La page passe en lecture seule : une session neuve, qui a pu manquer une trame.
  const next = fakeSession({ synced: false, status: "connecting" });
  await rendered.rerender({ pageId: "p", session: next });
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
  await settle(() => next.set({ status: "connected", synced: true }));
  await advance(COMMENTS_MAX_WAIT_MS);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([thread("t1")]));
  // L'ancienne session ne dit plus rien au hook : ni ses trames, ni ses retours.
  await settle(() => old.frame("comments"));
  await settle(() => old.set({ status: "disconnected" }));
  await settle(() => old.set({ status: "connected" }));
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
  // La neuve, si.
  await settle(() => next.frame("comments"));
  await advance(COMMENTS_MAX_WAIT_MS);
  expect(reads).toHaveLength(3);
});

test("state_is_bound_to_the_page", async () => {
  const { rendered } = await loaded([thread("t1")]);
  const other = fakeSession();
  await rendered.rerender({ pageId: "q", session: other });
  // Rien de la page d'avant ne reste affiché.
  expect(rendered.current.threads).toBeNull();
  expect(rendered.current.state).toBe("loading");
  await advance(0);
  expect(reads).toHaveLength(2);
  expect(vi.mocked(getComments)).toHaveBeenLastCalledWith("q");
  await settle(() => reads[1].resolve([thread("u1")]));
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["u1"]);
});

test("nothing_is_called_after_unmount", async () => {
  const { session, rendered } = await loaded();
  await settle(() => session.frame());
  await rendered.unmount();
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
});

test("a_read_answered_after_unmount_changes_nothing", async () => {
  const { session, rendered } = await loaded();
  await settle(() => session.frame());
  await advance(COMMENTS_DEBOUNCE_MS);
  expect(reads).toHaveLength(2);
  await rendered.unmount();
  await settle(() => reads[1].reject(new ApiError(403, "forbidden", "refus")));
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
  expect(onRefused).not.toHaveBeenCalled();
});

test("a_write_shows_its_result_at_once_and_rereads", async () => {
  const { rendered } = await loaded([thread("t1")]);
  const created = thread("t2", { blockId: "b1" });
  vi.mocked(openThread).mockResolvedValue(created);
  let result: Thread | undefined;
  await settle(async () => {
    result = await rendered.current.open("b1", "  Bonjour  ");
  });
  expect(result).toEqual(created);
  expect(vi.mocked(openThread)).toHaveBeenCalledWith("p", { blockId: "b1", body: "  Bonjour  " });
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1", "t2"]);
  // Et la relecture part sans attendre la trame.
  await advance(0);
  expect(reads).toHaveLength(2);
  // La réponse de la relecture, qui porte déjà le fil, ne le double pas.
  await settle(() => reads[1].resolve([thread("t1"), created]));
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1", "t2"]);
});

test("each_write_reaches_its_route", async () => {
  const first = message("t1-1");
  const { rendered } = await loaded([thread("t1", { comments: [first] })]);
  const reply = message("t1-2", { body: "réponse" });
  vi.mocked(replyToThread).mockResolvedValue(reply);
  await settle(() => rendered.current.reply("t1", "réponse"));
  expect(vi.mocked(replyToThread)).toHaveBeenCalledWith("p", "t1", { body: "réponse" });
  expect(rendered.current.threads?.[0].comments.map((c) => c.id)).toEqual(["t1-1", "t1-2"]);

  const edited = { ...first, body: "corrigé", editedAt: 2_000 };
  vi.mocked(editComment).mockResolvedValue(edited);
  await settle(() => rendered.current.edit("t1", "t1-1", "corrigé"));
  expect(vi.mocked(editComment)).toHaveBeenCalledWith("p", "t1", "t1-1", { body: "corrigé" });
  expect(rendered.current.threads?.[0].comments[0].body).toBe("corrigé");

  const reacted = { ...edited, reactions: [{ emoji: "👍", users: [author("alice")] }] };
  vi.mocked(setReaction).mockResolvedValue(reacted);
  await settle(() => rendered.current.react("t1", "t1-1", "👍", true));
  expect(vi.mocked(setReaction)).toHaveBeenCalledWith("p", "t1", "t1-1", "👍", true);
  expect(rendered.current.threads?.[0].comments[0].reactions).toHaveLength(1);

  const resolved = { ...rendered.current.threads![0], resolvedAt: 3_000, resolvedBy: author("alice") };
  vi.mocked(resolveThread).mockResolvedValue(resolved);
  await settle(() => rendered.current.resolve("t1", true));
  expect(vi.mocked(resolveThread)).toHaveBeenCalledWith("p", "t1", { resolved: true });
  expect(rendered.current.threads?.[0].resolvedAt).toBe(3_000);

  vi.mocked(deleteComment).mockResolvedValue(undefined);
  await settle(() => rendered.current.remove("t1", "t1-2"));
  expect(vi.mocked(deleteComment)).toHaveBeenCalledWith("p", "t1", "t1-2");
  expect(rendered.current.threads?.[0].comments.map((c) => c.id)).toEqual(["t1-1"]);
  // Le fil part avec son dernier message.
  await settle(() => rendered.current.remove("t1", "t1-1"));
  expect(rendered.current.threads).toEqual([]);
});

test("a_read_older_than_a_write_is_done_again", async () => {
  const { session, rendered } = await loaded([thread("t1")]);
  await settle(() => session.frame());
  await advance(COMMENTS_DEBOUNCE_MS);
  expect(reads).toHaveLength(2);
  // Une écriture passe pendant la lecture : la réponse peut dater d'avant elle.
  const created = thread("t2");
  vi.mocked(openThread).mockResolvedValue(created);
  await settle(() => rendered.current.open(null, "neuf"));
  await settle(() => reads[1].resolve([thread("t1")]));
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1", "t2"]);
  await advance(0);
  expect(reads).toHaveLength(3);
  await settle(() => reads[2].resolve([thread("t1"), created]));
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1", "t2"]);
});

test("a_refused_write_is_rethrown_tells_the_page_and_rereads", async () => {
  const { rendered } = await loaded([thread("t1")]);
  const refusal = new ApiError(403, "forbidden", "il faut « Peut commenter » sur cette page");
  vi.mocked(replyToThread).mockRejectedValue(refusal);
  let caught: unknown;
  await settle(async () => {
    try {
      await rendered.current.reply("t1", "non");
    } catch (error) {
      caught = error;
    }
  });
  expect(caught).toBe(refusal);
  expect(onRefused).toHaveBeenCalledWith(refusal);
  // Rien n'a changé à l'écran, et les fils sont relus.
  expect(rendered.current.threads?.[0].comments).toHaveLength(1);
  await advance(0);
  expect(reads).toHaveLength(2);
});

test("a_write_that_does_not_reach_the_server_does_not_tell_the_page", async () => {
  const { rendered } = await loaded([thread("t1")]);
  vi.mocked(replyToThread).mockRejectedValue(unreachable());
  await settle(() => rendered.current.reply("t1", "non").catch(() => {}));
  expect(onRefused).not.toHaveBeenCalled();
});

/**
 * Le compte de la session, perdu puis revenu : `afterLogin()` est tenue au retour du compte
 * (`useAfterLogin`).
 */
function account() {
  const waiting: (() => void)[] = [];
  return {
    afterLogin: () => new Promise<void>((resume) => waiting.push(resume)),
    back: () => settle(() => waiting.splice(0).forEach((resume) => resume())),
  };
}

type WithLogin = Props & { afterLogin: () => Promise<void> };
const withLogin = ({ pageId, session, afterLogin }: WithLogin) =>
  useComments(pageId, session, { onRefused, afterLogin });

test("the_list_is_read_again_once_the_account_is_back", async () => {
  // La session est échue, la connexion de synchro reste ouverte (rien ne la fait revenir), et la
  // première lecture des fils répond `401`.
  const login = account();
  const session = fakeSession();
  const rendered = await renderHook(withLogin, { pageId: "p", session, afterLogin: login.afterLogin });
  await advance(0);
  await settle(() => reads[0].reject(unauthenticated()));
  await advance(60_000);
  expect(
    `reads: ${reads.length} state: ${rendered.current.state} threads: ${rendered.current.threads}`,
  ).toBe("reads: 1 state: loading threads: null");
  // Le compte revient : les fils sont relus, une fois.
  await login.back();
  await advance(0);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([thread("t1")]));
  expect(rendered.current.state).toBe("ready");
  expect(rendered.current.threads?.map((t) => t.id)).toEqual(["t1"]);
  await advance(COMMENTS_MAX_WAIT_MS * 2);
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
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  await settle(() => rendered.current.reload());
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(1);
  // Le compte revient : une seule lecture les sert toutes.
  await login.back();
  await advance(0);
  expect(reads).toHaveLength(2);
  await settle(() => reads[1].resolve([thread("t1")]));
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(reads).toHaveLength(2);
});

test("the_wait_for_the_account_does_not_outlive_the_unmount_or_the_page", async () => {
  const login = account();
  const session = fakeSession();
  const rendered = await renderHook(withLogin, { pageId: "p", session, afterLogin: login.afterLogin });
  await advance(0);
  await settle(() => reads[0].reject(unauthenticated()));
  // Une autre page : ses fils sont lus ; ceux de la première ne le seront plus.
  await rendered.rerender({ pageId: "q", session: fakeSession(), afterLogin: login.afterLogin });
  await advance(0);
  expect(vi.mocked(getComments).mock.calls.map(([id]) => id)).toEqual(["p", "q"]);
  await settle(() => reads[1].reject(unauthenticated()));
  // La page se ferme pendant l'attente : le compte revenu, rien n'est relu.
  await rendered.unmount();
  await login.back();
  await advance(COMMENTS_MAX_WAIT_MS * 2);
  expect(vi.mocked(getComments).mock.calls.map(([id]) => id)).toEqual(["p", "q"]);
});
