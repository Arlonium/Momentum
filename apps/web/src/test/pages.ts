// Des pages et des espaces pour les tests unitaires.
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import { ApiError } from "../api";

/** Une page de l'espace `space`, à la racine de sa section « Espace », en accès complet. */
export function page(id: string, overrides: Partial<PageNode> = {}): PageNode {
  return {
    id,
    spaceId: "space",
    parentId: null,
    kind: "page",
    title: id.toUpperCase(),
    position: "a0",
    level: "full",
    section: "space",
    updatedAt: 0,
    trashedAt: null,
    ...overrides,
  };
}

export function space(id: string, overrides: Partial<SpaceInfo> = {}): SpaceInfo {
  return { id, name: id.toUpperCase(), role: "owner", ...overrides };
}

/** Les erreurs que le serveur rend, telles que `api.ts` les lève. */
export const notFound = () => new ApiError(404, "not_found", "page introuvable");
export const unauthenticated = () =>
  new ApiError(401, "unauthenticated", "session absente ou échue : se connecter");
export const unreachable = () => new ApiError(0, null, "Le serveur Mementum ne répond pas.");
