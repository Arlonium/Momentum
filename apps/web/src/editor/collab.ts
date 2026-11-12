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

/** Un compte qui a la page ouverte, tel que le serveur le dit (trame `roster`, R136). */
export type RosterUser = { id: string; name: string; canWrite: boolean };

/** Ce que le serveur dit d'une page par sa connexion de synchro, hors liste des présents. */
export type PageEvent = "comments" | "versions";

/** Type des trames de page du serveur : y-websocket ne le connaît pas, la session le retire. */
const MESSAGE_PAGE_EVENT = 100;

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
  /**
   * Qui a la page ouverte, toi compris, lecteurs compris (R136). Remplacée à chaque trame
   * `roster`, jamais modifiée sur place ; vide tant que la connexion n'en a rien dit, et
   * vidée quand elle tombe : la liste d'une connexion coupée ne reste pas affichée, la
   * suivante en apporte une neuve.
   */
  roster: RosterUser[] = [];
  private readonly pageListeners = new Set<(event: PageEvent) => void>();
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
      // Rien ne passe d'un onglet à l'autre hors du serveur. Par défaut, y-websocket relie
      // par `BroadcastChannel` les onglets d'une même page : une frappe faite sans le droit
      // de modifier y passerait, puis reviendrait dans les documents neufs d'une session
      // rouverte, sans que `check` n'ait rien vu.
      disableBc: true,
      WebSocketPolyfill: gatedWebSocket(
        mode.check,
        (data) => this.beforeMessage(data),
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
      if (status === "connected" && !this.readOnly) {
        // y-websocket réannonce son état à l'ancienne horloge, et le serveur ignore ce renvoi
        // (il lui faut une horloge plus haute, R112) : sans cela, le curseur de qui ne tape pas
        // ne revient pas après une coupure. Rien à annoncer en lecture seule (état nul).
        const state = this.provider.awareness.getLocalState();
        if (state !== null) this.provider.awareness.setLocalState(state);
      }
      if (status === "disconnected" && this.roster.length > 0) this.roster = [];
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

  /** Appelle `listener` quand le serveur dit qu'il y a du neuf à relire par l'API. */
  onPageEvent(listener: (event: PageEvent) => void): () => void {
    this.pageListeners.add(listener);
    return () => this.pageListeners.delete(listener);
  }

  destroy(): void {
    this.listeners.clear();
    this.pageListeners.clear();
    this.serverStateListeners.clear();
    for (const cleanup of this.cleanups.splice(0).reverse()) cleanup();
  }

  private emit(): void {
    for (const listener of [...this.listeners]) listener();
  }

  /**
   * Message de la WebSocket, avant y-websocket : retire la trame de page du serveur, et
   * repère la première étape 2. Dans les deux modes : un lecteur voit les présents aussi.
   */
  private beforeMessage(data: unknown): boolean {
    if (!(data instanceof ArrayBuffer)) return false;
    const bytes = new Uint8Array(data);
    if (bytes[0] === MESSAGE_PAGE_EVENT) {
      // Le serveur écrit une trame par message WebSocket, et y-websocket ne lit que le premier
      // message d'un message WebSocket : un message qui commence par le type 100 est pris en
      // entier.
      this.takePageFrame(bytes);
      return true;
    }
    // L'état du serveur ne déverrouille qu'un éditeur qui écrit (`onServerState`).
    if (this.readOnly || this.synced || this.serverStateListeners.size === 0) return false;
    // Message « sync » (0), étape 2 (1) : deux entiers variables d'un octet chacun.
    if (bytes[0] !== 0 || bytes[1] !== 1) return false;
    const listeners = [...this.serverStateListeners];
    this.serverStateListeners.clear();
    for (const listener of listeners) listener();
    return false;
  }

  /**
   * Lit la trame de page en tête de `bytes` (type 100, longueur en entier variable, JSON en
   * UTF-8). Illisible, tronquée ou inconnue : rien n'est fait, sans erreur.
   */
  private takePageFrame(bytes: Uint8Array): void {
    let at = 1;
    let length = 0;
    for (let shift = 0; ; shift += 7) {
      if (at >= bytes.length || shift > 28) return;
      const byte = bytes[at++];
      length += (byte & 0x7f) * 2 ** shift;
      if (byte < 0x80) break;
    }
    if (at + length > bytes.length) return;
    try {
      const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(at, at + length));
      this.applyPageFrame(JSON.parse(text));
    } catch {
      // JSON ou UTF-8 illisible : la trame est retirée sans effet.
    }
  }

  private applyPageFrame(frame: unknown): void {
    if (typeof frame !== "object" || frame === null) return;
    const { type, users } = frame as { type?: unknown; users?: unknown };
    if (type === "roster") {
      if (!Array.isArray(users)) return;
      // Un compte une fois : la première ligne gagne (les identifiants sont des clés d'affichage).
      const seen = new Set<string>();
      this.roster = [];
      for (const user of users) {
        if (!isRosterUser(user) || seen.has(user.id)) continue;
        seen.add(user.id);
        this.roster.push({ id: user.id, name: user.name, canWrite: user.canWrite });
      }
      this.emit();
    } else if (type === "comments" || type === "versions") {
      for (const listener of [...this.pageListeners]) listener(type);
    }
  }
}

function isRosterUser(value: unknown): value is RosterUser {
  if (typeof value !== "object" || value === null) return false;
  const { id, name, canWrite } = value as Record<string, unknown>;
  return typeof id === "string" && typeof name === "string" && typeof canWrite === "boolean";
}

/** `ws(s)://<hôte de la page>/sync` : la WebSocket passe par le même hôte que l'API. */
function syncUrl(): string {
  return `${location.origin.replace(/^http/, "ws")}/sync`;
}
