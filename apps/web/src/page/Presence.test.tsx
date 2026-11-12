// Les présents dans la barre du haut : ceux de la liste du serveur, hors soi, cinq au plus.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { RosterUser } from "../editor/collab";
import { colorOf } from "../session/session";
import "../test/hooks";
import { Presence } from "./Presence";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

/** Une session réduite à ce que le composant lit : la liste, et ses changements. */
function fakeSession(roster: RosterUser[]) {
  const listeners = new Set<() => void>();
  const session = {
    roster,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    set(next: RosterUser[]) {
      session.roster = next;
      for (const listener of listeners) listener();
    },
  };
  return session;
}

const user = (n: number, canWrite = true): RosterUser => ({
  id: `00000000-0000-0000-0000-00000000000${n}`,
  name: `Personne ${n}`,
  canWrite,
});

const avatars = () => [...host.querySelectorAll<HTMLElement>(".mm-avatar")];

test("the_account_itself_is_not_shown", async () => {
  const me = user(1);
  const session = fakeSession([me, user(2)]);
  await act(async () => root.render(<Presence session={session} selfId={me.id} />));
  expect(avatars().map((a) => a.getAttribute("aria-label"))).toEqual(["Personne 2"]);
});

test("nothing_is_drawn_when_nobody_else_is_there", async () => {
  const me = user(1);
  const session = fakeSession([me]);
  await act(async () => root.render(<Presence session={session} selfId={me.id} />));
  expect(host.innerHTML).toBe("");
});

test("an_avatar_has_the_initial_the_colour_and_the_name", async () => {
  const reader = { ...user(2, false), name: "élodie" };
  const session = fakeSession([reader]);
  await act(async () => root.render(<Presence session={session} selfId="autre" />));
  const [avatar] = avatars();
  expect(avatar.textContent).toBe("É");
  expect(avatar.getAttribute("aria-label")).toBe("élodie (lecture)");
  expect(avatar.getAttribute("title")).toBe("élodie (lecture)");
  // La couleur du compte, celle de son curseur (le navigateur la relit en `rgb(…)`).
  const probe = document.createElement("i");
  probe.style.backgroundColor = colorOf(reader.id);
  expect(probe.style.backgroundColor).not.toBe("");
  expect(avatar.style.backgroundColor).toBe(probe.style.backgroundColor);
});

test("five_at_most_then_the_count_of_the_others", async () => {
  const session = fakeSession([1, 2, 3, 4, 5, 6, 7, 8].map((n) => user(n)));
  await act(async () => root.render(<Presence session={session} selfId="autre" />));
  expect(avatars()).toHaveLength(6);
  const more = avatars()[5];
  expect(more.textContent).toBe("+3");
  expect(more.getAttribute("aria-label")).toBe("3 autres personnes");
  await act(async () => session.set([1, 2, 3, 4, 5, 6].map((n) => user(n))));
  expect(avatars()[5].textContent).toBe("+1");
  expect(avatars()[5].getAttribute("aria-label")).toBe("1 autre personne");
});

test("the_name_is_text_never_markup", async () => {
  const name = '<img src=x onerror="alert(1)">';
  const session = fakeSession([{ ...user(2), name }]);
  await act(async () => root.render(<Presence session={session} selfId="autre" />));
  expect(host.querySelector("img")).toBeNull();
  expect(avatars()[0].getAttribute("aria-label")).toBe(name);
});

test("the_list_follows_the_session", async () => {
  const session = fakeSession([]);
  await act(async () => root.render(<Presence session={session} selfId="autre" />));
  expect(avatars()).toHaveLength(0);
  await act(async () => session.set([user(2)]));
  expect(avatars()).toHaveLength(1);
  await act(async () => session.set([]));
  expect(avatars()).toHaveLength(0);
});
