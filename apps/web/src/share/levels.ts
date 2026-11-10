// Ce que partagent les réglages de l'espace et le partage des pages : les libellés des rôles
// et des niveaux d'accès, et le lien d'une invitation.
//
// Le jeton d'une invitation voyage dans le fragment du lien (`/invite#<jeton>`) : le
// navigateur n'envoie jamais le fragment au serveur, ni dans la requête de la page ni dans
// l'en-tête `Referer`. La page d'invitation le lit, puis l'efface de l'adresse.
import type { Level, Role } from "@mementum/protocol";

/** Les rôles dans un espace, tels que l'interface les nomme. */
export const ROLE_LABELS: Record<Role, string> = {
  owner: "Propriétaire",
  admin: "Admin",
  member: "Membre",
  guest: "Invité",
};

/** Les niveaux d'accès à une page, du plus faible au plus fort. */
export const LEVEL_LABELS: Record<Level, string> = {
  read: "Peut lire",
  comment: "Peut commenter",
  edit: "Peut modifier",
  full: "Accès complet",
};

/** Un jeton d'invitation du serveur : 32 octets en base64url, sans remplissage. */
const TOKEN = /^#([A-Za-z0-9_-]{43})$/;

/** Le lien d'une invitation, sur cette instance : le jeton reste dans le fragment. */
export function invitationLink(token: string): string {
  return `${location.origin}/invite#${token}`;
}

/** Le jeton d'un fragment `#<jeton>` (`location.hash`), sinon `null`. */
export function invitationToken(hash: string): string | null {
  return TOKEN.exec(hash)?.[1] ?? null;
}
