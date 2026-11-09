// Outils partagés des tests de bout en bout. Les appels REST passent par le mandataire de
// Vite (`baseURL`), comme ceux de l'application.
import {
  expect,
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  type Page,
} from "@playwright/test";
import type { CreatePage, PageNode, Tree } from "@mementum/protocol";
import { readFileSync } from "node:fs";
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";
// Déclare `window.__mementum` pour les fonctions passées à `page.evaluate`.
import type {} from "../src/editor/debug";

/** Version du registre des blocs (`spec/schema/blocks.json`), celle du client et du serveur. */
export const REGISTRY_VERSION: number = JSON.parse(
  readFileSync(new URL("../../../spec/schema/blocks.json", import.meta.url), "utf8"),
).version;

/** Crée une page par l'API (`POST /api/v1/pages`). */
export async function createPage(request: APIRequestContext, body: CreatePage = {}): Promise<PageNode> {
  const res = await request.post("/api/v1/pages", { data: body });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as PageNode;
}

/** L'arbre des pages (`GET /api/v1/tree`). */
export async function getTree(request: APIRequestContext): Promise<Tree> {
  const res = await request.get("/api/v1/tree");
  expect(res.ok()).toBe(true);
  return (await res.json()) as Tree;
}

/** Le MMD de la page, tel que le serveur le relit. */
export async function getMmd(request: APIRequestContext, id: string): Promise<string> {
  const res = await request.get(`/api/v1/pages/${id}/mmd`);
  expect(res.ok(), await res.text()).toBe(true);
  return res.text();
}

/** Remplace le contenu de la page par du MMD. */
export async function putMmd(request: APIRequestContext, id: string, mmd: string): Promise<void> {
  const res = await request.put(`/api/v1/pages/${id}/mmd`, {
    data: mmd,
    headers: { "content-type": "text/markdown" },
  });
  expect(res.status(), await res.text()).toBe(204);
}

/** Attend la première synchronisation de l'éditeur (il est alors déverrouillé). */
export async function waitSynced(p: Page): Promise<void> {
  await p.waitForFunction(() => window.__mementum?.synced === true, null, { timeout: 30_000 });
}

const contexts: BrowserContext[] = [];

/** Un nouveau contexte de navigateur (un autre utilisateur), fermé par `closeContexts`. */
export async function newUser(browser: Browser): Promise<Page> {
  const context = await browser.newContext();
  contexts.push(context);
  return context.newPage();
}

/** Ferme les contextes ouverts par `newUser` et `openPage`. */
export async function closeContexts(): Promise<void> {
  await Promise.all(contexts.splice(0).map((c) => c.close()));
}

/** Ouvre la page `id` pour un nouvel utilisateur et attend la synchro. */
export async function openPage(browser: Browser, id: string): Promise<Page> {
  const p = await newUser(browser);
  await p.goto(`/p/${id}`);
  await waitSynced(p);
  return p;
}

/** Lit un entier variable de lib0 (7 bits par octet, poids faibles d'abord). */
function readVarUint(bytes: number[], at: number): [value: number, next: number] {
  let value = 0;
  let shift = 0;
  for (;;) {
    const byte = bytes[at++];
    value += (byte & 0x7f) * 2 ** shift;
    if (byte < 0x80) return [value, at];
    shift += 7;
  }
}

/**
 * Une trame y-websocket écrit-elle dans le document ? Messages « sync » (0) seulement :
 * une mise à jour (sous-type 2), ou une étape 2 (sous-type 1) dont la mise à jour n'est pas
 * vide (`[0, 0]` : ni structures ni suppressions). L'étape 1 et la présence n'écrivent rien.
 */
export function isWrite(bytes: number[]): boolean {
  if (bytes[0] !== 0) return false;
  if (bytes[1] === 2) return true;
  if (bytes[1] !== 1) return false;
  const [length, at] = readVarUint(bytes, 2);
  return !(length === 2 && bytes[at] === 0 && bytes[at + 1] === 0);
}

/**
 * Les trames d'écriture envoyées par les WebSocket de synchro que la page ouvre après cet
 * appel. Le serveur envoie son étape 1 dès la connexion : un document neuf y répond par une
 * étape 2 vide, qui n'est pas comptée.
 */
export function watchWrites(p: Page): number[][] {
  const writes: number[][] = [];
  p.on("websocket", (ws) => {
    if (!ws.url().includes("/sync/")) return;
    ws.on("framesent", ({ payload }) => {
      const bytes = [...(typeof payload === "string" ? Buffer.from(payload) : payload)];
      if (isWrite(bytes)) writes.push(bytes);
    });
  });
  return writes;
}

/**
 * Ajoute à la page, par un client Yjs nu (sans éditeur, au schéma courant), un bloc d'un
 * type que BlockNote ne connaît pas : `blockContainer > futureWidget`. C'est la forme d'un
 * bloc d'une version plus récente ; le serveur le relit `<block … foreign=true/>`.
 */
export async function injectForeignBlock(
  request: APIRequestContext,
  baseURL: string,
  id: string,
): Promise<void> {
  const doc = new Y.Doc();
  const provider = new WebsocketProvider(`${baseURL.replace(/^http/, "ws")}/sync`, id, doc, {
    params: { schema: String(REGISTRY_VERSION) },
    disableBc: true,
  });
  try {
    await expect.poll(() => provider.synced, { timeout: 10_000 }).toBe(true);
    const group = doc.getXmlFragment("document-store").get(0) as Y.XmlElement;
    const container = new Y.XmlElement("blockContainer");
    container.setAttribute("id", "b_future01");
    const widget = new Y.XmlElement("futureWidget");
    widget.setAttribute("a", "1");
    container.insert(0, [widget]);
    group.insert(group.length, [container]);
    await expect.poll(() => getMmd(request, id)).toContain('type="futureWidget"');
  } finally {
    provider.destroy();
    doc.destroy();
  }
}

/** Le fragment partagé, vu par Yjs (indépendant du rendu). */
export async function fragmentXml(p: Page): Promise<string> {
  return p.evaluate(() => window.__mementum!.doc.getXmlFragment("document-store").toString());
}

/** L'id de la page ouverte (`/p/:id`). */
export function pageIdOf(p: Page): string {
  const match = /\/p\/([^/?#]+)/.exec(new URL(p.url()).pathname);
  if (!match) throw new Error(`pas une page : ${p.url()}`);
  return decodeURIComponent(match[1]);
}

/** Centile `q` (entre 0 et 1), au plus proche rang. */
export function percentile(values: number[], q: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.round((sorted.length - 1) * q)];
}

/** Suffixe unique, pour des titres qui ne se confondent pas entre tests. */
export function unique(): string {
  return `${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;
}
