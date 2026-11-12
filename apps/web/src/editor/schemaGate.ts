// WebSocket de synchronisation qui ne s'ouvre qu'après une vérification : la version de
// schéma du serveur (Ruling R35) et, depuis les comptes, les droits sur la page
// (`page/usePageGate.ts`). y-websocket la crée à chaque connexion et à chaque reconnexion :
// un serveur mis à jour pendant que l'onglet est ouvert, ou une page dont les droits ont
// changé, n'en reçoit donc jamais rien, ni étape de synchro ni frappe, puisque la
// vérification précède l'ouverture.

/** `true` : même schéma, on se connecte ; `false` : schéma différent, jamais de connexion. */
export type SchemaCheck = () => Promise<boolean>;

/**
 * Ce qui est vérifié avant chaque connexion. `true` : on se connecte. `false` : jamais par
 * cette session, que l'application remplace ou ferme. Rejetée : le serveur ne répond pas,
 * la connexion échoue, et y-websocket réessaiera.
 */
export type ConnectionCheck = () => Promise<boolean>;

const CONNECTING = 0;
const OPEN = 1;
const CLOSING = 2;
const CLOSED = 3;

/**
 * Ce que `beforeMessage` décide d'un message reçu : `true`, il l'a pris et y-websocket ne le
 * voit pas ; `false`, il suit tel quel ; un `ArrayBuffer`, y-websocket reçoit cela à la place
 * (ce qui reste d'un message groupé dont la tête a été prise).
 */
export type MessageVerdict = boolean | ArrayBuffer;

/**
 * Classe WebSocket pour l'option `WebSocketPolyfill` de y-websocket. `beforeMessage` voit
 * chaque message reçu juste avant y-websocket, dans la même tâche, et peut le prendre.
 */
export function gatedWebSocket(
  check: ConnectionCheck,
  beforeMessage?: (data: unknown) => MessageVerdict,
): typeof WebSocket {
  class GatedWebSocket {
    static readonly CONNECTING = CONNECTING;
    static readonly OPEN = OPEN;
    static readonly CLOSING = CLOSING;
    static readonly CLOSED = CLOSED;
    readonly CONNECTING = CONNECTING;
    readonly OPEN = OPEN;
    readonly CLOSING = CLOSING;
    readonly CLOSED = CLOSED;

    onopen: ((event: Event) => void) | null = null;
    onmessage: ((event: MessageEvent) => void) | null = null;
    onerror: ((event: Event) => void) | null = null;
    onclose: ((event: CloseEvent) => void) | null = null;
    binaryType: BinaryType = "blob";

    private socket: WebSocket | null = null;
    private state = CONNECTING;

    constructor(url: string | URL, protocols?: string | string[]) {
      check().then(
        (connect) => {
          // Fermée pendant la vérification, ou vérification qui dit non (autre schéma, droit
          // perdu) : la session est alors remplacée par une session en lecture seule, ou
          // fermée, et cette socket reste close.
          if (this.state !== CONNECTING || !connect) return;
          const socket = new WebSocket(url, protocols);
          socket.binaryType = this.binaryType;
          socket.onopen = (event) => this.onopen?.(event);
          socket.onmessage = (event) => {
            const verdict = beforeMessage?.(event.data) ?? false;
            if (verdict === true) return;
            // Un `MessageEvent` n'est pas modifiable : y-websocket ne lit que `data`.
            this.onmessage?.(verdict === false ? event : ({ data: verdict } as MessageEvent));
          };
          socket.onerror = (event) => this.onerror?.(event);
          socket.onclose = (event) => {
            this.state = CLOSED;
            this.onclose?.(event);
          };
          this.socket = socket;
        },
        () => {
          if (this.state !== CONNECTING) return;
          // Serveur injoignable : comme une connexion refusée, y-websocket réessaiera.
          this.state = CLOSED;
          this.onerror?.(new Event("error"));
          this.onclose?.(new CloseEvent("close", { code: 1006 }));
        },
      );
    }

    get readyState(): number {
      return this.socket ? this.socket.readyState : this.state;
    }

    send(data: Parameters<WebSocket["send"]>[0]): void {
      if (!this.socket) throw new DOMException("WebSocket pas encore ouverte", "InvalidStateError");
      this.socket.send(data);
    }

    close(code?: number, reason?: string): void {
      if (this.socket) this.socket.close(code, reason);
      else this.state = CLOSED;
    }
  }
  return GatedWebSocket as unknown as typeof WebSocket;
}
