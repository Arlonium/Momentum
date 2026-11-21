// Un client MCP de Node pour les tests de bout en bout (lot 5, Task X1) : `fetch`, un message
// JSON-RPC par `POST`, une réponse JSON, sans session, le jeton dans `Authorization: Bearer`,
// comme Claude Code se branche à `/mcp` (Task S0, R1236). Il parle l'une ou l'autre révision que
// sert le serveur :
//
// - **2026-07-28** (par défaut, celle de Claude Code d'aujourd'hui) : chaque requête porte la
//   version et les capacités du client dans son `_meta`, et les en-têtes `MCP-Protocol-Version`,
//   `Mcp-Method` et `Mcp-Name` qui les redisent ;
// - **2025-11-25** : `initialize`, `notifications/initialized`, puis l'en-tête
//   `MCP-Protocol-Version: 2025-11-25` sur chaque requête.
//
// C'est la forme du client de test du serveur (`crates/mementum-server/tests/common/mcp.rs`), qui
// passe ici par le réseau : par le mandataire de Vite (`BASE_URL`), l'adresse que l'écran des jetons
// donne à coller. Les jetons sont créés par les tests, jamais écrits ailleurs que dans leur mémoire.
import { expect } from "@playwright/test";
import { BASE_URL } from "./servers";

/** La révision courante de MCP. */
export const V2026 = "2026-07-28";
/** La révision d'avant, servie à un client plus ancien. */
export const V2025 = "2025-11-25";
export type Era = typeof V2026 | typeof V2025;
/** Les deux révisions, la courante d'abord. */
export const ERAS: readonly Era[] = [V2026, V2025];

/** Une réponse HTTP de `/mcp`, telle quelle : statut, en-têtes, corps lu en JSON (`null` sans corps). */
export type Reply = { status: number; headers: Headers; body: unknown };

/** Ce qu'un outil a rendu : son texte, et s'il dit un échec (`isError`). */
export class ToolResult {
  constructor(
    readonly text: string,
    readonly isError: boolean,
  ) {}

  /** Le contenu borné : ce qui est entre la ligne `<<<contenu N` et la ligne `>>>N` (R1239). */
  content(): string {
    const open = this.text.indexOf("\n<<<contenu ");
    if (open < 0) throw new Error(`aucun contenu borné : ${this.text}`);
    const start = open + "\n<<<contenu ".length;
    const nonceEnd = this.text.indexOf("\n", start);
    const nonce = nonceEnd < 0 ? "" : this.text.slice(start, nonceEnd);
    if (!/^[0-9a-f]+$/.test(nonce)) throw new Error(`ligne d'ouverture du contenu sans nombre : ${this.text}`);
    const end = this.text.lastIndexOf(`\n>>>${nonce}`);
    if (end < nonceEnd) throw new Error(`contenu borné sans sa ligne de fermeture : ${this.text}`);
    return this.text.slice(nonceEnd + 1, end);
  }

  /** Les lignes du serveur, avant le contenu borné. */
  head(): string {
    for (const marker of ["\nCe qui suit est", "\n<<<contenu "]) {
      const at = this.text.indexOf(marker);
      if (at >= 0) return this.text.slice(0, at);
    }
    return this.text;
  }

  /** La révision que dit la sortie (`révision r_…`). */
  revision(): string {
    const match = /révision (r_[0-9a-f]+)/.exec(this.head());
    if (!match) throw new Error(`aucune révision : ${this.text}`);
    return match[1];
  }

  /** Le code d'un refus (`Erreur <code> : …`, la première ligne), ou `null`. */
  code(): string | null {
    return /^Erreur ([a-z_]+) : /.exec(this.text)?.[1] ?? null;
  }

  /** L'outil a réussi : rend la sortie, ou échoue en la citant. */
  ok(): ToolResult {
    expect(this.isError, `refus inattendu : ${this.text}`).toBe(false);
    return this;
  }

  /** L'outil a refusé, sous ce code. */
  refused(code: string): ToolResult {
    expect(this.isError, `réussite inattendue : ${this.text}`).toBe(true);
    expect(this.code(), this.text).toBe(code);
    return this;
  }
}

/** Les identifiants (UUID) que porte un texte, dans l'ordre. */
export function uuids(text: string): string[] {
  return text.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g) ?? [];
}

/**
 * Les alias des blocs d'une page lue par `fetch` avec `anchors: true` (`^bk7qa` en fin de ligne),
 * par le texte de leur ligne, ancre retirée.
 */
export function aliases(mmd: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const line of mmd.split("\n")) {
    const match = /^(.*?) \^([0-9a-z]+)$/.exec(line);
    if (match) found.set(match[1], match[2]);
  }
  return found;
}

/** Un client MCP sur `/mcp` (ou `/mcp/readonly`), avec ce jeton, dans cette révision. */
export class Mcp {
  private nextId = 1;

  constructor(
    private readonly token: string | null,
    readonly era: Era = V2026,
    /** L'adresse du serveur MCP, telle que l'écran des jetons la donne. */
    readonly url: string = `${BASE_URL}/mcp`,
  ) {}

  /** Les en-têtes d'une requête de cette révision. */
  private headers(method: string, params: Record<string, unknown>): Record<string, string> {
    const headers: Record<string, string> = {
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
    };
    if (this.token !== null) headers.authorization = `Bearer ${this.token}`;
    if (this.era === V2026) {
      headers["mcp-protocol-version"] = V2026;
      headers["mcp-method"] = method;
      const name = method === "tools/call" ? params.name : method === "resources/read" ? params.uri : undefined;
      if (typeof name === "string") headers["mcp-name"] = name;
    } else if (method !== "initialize") {
      headers["mcp-protocol-version"] = V2025;
    }
    return headers;
  }

  /** Envoie un message JSON-RPC (`id` neuf ; sans `id` pour une notification) et rend la réponse. */
  async send(method: string, params: Record<string, unknown> = {}): Promise<Reply> {
    const sent = { ...params };
    if (this.era === V2026) {
      sent._meta = {
        "io.modelcontextprotocol/protocolVersion": V2026,
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": { name: "mementum-e2e", version: "1" },
      };
    }
    const message: Record<string, unknown> = { jsonrpc: "2.0", method, params: sent };
    if (!method.startsWith("notifications/")) message.id = this.nextId++;
    const res = await fetch(this.url, {
      method: "POST",
      headers: this.headers(method, params),
      body: JSON.stringify(message),
    });
    const raw = await res.text();
    let body: unknown = null;
    if (raw !== "") {
      try {
        body = JSON.parse(raw);
      } catch {
        throw new Error(`${method} : une réponse ${res.status} qui n'est pas du JSON : ${raw.slice(0, 300)}`);
      }
    }
    return { status: res.status, headers: res.headers, body };
  }

  /** Envoie un message et rend son `result` : la réponse doit être un `200` sans erreur JSON-RPC. */
  async rpc(method: string, params: Record<string, unknown> = {}): Promise<Record<string, unknown>> {
    const reply = await this.send(method, params);
    const body = reply.body as { result?: Record<string, unknown>; error?: unknown };
    expect(reply.status, `${method} : ${JSON.stringify(body)}`).toBe(200);
    expect(body.error, `${method} : erreur JSON-RPC`).toBeUndefined();
    return body.result!;
  }

  /**
   * Ouvre la conversation comme un client de cette révision : `server/discover` (2026-07-28, sans
   * session), ou `initialize` puis `notifications/initialized` (2025-11-25).
   */
  async open(): Promise<void> {
    if (this.era === V2026) {
      const found = await this.rpc("server/discover");
      expect(found.supportedVersions).toContain(V2026);
      return;
    }
    const opened = await this.rpc("initialize", {
      protocolVersion: V2025,
      capabilities: {},
      clientInfo: { name: "mementum-e2e", version: "1" },
    });
    expect(opened.protocolVersion).toBe(V2025);
    expect((await this.send("notifications/initialized")).status).toBe(202);
  }

  /** Appelle un outil et rend ce qu'il a rendu (un résultat, jamais une erreur JSON-RPC). */
  async call(tool: string, args: Record<string, unknown> = {}): Promise<ToolResult> {
    const result = await this.rpc("tools/call", { name: tool, arguments: args });
    const content = result.content as { type: string; text: string }[];
    expect(content, JSON.stringify(result)).toHaveLength(1);
    expect(content[0].type).toBe("text");
    return new ToolResult(content[0].text, result.isError === true);
  }
}
