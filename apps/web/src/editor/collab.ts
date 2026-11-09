// Collaboration d'une page : un Y.Doc synchronisé avec le serveur par la WebSocket
// `/sync/{id}` (protocole y-websocket), et l'identité d'invité affichée près du curseur.
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";
import { CLIENT_SCHEMA_VERSION } from "./schema";

/** Nom du fragment partagé, celui que le serveur lit et écrit (`mementum-crdt`). */
export const FRAGMENT = "document-store";

/** Couleurs des curseurs (celles des avatars de la maquette « Papier »). */
const PALETTE = ["#B4462B", "#2B59C3", "#23704F", "#6B4FBB", "#C27A12"];

const GUEST_KEY = "mementum.guest";

export type Guest = { name: string; color: string };

/**
 * L'invité de ce navigateur : le lot 1 n'a pas de comptes. « Invité » suivi de 4 chiffres
 * tirés au premier lancement et gardés dans `localStorage` ; la couleur en découle.
 */
export function guest(): Guest {
  let digits: string | null = null;
  try {
    digits = localStorage.getItem(GUEST_KEY);
  } catch {
    // stockage refusé (navigation privée stricte) : un invité pour cette visite seulement
  }
  if (!digits || !/^\d{4}$/.test(digits)) {
    digits = String(Math.floor(Math.random() * 10_000)).padStart(4, "0");
    try {
      localStorage.setItem(GUEST_KEY, digits);
    } catch {
      // idem
    }
  }
  return { name: `Invité ${digits}`, color: PALETTE[Number(digits) % PALETTE.length] };
}

export type SyncStatus = "connecting" | "connected" | "disconnected";

/**
 * Une page ouverte. `doc` est le document lié à l'éditeur ; `provider` le synchronise.
 *
 * En lecture seule (schéma différent de celui du serveur), l'éditeur est lié à une copie
 * locale qui reçoit tout du serveur et ne lui renvoie rien : y-prosemirror supprime les
 * nœuds qu'il ne sait pas construire, et ces suppressions ne doivent jamais quitter
 * l'onglet (Review Focus 2). Le document du serveur reste `provider.doc`.
 */
export class CollabSession {
  readonly pageId: string;
  readonly readOnly: boolean;
  readonly provider: WebsocketProvider;
  readonly doc: Y.Doc;
  readonly user: Guest = guest();
  /** La première synchronisation est faite : la racine de la page est celle du serveur. */
  synced = false;
  status: SyncStatus = "connecting";
  private readonly listeners = new Set<() => void>();
  private readonly cleanups: (() => void)[] = [];

  constructor(pageId: string, readOnly: boolean) {
    this.pageId = pageId;
    this.readOnly = readOnly;
    const shared = new Y.Doc();
    // Sans `schema`, le serveur traiterait la connexion comme celle du schéma 0, en
    // lecture seule (R30) : on la donne toujours.
    this.provider = new WebsocketProvider(syncUrl(), pageId, shared, {
      params: { schema: String(CLIENT_SCHEMA_VERSION) },
    });
    if (readOnly) {
      const local = new Y.Doc();
      const mirror = (update: Uint8Array) => Y.applyUpdate(local, update, this.provider);
      shared.on("update", mirror);
      this.cleanups.push(() => {
        shared.off("update", mirror);
        local.destroy();
      });
      this.doc = local;
    } else {
      this.doc = shared;
    }
    this.cleanups.push(() => {
      this.provider.destroy();
      shared.destroy();
    });

    const onSync = (isSynced: boolean) => {
      if (isSynced && !this.synced) {
        this.synced = true;
        this.emit();
      }
    };
    const onStatus = ({ status }: { status: SyncStatus }) => {
      this.status = status;
      this.emit();
    };
    this.provider.on("sync", onSync);
    this.provider.on("status", onStatus);
    if (this.provider.synced) onSync(true);
  }

  /** Appelle `listener` à chaque changement de `synced` ou de `status`. */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  destroy(): void {
    this.listeners.clear();
    for (const cleanup of this.cleanups.splice(0).reverse()) cleanup();
  }

  private emit(): void {
    for (const listener of [...this.listeners]) listener();
  }
}

/** `ws(s)://<hôte de la page>/sync` : la WebSocket passe par le même hôte que l'API. */
function syncUrl(): string {
  return `${location.origin.replace(/^http/, "ws")}/sync`;
}
