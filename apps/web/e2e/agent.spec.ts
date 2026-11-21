// Un agent et un humain sur la même page (lot 5, Task X1) : un client MCP de Node
// (`mcp-client.ts`, la forme de Claude Code) branché à `/mcp` par l'adresse que l'écran des jetons
// donne, avec un jeton créé par le test ; un vrai navigateur pour la personne. Le cas qui fonde le
// lot (demande d'Alex, conception §4.4) : un agent verse un compte rendu dans un espace pendant
// qu'une personne tape dans la page, et rien de ce qu'elle tape n'est perdu.
//
// Ce que ces tests ne font pas : brancher le vrai Claude Code (`claude mcp add` écrit dans la
// configuration de l'utilisateur, R1258 ; c'est la Step 3 de la Task X1, avec Alex), ni mesurer
// (Task X2).
import { expect, test, type APIRequestContext, type Locator, type Page, type Request } from "@playwright/test";
import type { CreateToken, TokenCreated, Tree } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  createSpace,
  firstSpace,
  getMe,
  getMmd,
  isWrite,
  newUser,
  openPage,
  ownerAndMember,
  PASSWORD,
  putMmd,
  sectionOf,
  setPageLevel,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  treeRow,
  UNKNOWN_ID,
  unique,
  versionsOf,
  waitFindable,
  waitSynced,
  word,
} from "./helpers";
import { aliases, Mcp, uuids, V2025, V2026 } from "./mcp-client";
import { E2E_VERSION_EVERY_SECS } from "./servers";

test.afterEach(closeContexts);

/** Un jeton créé par l'API, au nom du compte de `api` (le mot de passe redemandé, L2). */
async function apiToken(
  api: APIRequestContext,
  body: Pick<CreateToken, "level" | "scope"> & Partial<CreateToken>,
): Promise<TokenCreated> {
  const res = await api.post("/api/v1/tokens", {
    data: { name: `Agent ${unique()}`, canTrash: false, password: PASSWORD, ...body },
  });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as TokenCreated;
}

/**
 * Crée un jeton par l'écran « Jetons d'API », comme Alex : depuis le menu du compte, son nom, le
 * niveau, la portée (`space` : l'espace ouvert ; `pages` : ces titres, cherchés dans l'arbre
 * affiché), le mot de passe. Rend le jeton montré une fois, et l'adresse du serveur MCP que porte
 * la commande prête à coller (jamais le jeton, R1576). La fenêtre est fermée, l'écran reste.
 */
async function createTokenOnScreen(
  p: Page,
  options: {
    name: string;
    level: "Lecture" | "Modification";
    scope: "space" | "pages";
    /** Pour `space` : l'espace choisi (sinon celui que l'écran propose, l'espace ouvert). */
    spaceId?: string;
    /** Pour `pages` : les titres des pages choisies. */
    pages?: string[];
  },
): Promise<{ token: string; url: string }> {
  await spaceSwitcher(p).click();
  await sidebarOf(p).getByRole("link", { name: "Jetons d'API" }).click();
  await expect(p.getByRole("heading", { level: 1, name: "Jetons d'API" })).toBeFocused();
  await p.getByRole("button", { name: "Créer un jeton" }).click();
  const form = p.getByRole("dialog", { name: "Créer un jeton" });
  await form.getByLabel("Nom du jeton").fill(options.name);
  await form.getByRole("radio", { name: options.level, exact: true }).check();
  if (options.scope === "space") {
    await form.getByRole("radio", { name: "Un espace" }).check();
    if (options.spaceId) await form.getByLabel("Espace du jeton").selectOption(options.spaceId);
  } else {
    await form.getByRole("radio", { name: /^Des pages/ }).check();
    for (const title of options.pages ?? []) {
      await form.getByLabel("Chercher une page").fill(title);
      await form.getByRole("checkbox", { name: title, exact: true }).check();
    }
  }
  await form.getByLabel("Mot de passe actuel").fill(PASSWORD);
  await form.getByRole("button", { name: "Créer le jeton" }).click();
  const shown = p.getByRole("dialog", { name: "Jeton créé" });
  const field = shown.getByLabel("Jeton d'API");
  await expect(field).toHaveValue(/^mmt_[A-Za-z0-9_-]{43}$/);
  const token = await field.inputValue();
  const lines = await shown.locator("code.mm-token-command").allTextContents();
  const url = lines.map((line) => /(https?:\/\/[^\s'"]+\/mcp)\b/.exec(line)?.[1]).find(Boolean);
  expect(url, lines.join("\n")).toBeTruthy();
  for (const line of lines) expect(line).not.toContain(token);
  await shown.getByRole("button", { name: "Fermer" }).click();
  await expect(shown).toHaveCount(0);
  return { token, url: url! };
}

/** L'éditeur de la page ouverte. */
function editorOf(p: Page) {
  return p.locator(".mm-editor .bn-editor");
}

/** Le texte de chaque bloc de premier niveau de l'éditeur de l'onglet, dans l'ordre. */
async function blockTexts(p: Page): Promise<string[]> {
  return p.evaluate(() =>
    (window.__mementum!.editor.document as unknown as { content?: unknown }[]).map((block) =>
      Array.isArray(block.content)
        ? (block.content as { text?: string }[]).map((item) => item.text ?? "").join("")
        : "",
    ),
  );
}

/** Pose le curseur à la fin du bloc qui porte `text`, d'un clic, comme une personne. */
async function cursorAtEndOf(p: Page, text: string): Promise<void> {
  await p.locator(".mm-editor .bn-block-content").filter({ hasText: text }).first().click();
  await p.keyboard.press("End");
}

/** Attend que le serveur garde exactement `mmd` pour la page (lu par la session de `api`). */
async function serverHolds(api: APIRequestContext, pageId: string, mmd: string): Promise<void> {
  await expect.poll(() => getMmd(api, pageId), { timeout: 15_000 }).toBe(mmd);
}

/** La retenue des trames que le serveur envoie à un onglet (`serverFrames`). */
type ServerFrames = {
  /**
   * Commence la retenue juste après une trame reçue du serveur : tout de suite si la dernière date de
   * moins de 5 s, sinon à la suivante (une page calme n'en reçoit qu'une toutes les 20 s).
   */
  hold: () => Promise<void>;
  /** Rend à l'onglet, dans l'ordre, ce que la retenue gardait ; elle a duré moins de 10 s. */
  release: () => void;
  /** Les trames d'écriture que l'onglet a envoyées au serveur depuis le début de la retenue. */
  sentWhileHeld: () => number;
};

/** Le budget d'une retenue : y-websocket ferme une connexion qui n'a rien reçu depuis 30 s. */
const HOLD_AFTER_FRAME_MS = 5_000;
const HOLD_AT_MOST_MS = 10_000;

/**
 * Retient ce que le serveur envoie à l'onglet sur ses connexions de synchro, à appeler avant `goto` :
 * entre `hold()` et `release()`, ce que l'onglet tape part au serveur, mais rien de ce que le serveur
 * diffuse ne lui arrive. Une écriture de l'agent et une frappe se croisent ainsi à coup sûr : la
 * personne tape sur une page où l'agent a déjà écrit, sans le savoir (R1482, R1631). y-websocket
 * compte ses 30 s depuis la dernière trame reçue : la retenue commence moins de 5 s après une trame,
 * et dure moins de 10 s.
 */
async function serverFrames(p: Page): Promise<ServerFrames> {
  let held: (() => void)[] | null = null;
  let heldSince = 0;
  let lastFrame = 0;
  let sent = 0;
  const waiting: (() => void)[] = [];
  await p.routeWebSocket(/\/sync\//, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((message) => {
      if (held) {
        held.push(() => ws.send(message));
        return;
      }
      ws.send(message);
      lastFrame = Date.now();
      for (const wake of waiting.splice(0)) wake();
    });
    ws.onMessage((message) => {
      const bytes = typeof message === "string" ? Buffer.from(message) : message;
      if (held && isWrite([...bytes])) sent += 1;
      server.send(message);
    });
  });
  return {
    hold: async () => {
      if (Date.now() - lastFrame > HOLD_AFTER_FRAME_MS) {
        await new Promise<void>((resolve, reject) => {
          const timer = setTimeout(() => reject(new Error("aucune trame du serveur depuis 25 s")), 25_000);
          waiting.push(() => {
            clearTimeout(timer);
            resolve();
          });
        });
      }
      held = [];
      heldSince = Date.now();
      sent = 0;
    },
    release: () => {
      expect(Date.now() - heldSince, "une retenue des trames du serveur de plus de 10 s").toBeLessThan(
        HOLD_AT_MOST_MS,
      );
      const queued = held ?? [];
      held = null;
      for (const send of queued) send();
      if (queued.length > 0) lastFrame = Date.now();
    },
    sentWhileHeld: () => sent,
  };
}

/** Le bouton du menu « … » de la page ouverte, qui porte « Historique ». */
function pageMenuButton(p: Page) {
  return p.getByRole("button", { name: "Actions de la page", exact: true });
}

/** Ouvre l'historique de la page par le menu « … », et attend que la liste soit lue. */
async function openHistory(p: Page) {
  await expect(pageMenuButton(p)).toBeVisible();
  await pageMenuButton(p).click();
  await p.getByRole("menuitem", { name: "Historique" }).click();
  const dialog = p.getByRole("dialog", { name: "Historique" });
  await expect(dialog.getByRole("list", { name: "Versions de la page" })).toHaveAttribute("aria-busy", "false");
  return dialog;
}

/** Les lignes de la liste des versions : « Version actuelle », puis chaque version. */
function versionItems(dialog: Locator) {
  return dialog.getByRole("list", { name: "Versions de la page" }).getByRole("listitem");
}

/** Attend plus d'un intervalle de versions du serveur des e2e (`E2E_VERSION_EVERY_SECS`). */
async function pastTheVersionInterval(): Promise<void> {
  await new Promise((done) => setTimeout(done, E2E_VERSION_EVERY_SECS * 1_000 + 200));
}

test("an_agent_pours_a_page_into_the_space_and_it_appears_in_the_sidebar", async ({ browser }) => {
  // Le cas d'Alex : un espace « Pro » à côté du sien, un jeton limité à « Pro » créé à l'écran, et
  // l'agent qui y verse un compte rendu. La ligne paraît dans sa barre latérale sans recharger.
  const alex = await signUpAccount(`Alex ${unique()}`);
  const api = await apiAs(alex);
  const own = await firstSpace(api);
  const pro = await createSpace(api, `Pro ${unique()}`);
  const p = await newUser(browser, alex);
  // Le canal de l'espace et les lectures de l'arbre, dans l'ordre où l'onglet les voit (un rang, pas
  // une heure : deux événements de la même milliseconde gardent leur ordre).
  let rank = 0;
  const lsnFrames: { at: number; lsn: number }[] = [];
  const treeSent = new Map<Request, number>();
  const treeReads: { sentAt: number; lsn: number; ids: string[] }[] = [];
  const isTreeRead = (request: Request) =>
    request.method() === "GET" && /\/api\/v1\/spaces\/[^/]+\/tree$/.test(new URL(request.url()).pathname);
  p.on("websocket", (ws) => {
    if (!ws.url().includes("/events/")) return;
    ws.on("framereceived", ({ payload }) => {
      const match = /^\{"lsn":(\d+)\}$/.exec(typeof payload === "string" ? payload : payload.toString("utf8"));
      if (match) lsnFrames.push({ at: ++rank, lsn: Number(match[1]) });
    });
  });
  p.on("request", (request) => {
    if (isTreeRead(request)) treeSent.set(request, ++rank);
  });
  p.on("response", async (response) => {
    if (!isTreeRead(response.request()) || response.status() !== 200) return;
    const tree = (await response.json()) as Tree;
    treeReads.push({
      sentAt: treeSent.get(response.request()) ?? -1,
      lsn: tree.lsn,
      ids: tree.nodes.map((node) => node.id),
    });
  });
  await p.goto(`/s/${pro.id}`);
  await expect(spaceSwitcher(p, pro.name)).toBeVisible();

  const { token, url } = await createTokenOnScreen(p, {
    name: "Claude Code",
    level: "Modification",
    scope: "space",
    spaceId: pro.id,
  });
  // L'écran des jetons garde la barre latérale de « Pro » : la ligne y paraîtra.
  await expect(spaceSwitcher(p, pro.name)).toBeVisible();
  await p.evaluate(() => ((window as unknown as { __notReloaded: boolean }).__notReloaded = true));

  const agent = new Mcp(token, V2026, url);
  await agent.open();
  const me = (await agent.call("workspace_info")).ok();
  expect(me.content()).toContain(`${pro.name} (${pro.id})`);
  expect(me.content()).not.toContain(own.id);
  const title = `Compte rendu ${unique()}`;
  const knownLsn = Math.max(...treeReads.map((read) => read.lsn));
  const calledAt = ++rank;
  const created = (
    await agent.call("create_pages", {
      space: pro.name,
      pages: [{ title, markdown: "# Décisions\n\nSortir la v1 avant décembre.\n\n- Relire le plan.\n" }],
    })
  ).ok();
  const [pageId] = uuids(created.content());

  // Sans recharger, dans « Privé » (la section d'une page qu'un agent crée sans rien dire, R1218).
  await expect(sectionOf(p, "Privé").locator(`[data-page-id="${pageId}"]`)).toBeVisible();
  await expect(treeRow(p, pageId)).toContainText(title);
  expect(await p.evaluate(() => (window as unknown as { __notReloaded?: boolean }).__notReloaded)).toBe(true);
  // Par le canal de l'espace : la lecture de l'arbre qui apporte la page suit une trame `{"lsn":N}`
  // reçue après l'appel de l'agent, d'une version plus haute que celle de l'arbre affiché (R1632).
  await expect.poll(() => treeReads.some((read) => read.ids.includes(pageId))).toBe(true);
  const bringing = treeReads.find((read) => read.ids.includes(pageId))!;
  const announced = lsnFrames.find((frame) => frame.at > calledAt && frame.lsn > knownLsn);
  expect(announced, JSON.stringify({ lsnFrames, knownLsn, calledAt, bringing })).toBeDefined();
  expect(announced!.at).toBeLessThan(bringing.sentAt);
  expect(bringing.lsn).toBeGreaterThanOrEqual(announced!.lsn);

  // Alex l'ouvre : le compte rendu de l'agent.
  await treeRow(p, pageId).getByRole("link").click();
  await waitSynced(p);
  await expect(editorOf(p)).toContainText("Décisions");
  await expect(editorOf(p)).toContainText("Sortir la v1 avant décembre.");
  await expect(editorOf(p)).toContainText("Relire le plan.");

  // Le jeton n'ouvre que « Pro » : l'espace d'Alex n'existe pas pour l'agent.
  (await agent.call("create_pages", { space: own.id, pages: [{ title: "Ailleurs" }] })).refused("not_found");
});

test("an_agent_edits_while_a_human_types_and_nothing_is_lost", async ({ browser }) => {
  // Le T5 de la phase 0, de bout en bout : 200 frappes dans le premier bloc, pendant que l'agent fait
  // 20 appels `str_replace` et `insert` sur d'autres blocs, chacun sur la révision que l'appel d'avant
  // lui a rendue (`rebase`, R1231). Le texte de la personne entier et dans l'ordre, les 20 éditions
  // présentes, aucun conflit, la même page chez le serveur et dans l'onglet. Trois de ces écritures
  // croisent la frappe à coup sûr : l'onglet ne les reçoit qu'une fois cinq frappes parties (R1630).
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const { token } = await apiToken(api, { level: "edit", scope: "space", spaceId: space.id });
  const agent = new Mcp(token);
  await agent.open();
  const points = Array.from({ length: 10 }, (_, i) => (i === 4 ? `Point ${i + 1} 🚀.` : `Point ${i + 1}.`));
  const created = (
    await agent.call("create_pages", {
      space: space.id,
      section: "space",
      pages: [{ title: `Réunion ${unique()}`, markdown: ["Notes de réunion.", ...points].join("\n\n") + "\n" }],
    })
  ).ok();
  const [pageId] = uuids(created.content());
  const read = (await agent.call("fetch", { id: pageId, anchors: true })).ok();
  const anchors = aliases(read.content());
  let revision = read.revision();

  const p = await newUser(browser, alice);
  const frames = await serverFrames(p);
  await p.goto(`/p/${pageId}`);
  await waitSynced(p);
  await cursorAtEndOf(p, "Notes de réunion.");
  // 200 frappes : une espace, puis des lettres par mots de vingt-six.
  const typed = ` ${"abcdefghijklmnopqrstuvwxyz ".repeat(8).slice(0, 199)}`;
  expect(typed).toHaveLength(200);

  const conflicts: string[] = [];
  // Les écritures croisées à coup sûr (leur rang parmi les vingt), et les frappes parties pendant
  // chacune.
  const crossing = new Set([2, 9, 16]);
  const crossings: number[] = [];
  // La frappe suit l'agent, par tranches de dix : la tranche k part quand l'appel k est lancé (la
  // retenue posée, pour un appel croisé). Chaque appel a donc des frappes en vol, et un appel croisé a
  // les siennes pendant la retenue, quelle que soit la vitesse du coureur : libre, la frappe pouvait
  // finir avant le troisième appel croisé sur un coureur lent, qui attendait alors des frappes qui ne
  // venaient plus (D9 de la passe de fin du lot 5 : `sentWhileHeld` à 0 sur la CI).
  const calls = points.length * 2;
  expect(calls).toBe(20);
  const launched = Array.from({ length: calls }, () => {
    let go!: () => void;
    const at = new Promise<void>((resolve) => (go = resolve));
    return { at, go };
  });
  const slice = typed.length / calls;
  const typing = (async () => {
    for (const [k, call] of launched.entries()) {
      await call.at;
      await p.keyboard.type(typed.slice(k * slice, (k + 1) * slice), { delay: 15 });
    }
  })();
  const agentWork = (async () => {
    let n = 0;
    for (const [i, point] of points.entries()) {
      const done = point.replace(/\.$/, ", fait.");
      for (const operation of [
        { op: "str_replace", old: point, new: done },
        { op: "insert", after: anchors.get(point), markdown: `Ajout ${i + 1}.\n` },
      ]) {
        const crossed = crossing.has(n);
        if (crossed) await frames.hold();
        launched[n++].go();
        const result = await agent.call("update_page", {
          page: pageId,
          base_revision: revision,
          operations: [operation],
        });
        if (result.isError) conflicts.push(result.text);
        else revision = result.revision();
        if (crossed) {
          await expect.poll(() => frames.sentWhileHeld(), { timeout: 8_000 }).toBeGreaterThanOrEqual(5);
          crossings.push(frames.sentWhileHeld());
          frames.release();
        }
        await new Promise((r) => setTimeout(r, 120));
      }
    }
  })();
  await Promise.all([typing, agentWork]);
  expect(conflicts).toEqual([]);
  expect(crossings).toHaveLength(3);

  const wanted = [
    `Notes de réunion.${typed}`,
    ...points.flatMap((point, i) => [point.replace(/\.$/, ", fait."), `Ajout ${i + 1}.`]),
  ];
  await serverHolds(api, pageId, wanted.join("\n\n") + "\n");
  await expect.poll(() => blockTexts(p)).toEqual(wanted);
  // L'agent relit la même page que le serveur garde.
  const reread = (await agent.call("fetch", { id: pageId })).ok();
  expect(reread.content()).toContain(wanted.join("\n\n"));
});

test("an_agent_and_a_human_in_the_same_block", async ({ browser }) => {
  // Les deux dans un même bloc, leurs écritures croisées (l'onglet tape sans avoir reçu celle de
  // l'agent). `str_replace` ne récrit que ce qu'il remplace : les deux restent. `replace_block` : une
  // frappe arrivée avant lui le fait refuser (`revision_conflict`, avec le bloc à jour, R1231) ; une
  // frappe qui le croise reste au bord du texte écrit quand le MMD neuf porte un bloc de premier
  // niveau du type du bloc remplacé, qui en hérite l'identifiant et l'élément de contenu (R1232,
  // R1283), et est perdue sinon — la limite LA-a, dite ici telle qu'elle est (R1629, qui récrit
  // R1480). Ce test ne prouve qu'une frappe en fin de bloc, après un suffixe commun.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const { token } = await apiToken(api, { level: "edit", scope: "space", spaceId: space.id });
  const agent = new Mcp(token);
  await agent.open();
  const page = await createPage(api, { spaceId: space.id, title: `Même bloc ${unique()}` });
  await putMmd(api, page.id, "Sortir la v1.\n\nBloc à remplacer.\n");

  const p = await newUser(browser, alice);
  const frames = await serverFrames(p);
  await p.goto(`/p/${page.id}`);
  await waitSynced(p);

  await test.step("str_replace : les deux restent", async () => {
    await cursorAtEndOf(p, "Sortir la v1.");
    await frames.hold();
    (await agent.call("update_page", { page: page.id, operations: [{ op: "str_replace", old: "la v1", new: "la v2" }] })).ok();
    await p.keyboard.type(" Avant décembre.");
    // Ils se sont croisés : le serveur a les deux, l'onglet ne sait encore rien de l'agent.
    await serverHolds(api, page.id, "Sortir la v2. Avant décembre.\n\nBloc à remplacer.\n");
    expect((await blockTexts(p))[0]).toBe("Sortir la v1. Avant décembre.");
    frames.release();
    await expect.poll(() => blockTexts(p)).toEqual(["Sortir la v2. Avant décembre.", "Bloc à remplacer."]);
  });

  await test.step("replace_block : refusé sur une frappe arrivée avant lui", async () => {
    const before = (await agent.call("fetch", { id: page.id, anchors: true })).ok();
    const anchor = aliases(before.content()).get("Bloc à remplacer.")!;
    await cursorAtEndOf(p, "Bloc à remplacer.");
    await p.keyboard.type(" Tapé avant.");
    await serverHolds(api, page.id, "Sortir la v2. Avant décembre.\n\nBloc à remplacer. Tapé avant.\n");
    const refused = (
      await agent.call("update_page", {
        page: page.id,
        base_revision: before.revision(),
        operations: [{ op: "replace_block", anchor, markdown: "Bloc récrit par l'agent.\n" }],
      })
    ).refused("revision_conflict");
    // L'agent reçoit le bloc à jour, ce que la personne a tapé compris ; rien n'est écrit.
    expect(refused.content()).toContain("Bloc à remplacer. Tapé avant.");
    expect(await getMmd(api, page.id)).toBe("Sortir la v2. Avant décembre.\n\nBloc à remplacer. Tapé avant.\n");
  });

  await test.step("replace_block par un bloc de son type : une frappe qui le croise reste, au bord du texte écrit", async () => {
    // Le premier bloc de premier niveau du type du bloc remplacé hérite son identifiant et son élément
    // de contenu (R1232), écrit par préfixe et suffixe (R1283) : la frappe, ancrée au point final
    // que l'écriture garde, reste après le texte de l'agent.
    const reread = (await agent.call("fetch", { id: page.id, anchors: true })).ok();
    const anchor = aliases(reread.content()).get("Bloc à remplacer. Tapé avant.")!;
    await frames.hold();
    (
      await agent.call("update_page", {
        page: page.id,
        base_revision: reread.revision(),
        operations: [{ op: "replace_block", anchor, markdown: "Bloc récrit par l'agent.\n" }],
      })
    ).ok();
    // La personne tape encore dans le bloc qu'elle voit, déjà récrit chez le serveur.
    await p.keyboard.type(" Tapé pendant.");
    await serverHolds(api, page.id, "Sortir la v2. Avant décembre.\n\nBloc récrit par l'agent. Tapé pendant.\n");
    expect((await blockTexts(p))[1]).toBe("Bloc à remplacer. Tapé avant. Tapé pendant.");
    frames.release();
    await expect
      .poll(() => blockTexts(p))
      .toEqual(["Sortir la v2. Avant décembre.", "Bloc récrit par l'agent. Tapé pendant."]);
  });

  await test.step("replace_block sans bloc de son type : une frappe qui le croise est perdue (LA-a)", async () => {
    // Un titre seul remplace le paragraphe : aucun bloc n'hérite de son élément de contenu.
    const reread = (await agent.call("fetch", { id: page.id, anchors: true })).ok();
    const anchor = aliases(reread.content()).get("Bloc récrit par l'agent. Tapé pendant.")!;
    await cursorAtEndOf(p, "Tapé pendant.");
    await frames.hold();
    (
      await agent.call("update_page", {
        page: page.id,
        base_revision: reread.revision(),
        operations: [{ op: "replace_block", anchor, markdown: "## Titre posé par l'agent\n" }],
      })
    ).ok();
    await p.keyboard.type(" Tapé encore.");
    // Une barrière : une frappe dans l'autre bloc, sur la même connexion, que le serveur garde ; il a
    // donc traité celles d'avant.
    await cursorAtEndOf(p, "Avant décembre.");
    await p.keyboard.type(" Fin.");
    await serverHolds(api, page.id, "Sortir la v2. Avant décembre. Fin.\n\n## Titre posé par l'agent\n");
    expect((await blockTexts(p))[1]).toBe("Bloc récrit par l'agent. Tapé pendant. Tapé encore.");
    frames.release();
    // La frappe est perdue, pour l'onglet comme pour le serveur ; rien d'autre ne l'est. Le jour où
    // elle sera gardée, ce test rougit : la limite sera levée, à dire ailleurs.
    await expect
      .poll(() => blockTexts(p))
      .toEqual(["Sortir la v2. Avant décembre. Fin.", "Titre posé par l'agent"]);
    expect(await getMmd(api, page.id)).toBe("Sortir la v2. Avant décembre. Fin.\n\n## Titre posé par l'agent\n");
  });
});

test("a_token_scoped_to_a_page_finds_nothing_else", async ({ browser }) => {
  // Un jeton créé à l'écran pour une page et ses sous-pages : la recherche, `fetch`, le plan de
  // l'espace et le fil d'Ariane ne disent rien du reste de l'espace, ni de la page parente de la
  // page choisie. Une page hors de portée répond comme une page qui n'existe pas.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const tag = unique();
  const folder = await createPage(api, { spaceId: space.id, title: `Dossier ${tag}` });
  const project = await createPage(api, { parentId: folder.id, title: `Projet ${tag}` });
  const sub = await createPage(api, { parentId: project.id, title: `Sous-page ${tag}` });
  const other = await createPage(api, { spaceId: space.id, title: `Autre ${tag}` });
  const searched = word();
  for (const id of [folder.id, project.id, sub.id, other.id]) await putMmd(api, id, `Le mot ${searched} est ici.\n`);
  for (const id of [folder.id, project.id, sub.id, other.id]) await waitFindable(api, space.id, searched, id);
  /** Ni le titre ni l'identifiant d'une page hors de portée. */
  const nothingHidden = (text: string, what: string) => {
    for (const hidden of [folder.title, other.title, folder.id, other.id]) expect(text, what).not.toContain(hidden);
  };

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect(treeRow(p, folder.id)).toBeVisible();
  const { token, url } = await createTokenOnScreen(p, {
    name: "Agent du projet",
    level: "Lecture",
    scope: "pages",
    pages: [project.title],
  });
  const agent = new Mcp(token, V2026, url);
  await agent.open();

  // La recherche : la page choisie et sa sous-page, rien d'autre ; le fil d'Ariane part de la page
  // choisie, sans sa page parente.
  const found = (await agent.call("search", { query: searched })).ok();
  expect(uuids(found.content()).filter((id) => id !== space.id).sort()).toEqual([project.id, sub.id].sort());
  expect(found.content()).toContain(`chemin : ${project.title}`);
  nothingHidden(found.text, "search");

  // `fetch` d'une page hors de portée : la réponse d'une page qui n'existe pas, octet pour octet.
  const unknown = (await agent.call("fetch", { id: UNKNOWN_ID })).refused("not_found");
  for (const hidden of [folder, other]) {
    const unseen = (await agent.call("fetch", { id: hidden.id })).refused("not_found");
    expect(unseen.text).toBe(unknown.text);
  }
  // La sous-page se lit, son chemin sans la page parente de la page choisie.
  const subRead = (await agent.call("fetch", { id: sub.id })).ok();
  expect(subRead.content()).toContain(`chemin : ${project.title}\n`);
  nothingHidden(subRead.text, "fetch de la sous-page");

  // Le plan de l'espace et `workspace_info` : la page choisie et sa sous-page.
  for (const [tool, args] of [
    ["fetch", { id: space.id }],
    ["workspace_info", { space: space.id }],
  ] as const) {
    const seen = (await agent.call(tool, args)).ok();
    expect(seen.content(), tool).toContain(project.title);
    nothingHidden(seen.text, tool);
  }
});

test("a_revoked_token_stops_between_two_calls", async ({ browser }) => {
  // Un agent de l'ancienne révision du protocole (2025-11-25) écrit, la personne révoque son jeton à
  // l'écran, et l'appel suivant est refusé : `401`, rien d'écrit. L'activité du jeton nomme l'outil.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const page = await createPage(api, { spaceId: space.id, title: `Révocation ${unique()}` });
  await putMmd(api, page.id, "Premier paragraphe.\n");
  const name = `Agent révoqué ${unique()}`;
  const { token, info } = await apiToken(api, { name, level: "edit", scope: "space", spaceId: space.id });
  const agent = new Mcp(token, V2025);
  await agent.open();
  const insert = (markdown: string) =>
    agent.send("tools/call", {
      name: "update_page",
      arguments: { page: page.id, operations: [{ op: "insert", position: "end", markdown }] },
    });
  const first = await insert("Avant la révocation.\n");
  expect(first.status).toBe(200);
  expect((first.body as { result?: { isError?: boolean } }).result?.isError, JSON.stringify(first.body)).toBe(false);
  await serverHolds(api, page.id, "Premier paragraphe.\n\nAvant la révocation.\n");

  const p = await newUser(browser, alice);
  await p.goto("/account/tokens");
  const line = p.locator("li[data-token-id]").filter({ hasText: name });
  await line.getByRole("button", { name: "Révoquer" }).click();
  await p.getByRole("alertdialog").getByRole("button", { name: "Révoquer" }).click();
  await expect(line).toContainText("Révoqué");

  const second = await insert("Après la révocation.\n");
  expect(second.status).toBe(401);
  expect(second.headers.get("www-authenticate") ?? "").toMatch(/^Bearer/);
  expect((second.body as { error?: string }).error).toBe("invalid_token");
  expect(await getMmd(api, page.id)).toBe("Premier paragraphe.\n\nAvant la révocation.\n");

  // L'activité du jeton : l'appel d'outil, par son nom, et la page qu'il visait.
  await expect
    .poll(async () => {
      const res = await api.get(`/api/v1/tokens/${info.id}/calls`);
      return ((await res.json()) as { calls: { tool: string }[] }).calls.map((call) => call.tool);
    })
    .toContain("update_page");
  await p.reload();
  const reloaded = p.locator("li[data-token-id]").filter({ hasText: name });
  await reloaded.getByRole("button", { name: "Activité" }).click();
  const call = reloaded.locator("li[data-call-id]").filter({ hasText: "update_page" });
  await expect(call).toHaveCount(1);
  await expect(call.locator(".mm-token-call-outcome")).toHaveText("Réussi");
  await expect(call).toContainText(page.title);
});

test("replace_page_is_refused_while_the_tab_can_write", async ({ browser }) => {
  // R1233 : `replace_page` par jeton est refusé tant qu'une connexion qui peut écrire est ouverte sur
  // la page ; l'onglet d'un lecteur n'en est pas une.
  const { alice, aliceApi, bruno, brunoId, space } = await ownerAndMember();
  const page = await createPage(aliceApi, { spaceId: space.id, section: "private", title: `Récriture ${unique()}` });
  await setPageLevel(aliceApi, page.id, brunoId, "read");
  await putMmd(aliceApi, page.id, "Le texte d'Alice.\n");
  const { token } = await apiToken(aliceApi, { level: "edit", scope: "space", spaceId: space.id });
  const agent = new Mcp(token);
  await agent.open();
  const revision = (await agent.call("fetch", { id: page.id })).ok().revision();
  const replace = () =>
    agent.call("update_page", {
      page: page.id,
      base_revision: revision,
      operations: [{ op: "replace_page", markdown: "Le texte de l'agent.\n" }],
    });

  const tab = await openPage(browser, page.id, alice);
  const refused = (await replace()).refused("page_in_use");
  expect(refused.text).toContain("str_replace");
  expect(await getMmd(aliceApi, page.id)).toBe("Le texte d'Alice.\n");

  // L'onglet d'Alice fermé, celui de Bruno, qui ne fait que lire, ouvert : la récriture passe.
  await tab.context().close();
  const reader = await openPage(browser, page.id, bruno);
  await expect(reader.getByRole("status").filter({ hasText: "Tu peux lire cette page" })).toBeVisible();
  await expect.poll(async () => (await replace()).isError, { timeout: 15_000 }).toBe(false);
  await expect(editorOf(reader)).toContainText("Le texte de l'agent.");
  expect(await getMmd(aliceApi, page.id)).toBe("Le texte de l'agent.\n");
});

test("an_agent_write_shows_in_the_history_as_an_agent", async ({ browser }) => {
  // Une version dont l'auteur a écrit par un jeton le dit : « Alice (par un agent) » ; celle qu'Alice
  // a écrite elle-même, non (R1234, C2). Le nom du jeton n'y paraît jamais.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const page = await createPage(api, { spaceId: space.id, title: `Versions ${unique()}` });
  await putMmd(api, page.id, "Texte d'Alice.\n");
  const name = `Jeton secret ${unique()}`;
  const { token } = await apiToken(api, { name, level: "edit", scope: "space", spaceId: space.id });
  const agent = new Mcp(token);
  await agent.open();
  const append = async (markdown: string) =>
    (
      await agent.call("update_page", { page: page.id, operations: [{ op: "insert", position: "end", markdown }] })
    ).ok();
  // Chaque écriture de l'agent suit la précédente de plus d'un intervalle : le serveur coupe une
  // version juste avant elle, qui garde le contenu d'avant.
  await pastTheVersionInterval();
  await append("Ajout de l'agent.\n");
  await pastTheVersionInterval();
  await append("Second ajout.\n");
  await expect.poll(async () => (await versionsOf(api, page.id)).some((v) => v.agentAuthors.length > 0)).toBe(true);
  const me = (await getMe(api)).user;

  const p = await openPage(browser, page.id, alice);
  const dialog = await openHistory(p);
  const byAgent = versionItems(dialog).filter({ hasText: "(par un agent)" });
  await expect(byAgent).toHaveCount(1);
  await expect(byAgent.locator(".mm-history-names")).toHaveText(`${me.name} (par un agent)`);
  const byHand = versionItems(dialog).filter({ hasText: me.name }).filter({ hasNotText: "(par un agent)" });
  await expect(byHand).toHaveCount(1);
  await expect(dialog).not.toContainText(name);
  // La version écrite par l'agent porte son premier ajout, pas le second.
  await byAgent.getByRole("button").click();
  const preview = dialog.locator(".mm-preview-editor .bn-editor");
  await expect(preview).toContainText("Ajout de l'agent.");
  await expect(preview).not.toContainText("Second ajout.");
});

test("what_an_agent_replaced_is_restored_from_its_before_agent_version", async ({ browser }) => {
  // Alice tape un paragraphe ; l'agent le remplace par `replace_block`. Une version « gardée avant
  // qu'un agent modifie la page » le garde (R1234, R1552), et Alice le restaure depuis l'historique.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const page = await createPage(api, { spaceId: space.id, title: `Remplacé ${unique()}` });
  await putMmd(api, page.id, "Introduction.\n\nÀ compléter.\n");
  const { token } = await apiToken(api, { level: "edit", scope: "space", spaceId: space.id });
  const agent = new Mcp(token);
  await agent.open();

  const p = await openPage(browser, page.id, alice);
  await cursorAtEndOf(p, "À compléter.");
  await p.keyboard.type(" Le paragraphe qu'Alice a tapé.");
  await serverHolds(api, page.id, "Introduction.\n\nÀ compléter. Le paragraphe qu'Alice a tapé.\n");

  const read = (await agent.call("fetch", { id: page.id, anchors: true })).ok();
  const anchor = aliases(read.content()).get("À compléter. Le paragraphe qu'Alice a tapé.")!;
  (
    await agent.call("update_page", {
      page: page.id,
      base_revision: read.revision(),
      operations: [{ op: "replace_block", anchor, markdown: "Le texte de l'agent.\n" }],
    })
  ).ok();
  await expect(editorOf(p)).toContainText("Le texte de l'agent.");
  await expect(editorOf(p)).not.toContainText("Le paragraphe qu'Alice a tapé.");

  const dialog = await openHistory(p);
  const kept = versionItems(dialog).filter({ hasText: "gardée avant qu'un agent modifie la page" });
  await expect(kept).toHaveCount(1);
  await kept.getByRole("button").click();
  await expect(dialog.locator(".mm-preview-editor .bn-editor")).toContainText("Le paragraphe qu'Alice a tapé.");
  await dialog.getByRole("button", { name: "Restaurer cette version" }).click();
  await p.getByRole("alertdialog", { name: /^Restaurer cette version/ }).getByRole("button", { name: "Restaurer", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(p.getByRole("status").filter({ hasText: "Version restaurée." })).toHaveCount(1);

  await expect(editorOf(p)).toContainText("Le paragraphe qu'Alice a tapé.");
  await expect(editorOf(p)).not.toContainText("Le texte de l'agent.");
  await serverHolds(api, page.id, "Introduction.\n\nÀ compléter. Le paragraphe qu'Alice a tapé.\n");
});
