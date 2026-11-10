// La session du client : le compte connecté, que `SessionGate` donne à l'application
// (`useSession`), et la couleur de présence d'un compte.
import type { Me } from "@mementum/protocol";
import { createContext, useContext } from "react";

/** Couleurs de présence (celles des avatars de la maquette « Papier »). */
export const PRESENCE_COLORS = ["#B4462B", "#2B59C3", "#23704F", "#6B4FBB", "#C27A12"] as const;

/**
 * Couleur de présence d'un compte (curseur, avatar) : tirée de son identifiant, donc la même
 * sur tous les onglets et à chaque chargement. FNV-1a sur 32 bits, puis la palette.
 */
export function colorOf(userId: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < userId.length; i++) {
    hash ^= userId.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return PRESENCE_COLORS[hash % PRESENCE_COLORS.length];
}

export type Session = {
  /** Le compte connecté et ses espaces. */
  me: Me;
  /**
   * Relit le compte (`GET /api/v1/me`). Session perdue : la connexion est demandée. Session
   * passée à un autre compte (dans un autre onglet) : l'onglet est rechargé (Ruling R115).
   */
  refresh(): Promise<void>;
};

export const SessionContext = createContext<Session | null>(null);

/** Le compte connecté. Seulement sous `SessionGate`, qui ne rend l'application qu'avec lui. */
export function useSession(): Session {
  const session = useContext(SessionContext);
  if (!session) throw new Error("useSession hors de SessionGate");
  return session;
}
