// Collaboration d'une page : un Y.Doc synchronisé avec le serveur par la WebSocket
// `/sync/{id}` (protocole y-websocket), et le compte affiché près du curseur.
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";
import { gatedWebSocket, type ConnectionCheck } from "./schemaGate";
import { CLIENT_SCHEMA_VERSION } from "./version";

/** Nom du fragment partagé, celui que le serveur lit et écrit (`mementum-crdt`). */
export const FRAGMENT = "document-store";

/**
 * Ce que l'onglet annonce aux autres présents (curseur, présence) : le nom du compte et sa
 * couleur (`colorOf` de `session/session.ts`).
 */
export type PresenceUser = { name: string; color: string };

export type SyncStatus = "connecting" | "connected" | "disconnected";

/**
 * Mode d'une session. En lecture seule, l'onglet ne fait que recevoir. Dans les deux modes,
 * chaque connexion n'ouvre sa WebSocket qu'après `check` : la version de schéma relue (R35)
 * pour qui écrit, et la page relue, avec ce que le compte peut encore y faire
 * (`page/usePageGate.ts`).
 */
export type SessionMode = { readOnly: boolean; check: ConnectionCheck };

/**
 * Une page ouverte. `doc` est le document lié à l'éditeur ; `provider` le synchronise.
 *
 * En lecture seule (schéma différent de celui du serveur, niveau qui ne permet pas de
 * modifier, page à la corbeille), l'éditeur est lié à une copie locale qui reçoit tout du
 * serveur et ne lui renvoie rien : y-prosemirror supprime les nœuds qu'il ne sait pas
 * construire, et ces suppressions ne doivent jamais quitter l'onglet (Review Focus 2). Le
 * document du serveur reste `provider.doc`. Les deux `Y.Doc` sont neufs : une session qui
 * passe en lecture seule n'en réutilise aucun, car l'ancien document porte déjà des
 * éditions et des suppressions locales.
 */
export class CollabSession {
  readonly pageId: string;
  readonly readOnly: boolean;
  readonly provider: WebsocketProvider;
  readonly doc: Y.Doc;
  /** Annoncé aux autres présents ; rien en lecture seule. */
  readonly user: PresenceUser;
  /** La première synchronisation est faite : la racine de la page est celle du serveur. */
  synced = false;
  status: SyncStatus = "connecting";
  private readonly listeners = new Set<() => void>();
  private readonly serverStateListeners = new Set<() => void>();
  private readonly cleanups: (() => void)[] = [];

  constructor(pageId: string, mode: SessionMode, user: PresenceUser) {
    this.pageId = pageId;
    this.readOnly = mode.readOnly;
    this.user = user;
    const shared = new Y.Doc();
    // Sans `schema`, le serveur traiterait la connexion comme celle du schéma 0, en
    // lecture seule (R30) : on la donne toujours.
    this.provider = new WebsocketProvider(syncUrl(), pageId, shared, {
      params: { schema: String(CLIENT_SCHEMA_VERSION) },
      WebSocketPolyfill: gatedWebSocket(
        mode.check,
        // L'état du serveur ne déverrouille qu'un éditeur qui écrit (`onServerState`).
        mode.readOnly ? undefined : (data) => this.beforeMessage(data),
      ),
    });
    if (mode.readOnly) {
      // Ni curseur ni présence : rien de cet onglet n'est annoncé.
      this.provider.awareness.setLocalState(null);
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

  /**
   * Appelle `listener` une fois, quand l'état du serveur arrive (première étape 2 de la
   * synchro) : dans la tâche même où y-websocket va l'appliquer, juste avant, si bien
   * qu'aucune frappe ne peut se glisser entre les deux. Session modifiable seulement.
   */
  onServerState(listener: () => void): () => void {
    if (this.synced) return () => {};
    this.serverStateListeners.add(listener);
    return () => this.serverStateListeners.delete(listener);
  }

  destroy(): void {
    this.listeners.clear();
    this.serverStateListeners.clear();
    for (const cleanup of this.cleanups.splice(0).reverse()) cleanup();
  }

  private emit(): void {
    for (const listener of [...this.listeners]) listener();
  }

  /** Message de la WebSocket, avant y-websocket : repère la première étape 2. */
  private beforeMessage(data: unknown): void {
    if (this.synced || this.serverStateListeners.size === 0 || !(data instanceof ArrayBuffer)) {
      return;
    }
    // Message « sync » (0), étape 2 (1) : deux entiers variables d'un octet chacun.
    const head = new Uint8Array(data, 0, Math.min(2, data.byteLength));
    if (head[0] !== 0 || head[1] !== 1) return;
    const listeners = [...this.serverStateListeners];
    this.serverStateListeners.clear();
    for (const listener of listeners) listener();
  }
}

/** `ws(s)://<hôte de la page>/sync` : la WebSocket passe par le même hôte que l'API. */
function syncUrl(): string {
  return `${location.origin.replace(/^http/, "ws")}/sync`;
}
