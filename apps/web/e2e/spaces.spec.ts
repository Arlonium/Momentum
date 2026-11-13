// Espaces et sections, depuis l'interface : le sélecteur d'espace, les trois sections de la
// barre latérale (« Espace », « Partagé », « Privé »), ce que chacun y voit, l'espace gardé
// d'une visite à l'autre, et les actions d'une ligne selon ce qu'on peut faire de sa page. Le
// serveur fait foi pour les droits : chaque test relit aussi ce qu'il répond. La corbeille est
// dans `trash.spec.ts`, le déplacement des pages dans `moves.spec.ts`.
import { expect, test, type Page } from "@playwright/test";
import type { PageNode } from "@mementum/protocol";
import {
  apiAs,
  byPosition,
  closeContexts,
  createPage,
  createSpace,
  firstSpace,
  getMe,
  getTree,
  guestOf,
  memberOf,
  newUser,
  NO_SESSION,
  openRowMenu,
  ownerAndMember,
  pageIdOf,
  sectionOf,
  setPageLevel,
  shownRows,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  titleOf,
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
  // L'écran se compare une fois la barre latérale chargée : lu pendant qu'elle dit encore
  // « Chargement… », le texte de référence différait des suivants, pris un peu plus tard
  // (échec vu trois fois le 2026-11-13, sous la charge de plusieurs suites lancées de front).
  const loading = b.locator(".mm-side-note", { hasText: "Chargement…" });
  await expect(loading).toHaveCount(0);
  const unknown = await b.locator("body").innerText();

  // Un espace où B n'a aucun rôle : exactement le même écran, sans rien de cet espace, pour
  // son accueil, sa corbeille et ses réglages.
  for (const path of [`/s/${foreign.id}`, `/s/${foreign.id}/trash`, `/s/${foreign.id}/settings`]) {
    await b.goto(path);
    await expect(missing).toBeVisible();
    await expect(b).toHaveURL(`${BASE_URL}${path}`);
    await expect(loading).toHaveCount(0);
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

test("row_actions_show_for_keyboard_focus_only", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  const c = await createPage(api, { title: "Charlie" });
  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  const more = (id: string) => treeRow(p, id).getByRole("button", { name: /^Actions de / });
  const add = (id: string) => treeRow(p, id).getByRole("button", { name: /^Ajouter une sous-page à / });

  // À la souris : « Bravo » part à la corbeille, et le focus va à la ligne qui prend sa place.
  // Elle ne montre pas pour autant ses « … » et « + » : personne ne l'a survolée, ni n'y est
  // venu au clavier.
  await (await openRowMenu(p, b.id, "Bravo"))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(treeRow(p, b.id)).toHaveCount(0);
  await expect(treeRow(p, c.id).getByRole("link")).toBeFocused();
  await p.mouse.move(800, 500);
  await expect(more(c.id)).toHaveCSS("opacity", "0");
  await expect(add(c.id)).toHaveCSS("opacity", "0");

  // Au clavier, la ligne où l'on arrive les montre ; celle qu'on quitte les cache.
  await p.keyboard.press("Shift+Tab");
  await expect(add(a.id)).toBeFocused();
  await expect(more(a.id)).toHaveCSS("opacity", "1");
  await expect(add(a.id)).toHaveCSS("opacity", "1");
  await expect(more(c.id)).toHaveCSS("opacity", "0");
  await p.keyboard.press("Tab");
  await expect(treeRow(p, c.id).getByRole("link")).toBeFocused();
  await expect(more(c.id)).toHaveCSS("opacity", "1");
  await expect(more(a.id)).toHaveCSS("opacity", "0");
});

test("row_actions_are_always_shown_without_hover", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const long = await createPage(api, {
    title: `Un titre bien trop long pour sa ligne ${"très ".repeat(12)}long`,
  });

  // Un écran tactile : rien n'y survole. Ce qui ne se montre qu'au survol y serait invisible,
  // et pourtant cliquable.
  const p = await newUser(browser, alice, { hasTouch: true, isMobile: true });
  await p.goto(`/s/${space.id}`);
  await expect(treeRow(p, a.id)).toBeVisible();
  expect(await p.evaluate(() => matchMedia("(hover: none)").matches)).toBe(true);

  const row = treeRow(p, long.id);
  const more = row.getByRole("button", { name: /^Actions de / });
  const add = row.getByRole("button", { name: /^Ajouter une sous-page à / });
  await expect(more).toHaveCSS("opacity", "1");
  await expect(add).toHaveCSS("opacity", "1");
  await expect(
    sectionOf(p, "Espace").getByRole("button", { name: "Ajouter une page dans « Espace »" }),
  ).toHaveCSS("opacity", "1");
  // Le titre s'arrête avant eux.
  const link = (await row.getByRole("link").boundingBox())!;
  expect(link.x + link.width).toBeLessThanOrEqual((await more.boundingBox())!.x);
  // Et ils répondent au doigt. Le menu ouvert, le titre garde sa place : il n'a pas à en
  // céder à des boutons qui ont déjà la leur.
  const title = row.locator(".tree-title");
  const width = (await title.boundingBox())!.width;
  await more.tap();
  await expect(p.getByRole("menu", { name: /^Actions de / })).toBeVisible();
  await expect(row).toHaveClass(/is-menu-open/);
  expect((await title.boundingBox())!.width).toBe(width);
});

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
  // Tout ce que l'onglet demande d'écrire au serveur à partir d'ici est noté.
  const writes: string[] = [];
  b.on("request", (request) => {
    if (request.method() !== "GET") {
      writes.push(`${request.method()} ${new URL(request.url()).pathname}`);
    }
  });
  await treeRow(b, read.id).dragTo(editable);
  await treeRow(b, sub.id).dragTo(editable);
  // Puis un geste permis : une sous-page de la page qu'il peut modifier. Sa demande part
  // après celles qu'auraient faites les deux glissers ; c'est la seule que l'onglet ait
  // envoyée.
  const start = new URL(b.url()).pathname;
  await editable.hover();
  await editable.getByRole("button", { name: "Ajouter une sous-page à « À modifier »" }).click();
  await b.waitForURL((url) => url.pathname.startsWith("/p/") && url.pathname !== start);
  const created = pageIdOf(b);
  expect(writes).toEqual(["POST /api/v1/pages"]);
  // Aucune page n'a changé de place.
  const places = (nodes: PageNode[]) => nodes.map((n) => [n.id, n.parentId, n.position]);
  const after = await getTree(brunoApi, space.id);
  expect(places(after.nodes.filter((n) => n.id !== created))).toEqual(places(before.nodes));
  await expect(sidebarOf(b).getByRole("alert")).toHaveCount(0);
});

test("role_changed_elsewhere_reaches_the_sidebar_on_return", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const team = await createPage(aliceApi, { title: `Équipe ${unique()}` });
  const shared = await createPage(aliceApi, { section: "private", title: `Partagée ${unique()}` });
  const dana = await guestOf(alice, shared.id, `Dana ${unique()}`, "read");
  const { user } = await getMe(await apiAs(dana));

  const d = await newUser(browser, dana);
  // L'horloge de l'onglet est la nôtre : on n'attend pas cinq secondes pour de bon.
  await d.clock.install();
  await d.goto(`/s/${space.id}`);
  // Invitée : « Partagé » seulement, et rien pour créer une page.
  const newPage = sidebarOf(d).getByRole("button", { name: "Nouvelle page" });
  await expect(treeRow(d, shared.id)).toBeVisible();
  await expect(sectionOf(d, "Espace")).toHaveCount(0);
  await expect(newPage).toHaveCount(0);

  // A la passe « Membre ». Rien ne le dit à l'onglet de D.
  const promoted = await aliceApi.patch(`/api/v1/spaces/${space.id}/members/${user.id}`, {
    data: { role: "member" },
  });
  expect(promoted.status(), await promoted.text()).toBe(200);

  // D revient sur son onglet : sans ouvrir le menu de l'espace, elle a les sections d'un
  // membre, la page d'équipe, et de quoi créer une page.
  await d.clock.fastForward(5_500);
  await d.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(newPage).toBeVisible();
  await expect(sectionOf(d, "Espace").locator(`[data-page-id="${team.id}"]`)).toBeVisible();
  await expect(sectionOf(d, "Privé")).toBeVisible();
  await expect(sectionOf(d, "Partagé").locator(`[data-page-id="${shared.id}"]`)).toBeVisible();
});
