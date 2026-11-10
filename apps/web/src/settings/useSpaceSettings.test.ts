// Ce que lisent les réglages d'un espace : ses membres, et ses invitations pour qui les gère.
// Deux relectures ne se croisent pas, et une relecture qui échoue ne retire rien de l'écran.
import type { Member, PendingInvitation, Role } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { ApiError, getMembers, getSpaceInvitations } from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { notFound, unauthenticated, unreachable } from "../test/pages";
import { useSpaceSettings } from "./useSpaceSettings";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getMembers: vi.fn(),
  getSpaceInvitations: vi.fn(),
}));

const member = (userId: string, role: Role): Member => ({
  userId,
  name: userId.toUpperCase(),
  email: `${userId}@exemple.fr`,
  role,
});

const pending = (id: string): PendingInvitation => ({
  id,
  email: `${id}@exemple.fr`,
  role: "member",
  level: null,
  pageId: null,
  expiresAt: 1_800_000_000_000,
});

const forbidden = () => new ApiError(403, "forbidden", "réservé aux admins de l'espace");

/** Les lectures des membres et des invitations, dans l'ordre : le test tient ou rompt chacune. */
let members: Deferred<Member[]>[] = [];
let invitations: Deferred<PendingInvitation[]>[] = [];
const onForbidden = vi.fn();
const onSessionLost = vi.fn();

beforeEach(() => {
  members = [];
  invitations = [];
  onForbidden.mockClear();
  onSessionLost.mockClear();
  vi.mocked(getMembers).mockReset();
  vi.mocked(getMembers).mockImplementation(() => {
    const answer = deferred<Member[]>();
    members.push(answer);
    return answer.promise;
  });
  vi.mocked(getSpaceInvitations).mockReset();
  vi.mocked(getSpaceInvitations).mockImplementation(() => {
    const answer = deferred<PendingInvitation[]>();
    invitations.push(answer);
    return answer.promise;
  });
});

const hook = () => useSpaceSettings("pro", "alice", onForbidden, onSessionLost);

const owner = [member("alice", "owner"), member("bruno", "member")];

/** Les réglages lus : Alice, propriétaire, et une invitation qui attend. */
async function read() {
  const settings = await renderHook(hook, undefined);
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[0].resolve(owner));
  await settle(() => invitations[0].resolve([pending("i1")]));
  expect(settings.current.load).toEqual({
    name: "ready",
    members: owner,
    invitations: [pending("i1")],
  });
  return settings;
}

test("manager_reads_members_then_invitations", async () => {
  const settings = await renderHook(hook, undefined);
  expect(settings.current.load).toEqual({ name: "loading" });
  await settle(() => {
    void settings.current.reload();
  });
  expect(getMembers).toHaveBeenCalledExactlyOnceWith("pro");
  await settle(() => members[0].resolve(owner));
  expect(getSpaceInvitations).toHaveBeenCalledExactlyOnceWith("pro");
  await settle(() => invitations[0].resolve([pending("i1")]));
  expect(settings.current.load).toEqual({ name: "ready", members: owner, invitations: [pending("i1")] });
  expect(settings.current.reloadError).toBeNull();
});

test("simple_member_does_not_ask_for_invitations", async () => {
  const settings = await renderHook(hook, undefined);
  await settle(() => {
    void settings.current.reload();
  });
  const list = [member("zoe", "owner"), member("alice", "member")];
  await settle(() => members[0].resolve(list));
  expect(settings.current.load).toEqual({ name: "ready", members: list, invitations: [] });
  expect(getSpaceInvitations).not.toHaveBeenCalled();
});

test("older_reload_arriving_late_is_dropped", async () => {
  const settings = await read();
  // Deux relectures se suivent de près (deux actions) ; la première répond après la seconde.
  await settle(() => {
    void settings.current.reload();
    void settings.current.reload();
  });
  expect(members).toHaveLength(3);
  const latest = [member("alice", "owner")];
  await settle(() => members[2].resolve(latest));
  await settle(() => invitations[1].resolve([]));
  expect(settings.current.load).toEqual({ name: "ready", members: latest, invitations: [] });
  // La plus ancienne arrive enfin : elle ne remet pas à l'écran ce qui n'est plus vrai.
  await settle(() => members[1].resolve(owner));
  await settle(() => invitations[2]?.resolve([pending("i1")]));
  expect(settings.current.load).toEqual({ name: "ready", members: latest, invitations: [] });
});

test("failed_reload_keeps_what_is_shown_and_says_so", async () => {
  const settings = await read();
  const before = settings.current.load;
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[1].reject(unreachable()));
  // Les membres et les invitations déjà lus restent à l'écran, avec le message.
  expect(settings.current.load).toBe(before);
  expect(settings.current.reloadError).toBe("Le serveur Mementum ne répond pas.");
  // La relecture suivante qui réussit l'efface.
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[2].resolve(owner));
  await settle(() => invitations[1].resolve([]));
  expect(settings.current.load).toEqual({ name: "ready", members: owner, invitations: [] });
  expect(settings.current.reloadError).toBeNull();
});

test("first_read_that_fails_says_so_in_place_of_the_settings", async () => {
  const settings = await renderHook(hook, undefined);
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[0].reject(unreachable()));
  expect(settings.current.load).toEqual({
    name: "failed",
    message: "Le serveur Mementum ne répond pas.",
  });
  expect(settings.current.reloadError).toBeNull();
});

test("space_without_a_role_replaces_everything", async () => {
  const settings = await read();
  // Retiré de l'espace entre-temps : il répond comme un espace qui n'existe pas.
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[1].reject(notFound()));
  expect(settings.current.load).toEqual({ name: "missing" });
});

test("guest_is_told_to_the_caller", async () => {
  const settings = await read();
  const before = settings.current.load;
  // Devenu invité entre-temps : la liste des membres lui est refusée.
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[1].reject(forbidden()));
  expect(onForbidden).toHaveBeenCalledTimes(1);
  expect(settings.current.load).toBe(before);
});

test("role_lowered_between_the_two_reads_reads_the_members_again", async () => {
  const settings = await renderHook(hook, undefined);
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[0].resolve(owner));
  // Entre les deux appels, Alice est devenue simple membre : les invitations lui sont refusées.
  await settle(() => invitations[0].reject(forbidden()));
  expect(members).toHaveLength(2);
  const lowered = [member("alice", "member"), member("bruno", "owner")];
  await settle(() => members[1].resolve(lowered));
  expect(settings.current.load).toEqual({ name: "ready", members: lowered, invitations: [] });
  expect(onForbidden).not.toHaveBeenCalled();
});

test("lost_session_keeps_what_is_shown", async () => {
  const settings = await read();
  const before = settings.current.load;
  await settle(() => {
    void settings.current.reload();
  });
  // La connexion est demandée par-dessus l'application : rien ne change ici.
  await settle(() => members[1].reject(unauthenticated()));
  expect(settings.current.load).toBe(before);
  expect(settings.current.reloadError).toBeNull();
  // La lecture se signale : elle sera refaite au retour du compte.
  expect(onSessionLost).toHaveBeenCalledTimes(1);
});

test("first_read_lost_with_the_session_signals_itself", async () => {
  const settings = await renderHook(hook, undefined);
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[0].reject(unauthenticated()));
  // Rien n'a été lu : l'écran en est à « Chargement… », et ne doit pas y rester une fois le
  // compte revenu.
  expect(settings.current.load).toEqual({ name: "loading" });
  expect(onSessionLost).toHaveBeenCalledTimes(1);
  // Les invitations aussi peuvent tomber sur la session perdue.
  await settle(() => {
    void settings.current.reload();
  });
  await settle(() => members[1].resolve(owner));
  await settle(() => invitations[0].reject(unauthenticated()));
  expect(settings.current.load).toEqual({ name: "loading" });
  expect(onSessionLost).toHaveBeenCalledTimes(2);
});
