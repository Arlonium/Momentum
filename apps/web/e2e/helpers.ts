// Outils partagés des tests de bout en bout. Les appels REST passent par le mandataire de
// Vite (`baseURL`), comme ceux de l'application.
//
// Comptes : `global-setup.ts` crée le compte par défaut, dont la session (`storageState`)
// est celle de tous les contextes des tests (configurations de Playwright). `newUser` et
// `apiAs` prennent ce compte, ou un autre, créé par `signUpAccount`. Chaque compte a son
// espace (`firstSpace`), où d'autres le rejoignent par une invitation (`joinSpace`).
import {
  expect,
  request,
  type APIRequestContext,
  type Browser,
  type BrowserContext,
  type BrowserContextOptions,
  type Locator,
  type Page,
} from "@playwright/test";
import type {
  Accepted,
  CreatePage,
  InvitationCreated,
  InviteRole,
  InviteToSpace,
  Level,
  Me,
  Member,
  PageNode,
  PendingInvitation,
  SignUp,
  SpaceInfo,
  Tree,
} from "@mementum/protocol";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";
// Déclare `window.__mementum` pour les fonctions passées à `page.evaluate`.
import type {} from "../src/editor/debug";
import { accountsDir, BASE_URL } from "./servers";

/** Version du registre des blocs (`spec/schema/blocks.json`), celle du client et du serveur. */
export const REGISTRY_VERSION: number = JSON.parse(
  readFileSync(new URL("../../../spec/schema/blocks.json", import.meta.url), "utf8"),
).version;

/**
 * Crée une page par l'API (`POST /api/v1/pages`), avec la session de `request`. Une
 * sous-page (`parentId`) hérite de sa page parente. À la racine, sans autre précision : dans
 * le premier espace du compte, et dans la section « Espace », que voient tous ses membres.
 */
export async function createPage(request: APIRequestContext, body: CreatePage = {}): Promise<PageNode> {
  const data: CreatePage =
    body.parentId != null
      ? body
      : { section: "space", ...body, spaceId: body.spaceId ?? (await firstSpaceId(request)) };
  const res = await request.post("/api/v1/pages", { data });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as PageNode;
}

/**
 * Les pages d'un espace que voit le compte de `request` (`GET /api/v1/spaces/{id}/tree`) ;
 * sans `spaceId`, son premier espace.
 */
export async function getTree(request: APIRequestContext, spaceId?: string): Promise<Tree> {
  const res = await request.get(`/api/v1/spaces/${spaceId ?? (await firstSpaceId(request))}/tree`);
  expect(res.status(), await res.text()).toBe(200);
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

/**
 * Attend la première synchronisation de l'éditeur (il est alors déverrouillé, hors lecture
 * seule). Lit `data-synced` sur le conteneur de l'éditeur (`.mm-editor`), posé en
 * développement comme en production, à la différence de `window.__mementum`.
 */
export async function waitSynced(p: Page): Promise<void> {
  await p.waitForFunction(
    () => document.querySelector(".mm-editor")?.getAttribute("data-synced") === "true",
    null,
    { timeout: 30_000 },
  );
}

/** Le bandeau d'un onglet dont le schéma n'est plus celui du serveur (lecture seule, R35). */
export const READ_ONLY_BANNER =
  "Une nouvelle version de Mementum est disponible. Recharge la page pour continuer à écrire.";

/** `/api/v1/info` annonce désormais un schéma plus récent que celui du client. */
export async function serverSchemaBumped(p: Page): Promise<void> {
  await p.route("**/api/v1/info", async (route) => {
    const response = await route.fetch();
    const info = (await response.json()) as { schemaVersion: number };
    await route.fulfill({ response, json: { ...info, schemaVersion: info.schemaVersion + 1 } });
  });
}

/** Mot de passe des comptes de test : une constante manifestement fausse. */
export const PASSWORD = "mot-de-passe-de-test";

/** Un compte de test, et le fichier de sa session (`storageState` de Playwright). */
export type Account = { email: string; name: string; password: string; storageState: string };

/** Aucun cookie : un navigateur ou un client sans session. */
export const NO_SESSION = { cookies: [], origins: [] };

/**
 * Crée le compte par l'API (`POST /api/v1/auth/signup`, inscriptions ouvertes) et
 * enregistre sa session dans `storageState`. Sert aussi à `global-setup.ts`.
 */
export async function createAccount(
  body: SignUp,
  storageState: string,
  baseURL: string = BASE_URL,
): Promise<Account> {
  const api = await request.newContext({ baseURL, storageState: NO_SESSION });
  try {
    const res = await api.post("/api/v1/auth/signup", { data: body });
    if (res.status() !== 201) {
      throw new Error(`inscription de ${body.email} refusée : ${res.status()} ${await res.text()}`);
    }
    await api.storageState({ path: storageState });
  } finally {
    await api.dispose();
  }
  return { email: body.email, name: body.name, password: body.password, storageState };
}

/**
 * Un compte neuf, créé par l'API, avec sa session. Sans `invitation`, il a son propre espace ;
 * avec le jeton d'une invitation, il n'a que l'espace où elle l'invite.
 */
export async function signUpAccount(name?: string, invitation?: string): Promise<Account> {
  const tag = unique();
  return createAccount(
    {
      email: `compte-${tag}@exemple.fr`,
      name: name ?? `Compte ${tag}`,
      password: PASSWORD,
      ...(invitation === undefined ? {} : { invitation }),
    },
    join(accountsDir(), `${tag}.storage.json`),
  );
}

/** Où `global-setup.ts` décrit le compte par défaut. */
export function defaultAccountFile(): string {
  return join(accountsDir(), "default.json");
}

let defaultAccountCache: Account | null = null;

/** Le compte par défaut, créé par `global-setup.ts` : celui de tous les tests. */
export function defaultAccount(): Account {
  defaultAccountCache ??= JSON.parse(readFileSync(defaultAccountFile(), "utf8")) as Account;
  return defaultAccountCache;
}

const contexts: BrowserContext[] = [];
const apis: APIRequestContext[] = [];

/**
 * Un nouveau contexte de navigateur (un autre onglet, une autre machine), avec la session
 * de `account` (par défaut, le compte du `globalSetup`), fermé par `closeContexts`.
 * `options` : ce que ce navigateur a de particulier (un écran tactile).
 */
export async function newUser(
  browser: Browser,
  account: Account = defaultAccount(),
  options: BrowserContextOptions = {},
): Promise<Page> {
  const context = await browser.newContext({ ...options, storageState: account.storageState });
  contexts.push(context);
  return context.newPage();
}

/**
 * Un client de l'API avec la session de `account` (par défaut, le compte du `globalSetup`),
 * fermé par `closeContexts`.
 */
export async function apiAs(account: Account = defaultAccount()): Promise<APIRequestContext> {
  const api = await request.newContext({ baseURL: BASE_URL, storageState: account.storageState });
  apis.push(api);
  return api;
}

/** Ferme les contextes ouverts par `newUser`, `openPage` et `apiAs`. */
export async function closeContexts(): Promise<void> {
  await Promise.all([
    ...contexts.splice(0).map((c) => c.close()),
    ...apis.splice(0).map((a) => a.dispose()),
  ]);
}

/** Le compte de la session de `api` et ses espaces (`GET /api/v1/me`). */
export async function getMe(api: APIRequestContext): Promise<Me> {
  const res = await api.get("/api/v1/me");
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as Me;
}

/** Le premier espace du compte de `api` : celui que son inscription a créé. */
export async function firstSpace(api: APIRequestContext): Promise<SpaceInfo> {
  const { spaces } = await getMe(api);
  expect(spaces.length, "le compte n'a aucun espace").toBeGreaterThan(0);
  return spaces[0];
}

const firstSpaceIds = new WeakMap<APIRequestContext, string>();

/** L'id du premier espace du compte de `api`, lu une fois par client. */
async function firstSpaceId(api: APIRequestContext): Promise<string> {
  let id = firstSpaceIds.get(api);
  if (id === undefined) {
    id = (await firstSpace(api)).id;
    firstSpaceIds.set(api, id);
  }
  return id;
}

/** Crée un espace par l'API (`POST /api/v1/spaces`) : le compte de `api` en est propriétaire. */
export async function createSpace(api: APIRequestContext, name: string): Promise<SpaceInfo> {
  const res = await api.post("/api/v1/spaces", { data: { name } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as SpaceInfo;
}

/** Les personnes qui ont un rôle dans l'espace (`GET /api/v1/spaces/{id}/members`). */
export async function getMembers(api: APIRequestContext, spaceId: string): Promise<Member[]> {
  const res = await api.get(`/api/v1/spaces/${spaceId}/members`);
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as Member[];
}

/** Invite dans l'espace (`POST /api/v1/spaces/{id}/invitations`) : l'id et le jeton, montré une fois. */
export async function inviteToSpace(
  api: APIRequestContext,
  spaceId: string,
  body: InviteToSpace,
): Promise<InvitationCreated> {
  const res = await api.post(`/api/v1/spaces/${spaceId}/invitations`, { data: body });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as InvitationCreated;
}

/** Les invitations de l'espace qui attendent (`GET /api/v1/spaces/{id}/invitations`). */
export async function getSpaceInvitations(
  api: APIRequestContext,
  spaceId: string,
): Promise<PendingInvitation[]> {
  const res = await api.get(`/api/v1/spaces/${spaceId}/invitations`);
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as PendingInvitation[];
}

/** Le compte de la session de `api` accepte l'invitation (`POST /api/v1/invitations/accept`). */
export async function acceptInvitation(api: APIRequestContext, token: string): Promise<Accepted> {
  const res = await api.post("/api/v1/invitations/accept", { data: { token } });
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as Accepted;
}

/** `account` rejoint l'espace avec `role` : `owner` l'invite, et il accepte, par l'API. */
export async function joinSpace(
  owner: APIRequestContext,
  spaceId: string,
  account: Account,
  role: InviteRole,
): Promise<void> {
  const { token } = await inviteToSpace(owner, spaceId, { email: account.email, role });
  await acceptInvitation(await apiAs(account), token);
}

/**
 * Un compte neuf qui n'a que l'espace `spaceId` : `owner` l'y invite avec `role`, et il
 * s'inscrit avec cette invitation.
 */
export async function memberOf(
  owner: Account,
  spaceId: string,
  name: string,
  role: InviteRole = "member",
): Promise<Account> {
  const { token } = await inviteToSpace(await apiAs(owner), spaceId, {
    email: `invite-${unique()}@exemple.fr`,
    role,
  });
  return signUpAccount(name, token);
}

/**
 * Donne à un compte de l'espace le niveau `level` sur la page, et sur ses sous-pages par
 * héritage (`PUT /api/v1/pages/{id}/access/users/{userId}`). `api` doit y avoir l'accès complet.
 */
export async function setPageLevel(
  api: APIRequestContext,
  pageId: string,
  userId: string,
  level: Level,
): Promise<void> {
  const res = await api.put(`/api/v1/pages/${pageId}/access/users/${userId}`, { data: { level } });
  expect(res.status(), await res.text()).toBe(200);
}

/** La barre latérale de l'application. */
export function sidebarOf(p: Page): Locator {
  return p.getByRole("navigation", { name: "Barre latérale" });
}

/**
 * Le sélecteur d'espace, en tête de barre latérale : il nomme l'espace ouvert (`spaceName`,
 * s'il est donné), et ouvre la liste des espaces et le menu du compte.
 */
export function spaceSwitcher(p: Page, spaceName?: string): Locator {
  return sidebarOf(p).getByRole("button", {
    name: spaceName === undefined ? /, espaces et compte$/ : `${spaceName}, espaces et compte`,
    exact: true,
  });
}

/** On est dans l'application, sous le compte `name` : le sélecteur d'espace, ouvert, le nomme. */
export async function expectSignedInAs(p: Page, name: string): Promise<void> {
  const switcher = spaceSwitcher(p);
  await switcher.click();
  await expect(sidebarOf(p).getByRole("group", { name: "Compte" })).toContainText(name);
  await p.keyboard.press("Escape");
  await expect(switcher).toHaveAttribute("aria-expanded", "false");
}

/** Une section de la barre latérale, avec son titre et son arbre. */
export function sectionOf(p: Page, label: "Espace" | "Partagé" | "Privé"): Locator {
  return sidebarOf(p).getByRole("group", { name: label, exact: true });
}

/** La ligne d'une page dans la barre latérale. */
export function treeRow(p: Page, id: string): Locator {
  return sidebarOf(p).locator(`[role="treeitem"][data-page-id="${id}"]`);
}

/** Le titre de la page ouverte. */
export function titleOf(p: Page): Locator {
  return p.getByRole("textbox", { name: "Titre de la page" });
}

/** Les titres des lignes de la barre latérale, de haut en bas, avec leur niveau. */
export async function shownRows(p: Page): Promise<string[]> {
  return sidebarOf(p)
    .getByRole("treeitem")
    .evaluateAll((rows) =>
      rows.map((row) => `${row.getAttribute("aria-level")} ${row.querySelector("a")?.textContent}`),
    );
}

/** Ouvre le menu « … » de la ligne d'une page, et le rend. */
export async function openRowMenu(p: Page, id: string, title: string): Promise<Locator> {
  await treeRow(p, id).hover();
  await treeRow(p, id).getByRole("button", { name: `Actions de « ${title} »` }).click();
  const menu = p.getByRole("menu", { name: `Actions de « ${title} »` });
  await expect(menu).toBeVisible();
  return menu;
}

/** La ligne d'une page dans la liste de la corbeille. */
export function trashItem(p: Page, title: string): Locator {
  return p
    .getByRole("list", { name: "Pages à la corbeille" })
    .getByRole("listitem")
    .filter({ hasText: title });
}

/** Le bandeau d'une page ouverte qui est à la corbeille. */
export function trashBanner(p: Page): Locator {
  return p.getByRole("status").filter({ hasText: "Cette page est à la corbeille" });
}

/** Échoue si la page ouvre une fenêtre du navigateur (`window.confirm`, `alert`…). */
export function forbidNativeDialogs(p: Page): string[] {
  const opened: string[] = [];
  p.on("dialog", (dialog) => {
    opened.push(dialog.message());
    void dialog.dismiss();
  });
  return opened;
}

/** Ouvre la page `id` avec la session de `account` et attend la synchro. */
export async function openPage(browser: Browser, id: string, account?: Account): Promise<Page> {
  const p = await newUser(browser, account);
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

/** Le nom du cookie de session du serveur. */
const SESSION_COOKIE = "mementum_session";

/**
 * Une classe `WebSocket` qui joint le cookie de session de `request` à sa demande de
 * connexion. Un navigateur le joint de lui-même (même origine) ; une WebSocket ouverte par
 * Node, non : sans lui, `/sync` répond `401` dès qu'il exige une session.
 */
async function webSocketWithSession(request: APIRequestContext): Promise<typeof WebSocket> {
  const { cookies } = await request.storageState();
  const session = cookies.find((cookie) => cookie.name === SESSION_COOKIE);
  if (!session) throw new Error("ce client de l'API n'a pas de session");
  const cookie = `${session.name}=${session.value}`;
  return class extends WebSocket {
    constructor(url: string | URL, protocols?: string | string[]) {
      // Le second argument de la WebSocket de Node (undici) admet des en-têtes ; le type du
      // DOM, lui, ne connaît que les sous-protocoles.
      super(url, { protocols, headers: { cookie } } as unknown as string[]);
    }
  };
}

/**
 * Ajoute à la page, par un client Yjs nu (sans éditeur, au schéma courant), un bloc d'un
 * type que BlockNote ne connaît pas : `blockContainer > futureWidget`. C'est la forme d'un
 * bloc d'une version plus récente ; le serveur le relit `<block … foreign=true/>`. Le client
 * se connecte avec la session de `request`.
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
    WebSocketPolyfill: await webSocketWithSession(request),
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

/** Un id de page ou d'espace que le serveur ne connaît pas. */
export const UNKNOWN_ID = "00000000-0000-7000-8000-000000000000";

/** Les pages dans l'ordre de leur `position` (index fractionnaire, comparé en ASCII). */
export function byPosition<T extends { position: string }>(nodes: T[]): T[] {
  return [...nodes].sort((a, b) => (a.position < b.position ? -1 : a.position > b.position ? 1 : 0));
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
