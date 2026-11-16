// Les favoris (Ruling R254) : la section « Favoris », en tête de la barre latérale, montre les
// pages favorites du compte dans l'espace, dans l'ordre où il les a ajoutées. Un favori se pose
// et se retire par le menu « … » de la page ou de sa ligne, à tout niveau ; il n'avance pas la
// version de l'espace, si bien qu'un autre onglet du compte ne l'apprend qu'à sa prochaine
// lecture de l'arbre (retour sur l'onglet), et que personne d'autre n'en relit rien.
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  newUser,
  openPage,
  openRowMenu,
  ownerAndMember,
  setPageLevel,
  sidebarOf,
  signUpAccount,
  treeRow,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

/** La section « Favoris » de la barre latérale. */
function favoritesOf(p: Page) {
  return sidebarOf(p).getByRole("group", { name: "Favoris", exact: true });
}

/** Les titres de « Favoris », de haut en bas. */
async function favorites(p: Page): Promise<string[]> {
  const list = favoritesOf(p);
  if ((await list.count()) === 0) return [];
  return list.getByRole("link").allTextContents();
}

/** Le bouton du menu « … » de la barre du haut de la page. */
function pageMenuButton(p: Page) {
  return p.getByRole("button", { name: "Actions de la page", exact: true });
}

/** Les favoris du compte de `api` dans l'espace, tels que le serveur les rend. */
async function serverFavorites(api: APIRequestContext, spaceId: string): Promise<string[]> {
  const res = await api.get(`/api/v1/spaces/${spaceId}/favorites`);
  expect(res.status(), await res.text()).toBe(200);
  return ((await res.json()) as { pages: string[] }).pages;
}

/** Une réponse à la lecture de l'arbre, ou des favoris, de l'espace. */
const isTreeRead = (url: string, spaceId: string) =>
  new URL(url).pathname === `/api/v1/spaces/${spaceId}/tree`;
const isFavoritesRead = (url: string, spaceId: string) =>
  new URL(url).pathname === `/api/v1/spaces/${spaceId}/favorites`;

/** Loin de la barre latérale : « Favoris » n'y est plus gelé (R411). */
const AWAY = { x: 900, y: 420 };

/**
 * Barrière : `id` est renommée par l'API, et l'onglet le montre dans son arbre. Elle s'appelle
 * une fois lu par le test le corps de la lecture des favoris qu'elle doit couvrir : le navigateur
 * l'a donc reçu avant que parte la lecture de l'arbre qui apporte ce renommage, et l'onglet traite
 * ses réponses dans cet ordre (les favoris sont rendus avant l'arbre renommé). C'est ce qui permet
 * d'affirmer qu'une liste n'a **pas** changé, sans attente fixe.
 */
async function renamedInTree(p: Page, api: APIRequestContext, id: string): Promise<string> {
  const title = `Barrière ${unique()}`;
  const res = await api.patch(`/api/v1/pages/${id}`, { data: { title } });
  expect(res.status(), await res.text()).toBe(200);
  await expect(treeRow(p, id).getByRole("link")).toHaveText(title);
  return title;
}

test("favorite_appears_in_the_other_tab_of_the_same_account_when_it_comes_back", async ({
  browser,
}) => {
  const account = await signUpAccount(`Fanny ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const page = await createPage(api, { section: "private", title: `Favorite ${unique()}` });
  const other = await createPage(api, { section: "private", title: `Autre ${unique()}` });
  // Deux onglets du même navigateur, donc du même compte. L'horloge du second est la nôtre : on
  // n'attend pas pour de bon les cinq secondes qui séparent deux relectures au retour (R97).
  const b = await newUser(browser, account);
  await b.clock.install();
  await b.goto(`/p/${other.id}`);
  await waitSynced(b);
  await expect(treeRow(b, page.id)).toBeVisible();
  await b.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));
  const a = await b.context().newPage();
  await a.goto(`/p/${page.id}`);
  await waitSynced(a);

  // Posé dans le premier onglet, par le menu de la page : il s'y voit aussitôt.
  await pageMenuButton(a).click();
  await a.getByRole("menuitem", { name: "Ajouter aux favoris" }).click();
  await expect.poll(() => favorites(a)).toEqual([page.title]);
  await expect.poll(() => serverFavorites(api, space.id)).toEqual([page.id]);
  // Le second onglet n'en sait rien : le favori n'a pas avancé la version de l'espace.
  await expect(favoritesOf(b)).toHaveCount(0);

  // Il revient au premier plan : l'arbre est relu, et les favoris avec lui, sans recharger.
  await b.clock.fastForward(5_500);
  await b.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => favorites(b)).toEqual([page.title]);
  expect(await b.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBe(true);
});

test("favoriting_makes_no_one_else_read_the_tree", async ({ browser }) => {
  const { alice, aliceApi, space, bruno } = await ownerAndMember();
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "space",
    title: `Commune ${unique()}`,
  });
  // L'onglet de Bruno, sur l'espace : ses lectures de l'arbre, et les versions que son canal
  // reçoit (le canal redit la sienne à chaque ping, R205).
  const b = await newUser(browser, bruno);
  let treeReads = 0;
  b.on("request", (request) => {
    if (isTreeRead(request.url(), space.id)) treeReads += 1;
  });
  const versions: number[] = [];
  b.on("websocket", (socket) => {
    if (!socket.url().includes("/events/")) return;
    socket.on("framereceived", (frame) => {
      if (typeof frame.payload === "string" && frame.payload !== "") {
        versions.push((JSON.parse(frame.payload) as { lsn: number }).lsn);
      }
    });
  });
  await b.goto(`/s/${space.id}`);
  await expect(treeRow(b, page.id)).toBeVisible();
  await expect.poll(() => versions.length, { timeout: 10_000 }).toBeGreaterThan(0);
  const opened = versions[0];
  const readsBefore = treeReads;

  // Alice pose et retire dix fois le favori de la page, par le menu de sa page.
  const a = await openPage(browser, page.id, alice);
  for (let n = 0; n < 10; n += 1) {
    const label = n % 2 === 0 ? "Ajouter aux favoris" : "Retirer des favoris";
    const sent = a.waitForResponse((r) => new URL(r.url()).pathname.endsWith("/favorite"));
    await pageMenuButton(a).click();
    await a.getByRole("menuitem", { name: label }).click();
    expect((await sent).status()).toBe(204);
  }
  await expect.poll(() => favorites(a)).toEqual([]);

  // Témoin : Alice renomme la page. La trame qui l'annonce porte exactement la version qui suit
  // celle de l'ouverture, et c'est la seule lecture de l'arbre que l'onglet de Bruno a faite.
  const renamed = `Renommée ${unique()}`;
  const res = await aliceApi.patch(`/api/v1/pages/${page.id}`, { data: { title: renamed } });
  expect(res.status(), await res.text()).toBe(200);
  await expect(treeRow(b, page.id).getByRole("link")).toHaveText(renamed);
  expect([...new Set(versions)]).toEqual([opened, opened + 1]);
  expect(treeReads - readsBefore).toBe(1);
});

test("favorite_of_one_is_not_shown_to_the_other", async ({ browser }) => {
  const { alice, aliceApi, space, bruno, brunoApi } = await ownerAndMember();
  const hers = await createPage(aliceApi, { spaceId: space.id, section: "space", title: `D'Alice ${unique()}` });
  const his = await createPage(aliceApi, { spaceId: space.id, section: "space", title: `De Bruno ${unique()}` });
  const a = await newUser(browser, alice);
  await a.goto(`/s/${space.id}`);
  const menu = await openRowMenu(a, hers.id, hers.title);
  await menu.getByRole("menuitem", { name: "Ajouter aux favoris" }).click();
  await expect.poll(() => favorites(a)).toEqual([hers.title]);

  // Bruno ouvre l'espace après coup : sa liste est lue, et vide.
  const b = await newUser(browser, bruno);
  const read = b.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  await b.goto(`/s/${space.id}`);
  await expect(treeRow(b, hers.id)).toBeVisible();
  expect(await (await read).json()).toEqual({ pages: [] });
  await expect(favoritesOf(b)).toHaveCount(0);

  // Témoin : son propre favori se montre, chez lui seul.
  const hisMenu = await openRowMenu(b, his.id, his.title);
  await hisMenu.getByRole("menuitem", { name: "Ajouter aux favoris" }).click();
  await expect.poll(() => favorites(b)).toEqual([his.title]);
  await a.reload();
  await expect(treeRow(a, his.id)).toBeVisible();
  await expect.poll(() => favorites(a)).toEqual([hers.title]);
  expect(await serverFavorites(aliceApi, space.id)).toEqual([hers.id]);
  expect(await serverFavorites(brunoApi, space.id)).toEqual([his.id]);
});

test("favorite_disappears_with_the_access_and_names_nothing", async ({ browser }) => {
  const { aliceApi, space, bruno, brunoId } = await ownerAndMember();
  const secret = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Secret ${unique()}`,
  });
  await setPageLevel(aliceApi, secret.id, brunoId, "read");
  const b = await newUser(browser, bruno);
  // Témoin : avant le retrait, l'arbre que l'onglet reçoit porte le titre.
  const firstTree = b.waitForResponse((r) => isTreeRead(r.url(), space.id));
  await b.goto(`/s/${space.id}`);
  expect(await (await firstTree).text()).toContain(secret.title);
  // En lecture, sa ligne offre le favori.
  const menu = await openRowMenu(b, secret.id, secret.title);
  await expect(menu.getByRole("menuitem")).toHaveText(["Ajouter aux favoris"]);
  await menu.getByRole("menuitem", { name: "Ajouter aux favoris" }).click();
  await expect.poll(() => favorites(b)).toEqual([secret.title]);
  await b.mouse.move(AWAY.x, AWAY.y);

  // Ce que l'onglet reçoit de l'API à partir du retrait.
  const received: { path: string; body: Promise<string> }[] = [];
  let watching = false;
  b.on("response", (response) => {
    const { pathname } = new URL(response.url());
    if (watching && pathname.startsWith("/api/")) {
      received.push({ path: pathname, body: response.text().catch(() => "") });
    }
  });
  watching = true;
  const res = await aliceApi.delete(`/api/v1/pages/${secret.id}/access/users/${brunoId}`);
  expect(res.status(), await res.text()).toBeLessThan(300);

  // L'accès retiré, la section ne porte plus la page (elle était seule : la section part).
  await expect(treeRow(b, secret.id)).toHaveCount(0);
  await expect(favoritesOf(b)).toHaveCount(0);
  // Les favoris ont été relus après l'arbre, et rien de ce qui est arrivé ne nomme la page.
  const favoritesPath = `/api/v1/spaces/${space.id}/favorites`;
  await expect.poll(() => received.filter((r) => r.path === favoritesPath).length).toBeGreaterThan(0);
  const paths = received.map((r) => r.path);
  expect(paths).toContain(`/api/v1/spaces/${space.id}/tree`);
  for (const { path, body } of received) {
    const text = await body;
    expect(text, path).not.toContain(secret.title);
    expect(text, path).not.toContain(secret.id);
  }
  // Le favori attend en base, invisible : il reviendrait avec l'accès (R254).
  expect(await serverFavorites(await apiAs(bruno), space.id)).toEqual([]);
});

test("favorite_from_the_page_menu_by_keyboard", async ({ browser }) => {
  const account = await signUpAccount(`Kim ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const page = await createPage(api, { section: "private", title: `Au clavier ${unique()}` });
  const p = await openPage(browser, page.id, account);
  const button = pageMenuButton(p);

  // Ouvert par Entrée : le focus est sur sa première entrée, le favori.
  await button.focus();
  await p.keyboard.press("Enter");
  await expect(p.getByRole("menuitem", { name: "Ajouter aux favoris" })).toBeFocused();
  await expect(p.getByRole("menu").getByRole("menuitem")).toHaveText(["Ajouter aux favoris", "Historique"]);
  await p.keyboard.press("Enter");
  // Le menu se ferme, le focus revient à son bouton, et la page est en tête de la barre latérale.
  await expect(p.getByRole("menu")).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect.poll(() => favorites(p)).toEqual([page.title]);
  await expect(favoritesOf(p).getByRole("link")).toHaveAttribute("aria-current", "page");
  await expect.poll(() => serverFavorites(api, space.id)).toEqual([page.id]);

  // Et retiré de même.
  await p.keyboard.press("Enter");
  await expect(p.getByRole("menuitem", { name: "Retirer des favoris" })).toBeFocused();
  await p.keyboard.press("Enter");
  await expect(favoritesOf(p)).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect.poll(() => serverFavorites(api, space.id)).toEqual([]);
});

test("favorites_hold_still_under_the_pointer_until_it_leaves", async ({ browser }) => {
  // R411 : « Favoris » est au-dessus des arbres. Une relecture qui l'allonge, pendant que le
  // pointeur est dans la barre, ne décale aucune ligne sous lui ; il sort, la liste arrive.
  const account = await signUpAccount(`Paul ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const first = await createPage(api, { section: "private", title: `Première ${unique()}` });
  const second = await createPage(api, { section: "private", title: `Seconde ${unique()}` });
  const p = await newUser(browser, account);
  const loaded = p.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  await p.goto(`/s/${space.id}`);
  await loaded;
  await treeRow(p, second.id).hover();
  const before = (await treeRow(p, second.id).boundingBox())!;

  // Ailleurs (un autre onglet du compte), un favori est posé ; puis une page est renommée, ce qui
  // fait relire l'arbre, et les favoris après lui.
  expect((await api.put(`/api/v1/pages/${first.id}/favorite`)).status()).toBe(204);
  const reread = p.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  const renamed = `Renommée ${unique()}`;
  const res = await api.patch(`/api/v1/pages/${first.id}`, { data: { title: renamed } });
  expect(res.status(), await res.text()).toBe(200);
  await expect(treeRow(p, first.id).getByRole("link")).toHaveText(renamed);
  expect(await (await reread).json()).toEqual({ pages: [first.id] });
  // Sous le pointeur, rien n'a bougé, une fois cette lecture rendue.
  const barrier = await createPage(api, { section: "private", title: `Témoin ${unique()}` });
  await expect(treeRow(p, barrier.id)).toBeVisible();
  await renamedInTree(p, api, barrier.id);
  await expect(favoritesOf(p)).toHaveCount(0);
  expect(await treeRow(p, second.id).boundingBox()).toEqual(before);

  // Le pointeur sort de la barre : la liste arrive, et les lignes descendent.
  await p.mouse.move(AWAY.x, AWAY.y);
  await expect.poll(() => favorites(p)).toEqual([renamed]);
  expect((await treeRow(p, second.id).boundingBox())!.y).toBeGreaterThan(before.y);
});

test("a_favorite_set_from_the_page_menu_shows_at_once_with_the_mouse_on_the_sidebar", async ({
  browser,
}) => {
  // La souris repose sur la barre latérale, et la personne travaille au clavier : le favori posé
  // par le menu de la page est son geste, il se voit tout de suite (R411).
  const account = await signUpAccount(`Lou ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const other = await createPage(api, { section: "private", title: `Autre ${unique()}` });
  const page = await createPage(api, { section: "private", title: `Au clavier ${unique()}` });
  const p = await openPage(browser, page.id, account);
  await expect.poll(() => serverFavorites(api, space.id)).toEqual([]);
  await treeRow(p, other.id).hover();
  await pageMenuButton(p).focus();
  await p.keyboard.press("Enter");
  await expect(p.getByRole("menuitem", { name: "Ajouter aux favoris" })).toBeFocused();
  await p.keyboard.press("Enter");
  await expect.poll(() => favorites(p)).toEqual([page.title]);
  await expect.poll(() => serverFavorites(api, space.id)).toEqual([page.id]);
});

test("favorites_are_read_after_the_tree_on_a_cold_open", async ({ browser }) => {
  // Rien sur le chemin de l'ouverture d'une page : la liste des favoris part une fois l'arbre de
  // l'espace reçu, jamais avant, et la page est demandée avant l'un et l'autre.
  const account = await signUpAccount(`Cole ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const page = await createPage(api, { section: "private", title: `À froid ${unique()}` });
  expect((await api.put(`/api/v1/pages/${page.id}/favorite`)).status()).toBe(204);
  const p = await newUser(browser, account);
  const events: string[] = [];
  p.on("request", (request) => {
    const { pathname } = new URL(request.url());
    if (pathname.startsWith("/api/")) events.push(`-> ${pathname}`);
  });
  p.on("requestfinished", (request) => {
    const { pathname } = new URL(request.url());
    if (pathname.startsWith("/api/")) events.push(`<- ${pathname}`);
  });
  await p.goto(`/p/${page.id}`);
  await waitSynced(p);
  await expect.poll(() => favorites(p)).toEqual([page.title]);
  const at = (event: string) => {
    const index = events.indexOf(event);
    expect(index, `${event} dans ${events.join(", ")}`).toBeGreaterThanOrEqual(0);
    return index;
  };
  const tree = `/api/v1/spaces/${space.id}/tree`;
  const list = `/api/v1/spaces/${space.id}/favorites`;
  expect(at(`<- ${tree}`), events.join(", ")).toBeLessThan(at(`-> ${list}`));
  expect(at(`-> /api/v1/pages/${page.id}`)).toBeLessThan(at(`-> ${list}`));
});

test("favorites_thaw_after_a_click_in_a_row_menu_over_the_content", async ({ browser }) => {
  // I1 : le menu d'une ligne, posé dans `body`, déborde sur le contenu. On y clique une entrée là
  // où elle déborde (le cas ordinaire : son milieu y est) ; le menu disparaît sous le pointeur, qui
  // bouge ensuite sur le contenu. « Favoris » n'est plus gelé : la liste reçue se montre.
  const account = await signUpAccount(`Menu ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const first = await createPage(api, { section: "private", title: `Première ${unique()}` });
  const second = await createPage(api, { section: "private", title: `Seconde ${unique()}` });
  const p = await newUser(browser, account);
  const loaded = p.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  await p.goto(`/s/${space.id}`);
  await loaded;
  const menu = await openRowMenu(p, second.id, second.title);
  const item = menu.getByRole("menuitem", { name: "Ajouter aux favoris" });
  const box = (await item.boundingBox())!;
  const nav = (await sidebarOf(p).boundingBox())!;
  const x = box.x + box.width - 8;
  expect(x).toBeGreaterThan(nav.x + nav.width + 4);
  await p.mouse.click(x, box.y + box.height / 2);
  await expect.poll(() => favorites(p)).toEqual([second.title]);
  await p.mouse.move(AWAY.x, AWAY.y, { steps: 5 });

  // Ailleurs, un favori est posé ; un renommage fait relire l'arbre, puis les favoris.
  expect((await api.put(`/api/v1/pages/${first.id}/favorite`)).status()).toBe(204);
  const reread = p.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  const renamed = `Renommée ${unique()}`;
  expect((await api.patch(`/api/v1/pages/${first.id}`, { data: { title: renamed } })).status()).toBe(200);
  expect(await (await reread).json()).toEqual({ pages: [second.id, first.id] });
  await expect.poll(() => favorites(p)).toEqual([second.title, renamed]);
});

test("favorite_from_another_tab_shows_after_a_row_menu_was_used_here", async ({ browser }) => {
  // I1, le cas de R254 : dans l'onglet B, une page est mise à la corbeille par le menu de sa ligne,
  // à la souris ; dans l'onglet A, un favori est posé ; de retour sur B, le pointeur sur le contenu,
  // le favori y paraît.
  const account = await signUpAccount(`Retour ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const page = await createPage(api, { section: "private", title: `Favorite ${unique()}` });
  const other = await createPage(api, { section: "private", title: `Autre ${unique()}` });
  const junk = await createPage(api, { section: "private", title: `À jeter ${unique()}` });
  const b = await newUser(browser, account);
  await b.clock.install();
  await b.goto(`/p/${other.id}`);
  await expect(treeRow(b, junk.id)).toBeVisible();
  const menu = await openRowMenu(b, junk.id, junk.title);
  const item = menu.getByRole("menuitem", { name: "Mettre à la corbeille" });
  const box = (await item.boundingBox())!;
  await b.mouse.click(box.x + box.width - 8, box.y + box.height / 2);
  await expect(treeRow(b, junk.id)).toHaveCount(0);
  await b.mouse.move(AWAY.x, AWAY.y, { steps: 5 });

  const a = await b.context().newPage();
  await a.goto(`/p/${page.id}`);
  await pageMenuButton(a).click();
  await a.getByRole("menuitem", { name: "Ajouter aux favoris" }).click();
  await expect.poll(() => favorites(a)).toEqual([page.title]);

  await b.bringToFront();
  await b.mouse.move(AWAY.x + 20, AWAY.y + 10, { steps: 3 });
  const reread = b.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  await b.clock.fastForward(5_500);
  await b.evaluate(() => window.dispatchEvent(new Event("focus")));
  expect(await (await reread).json()).toEqual({ pages: [page.id] });
  await expect.poll(() => favorites(b)).toEqual([page.title]);
});

test("favorites_hold_still_during_a_drag_in_the_tree", async ({ browser }) => {
  // I2 : au début d'un glisser HTML5, Chromium annule le pointeur (`pointercancel`, puis
  // `pointerleave`). Pendant que le pointeur tient une page, rien ne bouge sous lui.
  const account = await signUpAccount(`Glisse ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const alpha = await createPage(api, { section: "private", title: `Alpha ${unique()}` });
  const beta = await createPage(api, { section: "private", title: `Bêta ${unique()}` });
  const gamma = await createPage(api, { section: "private", title: `Gamma ${unique()}` });
  const p = await newUser(browser, account);
  const loaded = p.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  await p.goto(`/s/${space.id}`);
  await loaded;
  const start = (await treeRow(p, gamma.id).boundingBox())!;
  const target = (await treeRow(p, beta.id).boundingBox())!;
  // « Gamma » est tenue au bout de la souris, au-dessus de « Bêta ».
  await treeRow(p, gamma.id).hover();
  await p.mouse.down();
  await p.mouse.move(start.x + 20, start.y + 8, { steps: 3 });
  await p.mouse.move(target.x + 20, target.y + 8, { steps: 5 });
  await expect(treeRow(p, gamma.id)).toHaveClass(/is-dragging/);
  const during = (await treeRow(p, beta.id).boundingBox())!;

  // Pendant le glisser, un favori est posé ailleurs, et une page renommée.
  expect((await api.put(`/api/v1/pages/${alpha.id}/favorite`)).status()).toBe(204);
  const reread = p.waitForResponse((r) => isFavoritesRead(r.url(), space.id));
  const renamed = `Renommée ${unique()}`;
  expect((await api.patch(`/api/v1/pages/${alpha.id}`, { data: { title: renamed } })).status()).toBe(200);
  expect(await (await reread).json()).toEqual({ pages: [alpha.id] });
  // Cette lecture rendue, sous le pointeur qui tient une page, rien n'a bougé.
  await renamedInTree(p, api, alpha.id);
  await expect(favoritesOf(p)).toHaveCount(0);
  expect(await treeRow(p, beta.id).boundingBox()).toEqual(during);

  // Témoin : lâchée hors de la barre, la page n'est déposée nulle part, et la liste arrive.
  await p.mouse.move(AWAY.x, AWAY.y, { steps: 5 });
  await p.mouse.up();
  await expect.poll(() => favorites(p)).toHaveLength(1);
  await expect(treeRow(p, gamma.id)).not.toHaveClass(/is-dragging/);
});

test("a_refused_favorite_is_said_in_the_sidebar_until_the_next_gesture", async ({ browser }) => {
  // Le refus se dit en pied de barre latérale, tel que le serveur le dit (R410), et le geste
  // suivant l'efface, comme toute action (R468). Le premier envoi reçoit la réponse d'une page
  // qu'on ne voit plus.
  const account = await signUpAccount(`Refus ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const lost = await createPage(api, { section: "private", title: `Perdue ${unique()}` });
  const kept = await createPage(api, { section: "private", title: `Gardée ${unique()}` });
  const p = await newUser(browser, account);
  let refused = false;
  await p.route(`**/api/v1/pages/${lost.id}/favorite`, async (route) => {
    if (refused) return route.continue();
    refused = true;
    await route.fulfill({ status: 404, json: { error: "not_found", message: "page introuvable" } });
  });
  await p.goto(`/s/${space.id}`);
  const alert = sidebarOf(p).getByRole("alert");
  const menu = await openRowMenu(p, lost.id, lost.title);
  await menu.getByRole("menuitem", { name: "Ajouter aux favoris" }).click();
  await expect(alert).toHaveText("Le favori n'a pas pu être ajouté. Page introuvable");
  // Défait : la section n'a pas paru (une seule page, refusée).
  await expect(favoritesOf(p)).toHaveCount(0);
  // Le geste suivant, sur une autre page : le refus d'avant part, le favori se pose.
  const next = await openRowMenu(p, kept.id, kept.title);
  await next.getByRole("menuitem", { name: "Ajouter aux favoris" }).click();
  await expect(alert).toHaveCount(0);
  await expect.poll(() => favorites(p)).toEqual([kept.title]);
  await expect.poll(() => serverFavorites(api, space.id)).toEqual([kept.id]);
});
