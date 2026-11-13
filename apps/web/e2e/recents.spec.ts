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

test("active_row_stays_clear_of_the_footer_when_recents_grow", async ({ browser }) => {
  // Ouvrir une page l'ajoute à « Récents », au-dessus des arbres : la liste grandit d'une ligne
  // dans le rendu même où l'arbre montre la page ouverte, pas après. Sinon la ligne, montrée
  // juste au-dessus du pied, repasserait dessous. Chromium le masquerait en ancrant le
  // défilement (`overflow-anchor`) ; Safari ne le fait pas : l'ancrage est coupé ici.
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
