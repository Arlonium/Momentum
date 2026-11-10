// Avant chaque connexion de synchro, le schéma du serveur et la page sont relus : c'est ce
// qui dit à l'onglet qu'un droit a changé pendant que la page était ouverte.
import type { PageNode } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { getPage } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { notFound, page, unauthenticated, unreachable } from "../test/pages";
import { usePageGate } from "./usePageGate";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getPage: vi.fn(),
}));

/** Les appels à `getPage`, dans l'ordre : le test tient ou rompt chacun. */
let asked: { id: string; answer: Deferred<PageNode> }[] = [];
/** Ce que répond la relecture du schéma. */
let sameSchema = true;
const checkSchema = vi.fn(async () => sameSchema);
const onPage = vi.fn();
const onGone = vi.fn();
/** Le retour du compte, après une session perdue : le test le tient. */
let login: Deferred<void>;
const afterLogin = vi.fn(() => login.promise);

beforeEach(() => {
  asked = [];
  sameSchema = true;
  login = deferred<void>();
  for (const mock of [checkSchema, onPage, onGone, afterLogin]) mock.mockClear();
  vi.mocked(getPage).mockReset();
  vi.mocked(getPage).mockImplementation((id) => {
    const answer = deferred<PageNode>();
    asked.push({ id, answer });
    return answer.promise;
  });
});

const hook = ({ writable }: { writable: boolean }) =>
  usePageGate({ id: "x", writable, checkSchema, onPage, onGone, afterLogin });

/** Lance la vérification d'une connexion, et rend ce qu'elle a décidé jusqu'ici. */
function connecting(check: () => Promise<boolean>) {
  const state: { verdict?: boolean; error?: unknown } = {};
  check().then(
    (verdict) => (state.verdict = verdict),
    (error: unknown) => (state.error = error),
  );
  return state;
}

test("writable_session_connects_when_schema_and_level_allow", async () => {
  const gate = await renderHook(hook, { writable: true });
  const state = connecting(gate.current.check);
  // Le schéma et la page sont relus ensemble, avant toute connexion.
  await settle();
  expect(checkSchema).toHaveBeenCalledTimes(1);
  expect(asked.map((a) => a.id)).toEqual(["x"]);
  expect(state.verdict).toBeUndefined();
  const fresh = page("x", { level: "edit" });
  await settle(() => asked[0].answer.resolve(fresh));
  expect(state.verdict).toBe(true);
  // Ce que le serveur dit de la page est rendu à l'application, à chaque connexion.
  expect(onPage).toHaveBeenCalledExactlyOnceWith(fresh);
  expect(onGone).not.toHaveBeenCalled();
});

test("level_below_edit_or_trash_refuses_a_writable_session", async () => {
  const gate = await renderHook(hook, { writable: true });
  for (const fresh of [
    page("x", { level: "read" }),
    page("x", { level: "comment" }),
    page("x", { level: "full", trashedAt: 12 }),
  ]) {
    onPage.mockClear();
    const state = connecting(gate.current.check);
    await settle(() => asked[asked.length - 1].answer.resolve(fresh));
    // Rien ne part par cette session : l'application, prévenue, en ouvrira une en lecture
    // seule, sur un document neuf.
    expect(state.verdict, JSON.stringify(fresh)).toBe(false);
    expect(onPage).toHaveBeenCalledExactlyOnceWith(fresh);
  }
});

test("other_schema_refuses_a_writable_session", async () => {
  sameSchema = false;
  const gate = await renderHook(hook, { writable: true });
  const state = connecting(gate.current.check);
  await settle(() => asked[0].answer.resolve(page("x")));
  expect(state.verdict).toBe(false);
});

test("read_only_session_connects_whatever_the_level", async () => {
  const gate = await renderHook(hook, { writable: false });
  for (const fresh of [
    page("x", { level: "read" }),
    page("x", { level: "full" }),
    page("x", { level: "edit", trashedAt: 12 }),
  ]) {
    onPage.mockClear();
    const state = connecting(gate.current.check);
    await settle(() => asked[asked.length - 1].answer.resolve(fresh));
    // Elle ne fait que recevoir : tant que la page se lit, elle se connecte. L'application
    // apprend quand même ce que la page est devenue (niveau remonté, page restaurée).
    expect(state.verdict, JSON.stringify(fresh)).toBe(true);
    expect(onPage).toHaveBeenCalledExactlyOnceWith(fresh);
  }
  // Le schéma ne la concerne pas : il n'est pas relu.
  expect(checkSchema).not.toHaveBeenCalled();
});

test("missing_page_stops_the_session_for_good", async () => {
  for (const writable of [true, false]) {
    onGone.mockClear();
    const gate = await renderHook(hook, { writable });
    const state = connecting(gate.current.check);
    await settle(() => asked[asked.length - 1].answer.reject(notFound()));
    // Supprimée, ou plus partagée avec cette personne : la même réponse. Aucune connexion.
    expect(state.verdict).toBe(false);
    expect(onGone).toHaveBeenCalledTimes(1);
    expect(onPage).not.toHaveBeenCalled();
  }
});

test("lost_session_waits_for_the_account_then_reads_again", async () => {
  const gate = await renderHook(hook, { writable: true });
  const state = connecting(gate.current.check);
  await settle(() => asked[0].answer.reject(unauthenticated()));
  // La connexion est demandée par-dessus la page (`api.ts`) : ici, on attend le compte, sans
  // rien décider ni rien redemander.
  expect(afterLogin).toHaveBeenCalledTimes(1);
  expect(state).toEqual({});
  expect(asked).toHaveLength(1);
  expect(onGone).not.toHaveBeenCalled();
  // Le compte revient : la page est relue, et la connexion se fait.
  await settle(() => login.resolve());
  expect(asked).toHaveLength(2);
  const fresh = page("x");
  await settle(() => asked[1].answer.resolve(fresh));
  expect(state.verdict).toBe(true);
  expect(onPage).toHaveBeenCalledExactlyOnceWith(fresh);
});

test("unreachable_server_lets_the_connection_fail", async () => {
  const gate = await renderHook(hook, { writable: true });
  const state = connecting(gate.current.check);
  const failure = unreachable();
  await settle(() => asked[0].answer.reject(failure));
  // Comme une connexion refusée : y-websocket réessaiera, et la page reste telle quelle.
  expect(state.error).toBe(failure);
  expect(onPage).not.toHaveBeenCalled();
  expect(onGone).not.toHaveBeenCalled();
});

test("check_keeps_its_identity_while_the_mode_does_not_change", async () => {
  const gate = await renderHook(hook, { writable: true });
  const { check } = gate.current;
  await gate.rerender({ writable: true });
  // Une autre identité recréerait la session de synchro à chaque rendu.
  expect(gate.current.check).toBe(check);
  await gate.rerender({ writable: false });
  expect(gate.current.check).not.toBe(check);
});

test("reread_tells_what_the_page_became", async () => {
  const gate = await renderHook(hook, { writable: true });
  // Hors d'une connexion (le menu Partager vient de retirer l'accès complet) : la page est
  // relue, et l'application prévenue.
  let done = false;
  void gate.current.reread().then(() => (done = true));
  const fresh = page("x", { level: "read" });
  await settle(() => asked[0].answer.resolve(fresh));
  expect(done).toBe(true);
  expect(onPage).toHaveBeenCalledExactlyOnceWith(fresh);
  // Elle n'est plus là.
  void gate.current.reread();
  await settle(() => asked[1].answer.reject(notFound()));
  expect(onGone).toHaveBeenCalledTimes(1);
  // Le serveur ne répond pas : rien à dire, la prochaine connexion de synchro la relira.
  done = false;
  void gate.current.reread().then(() => (done = true));
  await settle(() => asked[2].answer.reject(unreachable()));
  expect(done).toBe(true);
  expect(onPage).toHaveBeenCalledTimes(1);
  expect(onGone).toHaveBeenCalledTimes(1);
});
