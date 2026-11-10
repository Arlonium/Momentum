// Espaces et sections, depuis l'interface : le sélecteur d'espace, les trois sections de la
// barre latérale (« Espace », « Partagé », « Privé »), ce que chacun y voit, et l'espace
// gardé d'une visite à l'autre. Le serveur fait foi pour les droits : chaque test relit
// aussi ce qu'il répond.
import { expect, test, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  createSpace,
  firstSpace,
  getMe,
  getTree,
  inviteToSpace,
  newUser,
  NO_SESSION,
  pageIdOf,
  sectionOf,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  treeRow,
  unique,
  waitSynced,
  type Account,
} from "./helpers";
import { BASE_URL } from "./servers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

const titleOf = (p: Page) => p.getByRole("textbox", { name: "Titre de la page" });

/** Un id de page ou d'espace que le serveur ne connaît pas. */
const UNKNOWN_ID = "00000000-0000-7000-8000-000000000000";

/** Un compte neuf qui n'a que l'espace `spaceId`, où `owner` l'invite comme membre. */
async function memberOf(owner: Account, spaceId: string, name: string): Promise<Account> {
  const { token } = await inviteToSpace(await apiAs(owner), spaceId, {
    email: `invite-${unique()}@exemple.fr`,
    role: "member",
  });
  return signUpAccount(name, token);
}

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

test("space_without_a_role_goes_to_the_first_space", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const foreign = await firstSpace(await apiAs(alice));
  const own = await firstSpace(await apiAs(bruno));

  const b = await newUser(browser, bruno);
  // Un espace qui n'existe pas : le premier espace du compte.
  await b.goto(`/s/${UNKNOWN_ID}`);
  await expect(b).toHaveURL(`${BASE_URL}/s/${own.id}`);
  await expect(b.getByRole("heading", { name: "Aucune page ouverte" })).toBeVisible();
  const unknown = await b.locator("body").innerText();
  // Un espace où B n'a aucun rôle : exactement le même écran, sans rien de cet espace.
  for (const path of [`/s/${foreign.id}`]) {
    await b.goto(path);
    await expect(b).toHaveURL(`${BASE_URL}/s/${own.id}`);
    await expect(b.getByRole("heading", { name: "Aucune page ouverte" })).toBeVisible();
    expect(await b.locator("body").innerText()).toBe(unknown);
  }
  expect(unknown).not.toContain(foreign.name);
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
