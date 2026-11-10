// Libellés des rôles et des niveaux, et lien d'invitation : le jeton voyage dans le fragment
// (`/invite#<jeton>`), que le navigateur n'envoie jamais au serveur.
import { expect, test } from "vitest";
import { canEdit, invitationLink, invitationToken, LEVEL_LABELS, ROLE_LABELS } from "./levels";

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

test("edit_and_full_can_edit", () => {
  // Lire et commenter ne permettent pas de modifier (au lot 2, commenter vaut lire).
  expect(canEdit("read")).toBe(false);
  expect(canEdit("comment")).toBe(false);
  expect(canEdit("edit")).toBe(true);
  expect(canEdit("full")).toBe(true);
});
