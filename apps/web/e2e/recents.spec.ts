// Les pages récentes (Ruling R255) : la section « Récents » de la barre latérale montre les
// cinq dernières pages ouvertes dans l'espace, la plus récente d'abord, gardées dans le
// navigateur (par compte et par espace), filtrées par l'arbre, et effacées à la déconnexion.
import { expect, test, type Browser, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  createSpace,
  firstSpace,
  getMe,
  logInWith,
  newUser,
  sectionOf,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  titleOf,
  trashBanner,
  treeRow,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

/** La section « Récents » de la barre latérale. */
function recentsOf(p: Page) {
  return sidebarOf(p).getByRole("group", { name: "Récents", exact: true });
}

/** Les titres de « Récents », de haut en bas. */
async function recents(p: Page): Promise<string[]> {
  const list = recentsOf(p);
  if ((await list.count()) === 0) return [];
  return list.getByRole("link").allTextContents();
}

/** Ouvre une page par sa ligne de l'arbre, et attend qu'elle soit là. */
async function openFromTree(p: Page, id: string, title: string): Promise<void> {
  await treeRow(p, id).getByRole("link").click();
  await expect(titleOf(p)).toHaveValue(title);
}

/** Un compte neuf, son espace, ses pages, et un onglet sur l'espace (sans page récente). */
async function accountWithPages(browser: Browser, count: number) {
  const account = await signUpAccount(`Rita ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const { user } = await getMe(api);
  const pages = [];
  for (let n = 1; n <= count; n += 1) {
    pages.push(await createPage(api, { section: "private", title: `Page ${n} ${unique()}` }));
  }
  const p = await newUser(browser, account);
  await p.goto(`/s/${space.id}`);
  await expect(sectionOf(p, "Privé")).toBeVisible();
  return { account, api, space, userId: user.id, pages, p };
}

test("opened_pages_are_listed_most_recent_first", async ({ browser }) => {
  const { pages, p } = await accountWithPages(browser, 6);
  // Rien d'ouvert : pas de section.
  await expect(treeRow(p, pages[0].id)).toBeVisible();
  await expect(recentsOf(p)).toHaveCount(0);

  for (const page of pages.slice(0, 3)) await openFromTree(p, page.id, page.title);
  await expect.poll(() => recents(p)).toEqual([pages[2].title, pages[1].title, pages[0].title]);
  // La page ouverte y est marquée.
  await expect(recentsOf(p).getByRole("link", { name: pages[2].title })).toHaveAttribute(
    "aria-current",
    "page",
  );

  // Rouverte : elle remonte en tête, sans doublon.
  await openFromTree(p, pages[0].id, pages[0].title);
  await expect.poll(() => recents(p)).toEqual([pages[0].title, pages[2].title, pages[1].title]);

  // Cinq au plus : les plus récentes.
  for (const page of pages.slice(3)) await openFromTree(p, page.id, page.title);
  await expect
    .poll(() => recents(p))
    .toEqual([pages[5].title, pages[4].title, pages[3].title, pages[0].title, pages[2].title]);

  // Un lien de « Récents » ouvre sa page.
  await recentsOf(p).getByRole("link", { name: pages[3].title }).click();
  await expect(titleOf(p)).toHaveValue(pages[3].title);
  await expect.poll(() => recents(p)).toEqual([
    pages[3].title,
    pages[5].title,
    pages[4].title,
    pages[0].title,
    pages[2].title,
  ]);
});

test("recents_belong_to_their_space", async ({ browser }) => {
  const { api, space, pages, p } = await accountWithPages(browser, 1);
  const other = await createSpace(api, `Autre ${unique()}`);
  const there = await createPage(api, {
    spaceId: other.id,
    section: "private",
    title: `Là-bas ${unique()}`,
  });
  await openFromTree(p, pages[0].id, pages[0].title);
  await expect.poll(() => recents(p)).toEqual([pages[0].title]);

  /** Passe à l'espace `name` par le sélecteur, sans recharger l'application. */
  const switchTo = async (name: string) => {
    await spaceSwitcher(p).click();
    await sidebarOf(p)
      .getByRole("list", { name: "Espaces" })
      .getByRole("link")
      .filter({ hasText: name })
      .click();
    await expect(spaceSwitcher(p, name)).toBeVisible();
  };
  // Dans l'autre espace : rien de celui-ci.
  await switchTo(other.name);
  await expect(treeRow(p, there.id)).toBeVisible();
  await expect(recentsOf(p)).toHaveCount(0);
  await expect(sidebarOf(p).getByText(pages[0].title)).toHaveCount(0);
  await openFromTree(p, there.id, there.title);
  await expect.poll(() => recents(p)).toEqual([there.title]);
  // De retour : les siennes.
  await switchTo(space.name);
  await expect(treeRow(p, pages[0].id)).toBeVisible();
  await expect.poll(() => recents(p)).toEqual([pages[0].title]);
});

test("recents_survive_a_reload_and_forget_a_trashed_page", async ({ browser }) => {
  const { api, pages, p } = await accountWithPages(browser, 3);
  const [a, b, c] = pages;
  await openFromTree(p, a.id, a.title);
  await openFromTree(p, b.id, b.title);
  await expect.poll(() => recents(p)).toEqual([b.title, a.title]);

  // Rechargée : la liste est la même, sans rien demander au serveur pour elle.
  await p.reload();
  await waitSynced(p);
  await expect.poll(() => recents(p)).toEqual([b.title, a.title]);

  // A part à la corbeille (ailleurs) : elle quitte « Récents » avec l'arbre.
  expect((await api.post(`/api/v1/pages/${a.id}/trash`)).status()).toBe(200);
  await expect(treeRow(p, a.id)).toHaveCount(0, { timeout: 5_000 });
  await expect.poll(() => recents(p)).toEqual([b.title]);
  await p.reload();
  await waitSynced(p);
  await expect.poll(() => recents(p)).toEqual([b.title]);

  // Une page à la corbeille qu'on ouvre n'y entre pas.
  await p.goto(`/p/${a.id}`);
  await expect(trashBanner(p)).toBeVisible();
  await openFromTree(p, c.id, c.title);
  await expect.poll(() => recents(p)).toEqual([c.title, b.title]);

  // Restaurée, elle revient à sa place : son identifiant était resté dans le navigateur.
  expect((await api.post(`/api/v1/pages/${a.id}/restore`)).status()).toBe(200);
  await expect(treeRow(p, a.id)).toBeVisible({ timeout: 5_000 });
  await expect.poll(() => recents(p)).toEqual([c.title, b.title, a.title]);
});

test("opening_a_page_from_the_tree_moves_no_row_under_the_pointer", async ({ browser }) => {
  // « Récents » grandit d'une ligne à chaque page ouverte (cinq fois par espace) : rien ne doit
  // en bouger sous le pointeur. Cliquer une page, puis son « … » sans bouger, ouvre le menu de
  // cette page, et pas celui d'une voisine (« Mettre à la corbeille » compris). Au-dessus des
  // sections, la liste les décalait d'une ligne ; dessous, elles ne bougent pas (R296).
  const { pages, p } = await accountWithPages(browser, 40);
  const sidebar = sidebarOf(p);
  await openFromTree(p, pages[0].id, pages[0].title);
  await expect.poll(() => recents(p)).toEqual([pages[0].title]);

  /** Clique la page, puis son « … » là où il était avant le clic. */
  const openThenMenu = async (index: number, before: string[]) => {
    const page = pages[index];
    const row = treeRow(p, page.id);
    const link = (await row.getByRole("link").boundingBox())!;
    const more = (await row.getByRole("button", { name: `Actions de « ${page.title} »` }).boundingBox())!;
    await p.mouse.click(link.x + 20, link.y + link.height / 2);
    await expect(row.getByRole("link")).toHaveAttribute("aria-current", "page");
    // La liste a grandi : c'est après elle que le pointeur reclique.
    await expect.poll(() => recents(p)).toEqual([page.title, ...before]);
    await p.mouse.click(more.x + more.width / 2, more.y + more.height / 2);
    await expect(p.getByRole("menu")).toHaveAccessibleName(`Actions de « ${page.title} »`);
    await p.keyboard.press("Escape");
    await expect(p.getByRole("menu")).toHaveCount(0);
  };

  // La barre en haut, où Chromium n'ancre pas le défilement.
  await sidebar.evaluate((el) => (el.scrollTop = 0));
  await openThenMenu(6, [pages[0].title]);

  // La barre défilée tout en bas, sans ancrage (Safari) : la dernière page de l'arbre.
  await p.addStyleTag({ content: ".mm-sidebar { overflow-anchor: none; }" });
  await sidebar.evaluate((el) => (el.scrollTop = el.scrollHeight));
  await expect(treeRow(p, pages[39].id)).toBeVisible();
  await openThenMenu(39, [pages[6].title, pages[0].title]);
});

test("active_row_stays_clear_of_the_footer_when_recents_grow", async ({ browser }) => {
  // Ouvrir une page l'ajoute à « Récents », qui grandit d'une ligne, après que l'arbre a fait
  // défiler la barre jusqu'à elle. Sous les sections (R296), la liste ne repousse pas sous le
  // pied la ligne qu'il vient de montrer ; au-dessus, elle l'y repoussait. Chromium le masquait
  // en ancrant le défilement (`overflow-anchor`) ; Safari ne le fait pas : l'ancrage est coupé.
  const { space, userId, pages, p } = await accountWithPages(browser, 40);
  await p.addStyleTag({ content: ".mm-sidebar { overflow-anchor: none; }" });
  const stored = () =>
    p.evaluate((key) => localStorage.getItem(key), `mementum.recents.${userId}.${space.id}`);
  const sidebar = sidebarOf(p);
  const clear = (id: string) =>
    treeRow(p, id).evaluate((row) => {
      const box = row.getBoundingClientRect();
      const at = (y: number) => row.contains(document.elementFromPoint(box.left + box.width / 2, y));
      return at(box.top + 1) && at(box.bottom - 1);
    });
  await openFromTree(p, pages[0].id, pages[0].title);
  await expect.poll(() => recents(p)).toEqual([pages[0].title]);

  // Une ligne coupée par le pied, de 12 px.
  const low = pages[25].id;
  await sidebar.evaluate((el) => (el.scrollTop = 0));
  await expect(treeRow(p, low)).toBeVisible();
  await sidebar.evaluate((el, id) => {
    const row = el.querySelector(`[data-page-id="${id}"]`)!.getBoundingClientRect();
    const footer = el.querySelector(".mm-side-footer")!.getBoundingClientRect();
    el.scrollTop += row.bottom - footer.top - 12;
  }, low);
  expect(await clear(low)).toBe(false);
  await treeRow(p, low).getByRole("link").evaluate((link: HTMLElement) => link.click());
  await expect(treeRow(p, low).getByRole("link")).toHaveAttribute("aria-current", "page");
  // La visite est écrite (le dernier changement qui pourrait la déplacer), et la liste montrée.
  await expect.poll(stored).toBe(JSON.stringify([low, pages[0].id]));
  await expect.poll(() => recents(p)).toEqual([pages[25].title, pages[0].title]);
  expect(await clear(low)).toBe(true);
});

/** Les clés de pages récentes du compte `userId` dans le stockage de l'onglet. */
async function keysOf(p: Page, userId: string): Promise<string[]> {
  const keys = await p.evaluate(() => Object.keys(localStorage));
  return keys.filter((key) => key.startsWith(`mementum.recents.${userId}.`));
}

test("a_tab_left_open_writes_no_recents_after_logging_out_in_another", async ({ browser }) => {
  const { space, userId, pages, p } = await accountWithPages(browser, 2);
  await openFromTree(p, pages[0].id, pages[0].title);
  expect(await keysOf(p, userId)).toEqual([`mementum.recents.${userId}.${space.id}`]);
  // Un second onglet du même navigateur, encore affiché quand le premier se déconnecte.
  const q = await p.context().newPage();
  await q.goto(`/p/${pages[0].id}`);
  await waitSynced(q);
  await spaceSwitcher(p).click();
  await sidebarOf(p).getByRole("button", { name: "Se déconnecter" }).click();
  await expect(p).toHaveURL(/\/login$/);
  expect(await keysOf(q, userId)).toEqual([]);

  // Il ouvre une autre page de sa barre latérale : la session n'est plus là (la connexion est
  // demandée par-dessus), et rien n'est récrit pour le compte parti.
  await treeRow(q, pages[1].id).getByRole("link").click();
  await expect(q.getByRole("dialog", { name: "Ta session a pris fin" })).toBeVisible();
  expect(await keysOf(q, userId)).toEqual([]);
});

test("recents_of_an_account_whose_session_ended_elsewhere_go_when_another_enters", async ({
  browser,
}) => {
  const { account, api, userId, pages, p } = await accountWithPages(browser, 1);
  await openFromTree(p, pages[0].id, pages[0].title);
  expect(await keysOf(p, userId)).toHaveLength(1);
  // Le mot de passe change ailleurs : toutes les sessions du compte sont fermées, celle de ce
  // navigateur comprise, sans déconnexion ici.
  const changed = await api.post("/api/v1/auth/password", {
    data: { current: account.password, new: `${account.password}-neuf` },
  });
  expect(changed.status(), await changed.text()).toBe(204);

  // Un autre compte se connecte dans ce navigateur : les récents du premier n'y restent pas.
  const other = await signUpAccount(`Olga ${unique()}`);
  await p.goto("/");
  await logInWith(p, other.email, other.password);
  await expect(spaceSwitcher(p)).toBeVisible();
  await expect.poll(() => keysOf(p, userId)).toEqual([]);
});

test("recents_are_gone_after_logging_out", async ({ browser }) => {
  const { space, userId, pages, p } = await accountWithPages(browser, 1);
  await openFromTree(p, pages[0].id, pages[0].title);
  await expect.poll(() => recents(p)).toEqual([pages[0].title]);
  const key = `mementum.recents.${userId}.${space.id}`;
  expect(await p.evaluate((k) => localStorage.getItem(k), key)).toBe(JSON.stringify([pages[0].id]));
  // Celles d'un autre compte, dans le même navigateur, ne sont pas les siennes.
  const other = "mementum.recents.01a1d000-0000-7000-8000-000000000000.ailleurs";
  await p.evaluate((k) => localStorage.setItem(k, '["x"]'), other);

  await spaceSwitcher(p).click();
  await sidebarOf(p).getByRole("button", { name: "Se déconnecter" }).click();
  await expect(p).toHaveURL(/\/login$/);
  // Plus aucune clé de ses pages récentes ; celle de l'autre compte reste.
  const left = await p.evaluate(() => Object.keys(localStorage));
  expect(left.filter((k) => k.startsWith(`mementum.recents.${userId}.`))).toEqual([]);
  expect(left).toContain(other);
});
