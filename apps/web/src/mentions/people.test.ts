// Les personnes que le menu `@` propose (Task C4, R251, R617) : pour un membre, les membres et
// invités de l'espace, lus à l'ouverture du menu et gardés une minute ; pour un invité, ou si la
// lecture est refusée, les comptes qui ont la page ouverte.
import type { Member } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, getMembers } from "../api";
import type { RosterUser } from "../editor/collab";
import { deferred } from "../test/hooks";
import { forgetPeople, loadPeople, PEOPLE_KEPT_MS, PEOPLE_WAIT_MS } from "./people";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getMembers: vi.fn(),
}));

const members = vi.mocked(getMembers);

const member = (userId: string, name: string, role: Member["role"] = "member"): Member => ({
  userId,
  name,
  email: `${userId}@exemple.fr`,
  role,
});

const ROSTER: RosterUser[] = [
  { id: "u-mona", name: "Mona", canWrite: true },
  { id: "u-ines", name: "Inès", canWrite: false },
];

beforeEach(() => {
  vi.useFakeTimers();
  members.mockReset();
});
afterEach(() => {
  forgetPeople();
  vi.useRealTimers();
});

test("a member is offered the members and guests of the space, read once a minute", async () => {
  members.mockResolvedValue([member("u-mona", "Mona", "owner"), member("u-tom", "Tom"), member("u-ines", "Inès", "guest")]);
  // Rien n'est lu avant qu'on le demande.
  expect(members).not.toHaveBeenCalled();
  const people = await loadPeople("s1", "member", ROSTER);
  // Un id et un nom, rien d'autre (pas d'adresse).
  expect(people).toEqual([
    { id: "u-mona", name: "Mona" },
    { id: "u-tom", name: "Tom" },
    { id: "u-ines", name: "Inès" },
  ]);
  expect(members.mock.calls).toEqual([["s1"]]);
  // Chaque frappe du menu redemande : la liste gardée sert, une minute.
  vi.advanceTimersByTime(PEOPLE_KEPT_MS - 1);
  expect(await loadPeople("s1", "admin", [])).toEqual(people);
  expect(members).toHaveBeenCalledTimes(1);
  vi.advanceTimersByTime(1);
  members.mockResolvedValue([member("u-mona", "Mona", "owner")]);
  expect(await loadPeople("s1", "owner", [])).toEqual([{ id: "u-mona", name: "Mona" }]);
  expect(members).toHaveBeenCalledTimes(2);
});

test("two requests at once read the members once, and another space has its own", async () => {
  const answer = deferred<Member[]>();
  members.mockReturnValue(answer.promise);
  const first = loadPeople("s1", "member", []);
  const second = loadPeople("s1", "member", []);
  expect(members).toHaveBeenCalledTimes(1);
  answer.resolve([member("u-tom", "Tom")]);
  expect(await first).toEqual([{ id: "u-tom", name: "Tom" }]);
  expect(await second).toEqual([{ id: "u-tom", name: "Tom" }]);
  members.mockResolvedValue([member("u-zoe", "Zoé")]);
  expect(await loadPeople("s2", "member", [])).toEqual([{ id: "u-zoe", name: "Zoé" }]);
  expect(members.mock.calls).toEqual([["s1"], ["s2"]]);
  // La liste gardée est celle du dernier espace lu : revenir au premier le relit.
  members.mockResolvedValue([member("u-tom", "Tom")]);
  expect(await loadPeople("s1", "member", [])).toEqual([{ id: "u-tom", name: "Tom" }]);
  expect(members).toHaveBeenCalledTimes(3);
});

test("a_guest_is_offered_who_is_on_the_page_without_reading_the_members", async () => {
  members.mockResolvedValue([member("u-tom", "Tom")]);
  expect(await loadPeople("s1", "guest", ROSTER)).toEqual([
    { id: "u-mona", name: "Mona" },
    { id: "u-ines", name: "Inès" },
  ]);
  // La route des membres n'est pas même demandée pour un invité.
  expect(members).not.toHaveBeenCalled();
  expect(await loadPeople("s1", "guest", [])).toEqual([]);
  // Témoin : un membre, au même instant, la lit.
  expect(await loadPeople("s1", "member", ROSTER)).toEqual([{ id: "u-tom", name: "Tom" }]);
});

test("a refused reading falls back on who is on the page, and is asked again", async () => {
  members.mockRejectedValue(new ApiError(403, "forbidden", "Il faut être membre de l'espace."));
  expect(await loadPeople("s1", "member", ROSTER)).toEqual([
    { id: "u-mona", name: "Mona" },
    { id: "u-ines", name: "Inès" },
  ]);
  members.mockResolvedValue([member("u-tom", "Tom")]);
  expect(await loadPeople("s1", "member", ROSTER)).toEqual([{ id: "u-tom", name: "Tom" }]);
  expect(members).toHaveBeenCalledTimes(2);
});

test("a reading that does not answer leaves the menu with who is on the page", async () => {
  const answer = deferred<Member[]>();
  members.mockReturnValue(answer.promise);
  let got: unknown = null;
  void loadPeople("s1", "member", ROSTER).then((people) => (got = people));
  await vi.advanceTimersByTimeAsync(PEOPLE_WAIT_MS - 1);
  expect(got).toBeNull();
  await vi.advanceTimersByTimeAsync(1);
  expect(got).toEqual([
    { id: "u-mona", name: "Mona" },
    { id: "u-ines", name: "Inès" },
  ]);
  // La réponse arrivée, la frappe suivante a les membres, sans autre lecture.
  answer.resolve([member("u-tom", "Tom")]);
  await vi.advanceTimersByTimeAsync(0);
  expect(await loadPeople("s1", "member", ROSTER)).toEqual([{ id: "u-tom", name: "Tom" }]);
  expect(members).toHaveBeenCalledTimes(1);
});

test("forgetting the people reads them again", async () => {
  members.mockResolvedValue([member("u-tom", "Tom")]);
  await loadPeople("s1", "member", []);
  forgetPeople();
  await loadPeople("s1", "member", []);
  expect(members).toHaveBeenCalledTimes(2);
});
