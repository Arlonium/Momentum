// Ce que partagent les réglages de l'espace et le partage des pages : les libellés des rôles
// et des niveaux d'accès, ce que l'accès général d'une page laisse choisir, et le lien d'une
// invitation.
//
// Le jeton d'une invitation voyage dans le fragment du lien (`/invite#<jeton>`) : le
// navigateur n'envoie jamais le fragment au serveur, ni dans la requête de la page ni dans
// l'en-tête `Referer`. La page d'invitation le lit, puis l'efface de l'adresse.
import type { Level, PageAccess, Role } from "@mementum/protocol";

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

/** Les niveaux, du plus faible au plus fort. */
export const LEVELS: readonly Level[] = ["read", "comment", "edit", "full"];

/**
 * Ce niveau permet-il de modifier la page : écrire, renommer, créer une sous-page, changer de
 * place parmi les mêmes frères, mettre à la corbeille et restaurer ? Le serveur fait foi.
 */
export function canEdit(level: Level): boolean {
  return level === "edit" || level === "full";
}

/** L'accès général d'une page, tel que le menu Partager le montre et le laisse régler. */
export type GeneralAccess = {
  /**
   * Le niveau qu'ont tous les membres de l'espace sur la page ; `null` : seulement les
   * personnes nommées (« Seulement les invités »).
   */
  level: Level | null;
  /** Ce niveau est celui d'une page parente, tel quel : la page n'a pas d'accès général à elle. */
  inherited: boolean;
  /**
   * « Seulement les invités » se choisit : la page retire son propre accès général. Sous
   * celui d'une page parente, elle y revient ; si elle n'en a pas à elle, il n'y a rien à
   * retirer, et le choix est grisé.
   */
  invitedOnly: boolean;
  /** Tous les niveaux, du plus faible au plus fort ; `allowed` : il se choisit ici. */
  levels: { level: Level; allowed: boolean }[];
};

/**
 * Ce que le menu Partager offre pour l'accès général. L'accès qu'une page parente donne aux
 * membres (`inheritedGeneral`) est un plancher (Ruling R93) : on le relève sur la sous-page,
 * on ne le baisse ni ne le retire (`409 inherited_access`). Le serveur fait foi.
 */
export function generalAccess({
  general,
  inheritedGeneral,
}: Pick<PageAccess, "general" | "inheritedGeneral">): GeneralAccess {
  const floor = inheritedGeneral === null ? 0 : LEVELS.indexOf(inheritedGeneral);
  const inherited = inheritedGeneral !== null && general === inheritedGeneral;
  return {
    level: general,
    inherited,
    invitedOnly: !inherited,
    levels: LEVELS.map((level, rank) => ({ level, allowed: rank >= floor })),
  };
}

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
