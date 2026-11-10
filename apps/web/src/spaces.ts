// L'espace ouvert au chargement. `/` mène au dernier espace ouvert, gardé dans le navigateur
// (`localStorage`), sinon au premier du compte. Ce n'est qu'une préférence d'affichage : le
// compte n'y a peut-être plus de rôle (on a pu l'en retirer, ou un autre compte s'est
// connecté dans ce navigateur), et c'est alors le premier espace qui s'ouvre.
import type { SpaceInfo } from "@mementum/protocol";

/** Clé du dernier espace ouvert, dans `localStorage`. */
export const LAST_SPACE_KEY = "mementum.space";

/** L'id du dernier espace ouvert dans ce navigateur, s'il a été gardé. */
export function readLastSpace(): string | null {
  try {
    return localStorage.getItem(LAST_SPACE_KEY);
  } catch {
    // stockage refusé par le navigateur : on s'en passe
    return null;
  }
}

/** Garde l'espace ouvert, pour la prochaine visite. */
export function writeLastSpace(spaceId: string): void {
  try {
    localStorage.setItem(LAST_SPACE_KEY, spaceId);
  } catch {
    // stockage refusé par le navigateur : l'accueil ouvrira le premier espace
  }
}

/**
 * L'espace à ouvrir : `last` si le compte y a encore un rôle, sinon son premier espace ;
 * `null` s'il n'en a aucun.
 */
export function preferredSpace(spaces: SpaceInfo[], last: string | null): SpaceInfo | null {
  return spaces.find((space) => space.id === last) ?? spaces[0] ?? null;
}
