// Dupliquer une page (Ruling R252) : « Dupliquer », dans le menu « … » de la page ou de sa ligne,
// copie la page et ses sous-pages visibles dans des pages neuves. La copie va sous la page
// parente, juste après la page, si le compte peut y créer ; sinon, pour un membre, à la racine de
// son « Privé ». Elle s'ouvre chez qui l'a demandée, son titre prêt à être changé ; les autres la
// voient paraître par le canal de l'espace s'ils y ont accès, et jamais sinon. Ses images sont à
// elle : elles survivent à la suppression de l'original.
import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";
import type { FileInfo, Level, PageNode } from "@mementum/protocol";
import {
  acceptInvitation,
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getTree,
  guestOf,
  holdFirstResponse,
  inviteToPage,
  newUser,
  openPage,
  openRowMenu,
  ownerAndMember,
  pageIdOf,
  putMmd,
  READ_ONLY_BANNER,
  sectionOf,
  serverSchemaBumped,
  setPageLevel,
  sidebarOf,
  signUpAccount,
  titleOf,
  treeRow,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

/** Le bouton du menu « … » de la barre du haut de la page. */
function pageMenuButton(p: Page) {
  return p.getByRole("button", { name: "Actions de la page", exact: true });
}

/** Duplique la page ouverte par son menu « … », et rend l'id de la copie, une fois ouverte. */
async function duplicateFromPageMenu(p: Page): Promise<string> {
  const original = pageIdOf(p);
  await pageMenuButton(p).click();
  await p.getByRole("menuitem", { name: "Dupliquer" }).click();
  await expect.poll(() => pageIdOf(p)).not.toBe(original);
  await waitSynced(p);
  return pageIdOf(p);
}

/**
 * Barrière : `id` est renommée par l'API, et l'onglet le voit dans son arbre. Ce que le serveur
 * avait écrit avant ce renommage (une copie) est alors dans tout arbre que l'onglet a lu depuis :
 * c'est ce qui permet d'affirmer qu'une ligne n'y est **pas**, sans attente fixe.
 */
async function renamedInTree(p: Page, api: APIRequestContext, id: string): Promise<void> {
  const title = `Barrière ${unique()}`;
  const res = await api.patch(`/api/v1/pages/${id}`, { data: { title } });
  expect(res.status(), await res.text()).toBe(200);
  await expect(treeRow(p, id).getByRole("link")).toHaveText(title);
}

/** Les pages de l'arbre du compte de `api` sous la page `parentId`, dans leur ordre. */
async function childrenOf(api: APIRequestContext, parentId: string): Promise<PageNode[]> {
  const { nodes } = await getTree(api);
  return nodes
    .filter((node) => node.parentId === parentId)
    .sort((a, b) => (a.position < b.position ? -1 : a.position > b.position ? 1 : 0));
}

/** Un PNG d'un pixel. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

test("duplicated_page_appears_for_everyone_and_opens_for_who_did_it", async ({ browser }) => {
  // Une sous-page d'équipe : Alice et Bruno peuvent créer sous sa page parente.
  const { alice, aliceApi, space, bruno, brunoApi } = await ownerAndMember();
  const team = await createPage(aliceApi, { spaceId: space.id, title: `Équipe ${unique()}` });
  const sub = await createPage(aliceApi, { parentId: team.id, title: `Compte rendu ${unique()}` });
  const after = await createPage(aliceApi, { parentId: team.id, title: `Suivante ${unique()}` });
  await putMmd(aliceApi, sub.id, "Le contenu de l'original.\n");

  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  await expect(treeRow(b, sub.id)).toBeVisible();
  const a = await openPage(browser, sub.id, alice);

  const copyId = await duplicateFromPageMenu(a);
  // Chez Alice : la copie s'ouvre, sous son titre, prête à être renommée (le focus, tout le titre
  // choisi), et son contenu est celui de l'original.
  const copyTitle = `${sub.title} (copie)`;
  await expect(titleOf(a)).toHaveValue(copyTitle);
  await expect(titleOf(a)).toBeFocused();
  expect(
    await titleOf(a).evaluate((input: HTMLInputElement) => [input.selectionStart, input.selectionEnd]),
  ).toEqual([0, copyTitle.length]);
  await expect(a.locator(".mm-editor")).toContainText("Le contenu de l'original.");
  await expect(treeRow(a, copyId).getByRole("link")).toHaveText(copyTitle);

  // Chez Bruno, sans recharger : la copie paraît en moins de deux secondes, sous la même page
  // parente, juste après l'original.
  await expect(treeRow(b, copyId).getByRole("link")).toHaveText(copyTitle, { timeout: 2_000 });
  expect((await childrenOf(brunoApi, team.id)).map((node) => node.id)).toEqual([
    sub.id,
    copyId,
    after.id,
  ]);
  const rows = await sidebarOf(b).getByRole("treeitem").evaluateAll((items) =>
    items.map((item) => item.getAttribute("data-page-id")),
  );
  expect(rows.indexOf(copyId)).toBe(rows.indexOf(sub.id) + 1);
});

test("duplicate_of_a_team_root_page_is_private", async ({ browser }) => {
  // Une page de racine d'« Espace » : la copie va dans le « Privé » d'Alice, et personne d'autre
  // ne la voit. Demandée au clavier, par le menu de sa ligne.
  const { alice, aliceApi, space, bruno, brunoApi } = await ownerAndMember();
  const team = await createPage(aliceApi, { spaceId: space.id, title: `Charte ${unique()}` });
  const other = await createPage(aliceApi, { spaceId: space.id, title: `Autre ${unique()}` });

  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  await expect(treeRow(b, team.id)).toBeVisible();
  const a = await newUser(browser, alice);
  await a.goto(`/s/${space.id}`);
  await expect(treeRow(a, team.id)).toBeVisible();

  const menu = await openRowMenu(a, team.id, team.title);
  await menu.getByRole("menuitem", { name: "Dupliquer" }).focus();
  await a.keyboard.press("Enter");
  await expect(a).toHaveURL(/\/p\//);
  await waitSynced(a);
  const copyId = pageIdOf(a);
  const copyTitle = `${team.title} (copie)`;
  await expect(titleOf(a)).toHaveValue(copyTitle);
  await expect(sectionOf(a, "Privé").locator(`[data-page-id="${copyId}"]`)).toBeVisible();
  await expect(sectionOf(a, "Espace").locator(`[data-page-id="${copyId}"]`)).toHaveCount(0);

  // Bruno ne la voit nulle part : ni dans sa barre (une fois relue après la copie), ni par l'API.
  await renamedInTree(b, aliceApi, other.id);
  await expect(treeRow(b, copyId)).toHaveCount(0);
  await expect(sidebarOf(b)).not.toContainText(copyTitle);
  expect((await getTree(brunoApi, space.id)).nodes.map((node) => node.id)).not.toContain(copyId);
  expect((await brunoApi.get(`/api/v1/pages/${copyId}`)).status()).toBe(404);
  // Témoin : l'original, lui, est toujours chez lui.
  await expect(treeRow(b, team.id)).toBeVisible();
});

test("duplicate_of_a_subpage_one_cannot_create_beside_goes_to_private", async ({ browser }) => {
  // Bruno, membre, lit une sous-page sans pouvoir modifier sa page parente : la copie ne peut pas
  // aller à côté d'elle ; elle s'ouvre dans son « Privé », et rien ne paraît sous la page parente
  // chez Alice.
  const { alice, aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const parent = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Dossier d'Alice ${unique()}`,
  });
  const read = await createPage(aliceApi, { parentId: parent.id, title: `À lire ${unique()}` });
  await setPageLevel(aliceApi, parent.id, brunoId, "read");

  const a = await openPage(browser, parent.id, alice);
  await expect(treeRow(a, read.id)).toBeVisible();
  const b = await openPage(browser, read.id, bruno);
  const copyId = await duplicateFromPageMenu(b);
  const copyTitle = `${read.title} (copie)`;
  await expect(titleOf(b)).toHaveValue(copyTitle);
  await expect(sectionOf(b, "Privé").locator(`[data-page-id="${copyId}"]`)).toBeVisible();
  // Elle est à lui : il peut la modifier.
  await expect(b.locator(".mm-editor")).toHaveAttribute("data-editable", "true");

  // Chez Alice, la page parente n'a toujours qu'une sous-page, dans sa barre comme sur le serveur.
  await renamedInTree(a, aliceApi, parent.id);
  expect((await childrenOf(aliceApi, parent.id)).map((node) => node.id)).toEqual([read.id]);
  await expect(treeRow(a, copyId)).toHaveCount(0);
  await expect(sidebarOf(a)).not.toContainText(copyTitle);
  // Témoin : la copie est bien sur le serveur, à la racine, chez Bruno seul.
  const copy = (await getTree(brunoApi, space.id)).nodes.find((node) => node.id === copyId);
  expect(copy).toMatchObject({ parentId: null, section: "private", level: "full" });
});

test("duplicated_page_keeps_its_images", async ({ browser }) => {
  // Une image envoyée dans la page : la copie a la sienne, à son adresse, et l'affiche encore une
  // fois l'original supprimé pour de bon.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Photos ${unique()}`,
  });
  const sent = await aliceApi.post(`/api/v1/pages/${page.id}/files?name=photo.png`, {
    data: PNG,
    headers: { "content-type": "application/octet-stream" },
  });
  expect(sent.status(), await sent.text()).toBe(201);
  const file = (await sent.json()) as FileInfo;
  await putMmd(aliceApi, page.id, `![](${file.url})\n`);

  const a = await openPage(browser, page.id, alice);
  await expect(a.locator(".mm-editor img")).toHaveAttribute("src", file.url);
  const copyId = await duplicateFromPageMenu(a);

  // L'original part à la corbeille, puis est supprimé pour de bon.
  expect((await aliceApi.post(`/api/v1/pages/${page.id}/trash`)).status()).toBe(200);
  const deleted = await aliceApi.delete(`/api/v1/pages/${page.id}`);
  expect(deleted.status(), await deleted.text()).toBe(204);
  expect((await aliceApi.get(file.url)).status()).toBe(404);

  // La copie, rouverte : son image nomme la copie, et s'affiche.
  await a.reload();
  await waitSynced(a);
  const image = a.locator(".mm-editor img");
  await expect(image).toHaveAttribute("src", new RegExp(`^/api/v1/pages/${copyId}/files/`));
  await expect
    .poll(() => image.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth))
    .toBe(1);
});

test("subpages_come_with_the_copy", async ({ browser }) => {
  // Une page et ses sous-pages, sur deux niveaux : la copie arrive avec les siennes, dans le même
  // ordre, et la barre latérale les montre sans recharger. La réponse ne porte que la page racine
  // de la copie : c'est l'onglet qui relit l'arbre. Son canal de l'espace est tenu ouvert et muet,
  // si bien que ni lui ni le minuteur de repli (qui se tait tant que le canal vit) ne le relisent
  // à sa place ; le chien de garde du canal (45 s) est loin.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Projet ${unique()}`,
  });
  const first = await createPage(aliceApi, { parentId: page.id, title: "Première" });
  await createPage(aliceApi, { parentId: first.id, title: "Détail" });
  await createPage(aliceApi, { parentId: page.id, title: "Seconde" });

  const a = await newUser(browser, alice);
  let channels = 0;
  await a.routeWebSocket(/\/events\//, (ws) => {
    channels += 1;
    // Ni serveur, ni trame : la connexion reste ouverte, muette. Une seconde serait refusée.
    if (channels > 1) void ws.close();
  });
  await a.goto(`/s/${space.id}`);
  await expect(treeRow(a, first.id)).toBeVisible();
  const menu = await openRowMenu(a, page.id, page.title);
  await menu.getByRole("menuitem", { name: "Dupliquer" }).click();
  await expect(a).toHaveURL(/\/p\//);
  await waitSynced(a);
  const copyId = pageIdOf(a);
  expect(copyId).not.toBe(page.id);

  // Sur le serveur : la même forme, des pages neuves.
  const copied = await childrenOf(aliceApi, copyId);
  expect(copied.map((node) => node.title)).toEqual(["Première", "Seconde"]);
  expect(copied.map((node) => node.id)).not.toContain(first.id);
  const deeper = await childrenOf(aliceApi, copied[0].id);
  expect(deeper.map((node) => node.title)).toEqual(["Détail"]);
  // Dans la barre, sans recharger : chacune sous sa page parente, à son niveau.
  const levelOf = (id: string) => treeRow(a, id);
  await expect(levelOf(copied[0].id)).toHaveAttribute("aria-level", "2");
  await expect(levelOf(copied[1].id)).toHaveAttribute("aria-level", "2");
  await expect(levelOf(deeper[0].id)).toHaveAttribute("aria-level", "3");
  const rows = await sidebarOf(a).getByRole("treeitem").evaluateAll((items) =>
    items.map((item) => item.getAttribute("data-page-id")),
  );
  const at = rows.indexOf(copyId);
  expect(rows.slice(at, at + 4)).toEqual([copyId, copied[0].id, deeper[0].id, copied[1].id]);
  // Le canal est resté le même, muet, de bout en bout.
  expect(channels).toBe(1);
});

/** Le panneau des commentaires de la page ouverte, et son bouton. */
function commentsButton(p: Page): Locator {
  return p.getByRole("button", { name: /^Commentaires/ });
}
function commentsPanel(p: Page): Locator {
  return p.getByRole("complementary", { name: "Commentaires" });
}

/** Le message du pied de la barre latérale qui dit une copie prête, sans s'être ouverte (R646). */
function readyCopy(p: Page, title: string, where: string): Locator {
  return sidebarOf(p)
    .getByRole("status")
    .filter({ hasText: `La copie « \u2068${title}\u2069 » est prête, ${where}.` });
}

test("a_comment_draft_written_while_the_copy_is_made_stays_and_the_copy_is_said_ready", async ({
  browser,
}) => {
  // R646 récrit (relecture, C1) : pendant la requête, Alice écrit un commentaire, puis referme le
  // panneau (les brouillons survivent à sa fermeture, pas au départ de la page). La copie, prête,
  // ne s'ouvre pas par-dessus : un geste a eu lieu depuis la demande. Le pied de la barre latérale
  // dit qu'elle est prête, et où ; la copie y est ; le brouillon est là.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Brouillon ${unique()}`,
  });
  const a = await openPage(browser, page.id, alice);
  const copyTitle = `${page.title} (copie)`;

  let held = await holdFirstResponse(a, "POST", `/api/v1/pages/${page.id}/duplicate`);
  await pageMenuButton(a).click();
  await a.getByRole("menuitem", { name: "Dupliquer" }).click();
  expect(await held.answered).toBe(201);
  await commentsButton(a).click();
  await commentsPanel(a).getByRole("textbox", { name: "Commenter la page" }).fill("Mon brouillon");
  await commentsPanel(a).getByRole("button", { name: "Fermer les commentaires" }).click();
  await expect(commentsPanel(a)).toHaveCount(0);
  held.release();
  // La réponse est traitée quand le message paraît : la page n'a pas changé.
  await expect(readyCopy(a, copyTitle, "dans « Privé »")).toBeVisible();
  expect(pageIdOf(a)).toBe(page.id);
  await expect(sectionOf(a, "Privé").getByRole("link", { name: copyTitle })).toBeVisible();
  await commentsButton(a).click();
  await expect(commentsPanel(a).getByRole("textbox", { name: "Commenter la page" })).toHaveValue(
    "Mon brouillon",
  );

  // Variante : le brouillon, puis Échap, qui laisse le panneau ouvert et le brouillon visible.
  await a.unrouteAll({ behavior: "wait" });
  held = await holdFirstResponse(a, "POST", `/api/v1/pages/${page.id}/duplicate`);
  await pageMenuButton(a).click();
  await a.getByRole("menuitem", { name: "Dupliquer" }).click();
  expect(await held.answered).toBe(201);
  const field = commentsPanel(a).getByRole("textbox", { name: "Commenter la page" });
  await field.fill("D'accord");
  await field.press("Escape");
  held.release();
  await expect(sectionOf(a, "Privé").getByRole("link", { name: copyTitle })).toHaveCount(2);
  await expect(readyCopy(a, copyTitle, "dans « Privé »")).toBeVisible();
  expect(pageIdOf(a)).toBe(page.id);
  if ((await commentsPanel(a).count()) === 0) await commentsButton(a).click();
  await expect(commentsPanel(a).getByRole("textbox", { name: "Commenter la page" })).toHaveValue(
    "D'accord",
  );
  // Le message part à la navigation suivante, comme les autres.
  await sectionOf(a, "Privé").getByRole("link", { name: copyTitle }).first().click();
  await expect(readyCopy(a, copyTitle, "dans « Privé »")).toHaveCount(0);
});

test("a_menu_open_when_the_copy_arrives_keeps_its_focus", async ({ browser }) => {
  // Relecture, C3 : pendant la requête, Alice ouvre le menu d'une autre ligne (« Dupliquer » y est
  // grisé, R644). La copie prête ne lui prend pas le focus : la touche suivante, prévue pour le menu,
  // ne remplace pas le titre de la copie.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const x = await createPage(aliceApi, { spaceId: space.id, section: "private", title: `X ${unique()}` });
  const y = await createPage(aliceApi, { spaceId: space.id, section: "private", title: `Y ${unique()}` });
  const a = await newUser(browser, alice);
  await a.goto(`/s/${space.id}`);
  await expect(treeRow(a, y.id)).toBeVisible();
  const held = await holdFirstResponse(a, "POST", `/api/v1/pages/${x.id}/duplicate`);
  await (await openRowMenu(a, x.id, x.title)).getByRole("menuitem", { name: "Dupliquer" }).click();
  expect(await held.answered).toBe(201);
  const menuY = await openRowMenu(a, y.id, y.title);
  await expect(menuY.getByRole("menuitem", { name: "Dupliquer" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  const first = menuY.getByRole("menuitem").first();
  await expect(first).toBeFocused();
  held.release();
  await expect(readyCopy(a, `${x.title} (copie)`, "dans « Privé »")).toBeVisible();
  await expect(first).toBeFocused();
  expect(new URL(a.url()).pathname).toBe(`/s/${space.id}`);
  // L'entrée est revenue, à sa place, menu ouvert (R466).
  await expect(menuY.getByRole("menuitem", { name: "Dupliquer" })).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("a_lost_answer_says_the_copy_may_have_been_made", async ({ browser }) => {
  // Relecture, C2 : le serveur répond 201, mais la réponse est coupée avant l'onglet. Ni « n'a pas
  // pu être dupliquée », ni invitation à réessayer comme pour un refus : le message dit que la copie
  // a peut-être été faite, et la barre latérale, relue, la montre.
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const page = await createPage(aliceApi, { spaceId: space.id, section: "private", title: `R ${unique()}` });
  const a = await openPage(browser, page.id, alice);
  let status = 0;
  await a.route(`**/api/v1/pages/${page.id}/duplicate`, async (route) => {
    const response = await route.fetch();
    status = response.status();
    await route.abort("connectionreset");
  });
  await pageMenuButton(a).click();
  await a.getByRole("menuitem", { name: "Dupliquer" }).click();
  await expect(sidebarOf(a).getByRole("alert")).toHaveText(
    "La réponse du serveur n'est pas arrivée : la copie a peut-être été faite. Regarde dans la barre latérale avant de réessayer.",
  );
  expect(status).toBe(201);
  await expect(sectionOf(a, "Privé").getByRole("link", { name: `${page.title} (copie)` })).toBeVisible();
  expect(pageIdOf(a)).toBe(page.id);
});

test("duplicate_is_offered_exactly_where_the_server_accepts_it", async ({ browser }) => {
  // Relecture, C4 : la règle (`duplicateOffered`), telle que les deux menus l'appliquent, contre le
  // serveur. Pour chaque cas : « Dupliquer » dans le menu de la page, dans celui de la ligne, et ce
  // que répond le serveur à la même demande. Bruno est membre ; Gaby, invitée.
  test.setTimeout(180_000);
  const { alice, aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const u = unique();
  const make = (body: Parameters<typeof createPage>[1]) => createPage(aliceApi, body);
  // Pour Bruno : une page d'« Espace » et sa sous-page ; une page privée d'Alice en lecture, une en
  // commentaire, avec leurs sous-pages ; une sous-page dont la page parente lui est invisible.
  const team = await make({ spaceId: space.id, title: `Équipe ${u}` });
  const teamSub = await make({ parentId: team.id, title: `Équipe-sous ${u}` });
  const read = await make({ spaceId: space.id, section: "private", title: `Lue ${u}` });
  const readSub = await make({ parentId: read.id, title: `Lue-sous ${u}` });
  await setPageLevel(aliceApi, read.id, brunoId, "read");
  const commented = await make({ spaceId: space.id, section: "private", title: `Commentée ${u}` });
  const commentedSub = await make({ parentId: commented.id, title: `Commentée-sous ${u}` });
  await setPageLevel(aliceApi, commented.id, brunoId, "comment");
  const hidden = await make({ spaceId: space.id, section: "private", title: `Cachée ${u}` });
  const orphan = await make({ parentId: hidden.id, title: `Orpheline ${u}` });
  await setPageLevel(aliceApi, orphan.id, brunoId, "read");
  // Pour Gaby : une page qu'elle peut modifier et sa sous-page ; une page en lecture, une en
  // commentaire, et leurs sous-pages ; une sous-page modifiable dont la page parente lui est
  // invisible.
  const edited = await make({ spaceId: space.id, section: "private", title: `Modifiable ${u}` });
  const editedSub = await make({ parentId: edited.id, title: `Modifiable-sous ${u}` });
  const gaby = await guestOf(alice, edited.id, `Gaby ${u}`, "edit");
  const gabyApi = await apiAs(gaby);
  const invite = async (pageId: string, level: Level) => {
    const { token } = await inviteToPage(aliceApi, pageId, {
      email: `gaby-${unique()}@exemple.fr`,
      level,
    });
    await acceptInvitation(gabyApi, token);
  };
  const gRead = await make({ spaceId: space.id, section: "private", title: `G-lue ${u}` });
  const gReadSub = await make({ parentId: gRead.id, title: `G-lue-sous ${u}` });
  await invite(gRead.id, "read");
  const gCommented = await make({ spaceId: space.id, section: "private", title: `G-commentée ${u}` });
  const gCommentedSub = await make({ parentId: gCommented.id, title: `G-commentée-sous ${u}` });
  await invite(gCommented.id, "comment");
  const gHidden = await make({ spaceId: space.id, section: "private", title: `G-cachée ${u}` });
  const gOrphan = await make({ parentId: gHidden.id, title: `G-orpheline ${u}` });
  await invite(gOrphan.id, "edit");

  const cases: { who: "bruno" | "gaby"; node: PageNode; offered: boolean }[] = [
    { who: "bruno", node: team, offered: true },
    { who: "bruno", node: teamSub, offered: true },
    { who: "bruno", node: read, offered: true },
    { who: "bruno", node: readSub, offered: true },
    { who: "bruno", node: commented, offered: true },
    { who: "bruno", node: commentedSub, offered: true },
    { who: "bruno", node: orphan, offered: true },
    { who: "gaby", node: edited, offered: false },
    { who: "gaby", node: editedSub, offered: true },
    { who: "gaby", node: gRead, offered: false },
    { who: "gaby", node: gReadSub, offered: false },
    { who: "gaby", node: gCommented, offered: false },
    { who: "gaby", node: gCommentedSub, offered: false },
    { who: "gaby", node: gOrphan, offered: false },
  ];
  const tabs = { bruno: await newUser(browser, bruno), gaby: await newUser(browser, gaby) };
  const apis = { bruno: brunoApi, gaby: gabyApi };
  const seen: string[] = [];
  const wanted: string[] = [];
  for (const { who, node, offered } of cases) {
    const p = tabs[who];
    await p.goto(`/p/${node.id}`);
    await expect(titleOf(p)).toHaveValue(node.title);
    // L'arbre est lu : la page parente de la page ouverte y est, si le compte la voit.
    await expect(treeRow(p, node.id)).toBeVisible();
    await pageMenuButton(p).click();
    const inPage = (await p.getByRole("menu").getByRole("menuitem").allTextContents()).includes(
      "Dupliquer",
    );
    await p.keyboard.press("Escape");
    const menu = await openRowMenu(p, node.id, node.title);
    const inRow = (await menu.getByRole("menuitem").allTextContents()).includes("Dupliquer");
    await p.keyboard.press("Escape");
    const status = (await apis[who].post(`/api/v1/pages/${node.id}/duplicate`)).status();
    const line = (page: boolean, row: boolean, answer: number) =>
      `${who} « ${node.title} » : page ${page}, ligne ${row}, serveur ${answer}`;
    seen.push(line(inPage, inRow, status));
    wanted.push(line(offered, offered, offered ? 201 : 403));
  }
  expect(seen).toEqual(wanted);

  // Une page à la corbeille par sa page parente : ni l'entrée, ni la copie (`409`).
  expect((await aliceApi.post(`/api/v1/pages/${team.id}/trash`)).status()).toBe(200);
  const b = tabs.bruno;
  await b.goto(`/p/${teamSub.id}`);
  await expect(titleOf(b)).toHaveValue(teamSub.title);
  await expect(b.getByRole("status").filter({ hasText: "Cette page est à la corbeille" })).toBeVisible();
  await pageMenuButton(b).click();
  await expect(b.getByRole("menu").getByRole("menuitem", { name: "Dupliquer" })).toHaveCount(0);
  await b.keyboard.press("Escape");
  expect((await brunoApi.post(`/api/v1/pages/${teamSub.id}/duplicate`)).status()).toBe(409);

  // Un onglet d'un autre schéma que le serveur (R35) : aucune création, ni dans le menu de la page,
  // ni dans celui de la ligne, pour la page même que le serveur laisserait copier.
  const stale = await newUser(browser, bruno);
  await serverSchemaBumped(stale);
  await stale.goto(`/p/${read.id}`);
  await expect(stale.getByText(READ_ONLY_BANNER)).toBeVisible();
  await expect(treeRow(stale, read.id)).toBeVisible();
  await pageMenuButton(stale).click();
  await expect(stale.getByRole("menu").getByRole("menuitem")).toHaveText(["Ajouter aux favoris"]);
  await stale.keyboard.press("Escape");
  const staleRow = await openRowMenu(stale, read.id, read.title);
  await expect(staleRow.getByRole("menuitem")).toHaveText(["Ajouter aux favoris"]);
});
