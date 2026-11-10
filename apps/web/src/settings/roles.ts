// Ce que la personne connectée peut faire aux autres membres d'un espace, d'après son rôle :
// la traduction côté client du tableau des rôles (le serveur fait foi, et refuse le reste).
// Les réglages s'en servent pour n'afficher que les actions permises.
import type { Member, Role } from "@mementum/protocol";

const ALL_ROLES: Role[] = ["owner", "admin", "member", "guest"];

/** Propriétaire ou admin : renomme l'espace, invite, révoque, retire. */
export function isManager(role: Role | undefined): boolean {
  return role === "owner" || role === "admin";
}

/**
 * Les rôles que `caller` peut donner à `target`, ou `null` s'il ne peut pas changer le sien :
 * un propriétaire donne tous les rôles ; un admin fait passer d'`admin` à `member` et retour,
 * sans toucher à un propriétaire ni à un invité. Son propre rôle ne se change pas ici
 * (`self` : l'id de la personne connectée).
 */
export function rolesFor(caller: Role | undefined, target: Member, self: string): Role[] | null {
  if (target.userId === self) return null;
  if (caller === "owner") return ALL_ROLES;
  if (caller === "admin" && (target.role === "admin" || target.role === "member")) {
    return ["admin", "member"];
  }
  return null;
}

/**
 * `caller` peut-il retirer `target` de l'espace ? Un propriétaire, tout le monde ; un admin,
 * qui n'est pas propriétaire. Soi-même, c'est « Quitter l'espace ».
 */
export function mayRemove(caller: Role | undefined, target: Member, self: string): boolean {
  if (target.userId === self) return false;
  return caller === "owner" || (caller === "admin" && target.role !== "owner");
}
