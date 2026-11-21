// Un agent et un humain sur la même page (lot 5, Task X1) : un client MCP de Node
// (`mcp-client.ts`, la forme de Claude Code) branché à `/mcp` par l'adresse que l'écran des jetons
// donne, avec un jeton créé par le test ; un vrai navigateur pour la personne. Le cas qui fonde le
// lot (demande d'Alex, conception §4.4) : un agent verse un compte rendu dans un espace pendant
// qu'une personne tape dans la page, et rien de ce qu'elle tape n'est perdu.
//
// Ce que ces tests ne font pas : brancher le vrai Claude Code (`claude mcp add` écrit dans la
// configuration de l'utilisateur, R1258 ; c'est la Step 3 de la Task X1, avec Alex), ni mesurer
// (Task X2).
import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";
import type { CreateToken, TokenCreated } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  createSpace,
  firstSpace,
  getMe,
  getMmd,
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

/**
 * Retient ce que le serveur envoie à l'onglet sur ses connexions de synchro, à appeler avant `goto` :
 * entre `hold()` et `release()`, ce que l'onglet tape part au serveur, mais rien de ce que le serveur
 * diffuse ne lui arrive. Une écriture de l'agent et une frappe se croisent ainsi à coup sûr : la
 * personne tape sur une page où l'agent a déjà écrit, sans le savoir. Quelques secondes au plus :
 * y-websocket ferme une connexion qui n'a rien reçu depuis 30 s.
 */
async function serverFrames(p: Page): Promise<{ hold: () => void; release: () => void }> {
  let held: (() => void)[] | null = null;
  await p.routeWebSocket(/\/sync\//, (ws) => {
    const server = ws.connectToServer();
    server.onMessage((message) => {
      if (held) held.push(() => ws.send(message));
      else ws.send(message);
    });
  });
  return {
    hold: () => {
      held = [];
    },
    release: () => {
      const queued = held ?? [];
      held = null;
      for (const send of queued) send();
    },
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
  const created = (
    await agent.call("create_pages", {
      space: pro.name,
      pages: [{ title, markdown: "# Décisions\n\nSortir la v1 avant décembre.\n\n- Relire le plan.\n" }],
    })
  ).ok();
  const [pageId] = uuids(created.content());

  // Sans recharger : la page arrive par le canal de l'espace, dans « Privé » (la section d'une page
  // qu'un agent crée sans rien dire, R1218).
  await expect(sectionOf(p, "Privé").locator(`[data-page-id="${pageId}"]`)).toBeVisible();
  await expect(treeRow(p, pageId)).toContainText(title);
  expect(await p.evaluate(() => (window as unknown as { __notReloaded?: boolean }).__notReloaded)).toBe(true);

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
  // présentes, aucun conflit, la même page chez le serveur et dans l'onglet.
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

  const p = await openPage(browser, pageId, alice);
  await cursorAtEndOf(p, "Notes de réunion.");
  // 200 frappes : une espace, puis des lettres par mots de vingt-six.
  const typed = ` ${"abcdefghijklmnopqrstuvwxyz ".repeat(8).slice(0, 199)}`;
  expect(typed).toHaveLength(200);

  const conflicts: string[] = [];
  // L'heure de chaque écriture de l'agent, pour savoir qu'elles ont croisé la frappe.
  const written: number[] = [];
  const agentWork = (async () => {
    for (const [i, point] of points.entries()) {
      const done = point.replace(/\.$/, ", fait.");
      for (const operation of [
        { op: "str_replace", old: point, new: done },
        { op: "insert", after: anchors.get(point), markdown: `Ajout ${i + 1}.\n` },
      ]) {
        const result = await agent.call("update_page", {
          page: pageId,
          base_revision: revision,
          operations: [operation],
        });
        written.push(Date.now());
        if (result.isError) conflicts.push(result.text);
        else revision = result.revision();
        await new Promise((r) => setTimeout(r, 120));
      }
    }
  })();
  const typing = (async () => {
    const start = Date.now();
    await p.keyboard.type(typed, { delay: 15 });
    return [start, Date.now()] as const;
  })();
  const [[typingFrom, typingTo]] = await Promise.all([typing, agentWork]);
  expect(conflicts).toEqual([]);
  // Une écriture sur quatre au moins a eu lieu pendant la frappe (sans quoi le test ne prouverait
  // rien) ; d'ordinaire, toutes.
  const crossed = written.filter((at) => at > typingFrom && at < typingTo).length;
  console.log(`X1 : ${crossed} écritures de l'agent sur 20 pendant les ${typingTo - typingFrom} ms de frappe`);
  expect(crossed).toBeGreaterThanOrEqual(5);

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
  // frappe qui le croise reste au bord du texte écrit tant que le bloc garde son type (R1283), et
  // est perdue quand il en change — la limite LA-a, dite ici telle qu'elle est (R1480).
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
    frames.hold();
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

  await test.step("replace_block du même type : une frappe qui le croise reste, au bord du texte écrit", async () => {
    // Le contenu d'un bloc qui garde son type s'écrit par préfixe et suffixe (R1283) : la frappe,
    // ancrée au point final que l'écriture garde, reste après le texte de l'agent.
    const reread = (await agent.call("fetch", { id: page.id, anchors: true })).ok();
    const anchor = aliases(reread.content()).get("Bloc à remplacer. Tapé avant.")!;
    frames.hold();
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

  await test.step("replace_block qui change le type du bloc : une frappe qui le croise est perdue (LA-a)", async () => {
    const reread = (await agent.call("fetch", { id: page.id, anchors: true })).ok();
    const anchor = aliases(reread.content()).get("Bloc récrit par l'agent. Tapé pendant.")!;
    await cursorAtEndOf(p, "Tapé pendant.");
    frames.hold();
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
  for (const id of [folder.id, sub.id, other.id]) await waitFindable(api, space.id, searched, id);

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
  for (const hidden of [folder.title, other.title]) expect(found.text).not.toContain(hidden);

  // `fetch` d'une page hors de portée : la réponse d'une page qui n'existe pas, octet pour octet.
  const unknown = (await agent.call("fetch", { id: UNKNOWN_ID })).refused("not_found");
  for (const hidden of [folder, other]) {
    const unseen = (await agent.call("fetch", { id: hidden.id })).refused("not_found");
    expect(unseen.text).toBe(unknown.text);
  }
  // La sous-page se lit, son chemin sans la page parente de la page choisie.
  const subRead = (await agent.call("fetch", { id: sub.id })).ok();
  expect(subRead.content()).toContain(`chemin : ${project.title}\n`);
  expect(subRead.text).not.toContain(folder.title);

  // Le plan de l'espace et `workspace_info` : la page choisie et sa sous-page.
  for (const [tool, args] of [
    ["fetch", { id: space.id }],
    ["workspace_info", { space: space.id }],
  ] as const) {
    const seen = (await agent.call(tool, args)).ok();
    expect(seen.content(), tool).toContain(project.title);
    for (const hidden of [folder.title, other.title]) expect(seen.text, tool).not.toContain(hidden);
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
  await expect.poll(async () => (await replace()).code(), { timeout: 15_000 }).toBeNull();
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
  // Alice tape un paragraphe ; l'agent le remplace par `replace_block`. Une version « avant une
  // modification par un agent » le garde (R1234), et Alice le restaure depuis l'historique.
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
  const kept = versionItems(dialog).filter({ hasText: "avant une modification par un agent" });
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
