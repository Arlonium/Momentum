// Les actions sur les membres que les réglages proposent, rôle par rôle (tableau des rôles).
import type { Member, Role } from "@mementum/protocol";
import { expect, test } from "vitest";
import { isManager, mayRemove, rolesFor } from "./roles";

const SELF = "moi";

/** Un membre de chaque rôle, plus la personne connectée. */
function member(role: Role, userId: string = role): Member {
  return { userId, name: userId, email: `${userId}@exemple.fr`, role };
}
const OTHERS: Member[] = [member("owner"), member("admin"), member("member"), member("guest")];

test("owner_changes_and_removes_everyone_but_self", () => {
  for (const target of OTHERS) {
    expect(rolesFor("owner", target, SELF), target.role).toEqual(["owner", "admin", "member", "guest"]);
    expect(mayRemove("owner", target, SELF), target.role).toBe(true);
  }
  expect(rolesFor("owner", member("owner", SELF), SELF)).toBeNull();
  expect(mayRemove("owner", member("owner", SELF), SELF)).toBe(false);
});

test("admin_view_leaves_owners_and_guests_alone", () => {
  // Une liste de rôles sur les lignes `admin` et `member` seulement, entre ces deux rôles.
  expect(rolesFor("admin", member("owner"), SELF)).toBeNull();
  expect(rolesFor("admin", member("guest"), SELF)).toBeNull();
  expect(rolesFor("admin", member("admin"), SELF)).toEqual(["admin", "member"]);
  expect(rolesFor("admin", member("member"), SELF)).toEqual(["admin", "member"]);
  // « Retirer » partout, sauf sur un propriétaire.
  expect(mayRemove("admin", member("owner"), SELF)).toBe(false);
  expect(mayRemove("admin", member("admin"), SELF)).toBe(true);
  expect(mayRemove("admin", member("member"), SELF)).toBe(true);
  expect(mayRemove("admin", member("guest"), SELF)).toBe(true);
  // Ni sur sa propre ligne.
  expect(rolesFor("admin", member("admin", SELF), SELF)).toBeNull();
  expect(mayRemove("admin", member("admin", SELF), SELF)).toBe(false);
});

test("member_and_guest_have_no_action", () => {
  for (const caller of ["member", "guest", undefined] as const) {
    for (const target of OTHERS) {
      expect(rolesFor(caller, target, SELF), `${caller} → ${target.role}`).toBeNull();
      expect(mayRemove(caller, target, SELF), `${caller} → ${target.role}`).toBe(false);
    }
  }
  expect([isManager("owner"), isManager("admin")]).toEqual([true, true]);
  expect([isManager("member"), isManager("guest"), isManager(undefined)]).toEqual([
    false,
    false,
    false,
  ]);
});
