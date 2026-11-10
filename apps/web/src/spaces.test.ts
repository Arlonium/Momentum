// L'espace ouvert au chargement : le dernier ouvert, gardé dans le navigateur, sinon le
// premier du compte.
import type { SpaceInfo } from "@mementum/protocol";
import { afterEach, expect, test, vi } from "vitest";
import { LAST_SPACE_KEY, preferredSpace, readLastSpace, writeLastSpace } from "./spaces";

const perso: SpaceInfo = { id: "perso", name: "Perso", role: "owner" };
const pro: SpaceInfo = { id: "pro", name: "Pro", role: "member" };

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

test("last_space_wins_when_the_account_still_has_it", () => {
  expect(preferredSpace([perso, pro], "pro")).toBe(pro);
  expect(preferredSpace([perso, pro], "perso")).toBe(perso);
});

test("unknown_last_space_falls_back_to_the_first", () => {
  // Jamais ouvert d'espace, ou plus de rôle dans le dernier : le premier du compte.
  expect(preferredSpace([perso, pro], null)).toBe(perso);
  expect(preferredSpace([perso, pro], "quitté")).toBe(perso);
  expect(preferredSpace([], "pro")).toBeNull();
  expect(preferredSpace([], null)).toBeNull();
});

test("last_space_is_kept_in_local_storage", () => {
  expect(readLastSpace()).toBeNull();
  writeLastSpace("pro");
  expect(localStorage.getItem(LAST_SPACE_KEY)).toBe("pro");
  expect(LAST_SPACE_KEY).toBe("mementum.space");
  expect(readLastSpace()).toBe("pro");
});

test("blocked_storage_is_not_an_error", () => {
  // Stockage refusé par le navigateur (navigation privée, réglage) : on s'en passe.
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new DOMException("refusé", "SecurityError");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("refusé", "SecurityError");
  });
  expect(readLastSpace()).toBeNull();
  expect(() => writeLastSpace("pro")).not.toThrow();
});
