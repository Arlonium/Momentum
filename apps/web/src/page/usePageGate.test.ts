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

test("older_answer_arriving_after_a_newer_one_is_dropped", async () => {
  const gate = await renderHook(hook, { writable: true });
  // Deux lectures de la page sont en route : celle d'une connexion de synchro, puis celle que
  // demande le menu Partager.
  const state = connecting(gate.current.check);
  await settle();
  void gate.current.reread();
  await settle();
  expect(asked).toHaveLength(2);
  // La plus récente répond d'abord ; l'autre, partie avant un changement, arrive ensuite.
  const fresh = page("x", { level: "edit", title: "Titre d'après" });
  await settle(() => asked[1].answer.resolve(fresh));
  await settle(() => asked[0].answer.resolve(page("x", { level: "read", title: "Titre d'avant" })));
  // Elle ne dit plus rien de la page à l'application.
  expect(onPage).toHaveBeenCalledExactlyOnceWith(fresh);
  // La connexion se décide d'après ce que l'application sait : refusée au vu de l'ancien
  // niveau, elle resterait fermée sans que rien ne la remplace.
  expect(state.verdict).toBe(true);
});

test("older_answer_that_the_page_is_gone_is_dropped_too", async () => {
  const gate = await renderHook(hook, { writable: false });
  const state = connecting(gate.current.check);
  await settle();
  void gate.current.reread();
  await settle();
  const fresh = page("x", { level: "read" });
  await settle(() => asked[1].answer.resolve(fresh));
  await settle(() => asked[0].answer.reject(notFound()));
  expect(onGone).not.toHaveBeenCalled();
  expect(onPage).toHaveBeenCalledExactlyOnceWith(fresh);
  expect(state.verdict).toBe(true);
});

test("answers_arriving_in_order_all_count", async () => {
  const gate = await renderHook(hook, { writable: true });
  const state = connecting(gate.current.check);
  await settle();
  void gate.current.reread();
  await settle();
  const before = page("x", { level: "edit", title: "Titre d'avant" });
  const after = page("x", { level: "edit", title: "Titre d'après" });
  await settle(() => asked[0].answer.resolve(before));
  await settle(() => asked[1].answer.resolve(after));
  expect(onPage.mock.calls).toEqual([[before], [after]]);
  expect(state.verdict).toBe(true);
});

test("older_answer_counts_when_the_newer_read_fails", async () => {
  const gate = await renderHook(hook, { writable: true });
  const state = connecting(gate.current.check);
  await settle();
  void gate.current.reread();
  await settle();
  // La plus récente n'a rien appris (serveur injoignable) : l'autre reste ce qu'on sait de
  // mieux, et la connexion ne reste pas refusée sans que l'application sache pourquoi.
  await settle(() => asked[1].answer.reject(unreachable()));
  const lowered = page("x", { level: "read" });
  await settle(() => asked[0].answer.resolve(lowered));
  expect(onPage).toHaveBeenCalledExactlyOnceWith(lowered);
  expect(state.verdict).toBe(false);
});

test("answers_arriving_once_unmounted_say_nothing", async () => {
  const gate = await renderHook(hook, { writable: true });
  const first = connecting(gate.current.check);
  const second = connecting(gate.current.check);
  await settle();
  // La page est quittée (ou rouverte dans une autre instance) : ces réponses ne sont plus à
  // personne, et ne doivent pas arriver après celles de la page rouverte.
  await gate.unmount();
  await settle(() => asked[0].answer.resolve(page("x", { level: "read" })));
  await settle(() => asked[1].answer.reject(notFound()));
  expect(onPage).not.toHaveBeenCalled();
  expect(onGone).not.toHaveBeenCalled();
  expect([first.verdict, second.verdict]).toEqual([false, false]);
});

// Une session en lecture seule ne fait que recevoir ; mais qui peut commenter, ou restaurer (une
// version, une page de la corbeille), écrit depuis cet onglet hors de l'éditeur. Pour lui, le
// schéma est relu aussi avant chaque connexion (R177) : un serveur mis à jour fait passer les
// commentaires et la restauration en lecture seule, sans rien changer à l'éditeur. Un simple
// lecteur n'a rien de plus sur le chemin de l'ouverture.

const outside = ({ writable, writesOutsideEditor }: { writable: boolean; writesOutsideEditor: boolean }) =>
  usePageGate({ id: "x", writable, writesOutsideEditor, checkSchema, onPage, onGone, afterLogin });

test("read_only_session_of_someone_who_writes_outside_the_editor_rereads_the_schema", async () => {
  sameSchema = false;
  const gate = await renderHook(outside, { writable: false, writesOutsideEditor: true });
  const state = connecting(gate.current.check);
  // Les deux relectures partent ensemble : l'ouverture n'attend pas l'une après l'autre.
  await settle();
  expect(checkSchema).toHaveBeenCalledTimes(1);
  expect(asked).toHaveLength(1);
  // Un autre schéma (`checkSchema` l'a dit à l'application) ne ferme pas la session : elle ne
  // fait que recevoir, l'éditeur reste tel quel.
  await settle(() => asked[0].answer.resolve(page("x", { level: "comment" })));
  expect(state.verdict).toBe(true);
});

test("a_schema_that_cannot_be_read_does_not_block_a_read_only_session", async () => {
  checkSchema.mockRejectedValueOnce(unreachable());
  const gate = await renderHook(outside, { writable: false, writesOutsideEditor: true });
  const state = connecting(gate.current.check);
  await settle(() => asked[0].answer.resolve(page("x", { level: "comment" })));
  // Comme avant pour cette session : seule la page décide ; le schéma sera relu à la
  // connexion suivante.
  expect(state.verdict).toBe(true);
  expect(state.error).toBeUndefined();
});

test("a_reader_reads_nothing_more_before_connecting", async () => {
  const gate = await renderHook(outside, { writable: false, writesOutsideEditor: false });
  const state = connecting(gate.current.check);
  await settle(() => asked[0].answer.resolve(page("x", { level: "read" })));
  expect(state.verdict).toBe(true);
  expect(checkSchema).not.toHaveBeenCalled();
});

test("what_one_may_write_outside_the_editor_does_not_change_the_check", async () => {
  const gate = await renderHook(outside, { writable: false, writesOutsideEditor: false });
  const { check } = gate.current;
  // Le niveau qui change (lecteur devenu commentateur) ne recrée pas la session de synchro :
  // la connexion suivante relit le schéma.
  await gate.rerender({ writable: false, writesOutsideEditor: true });
  expect(gate.current.check).toBe(check);
  const state = connecting(gate.current.check);
  await settle(() => asked[0].answer.resolve(page("x", { level: "comment" })));
  expect(checkSchema).toHaveBeenCalledTimes(1);
  expect(state.verdict).toBe(true);
});
