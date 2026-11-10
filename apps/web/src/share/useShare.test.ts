// Le menu Partager, côté données : ce qu'il lit, ce que chaque changement envoie, ce qu'il
// montre quand le serveur refuse, et ce qu'il fait quand la personne n'a plus l'accès complet.
import type { AccessEntry, InvitationCreated, PageAccess, PendingInvitation } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import {
  ApiError,
  getAccess,
  inviteToPage,
  removeUserAccess,
  revokeInvitation,
  setGeneralAccess,
  setUserLevel,
} from "../api";
import { deferred, renderHook, settle, type Deferred } from "../test/hooks";
import { notFound, unauthenticated, unreachable } from "../test/pages";
import { useShare } from "./useShare";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getAccess: vi.fn(),
  setGeneralAccess: vi.fn(),
  setUserLevel: vi.fn(),
  removeUserAccess: vi.fn(),
  inviteToPage: vi.fn(),
  revokeInvitation: vi.fn(),
}));

const entry = (userId: string, overrides: Partial<AccessEntry> = {}): AccessEntry => ({
  userId,
  name: userId.toUpperCase(),
  email: `${userId}@exemple.fr`,
  level: "full",
  inherited: false,
  ...overrides,
});

const pending = (id: string, email: string): PendingInvitation => ({
  id,
  email,
  role: null,
  level: "edit",
  pageId: "x",
  expiresAt: 1_800_000_000_000,
});

const access = (overrides: Partial<PageAccess> = {}): PageAccess => ({
  general: null,
  inheritedGeneral: null,
  entries: [entry("alice")],
  invitations: [],
  ...overrides,
});

const forbidden = () => new ApiError(403, "forbidden", "accès complet requis sur cette page");
const lastFull = () =>
  new ApiError(409, "last_full_access", "la page doit garder quelqu'un en accès complet");

/** Les lectures des accès, dans l'ordre : le test tient ou rompt chacune. */
let reads: Deferred<PageAccess>[] = [];
const onChanged = vi.fn();
const onAccessLost = vi.fn();

beforeEach(() => {
  reads = [];
  onChanged.mockClear();
  onAccessLost.mockClear();
  for (const mock of [setGeneralAccess, setUserLevel, removeUserAccess, inviteToPage, revokeInvitation]) {
    vi.mocked(mock).mockReset();
  }
  vi.mocked(getAccess).mockReset();
  vi.mocked(getAccess).mockImplementation(() => {
    const answer = deferred<PageAccess>();
    reads.push(answer);
    return answer.promise;
  });
});

const hook = ({ open }: { open: boolean }) => useShare("x", open, { onChanged, onAccessLost });

/** Le menu ouvert, ses accès lus. */
async function opened(first: PageAccess = access()) {
  const share = await renderHook(hook, { open: true });
  await settle(() => reads[0].resolve(first));
  expect(share.current.access).toEqual(first);
  return share;
}

test("opening_reads_the_access_and_reopening_reads_it_again", async () => {
  const share = await renderHook(hook, { open: false });
  // Fermé : rien n'est lu.
  expect(getAccess).not.toHaveBeenCalled();
  await share.rerender({ open: true });
  expect(getAccess).toHaveBeenCalledExactlyOnceWith("x");
  expect(share.current.access).toBeNull();
  expect(share.current.reading).toBe(true);
  const first = access();
  await settle(() => reads[0].resolve(first));
  expect(share.current.access).toEqual(first);
  expect(share.current.reading).toBe(false);

  // Refermé puis rouvert : ce qu'on savait reste montré pendant la relecture, puis l'état du
  // serveur le remplace.
  await share.rerender({ open: false });
  await share.rerender({ open: true });
  expect(getAccess).toHaveBeenCalledTimes(2);
  expect(share.current.access).toEqual(first);
  expect(share.current.reading).toBe(true);
  const second = access({ general: "read" });
  await settle(() => reads[1].resolve(second));
  expect(share.current.access).toEqual(second);
  expect(share.current.reading).toBe(false);
});

test("failed_read_says_so_and_keeps_what_was_read", async () => {
  const share = await renderHook(hook, { open: true });
  await settle(() => reads[0].reject(unreachable()));
  expect(share.current.access).toBeNull();
  expect(share.current.readError).toBe("Le serveur Mementum ne répond pas.");
  // « Réessayer » relit ; le message part quand la lecture réussit.
  const first = access();
  await settle(() => {
    void share.current.reload();
    reads[1].resolve(first);
  });
  expect(share.current.access).toEqual(first);
  expect(share.current.readError).toBeNull();
  // Une relecture qui échoue ensuite garde les accès déjà lus à l'écran.
  await settle(() => {
    void share.current.reload();
    reads[2].reject(unreachable());
  });
  expect(share.current.access).toEqual(first);
  expect(share.current.readError).toBe("Le serveur Mementum ne répond pas.");
  expect(onAccessLost).not.toHaveBeenCalled();
});

test("changes_send_what_was_chosen_and_show_what_the_server_answers", async () => {
  const share = await opened(access({ entries: [entry("alice"), entry("bruno", { level: "read" })] }));
  const reads_ = () => vi.mocked(getAccess).mock.calls.length;
  const before = reads_();

  const raised = access({ entries: [entry("alice"), entry("bruno", { level: "edit" })] });
  vi.mocked(setUserLevel).mockResolvedValue(raised);
  await settle(() => share.current.setLevel("bruno", "edit"));
  expect(setUserLevel).toHaveBeenCalledExactlyOnceWith("x", "bruno", "edit");
  expect(share.current.access).toEqual(raised);

  const removed = access();
  vi.mocked(removeUserAccess).mockResolvedValue(removed);
  await settle(() => share.current.remove("bruno"));
  expect(removeUserAccess).toHaveBeenCalledExactlyOnceWith("x", "bruno");
  expect(share.current.access).toEqual(removed);

  const general = access({ general: "read" });
  vi.mocked(setGeneralAccess).mockResolvedValue(general);
  await settle(() => share.current.setGeneral("read"));
  expect(setGeneralAccess).toHaveBeenLastCalledWith("x", "read");
  expect(share.current.access).toEqual(general);
  vi.mocked(setGeneralAccess).mockResolvedValue(removed);
  await settle(() => share.current.setGeneral(null));
  expect(setGeneralAccess).toHaveBeenLastCalledWith("x", null);
  expect(share.current.access).toEqual(removed);

  // La réponse du serveur suffit : rien n'est relu. L'application est prévenue à chaque
  // changement (la section de la page a pu changer).
  expect(reads_()).toBe(before);
  expect(onChanged).toHaveBeenCalledTimes(4);
  expect(share.current.error).toBeNull();
  expect(share.current.busy).toBeNull();
});

test("refused_change_shows_the_server_message_and_the_server_state", async () => {
  const share = await opened();
  vi.mocked(setUserLevel).mockRejectedValue(lastFull());
  await settle(() => {
    void share.current.setLevel("alice", "edit");
  });
  // Le message du serveur, tel quel ; les accès sont relus : l'écran revient à son état.
  expect(share.current.error).toBe("la page doit garder quelqu'un en accès complet");
  expect(reads).toHaveLength(2);
  const server = access({ entries: [entry("alice"), entry("bruno", { level: "read" })] });
  await settle(() => reads[1].resolve(server));
  expect(share.current.access).toEqual(server);
  expect(share.current.error).toBe("la page doit garder quelqu'un en accès complet");
  expect(share.current.busy).toBeNull();
  expect(onChanged).not.toHaveBeenCalled();
  expect(onAccessLost).not.toHaveBeenCalled();

  // Le refus part au changement suivant.
  vi.mocked(setUserLevel).mockResolvedValue(server);
  await settle(() => share.current.setLevel("bruno", "read"));
  expect(share.current.error).toBeNull();
});

test("answer_without_a_body_means_full_access_is_lost", async () => {
  const share = await opened();
  // `204` : le changement est fait, et la personne n'a plus l'accès complet (elle s'est
  // baissée ou retirée elle-même). Le menu se ferme, la page est relue.
  vi.mocked(removeUserAccess).mockResolvedValue(null);
  await settle(() => share.current.remove("alice"));
  expect(onAccessLost).toHaveBeenCalledExactlyOnceWith(null);
  expect(share.current.error).toBeNull();
  // Rien n'est relu : le serveur ne rendrait plus les accès.
  expect(reads).toHaveLength(1);
});

test("refusal_for_lack_of_full_access_is_passed_on", async () => {
  const share = await opened();
  // Quelqu'un d'autre a baissé le niveau de la personne pendant que le menu était ouvert.
  vi.mocked(setUserLevel).mockRejectedValue(forbidden());
  await settle(() => {
    void share.current.setLevel("alice", "edit");
  });
  await settle(() => reads[1].reject(forbidden()));
  // Le menu ne peut plus rien montrer : le refus est rendu à l'application, qui le dira.
  expect(onAccessLost).toHaveBeenCalledExactlyOnceWith("accès complet requis sur cette page");
});

test("page_that_is_gone_closes_the_menu", async () => {
  const share = await renderHook(hook, { open: true });
  await settle(() => reads[0].reject(notFound()));
  expect(onAccessLost).toHaveBeenCalledExactlyOnceWith(null);
  expect(share.current.readError).toBeNull();
});

test("invitation_shows_its_link_once_and_joins_the_list", async () => {
  const share = await opened();
  const created: InvitationCreated = { id: "i1", token: "jeton-de-test-manifestement-faux-000000000000" };
  vi.mocked(inviteToPage).mockResolvedValue(created);
  let done: boolean | undefined;
  await settle(() => {
    void share.current.invite("  Bruno@Exemple.fr ", "edit").then((ok) => (done = ok));
  });
  expect(inviteToPage).toHaveBeenCalledExactlyOnceWith("x", { email: "  Bruno@Exemple.fr ", level: "edit" });
  // Le lien est construit ici, le jeton dans son fragment ; l'adresse est montrée comme le
  // serveur la garde.
  expect(share.current.created).toEqual({
    id: "i1",
    email: "bruno@exemple.fr",
    level: "edit",
    link: `${location.origin}/invite#${created.token}`,
  });
  // La liste des invitations en attente est relue.
  expect(reads).toHaveLength(2);
  const listed = access({ invitations: [pending("i1", "bruno@exemple.fr")] });
  await settle(() => reads[1].resolve(listed));
  expect(done).toBe(true);
  expect(share.current.access).toEqual(listed);
  expect(share.current.created?.id).toBe("i1");
  expect(share.current.inviteError).toBeNull();
  // Une invitation n'est pas un droit : la barre latérale n'a rien à relire.
  expect(onChanged).not.toHaveBeenCalled();
});

test("invitation_link_stays_when_the_list_cannot_be_read_again", async () => {
  const share = await opened();
  vi.mocked(inviteToPage).mockResolvedValue({ id: "i1", token: "jeton-de-test" });
  await settle(() => {
    void share.current.invite("bruno@exemple.fr", "read");
  });
  await settle(() => reads[1].reject(unreachable()));
  // L'invitation existe, et son lien ne sera plus jamais donné : il reste à l'écran, avec les
  // accès déjà lus et le message de la relecture.
  expect(share.current.created?.link).toBe(`${location.origin}/invite#jeton-de-test`);
  expect(share.current.access).toEqual(access());
  expect(share.current.readError).toBe("Le serveur Mementum ne répond pas.");
  expect(share.current.inviteError).toBeNull();
});

test("refused_invitation_says_why", async () => {
  const share = await opened();
  vi.mocked(inviteToPage).mockRejectedValue(
    new ApiError(422, "invalid_email", "adresse e-mail invalide"),
  );
  let done: boolean | undefined;
  await settle(() => share.current.invite("pas une adresse", "edit").then((ok) => (done = ok)));
  expect(done).toBe(false);
  expect(share.current.inviteError).toBe("adresse e-mail invalide");
  expect(share.current.created).toBeNull();
  expect(reads).toHaveLength(1);
});

test("created_link_goes_when_its_invitation_no_longer_waits", async () => {
  const share = await opened();
  vi.mocked(inviteToPage).mockResolvedValue({ id: "i1", token: "jeton-de-test" });
  await settle(() => {
    void share.current.invite("bruno@exemple.fr", "read");
  });
  await settle(() => reads[1].resolve(access({ invitations: [pending("i1", "bruno@exemple.fr")] })));
  expect(share.current.created).not.toBeNull();

  // Révoquée : le lien ne vaut plus rien, et ne se montre plus.
  vi.mocked(revokeInvitation).mockResolvedValue(undefined);
  await settle(() => {
    void share.current.revoke("i1");
  });
  expect(revokeInvitation).toHaveBeenCalledExactlyOnceWith("i1");
  await settle(() => reads[2].resolve(access()));
  expect(share.current.access).toEqual(access());
  expect(share.current.created).toBeNull();
  expect(share.current.busy).toBeNull();
});

test("invitation_already_used_is_just_read_again", async () => {
  const share = await opened(access({ invitations: [pending("i1", "bruno@exemple.fr")] }));
  // Acceptée, échue ou révoquée ailleurs entre-temps (`404`) : la liste relue le montre, sans
  // message.
  vi.mocked(revokeInvitation).mockRejectedValue(notFound());
  await settle(() => {
    void share.current.revoke("i1");
  });
  const after = access({ entries: [entry("alice"), entry("bruno", { level: "edit" })] });
  await settle(() => reads[1].resolve(after));
  expect(share.current.access).toEqual(after);
  expect(share.current.error).toBeNull();
});

test("refused_revocation_is_said_when_full_access_was_lost", async () => {
  // Qui ne peut plus gérer l'invitation reçoit le même `404` que pour une invitation déjà
  // utilisée ; le serveur ne lui rend plus les accès non plus, et dit pourquoi.
  const share = await opened(access({ invitations: [pending("i1", "bruno@exemple.fr")] }));
  vi.mocked(revokeInvitation).mockRejectedValue(notFound());
  await settle(() => {
    void share.current.revoke("i1");
  });
  await settle(() => reads[1].reject(forbidden()));
  // Le menu se ferme, mais pas sans un mot : l'invitation vaut toujours.
  expect(onAccessLost).toHaveBeenCalledExactlyOnceWith("accès complet requis sur cette page");
  expect(share.current.busy).toBeNull();
});

test("refused_revocation_is_said_when_the_page_is_gone", async () => {
  const share = await opened(access({ invitations: [pending("i1", "bruno@exemple.fr")] }));
  vi.mocked(revokeInvitation).mockRejectedValue(notFound());
  await settle(() => {
    void share.current.revoke("i1");
  });
  // « Introuvable » n'apprendrait rien : ce qui n'a pas été fait est dit.
  await settle(() => reads[1].reject(notFound()));
  expect(onAccessLost).toHaveBeenCalledExactlyOnceWith("Cette invitation n'a pas pu être révoquée.");
});

test("refused_revocation_of_an_invitation_that_still_waits_says_so", async () => {
  const waiting = access({ invitations: [pending("i1", "bruno@exemple.fr")] });
  const share = await opened(waiting);
  vi.mocked(revokeInvitation).mockRejectedValue(notFound());
  await settle(() => {
    void share.current.revoke("i1");
  });
  // La liste relue la porte encore : le menu ne laisse pas croire qu'elle est révoquée.
  await settle(() => reads[1].resolve(waiting));
  expect(share.current.access).toEqual(waiting);
  expect(share.current.error).toBe("Cette invitation n'a pas pu être révoquée.");
  expect(share.current.busy).toBeNull();
  expect(onAccessLost).not.toHaveBeenCalled();
});

test("older_answer_arriving_late_is_dropped", async () => {
  const share = await renderHook(hook, { open: true });
  // Une seconde lecture part avant la réponse de la première.
  await settle(() => {
    void share.current.reload();
  });
  expect(reads).toHaveLength(2);
  const latest = access({ general: "edit" });
  await settle(() => reads[1].resolve(latest));
  await settle(() => reads[0].resolve(access({ general: "read" })));
  expect(share.current.access).toEqual(latest);
});

test("one_change_at_a_time", async () => {
  const share = await opened(access({ entries: [entry("alice"), entry("bruno", { level: "read" })] }));
  const first = deferred<PageAccess | null>();
  vi.mocked(setUserLevel).mockReturnValue(first.promise);
  await settle(() => {
    void share.current.setLevel("bruno", "edit");
  });
  expect(share.current.busy).toBe("user:bruno");
  // Un second geste pendant que le premier est en route n'envoie rien.
  await settle(() => {
    void share.current.remove("bruno");
    void share.current.setGeneral("read");
  });
  expect(removeUserAccess).not.toHaveBeenCalled();
  expect(setGeneralAccess).not.toHaveBeenCalled();
  await settle(() => first.resolve(access()));
  expect(share.current.busy).toBeNull();
});

test("lost_session_says_nothing", async () => {
  const share = await opened();
  // La connexion est demandée par-dessus l'application : pas de message de plus.
  vi.mocked(setUserLevel).mockRejectedValue(unauthenticated());
  await settle(() => share.current.setLevel("alice", "edit"));
  expect(share.current.error).toBeNull();
  expect(share.current.busy).toBeNull();
  expect(reads).toHaveLength(1);
  expect(onAccessLost).not.toHaveBeenCalled();
});
