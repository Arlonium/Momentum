// Dupliquer une page (Ruling R252) : « Dupliquer », dans le menu « … » de la page ou de sa ligne,
// copie la page et ses sous-pages visibles dans des pages neuves. La copie va sous la page
// parente, juste après la page, si le compte peut y créer ; sinon, pour un membre, à la racine de
// son « Privé ». Elle s'ouvre chez qui l'a demandée, son titre prêt à être changé ; les autres la
// voient paraître par le canal de l'espace s'ils y ont accès, et jamais sinon. Ses images sont à
// elle : elles survivent à la suppression de l'original.
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import type { FileInfo, PageNode } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getTree,
  newUser,
  openPage,
  openRowMenu,
  ownerAndMember,
  pageIdOf,
  putMmd,
  sectionOf,
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
