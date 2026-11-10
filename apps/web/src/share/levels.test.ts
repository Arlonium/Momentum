// Libellés des rôles et des niveaux, lien d'invitation (le jeton voyage dans le fragment,
// `/invite#<jeton>`, que le navigateur n'envoie jamais au serveur), et ce que le menu Partager
// offre pour l'accès général d'une page.
import { expect, test } from "vitest";
import {
  canEdit,
  generalAccess,
  invitationLink,
  invitationToken,
  LEVEL_LABELS,
  LEVELS,
  ROLE_LABELS,
} from "./levels";

/** Un jeton de la forme de ceux du serveur : 32 octets aléatoires en base64url, sans `=`. */
function serverToken(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(32));
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

test("invitation_link_roundtrip", () => {
  for (let i = 0; i < 50; i++) {
    const t = serverToken();
    const link = invitationLink(t);
    expect(invitationToken(new URL(link).hash)).toBe(t);
    // Sur cette instance, à l'adresse de la page d'invitation ; le jeton n'est que dans le
    // fragment, ni dans le chemin ni dans la requête.
    const url = new URL(link);
    expect(url.origin).toBe(location.origin);
    expect(url.pathname).toBe("/invite");
    expect(url.search).toBe("");
  }
  // Un jeton aux caractères propres au base64url (`-` et `_`) passe tel quel.
  const t = "-_" + "A".repeat(41);
  expect(invitationToken(new URL(invitationLink(t)).hash)).toBe(t);
});

test("invitation_token_ignores_other_fragments", () => {
  const t = serverToken();
  for (const hash of [
    "",
    "#",
    "#titre",
    "#b_3f9a",
    `#${t}&x=1`,
    `#${t}=`,
    `#${t.slice(1)}`,
    `#${t}A`,
    `#/${t}`,
    `#${t.slice(0, 42)}+`,
    `#${t.slice(0, 42)}/`,
    // Sans le `#` : ce n'est pas un fragment.
    t,
  ]) {
    expect(invitationToken(hash), hash).toBeNull();
  }
});

test("labels_cover_every_role_and_level", () => {
  expect(ROLE_LABELS).toEqual({
    owner: "Propriétaire",
    admin: "Admin",
    member: "Membre",
    guest: "Invité",
  });
  expect(LEVEL_LABELS).toEqual({
    read: "Peut lire",
    comment: "Peut commenter",
    edit: "Peut modifier",
    full: "Accès complet",
  });
});

test("levels_go_from_weakest_to_strongest", () => {
  expect(LEVELS).toEqual(["read", "comment", "edit", "full"]);
  // Chaque niveau a son libellé.
  expect(LEVELS.map((level) => LEVEL_LABELS[level])).toEqual([
    "Peut lire",
    "Peut commenter",
    "Peut modifier",
    "Accès complet",
  ]);
});

/** Les niveaux que l'accès général peut prendre, parmi ceux de `choices`. */
const allowed = (choices: ReturnType<typeof generalAccess>["levels"]) =>
  choices.filter((choice) => choice.allowed).map((choice) => choice.level);

test("general_access_of_a_page_without_a_parent_floor", () => {
  // Seulement les personnes nommées : tout se choisit.
  const closed = generalAccess({ general: null, inheritedGeneral: null });
  expect(closed).toMatchObject({ level: null, inherited: false, invitedOnly: true });
  expect(allowed(closed.levels)).toEqual(LEVELS);
  // Ouverte aux membres : on revient à « seulement les invités », ou on change de niveau.
  const open = generalAccess({ general: "read", inheritedGeneral: null });
  expect(open).toMatchObject({ level: "read", inherited: false, invitedOnly: true });
  expect(allowed(open.levels)).toEqual(LEVELS);
});

test("general_access_under_a_parent_floor", () => {
  // L'accès d'une page parente, tel quel : c'est le plancher (R93). Il ne se retire pas, et
  // ne se baisse pas.
  const floor = generalAccess({ general: "edit", inheritedGeneral: "edit" });
  expect(floor).toMatchObject({ level: "edit", inherited: true, invitedOnly: false });
  expect(allowed(floor.levels)).toEqual(["edit", "full"]);
  expect(floor.levels.map((choice) => choice.level)).toEqual(LEVELS);
  // Relevé sur la page : son accès à elle se retire (on revient au plancher), et ne descend
  // toujours pas dessous.
  const raised = generalAccess({ general: "full", inheritedGeneral: "edit" });
  expect(raised).toMatchObject({ level: "full", inherited: false, invitedOnly: true });
  expect(allowed(raised.levels)).toEqual(["edit", "full"]);
  // Le plancher le plus bas n'interdit aucun niveau, mais ne se retire pas.
  const lowest = generalAccess({ general: "read", inheritedGeneral: "read" });
  expect(lowest).toMatchObject({ level: "read", inherited: true, invitedOnly: false });
  expect(allowed(lowest.levels)).toEqual(LEVELS);
});

test("edit_and_full_can_edit", () => {
  // Lire et commenter ne permettent pas de modifier (au lot 2, commenter vaut lire).
  expect(canEdit("read")).toBe(false);
  expect(canEdit("comment")).toBe(false);
  expect(canEdit("edit")).toBe(true);
  expect(canEdit("full")).toBe(true);
});
