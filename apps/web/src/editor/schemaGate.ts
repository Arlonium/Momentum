// WebSocket de synchronisation qui ne s'ouvre qu'après avoir relu la version de schéma du
// serveur (Ruling R35). y-websocket la crée à chaque connexion et à chaque reconnexion :
// un serveur mis à jour pendant que l'onglet est ouvert n'en reçoit donc jamais rien, ni
// étape de synchro ni frappe, puisque la vérification précède l'ouverture.

/** `true` : même schéma, on se connecte ; `false` : schéma différent, jamais de connexion. */
export type SchemaCheck = () => Promise<boolean>;

const CONNECTING = 0;
const OPEN = 1;
const CLOSING = 2;
const CLOSED = 3;

/**
 * Classe WebSocket pour l'option `WebSocketPolyfill` de y-websocket. `beforeMessage` voit
 * chaque message reçu juste avant y-websocket, dans la même tâche.
 */
export function schemaGatedWebSocket(
  check: SchemaCheck,
  beforeMessage?: (data: unknown) => void,
): typeof WebSocket {
  class SchemaGatedWebSocket {
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
        (sameSchema) => {
          // Fermée pendant la vérification, ou serveur d'un autre schéma : la session est
          // alors remplacée par une session en lecture seule, et cette socket reste close.
          if (this.state !== CONNECTING || !sameSchema) return;
          const socket = new WebSocket(url, protocols);
          socket.binaryType = this.binaryType;
          socket.onopen = (event) => this.onopen?.(event);
          socket.onmessage = (event) => {
            beforeMessage?.(event.data);
            this.onmessage?.(event);
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
  return SchemaGatedWebSocket as unknown as typeof WebSocket;
}
