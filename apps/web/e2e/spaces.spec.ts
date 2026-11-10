// Espaces et sections, depuis l'interface : le sélecteur d'espace, les trois sections de la
// barre latérale (« Espace », « Partagé », « Privé »), ce que chacun y voit, l'espace gardé
// d'une visite à l'autre, la corbeille, et le déplacement des pages, à la souris ou au
// clavier. Le serveur fait foi pour les droits : chaque test relit aussi ce qu'il répond.
import { expect, test, type Page } from "@playwright/test";
import type { ErrorBody, PageNode } from "@mementum/protocol";
import {
  apiAs,
  byPosition,
  closeContexts,
  createPage,
  createSpace,
  firstSpace,
  forbidNativeDialogs,
  getMe,
  getTree,
  memberOf,
  newUser,
  NO_SESSION,
  openRowMenu,
  pageIdOf,
  sectionOf,
  setPageLevel,
  shownRows,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  titleOf,
  trashBanner,
  trashItem,
  treeRow,
  unique,
  UNKNOWN_ID,
  waitSynced,
} from "./helpers";
import { BASE_URL } from "./servers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

/** Crée une page par le « + » d'une section, la nomme, et rend son id. */
async function addPageIn(p: Page, section: "Espace" | "Privé", title: string): Promise<string> {
  const before = new URL(p.url()).pathname;
  await sectionOf(p, section)
    .getByRole("button", { name: `Ajouter une page dans « ${section} »` })
    .click();
  await p.waitForURL((url) => url.pathname.startsWith("/p/") && url.pathname !== before);
  const id = pageIdOf(p);
  await titleOf(p).fill(title);
  await titleOf(p).press("Enter");
  await expect(treeRow(p, id).getByRole("link")).toHaveText(title);
  return id;
}

/** Crée un espace depuis le sélecteur : on y arrive. Rend son id, lu dans l'adresse. */
async function createSpaceFromSwitcher(p: Page, name: string): Promise<string> {
  await spaceSwitcher(p).click();
  await sidebarOf(p).getByRole("button", { name: "Créer un espace" }).click();
  const dialog = p.getByRole("dialog", { name: "Créer un espace" });
  const field = dialog.getByLabel("Nom de l'espace");
  await expect(field).toBeFocused();
  await field.fill(name);
  await field.press("Enter");
  await expect(dialog).toHaveCount(0);
  await expect(spaceSwitcher(p, name)).toBeVisible();
  const match = /^\/s\/([^/]+)$/.exec(new URL(p.url()).pathname);
  expect(match, `adresse d'un espace attendue : ${p.url()}`).not.toBeNull();
  return decodeURIComponent(match![1]);
}

test("two_spaces_are_isolated", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const p = await newUser(browser, alice);
  await p.goto("/");
  const tag = unique();

  // Un espace « Pro », créé depuis le sélecteur, et une page dans sa section « Espace ».
  const pro = await createSpaceFromSwitcher(p, `Pro ${tag}`);
  const title = `Budget ${tag}`;
  const pageId = await addPageIn(p, "Espace", title);
  await expect(sectionOf(p, "Espace").locator(`[data-page-id="${pageId}"]`)).toBeVisible();

  // Un espace « Perso » : rien de « Pro » n'y apparaît.
  const perso = await createSpaceFromSwitcher(p, `Perso ${tag}`);
  expect(perso).not.toBe(pro);
  await expect(sectionOf(p, "Espace")).toBeVisible();
  await expect(sidebarOf(p).getByRole("treeitem")).toHaveCount(0);
  await expect(sidebarOf(p).getByText(title)).toHaveCount(0);
  expect((await getTree(api, perso)).nodes).toEqual([]);
  expect((await getTree(api, pro)).nodes.map((n) => n.id)).toEqual([pageId]);

  // Le sélecteur liste les espaces du compte, avec son rôle ; « Pro » rouvre sa page.
  await spaceSwitcher(p).click();
  const spaces = sidebarOf(p).getByRole("list", { name: "Espaces" }).getByRole("link");
  await expect(spaces).toHaveCount(3);
  const proLink = spaces.filter({ hasText: `Pro ${tag}` });
  await expect(proLink).toContainText("Propriétaire");
  await expect(spaces.filter({ hasText: `Perso ${tag}` })).toHaveAttribute("aria-current", "true");
  await proLink.click();
  await expect(p).toHaveURL(`${BASE_URL}/s/${pro}`);
  await expect(spaceSwitcher(p, `Pro ${tag}`)).toHaveAttribute("aria-expanded", "false");
  await expect(treeRow(p, pageId).getByRole("link")).toHaveText(title);
});

test("private_page_is_invisible_to_another_member", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const bruno = await memberOf(alice, space.id, `Bruno ${unique()}`);
  const brunoApi = await apiAs(bruno);

  // A crée une page par le « + » de sa section « Privé ».
  const a = await newUser(browser, alice);
  await a.goto("/");
  const title = `Journal ${unique()}`;
  const id = await addPageIn(a, "Privé", title);
  await expect(sectionOf(a, "Privé").locator(`[data-page-id="${id}"]`)).toBeVisible();
  expect((await getTree(aliceApi, space.id)).nodes.find((n) => n.id === id)?.section).toBe(
    "private",
  );
  // Une page de la section « Espace », pour savoir que l'arbre de B est chargé.
  const common = await createPage(aliceApi, { title: `Commun ${unique()}` });

  // B, membre du même espace, ne la voit ni dans sa barre latérale, ni par son adresse.
  expect((await getTree(brunoApi, space.id)).nodes.map((n) => n.id)).toEqual([common.id]);
  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  await expect(treeRow(b, common.id)).toBeVisible();
  await expect(treeRow(b, id)).toHaveCount(0);
  await expect(sidebarOf(b).getByText(title)).toHaveCount(0);
  // Par son adresse : l'écran d'une page qui n'existe pas, mot pour mot.
  const missing = b.getByRole("heading", { name: "Page introuvable" });
  await b.goto(`/p/${id}`);
  await expect(missing).toBeVisible();
  const refused = await b.getByRole("main").innerText();
  expect(refused).not.toContain(title);
  await b.goto(`/p/${UNKNOWN_ID}`);
  await expect(missing).toBeVisible();
  expect(await b.getByRole("main").innerText()).toBe(refused);
});

test("space_page_is_visible_to_another_member", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const bruno = await memberOf(alice, space.id, `Bruno ${unique()}`);

  // A crée une page par le « + » de la section « Espace ».
  const a = await newUser(browser, alice);
  await a.goto("/");
  const title = `Feuille de route ${unique()}`;
  const id = await addPageIn(a, "Espace", title);
  await expect(sectionOf(a, "Espace").locator(`[data-page-id="${id}"]`)).toBeVisible();

  // B la voit dans sa section « Espace », l'ouvre et peut y écrire.
  const seen = (await getTree(await apiAs(bruno), space.id)).nodes.find((n) => n.id === id);
  expect(seen).toMatchObject({ section: "space", level: "full" });
  const b = await newUser(browser, bruno);
  await b.goto("/");
  await expect(b).toHaveURL(`${BASE_URL}/s/${space.id}`);
  const row = sectionOf(b, "Espace").locator(`[data-page-id="${id}"]`);
  await expect(row.getByRole("link")).toHaveText(title);
  await row.getByRole("link").click();
  await expect(b).toHaveURL(`${BASE_URL}/p/${id}`);
  await expect(titleOf(b)).toHaveValue(title);
  await waitSynced(b);
  await expect(b.locator(".bn-editor")).toHaveAttribute("contenteditable", "true");
});

test("guest_sees_only_the_shared_section", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  // B, d'abord membre, a une page à lui ; A en a une pour tout l'espace.
  const bruno = await memberOf(alice, space.id, `Bruno ${unique()}`);
  const brunoApi = await apiAs(bruno);
  const own = await createPage(brunoApi, {
    spaceId: space.id,
    section: "private",
    title: `À Bruno ${unique()}`,
  });
  const forAll = await createPage(aliceApi, { title: `Pour l'espace ${unique()}` });
  // Par l'API, A fait de B un invité : il ne lui reste que l'accès posé sur sa page.
  const { user } = await getMe(brunoApi);
  const res = await aliceApi.patch(`/api/v1/spaces/${space.id}/members/${user.id}`, {
    data: { role: "guest" },
  });
  expect(res.status(), await res.text()).toBe(200);
  const tree = await getTree(brunoApi, space.id);
  expect(tree.nodes.map((n) => [n.id, n.section])).toEqual([[own.id, "shared"]]);

  const b = await newUser(browser, bruno);
  await b.goto("/");
  await expect(b).toHaveURL(`${BASE_URL}/s/${space.id}`);
  // Une seule section, « Partagé », avec sa page ; rien de la section « Espace ».
  const shared = sectionOf(b, "Partagé");
  await expect(shared.locator(`[data-page-id="${own.id}"]`).getByRole("link")).toHaveText(own.title);
  await expect(sectionOf(b, "Espace")).toHaveCount(0);
  await expect(sectionOf(b, "Privé")).toHaveCount(0);
  await expect(sidebarOf(b).getByRole("treeitem")).toHaveCount(1);
  await expect(sidebarOf(b).getByText(forAll.title)).toHaveCount(0);
  // Un invité ne crée pas de page à la racine : aucun bouton ne le propose.
  await expect(sidebarOf(b).getByRole("button", { name: "Nouvelle page" })).toHaveCount(0);
  await expect(sidebarOf(b).getByRole("button", { name: /^Ajouter une page dans/ })).toHaveCount(0);
  await expect(b.getByRole("main").getByRole("button", { name: "Nouvelle page" })).toHaveCount(0);
  // Le sélecteur dit son rôle, et ne mène pas aux réglages de l'espace.
  await spaceSwitcher(b).click();
  const spaces = sidebarOf(b).getByRole("list", { name: "Espaces" }).getByRole("link");
  await expect(spaces).toHaveCount(1);
  await expect(spaces).toContainText("Invité");
  await expect(sidebarOf(b).getByRole("link", { name: "Réglages de l'espace" })).toHaveCount(0);
  await b.keyboard.press("Escape");
  // Sa page s'ouvre toujours.
  await shared.locator(`[data-page-id="${own.id}"]`).getByRole("link").click();
  await expect(titleOf(b)).toHaveValue(own.title);
});

test("last_space_is_remembered", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const first = await firstSpace(api);
  const second = await createSpace(api, `Pro ${unique()}`);
  const stored = (p: Page) => p.evaluate(() => localStorage.getItem("mementum.space"));

  // Jamais venu : le premier espace du compte.
  const p = await newUser(browser, alice);
  await p.goto("/");
  await expect(p).toHaveURL(`${BASE_URL}/s/${first.id}`);
  await expect(spaceSwitcher(p, first.name)).toBeVisible();
  expect(await stored(p)).toBe(first.id);

  // Le second espace, ouvert par le sélecteur, est gardé.
  await spaceSwitcher(p).click();
  await sidebarOf(p).getByRole("list", { name: "Espaces" }).getByRole("link", { name: second.name }).click();
  await expect(p).toHaveURL(`${BASE_URL}/s/${second.id}`);
  await expect.poll(() => stored(p)).toBe(second.id);

  // De retour à l'accueil, ou après un rechargement : le dernier espace ouvert.
  await p.goto("/");
  await expect(p).toHaveURL(`${BASE_URL}/s/${second.id}`);
  await expect(spaceSwitcher(p, second.name)).toBeVisible();
  // Dans un autre onglet du même navigateur aussi.
  const other = await p.context().newPage();
  await other.goto("/");
  await expect(other).toHaveURL(`${BASE_URL}/s/${second.id}`);

  // Un espace gardé où le compte n'a pas de rôle : le premier.
  await p.evaluate((id) => localStorage.setItem("mementum.space", id), UNKNOWN_ID);
  await p.goto("/");
  await expect(p).toHaveURL(`${BASE_URL}/s/${first.id}`);
});

test("page_link_opens_its_own_space", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const first = await firstSpace(api);
  const second = await createSpace(api, `Pro ${unique()}`);
  const inFirst = await createPage(api, { title: `Ici ${unique()}` });
  const inSecond = await createPage(api, { spaceId: second.id, title: `Là-bas ${unique()}` });

  const p = await newUser(browser, alice);
  await p.goto(`/p/${inFirst.id}`);
  await expect(spaceSwitcher(p, first.name)).toBeVisible();
  await expect(treeRow(p, inFirst.id).getByRole("link")).toHaveAttribute("aria-current", "page");

  // L'adresse d'une page d'un autre espace : la barre latérale montre cet espace.
  await p.goto(`/p/${inSecond.id}`);
  await expect(titleOf(p)).toHaveValue(inSecond.title);
  await expect(spaceSwitcher(p, second.name)).toBeVisible();
  await expect(treeRow(p, inSecond.id).getByRole("link")).toHaveAttribute("aria-current", "page");
  await expect(treeRow(p, inFirst.id)).toHaveCount(0);
  expect(await p.evaluate(() => localStorage.getItem("mementum.space"))).toBe(second.id);
});

test("space_without_a_role_is_not_found", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const foreign = await firstSpace(await apiAs(alice));
  const own = await firstSpace(await apiAs(bruno));

  const b = await newUser(browser, bruno);
  const missing = b.getByRole("heading", { name: "Espace introuvable", level: 1 });
  // Un espace qui n'existe pas : l'écran le dit, à cette adresse (un lien collé ne mène pas
  // ailleurs sans un mot). La barre latérale reste celle d'un espace du compte.
  await b.goto(`/s/${UNKNOWN_ID}`);
  await expect(missing).toBeVisible();
  await expect(b).toHaveURL(`${BASE_URL}/s/${UNKNOWN_ID}`);
  await expect(spaceSwitcher(b, own.name)).toBeVisible();
  const unknown = await b.locator("body").innerText();

  // Un espace où B n'a aucun rôle : exactement le même écran, sans rien de cet espace, pour
  // son accueil, sa corbeille et ses réglages.
  for (const path of [`/s/${foreign.id}`, `/s/${foreign.id}/trash`, `/s/${foreign.id}/settings`]) {
    await b.goto(path);
    await expect(missing).toBeVisible();
    await expect(b).toHaveURL(`${BASE_URL}${path}`);
    expect(await b.locator("body").innerText()).toBe(unknown);
  }
  expect(unknown).not.toContain(foreign.name);

  // Son bouton mène au premier espace du compte.
  await b.getByRole("main").getByRole("button", { name: `Ouvrir « ${own.name} »` }).click();
  await expect(b).toHaveURL(`${BASE_URL}/s/${own.id}`);
  await expect(b.getByRole("heading", { name: "Aucune page ouverte" })).toBeVisible();
});

test("collapsed_section_is_remembered_per_space", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const first = await firstSpace(api);
  const second = await createSpace(api, `Pro ${unique()}`);
  const inFirst = await createPage(api, { title: `Ici ${unique()}` });
  const inSecond = await createPage(api, { spaceId: second.id, title: `Là-bas ${unique()}` });
  const mine = await createPage(api, { section: "private", title: `À moi ${unique()}` });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${first.id}`);
  const toggle = sectionOf(p, "Espace").getByRole("button", { name: "Espace", exact: true });
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(treeRow(p, inFirst.id)).toBeVisible();
  const privateTop = async () => (await treeRow(p, mine.id).boundingBox())!.y;
  const before = await privateTop();

  // Repliée : ses pages ne sont plus là, et la section suivante remonte d'autant.
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(treeRow(p, inFirst.id)).toHaveCount(0);
  await expect(treeRow(p, mine.id)).toBeVisible();
  expect(await privateTop()).toBeLessThan(before);

  // Gardé au rechargement.
  await p.reload();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(treeRow(p, mine.id)).toBeVisible();
  await expect(treeRow(p, inFirst.id)).toHaveCount(0);

  // Par espace : dans l'autre, la section est dépliée.
  await p.goto(`/s/${second.id}`);
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(treeRow(p, inSecond.id)).toBeVisible();

  // De retour dans le premier : toujours repliée ; un clic la rouvre.
  await p.goto(`/s/${first.id}`);
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(treeRow(p, inFirst.id)).toBeVisible();
});

test("trash_and_restore_from_the_sidebar", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const parent = await createPage(api, { title: `Projet ${unique()}` });
  const child = await createPage(api, { parentId: parent.id, title: `Notes ${unique()}` });
  const other = await createPage(api, { title: `Autre ${unique()}` });
  const visible = async () => (await getTree(api, space.id)).nodes.map((n) => n.id).sort();
  const trashedAt = async (id: string) => {
    const res = await api.get(`/api/v1/pages/${id}`);
    expect(res.status(), await res.text()).toBe(200);
    return ((await res.json()) as PageNode).trashedAt;
  };

  const a = await newUser(browser, alice);
  const nativeDialogs = forbidNativeDialogs(a);
  await a.goto(`/p/${parent.id}`);
  await waitSynced(a);
  const editor = a.locator(".bn-editor");
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(treeRow(a, child.id)).toBeVisible();

  // « Mettre à la corbeille », dans le menu « … » de la ligne : la page part avec sa sous-page.
  const menu = await openRowMenu(a, parent.id, parent.title);
  await menu.getByRole("menuitem", { name: "Mettre à la corbeille" }).click();
  await expect(menu).toHaveCount(0);
  await expect(treeRow(a, parent.id)).toHaveCount(0);
  await expect(treeRow(a, child.id)).toHaveCount(0);
  await expect(treeRow(a, other.id)).toBeVisible();
  expect(await visible()).toEqual([other.id]);
  expect(await trashedAt(parent.id)).not.toBeNull();
  // La page ouverte le dit, et ne s'écrit plus.
  await expect(trashBanner(a)).toBeVisible();
  await expect(titleOf(a)).toHaveValue(parent.title);
  await expect(titleOf(a)).toHaveAttribute("readonly", "");
  await expect(editor).toHaveAttribute("contenteditable", "false");

  // « Corbeille », en bas de la barre latérale : la page jetée y est, pas sa sous-page, qui
  // n'y est que par elle.
  await sidebarOf(a).getByRole("link", { name: "Corbeille" }).click();
  await expect(a).toHaveURL(`${BASE_URL}/s/${space.id}/trash`);
  await expect(a.getByRole("heading", { name: "Corbeille", level: 1 })).toBeVisible();
  await expect(trashItem(a, parent.title)).toBeVisible();
  await expect(trashItem(a, child.title)).toHaveCount(0);

  // « Restaurer » : elle revient dans la barre latérale, avec sa sous-page.
  await trashItem(a, parent.title).getByRole("button", { name: "Restaurer" }).click();
  await expect(trashItem(a, parent.title)).toHaveCount(0);
  await expect(a.getByText("La corbeille est vide.")).toBeVisible();
  await expect(treeRow(a, parent.id).getByRole("link")).toHaveText(parent.title);
  await expect(treeRow(a, child.id)).toHaveAttribute("aria-level", "2");
  expect(await visible()).toEqual([parent.id, child.id, other.id].sort());
  expect(await trashedAt(parent.id)).toBeNull();

  // Une sous-page partie avec sa page parente le dit aussi ; c'est la page parente qu'on
  // restaure, et le serveur le dit à qui essaie sur la sous-page.
  await treeRow(a, child.id).getByRole("link").click();
  await waitSynced(a);
  await (await openRowMenu(a, parent.id, parent.title))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(trashBanner(a)).toBeVisible();
  await expect(titleOf(a)).toHaveValue(child.title);
  await expect(editor).toHaveAttribute("contenteditable", "false");
  expect(await trashedAt(child.id)).not.toBeNull();
  const refused = await api.post(`/api/v1/pages/${child.id}/restore`);
  expect(refused.status()).toBe(409);
  const body = (await refused.json()) as ErrorBody;
  expect(body.error).toBe("parent_trashed");
  await trashBanner(a).getByRole("button", { name: "Restaurer" }).click();
  await expect(a.getByRole("main").getByRole("alert")).toHaveText(body.message);
  await expect(trashBanner(a)).toBeVisible();

  // La page parente, ouverte depuis la corbeille, se restaure par son bandeau.
  await sidebarOf(a).getByRole("link", { name: "Corbeille" }).click();
  await trashItem(a, parent.title).getByRole("link", { name: parent.title }).click();
  await expect(a).toHaveURL(`${BASE_URL}/p/${parent.id}`);
  await trashBanner(a).getByRole("button", { name: "Restaurer" }).click();
  await expect(trashBanner(a)).toHaveCount(0);
  await expect(treeRow(a, parent.id).getByRole("link")).toHaveAttribute("aria-current", "page");
  await expect(treeRow(a, child.id)).toBeVisible();
  await waitSynced(a);
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(titleOf(a)).not.toHaveAttribute("readonly", "");
  expect(await visible()).toEqual([parent.id, child.id, other.id].sort());
  expect(nativeDialogs).toEqual([]);
});

test("delete_forever_asks_first", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const parent = await createPage(api, { title: `Brouillon ${unique()}` });
  const child = await createPage(api, { parentId: parent.id, title: `Annexe ${unique()}` });
  const kept = await createPage(api, { title: `À garder ${unique()}` });
  for (const id of [parent.id, kept.id]) {
    expect((await api.post(`/api/v1/pages/${id}/trash`)).status()).toBe(200);
  }
  const status = async (id: string) => (await api.get(`/api/v1/pages/${id}`)).status();

  const a = await newUser(browser, alice);
  const nativeDialogs = forbidNativeDialogs(a);
  await a.goto(`/s/${space.id}/trash`);
  // La plus récente d'abord.
  await expect(
    a.getByRole("list", { name: "Pages à la corbeille" }).getByRole("listitem"),
  ).toHaveText([new RegExp(kept.title), new RegExp(parent.title)]);
  const remove = trashItem(a, parent.title).getByRole("button", { name: "Supprimer définitivement" });
  const confirm = a.getByRole("alertdialog", {
    name: `Supprimer définitivement « ${parent.title} » ?`,
  });

  // La suppression demande une confirmation dans la page. Le focus est sur « Annuler » :
  // Entrée ne supprime rien.
  await remove.click();
  await expect(confirm).toBeVisible();
  await expect(confirm.getByRole("button", { name: "Annuler" })).toBeFocused();
  await a.keyboard.press("Enter");
  await expect(confirm).toHaveCount(0);
  // Échap annule aussi.
  await remove.click();
  await expect(confirm).toBeVisible();
  await a.keyboard.press("Escape");
  await expect(confirm).toHaveCount(0);
  await expect(trashItem(a, parent.title)).toBeVisible();
  expect(await status(parent.id)).toBe(200);
  expect(await status(child.id)).toBe(200);

  // Confirmée : la page et sa sous-page n'existent plus ; l'autre est toujours à la corbeille.
  await remove.click();
  await confirm.getByRole("button", { name: "Supprimer définitivement" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(trashItem(a, parent.title)).toHaveCount(0);
  await expect(trashItem(a, kept.title)).toBeVisible();
  // Le bouton qui a ouvert la fenêtre est parti avec sa ligne : le focus va au titre.
  await expect(a.getByRole("heading", { name: "Corbeille", level: 1 })).toBeFocused();
  expect(await status(parent.id)).toBe(404);
  expect(await status(child.id)).toBe(404);
  expect(await status(kept.id)).toBe(200);
  const trash = await api.get(`/api/v1/spaces/${space.id}/trash`);
  expect(((await trash.json()) as PageNode[]).map((n) => n.id)).toEqual([kept.id]);
  expect(nativeDialogs).toEqual([]);
});

test("drag_a_page_under_another", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });
  const mine = await createPage(api, { section: "private", title: "Delta" });
  /** L'arbre du serveur : sous chaque page parente (ou à la racine), ses pages dans l'ordre. */
  const serverTree = async () => {
    const { nodes } = await getTree(api, space.id);
    const under: Record<string, string[]> = {};
    for (const node of byPosition(nodes)) {
      const parent = nodes.find((n) => n.id === node.parentId)?.title ?? "racine";
      (under[parent] ??= []).push(node.title);
    }
    return under;
  };

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie", "1 Delta"]);

  // « Charlie », lâchée au milieu de la ligne de « Bravo », devient sa sous-page.
  await treeRow(p, c.id).dragTo(treeRow(p, b.id));
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "2 Charlie", "1 Delta"]);
  await expect(treeRow(p, b.id)).toHaveAttribute("aria-expanded", "true");
  expect((await getTree(api, space.id)).nodes.find((n) => n.id === c.id)?.parentId).toBe(b.id);
  expect(await serverTree()).toEqual({ racine: ["Alpha", "Bravo", "Delta"], Bravo: ["Charlie"] });

  // « Bravo », lâchée sur le bord haut de « Alpha », passe avant elle, avec sa sous-page.
  await treeRow(p, b.id).dragTo(treeRow(p, a.id), { targetPosition: { x: 80, y: 2 } });
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "2 Charlie", "1 Alpha", "1 Delta"]);
  expect(await serverTree()).toEqual({ racine: ["Bravo", "Alpha", "Delta"], Bravo: ["Charlie"] });

  // Refusés, et sans effet : une page sous sa propre sous-page, et d'une section à l'autre.
  const before = await serverTree();
  await treeRow(p, b.id).dragTo(treeRow(p, c.id));
  await treeRow(p, mine.id).dragTo(treeRow(p, a.id));
  await treeRow(p, a.id).dragTo(treeRow(p, mine.id));
  await p.waitForTimeout(300);
  expect(await shownRows(p)).toEqual(["1 Bravo", "2 Charlie", "1 Alpha", "1 Delta"]);
  expect(await serverTree()).toEqual(before);
  await expect(sidebarOf(p).getByRole("alert")).toHaveCount(0);

  // Tout est gardé au rechargement.
  await p.reload();
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "2 Charlie", "1 Alpha", "1 Delta"]);
});

test("refused_move_comes_back_with_the_server_message", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo"]);

  // Pendant que la barre latérale est ouverte, « Bravo » part à la corbeille ailleurs.
  expect((await api.post(`/api/v1/pages/${b.id}/trash`)).status()).toBe(200);
  // Ce que le serveur répond désormais à qui y range une page.
  const refused = await api.patch(`/api/v1/pages/${a.id}`, { data: { parentId: b.id } });
  expect(refused.ok()).toBe(false);
  const body = (await refused.json()) as ErrorBody;

  // Le dépôt est refusé : le message du serveur, la page à sa place, et la barre latérale
  // telle que le serveur la voit.
  await treeRow(p, a.id).dragTo(treeRow(p, b.id));
  await expect(sidebarOf(p).getByRole("alert")).toContainText(body.message, { ignoreCase: true });
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha"]);
  expect((await getTree(api, space.id)).nodes.map((n) => [n.id, n.parentId])).toEqual([[a.id, null]]);
  // Le message part à la navigation suivante.
  await treeRow(p, a.id).getByRole("link").click();
  await expect(titleOf(p)).toHaveValue("Alpha");
  await expect(sidebarOf(p).getByRole("alert")).toHaveCount(0);
});

test("row_menu_by_keyboard", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  await createPage(api, { title: "Charlie" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  const button = treeRow(p, b.id).getByRole("button", { name: "Actions de « Bravo »" });
  const menu = p.getByRole("menu", { name: "Actions de « Bravo »" });
  const item = (name: string) => menu.getByRole("menuitem", { name });

  // Ouvert par Entrée : le focus est sur sa première action ; les flèches passent de l'une à
  // l'autre, en boucle ; Fin et Début vont aux extrémités.
  await button.focus();
  await p.keyboard.press("Enter");
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  await p.keyboard.press("ArrowDown");
  await expect(item("Déplacer vers le bas")).toBeFocused();
  await p.keyboard.press("ArrowDown");
  await expect(item("Imbriquer dans la page précédente")).toBeFocused();
  // « Sortir de la page parente » n'a pas de sens à la racine : grisée, les flèches la passent.
  await expect(item("Sortir de la page parente")).toHaveAttribute("aria-disabled", "true");
  await p.keyboard.press("ArrowDown");
  await expect(item("Mettre à la corbeille")).toBeFocused();
  await p.keyboard.press("ArrowDown");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  await p.keyboard.press("End");
  await expect(item("Mettre à la corbeille")).toBeFocused();
  await p.keyboard.press("ArrowUp");
  await expect(item("Imbriquer dans la page précédente")).toBeFocused();
  await p.keyboard.press("Home");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  // Échap le ferme et rend le focus à son bouton.
  await p.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-expanded", "false");

  // « Déplacer vers le haut » : la page monte d'un cran, sur le serveur aussi.
  await p.keyboard.press("Enter");
  await expect(item("Déplacer vers le haut")).toBeFocused();
  await p.keyboard.press("Enter");
  await expect(menu).toHaveCount(0);
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "1 Alpha", "1 Charlie"]);
  await expect(button).toBeFocused();
  const titles = async () =>
    byPosition((await getTree(api, space.id)).nodes).map((n) => n.title);
  expect(await titles()).toEqual(["Bravo", "Alpha", "Charlie"]);

  // En tête, elle ne monte pas plus haut : l'action est grisée, et le focus va à la suivante.
  await p.keyboard.press("Enter");
  await expect(item("Déplacer vers le haut")).toHaveAttribute("aria-disabled", "true");
  await expect(item("Déplacer vers le bas")).toBeFocused();
  // « Déplacer vers le bas », deux fois : elle finit dernière.
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  await p.keyboard.press("Enter");
  await p.keyboard.press("ArrowDown");
  await expect(item("Déplacer vers le bas")).toBeFocused();
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Charlie", "1 Bravo"]);
  expect(await titles()).toEqual(["Alpha", "Charlie", "Bravo"]);
  // Tab ferme le menu.
  await p.keyboard.press("Enter");
  await expect(menu).toBeVisible();
  await p.keyboard.press("Tab");
  await expect(menu).toHaveCount(0);
});

test("row_menu_nests_and_unnests_by_keyboard", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const c = await createPage(api, { title: "Charlie" });
  const d = await createPage(api, { parentId: c.id, title: "Delta" });
  const a = await createPage(api, { section: "private", title: "Alpha" });
  const b = await createPage(api, { section: "private", title: "Bravo" });
  const parentOf = async (id: string) =>
    (await getTree(api, space.id)).nodes.find((n) => n.id === id)?.parentId;

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  const start = ["1 Charlie", "2 Delta", "1 Alpha", "1 Bravo"];
  await expect.poll(() => shownRows(p)).toEqual(start);
  const button = treeRow(p, b.id).getByRole("button", { name: "Actions de « Bravo »" });
  const item = (title: string, name: string) =>
    p.getByRole("menu", { name: `Actions de « ${title} »` }).getByRole("menuitem", { name });
  const nest = "Imbriquer dans la page précédente";
  const unnest = "Sortir de la page parente";

  // Dans « Privé ». « Bravo », dernière : ni plus bas, ni hors d'une page parente.
  await button.focus();
  await p.keyboard.press("Enter");
  await expect(item("Bravo", "Déplacer vers le haut")).toBeFocused();
  await expect(item("Bravo", "Déplacer vers le bas")).toHaveAttribute("aria-disabled", "true");
  await expect(item("Bravo", unnest)).toHaveAttribute("aria-disabled", "true");
  // « Imbriquer dans la page précédente » : elle devient la sous-page de « Alpha », qui se
  // déplie ; le focus reste sur son bouton.
  await p.keyboard.press("ArrowDown");
  await expect(item("Bravo", nest)).toBeFocused();
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(["1 Charlie", "2 Delta", "1 Alpha", "2 Bravo"]);
  await expect(treeRow(p, a.id)).toHaveAttribute("aria-expanded", "true");
  expect(await parentOf(b.id)).toBe(a.id);
  await expect(button).toBeFocused();

  // Seule sous sa page parente : il ne lui reste qu'à en sortir, juste après elle.
  await p.keyboard.press("Enter");
  await expect(item("Bravo", unnest)).toBeFocused();
  for (const name of ["Déplacer vers le haut", "Déplacer vers le bas", nest]) {
    await expect(item("Bravo", name)).toHaveAttribute("aria-disabled", "true");
  }
  await p.keyboard.press("Enter");
  await expect.poll(() => shownRows(p)).toEqual(start);
  expect(await parentOf(b.id)).toBeNull();
  expect(
    byPosition((await getTree(api, space.id)).nodes)
      .filter((n) => n.section === "private")
      .map((n) => n.title),
  ).toEqual(["Alpha", "Bravo"]);

  // Dans « Espace », sortir une sous-page à la racine la rendrait privée : l'action est
  // grisée, comme le dépôt à la souris est refusé.
  await openRowMenu(p, d.id, "Delta");
  await expect(item("Delta", unnest)).toHaveAttribute("aria-disabled", "true");
  await expect(item("Delta", nest)).toHaveAttribute("aria-disabled", "true");
  await p.keyboard.press("Escape");
  expect(await parentOf(d.id)).toBe(c.id);
});

test("drop_target_is_shown_while_dragging", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });
  const before = (await getTree(api, space.id)).nodes;

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  const box = async (id: string) => (await treeRow(p, id).boundingBox())!;
  const line = sidebarOf(p).locator(".tree-drop-line");

  // « Charlie » est saisie : sa ligne s'efface.
  const from = await box(c.id);
  await p.mouse.move(from.x + 80, from.y + from.height / 2);
  await p.mouse.down();
  await p.mouse.move(from.x + 86, from.y + from.height / 2 + 4, { steps: 3 });
  await expect(treeRow(p, c.id)).toHaveClass(/is-dragging/);

  // Dès que le pointeur arrive au milieu de « Bravo », elle s'éclaire : la page deviendrait
  // sa sous-page.
  const onto = await box(b.id);
  await p.mouse.move(onto.x + 80, onto.y + onto.height / 2);
  await expect(treeRow(p, b.id)).toHaveClass(/is-drop-inside/);
  await expect(line).toHaveCount(0);

  // Sur le bord haut de « Alpha » : un trait, au-dessus d'elle ; plus aucune ligne éclairée.
  const first = await box(a.id);
  await p.mouse.move(first.x + 80, first.y + 2);
  await expect(line).toHaveCount(1);
  expect(Math.abs((await line.boundingBox())!.y - first.y)).toBeLessThanOrEqual(2);
  await expect(sidebarOf(p).locator(".is-drop-inside")).toHaveCount(0);

  // Sur le bord bas de « Bravo », juste au-dessus d'elle-même : rien ne changerait, et le
  // milieu de « Bravo » reste permis ; sur elle-même : rien.
  await p.mouse.move(from.x + 80, from.y + from.height / 2);
  await expect(line).toHaveCount(0);
  await expect(sidebarOf(p).locator(".is-drop-inside")).toHaveCount(0);

  // Lâchée hors de la barre latérale : rien ne bouge, et plus rien n'est marqué.
  await p.mouse.move(700, 400, { steps: 3 });
  await p.mouse.up();
  await expect(sidebarOf(p).locator(".is-dragging")).toHaveCount(0);
  await expect(line).toHaveCount(0);
  expect(await shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  expect((await getTree(api, space.id)).nodes).toEqual(before);
});

/** Deux comptes dans le même espace : A, sa propriétaire, et B, simple membre. */
async function ownerAndMember() {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const bruno = await memberOf(alice, space.id, `Bruno ${unique()}`);
  const brunoApi = await apiAs(bruno);
  const { user } = await getMe(brunoApi);
  return { alice, aliceApi, space, bruno, brunoApi, brunoId: user.id };
}

test("page_one_can_only_read_offers_no_action", async ({ browser }) => {
  const { aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  // Deux pages privées de A, partagées avec B : l'une en lecture, l'autre en modification.
  const read = await createPage(aliceApi, { section: "private", title: "À lire" });
  const sub = await createPage(aliceApi, { parentId: read.id, title: "Annexe" });
  const edit = await createPage(aliceApi, { section: "private", title: "À modifier" });
  await setPageLevel(aliceApi, read.id, brunoId, "read");
  await setPageLevel(aliceApi, edit.id, brunoId, "edit");
  const seen = byPosition((await getTree(brunoApi, space.id)).nodes).map((n) => [n.title, n.level, n.section]);
  expect(seen).toEqual(
    expect.arrayContaining([
      ["À lire", "read", "shared"],
      ["Annexe", "read", "shared"],
      ["À modifier", "edit", "shared"],
    ]),
  );
  const before = await getTree(brunoApi, space.id);

  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  const shared = sectionOf(b, "Partagé");
  await expect(shared.getByRole("treeitem")).toHaveCount(3);

  // En lecture, et sa sous-page avec elle : ni « … », ni « + », ni saisie pour la glisser.
  for (const [id, title] of [
    [read.id, "À lire"],
    [sub.id, "Annexe"],
  ]) {
    const row = treeRow(b, id);
    await row.hover();
    await expect(row.getByRole("link")).toHaveText(title);
    await expect(row.getByRole("button", { name: /^Actions de/ })).toHaveCount(0);
    await expect(row.getByRole("button", { name: /^Ajouter une sous-page/ })).toHaveCount(0);
    await expect(row).not.toHaveAttribute("draggable", "true");
  }
  // Seul le repli d'une page qui a des sous-pages reste : il ne change rien sur le serveur.
  await expect(treeRow(b, read.id).getByRole("button")).toHaveCount(1);
  await expect(treeRow(b, sub.id).getByRole("button")).toHaveCount(0);

  // En modification : les deux boutons, et la ligne se saisit.
  const editable = treeRow(b, edit.id);
  await editable.hover();
  await expect(editable.getByRole("button", { name: "Actions de « À modifier »" })).toBeVisible();
  await expect(
    editable.getByRole("button", { name: "Ajouter une sous-page à « À modifier »" }),
  ).toBeVisible();
  await expect(editable).toHaveAttribute("draggable", "true");

  // Glisser quand même la page en lecture (par son lien, qui se glisse comme tout lien) ne
  // la déplace pas, et rien ne s'y dépose : ni dans une autre page, ni dans sa sous-page.
  await treeRow(b, read.id).dragTo(editable);
  await treeRow(b, sub.id).dragTo(editable);
  await b.waitForTimeout(300);
  expect(await getTree(brunoApi, space.id)).toEqual(before);
  await expect(sidebarOf(b).getByRole("alert")).toHaveCount(0);
});

test("delete_forever_needs_full_access", async ({ browser }) => {
  const { alice, aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: "Brouillon commun" });
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");

  // B, qui peut la modifier, la met à la corbeille depuis sa barre latérale.
  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  await (await openRowMenu(b, shared.id, "Brouillon commun"))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(treeRow(b, shared.id)).toHaveCount(0);

  // Dans sa corbeille : « Restaurer », pas « Supprimer définitivement », que le serveur lui
  // refuserait.
  await sidebarOf(b).getByRole("link", { name: "Corbeille" }).click();
  const mine = trashItem(b, "Brouillon commun");
  await expect(mine.getByRole("button", { name: "Restaurer" })).toBeVisible();
  await expect(mine.getByRole("button", { name: "Supprimer définitivement" })).toHaveCount(0);
  await expect(mine.getByRole("button")).toHaveCount(1);
  const refused = await brunoApi.delete(`/api/v1/pages/${shared.id}`);
  expect(refused.status()).toBe(403);

  // A, en accès complet, a les deux.
  const a = await newUser(browser, alice);
  await a.goto(`/s/${space.id}/trash`);
  const hers = trashItem(a, "Brouillon commun");
  await expect(hers.getByRole("button", { name: "Restaurer" })).toBeVisible();
  await expect(hers.getByRole("button", { name: "Supprimer définitivement" })).toBeVisible();
});
