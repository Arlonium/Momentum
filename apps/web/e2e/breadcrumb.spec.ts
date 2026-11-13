// Le fil d'Ariane de la barre du haut (Ruling R256) : les pages parentes que le compte voit,
// puis la page. Il se lit dans l'arbre déjà chargé : il suit ce que d'autres déplacent, par le
// canal de l'espace, sans recharger ; et il ne montre jamais une page parente qu'on ne voit pas.
import { expect, test, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  guestOf,
  newUser,
  openPage,
  ownerAndMember,
  signUpAccount,
  unique,
} from "./helpers";

test.afterEach(closeContexts);

/** Le fil d'Ariane de la page ouverte. */
function breadcrumbOf(p: Page) {
  return p.getByRole("navigation", { name: "Fil d'Ariane" });
}

/** Ce que montre le fil, élément par élément : le titre d'une page, ou « … ». */
async function crumbs(p: Page): Promise<string[]> {
  return breadcrumbOf(p)
    .getByRole("listitem")
    .evaluateAll((items) =>
      items.map((item) => item.querySelector("a, button, [aria-current]")?.textContent ?? ""),
    );
}

test("breadcrumb_follows_a_move_made_by_someone_else", async ({ browser }) => {
  const { aliceApi, bruno } = await ownerAndMember();
  const tag = unique();
  const first = await createPage(aliceApi, { title: `Projets ${tag}` });
  const second = await createPage(aliceApi, { title: `Archives ${tag}` });
  const page = await createPage(aliceApi, { parentId: first.id, title: `Compte rendu ${tag}` });

  const b = await openPage(browser, page.id, bruno);
  await expect.poll(() => crumbs(b)).toEqual([first.title, page.title]);
  // Un repère dans la fenêtre : il disparaîtrait si elle se rechargeait.
  await b.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));

  // Alice range la page ailleurs ; le fil de Bruno suit, sans recharger.
  const moved = await aliceApi.patch(`/api/v1/pages/${page.id}`, {
    data: { parentId: second.id },
  });
  expect(moved.status()).toBe(200);
  await expect.poll(() => crumbs(b), { timeout: 5_000 }).toEqual([second.title, page.title]);
  // Puis à la racine.
  const rooted = await aliceApi.patch(`/api/v1/pages/${page.id}`, { data: { parentId: null } });
  expect(rooted.status()).toBe(200);
  await expect.poll(() => crumbs(b), { timeout: 5_000 }).toEqual([page.title]);
  expect(await b.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBe(true);
});

test("guest_breadcrumb_starts_at_the_page_he_sees", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const secret = `Racine secrète ${unique()}`;
  const root = await createPage(aliceApi, { section: "private", title: secret });
  const shared = await createPage(aliceApi, { parentId: root.id, title: `Partagée ${unique()}` });
  const leaf = await createPage(aliceApi, { parentId: shared.id, title: `Sous-page ${unique()}` });
  const gina = await guestOf(alice, shared.id, `Gina ${unique()}`, "read");

  // Témoin : Alice, qui voit la page de racine, la lit dans son fil.
  const a = await openPage(browser, leaf.id, alice);
  await expect.poll(() => crumbs(a)).toEqual([secret, shared.title, leaf.title]);

  // Tout ce que reçoit l'onglet de Gina : réponses de l'API et trames de ses WebSocket.
  const g = await newUser(browser, gina);
  const received: string[] = [];
  g.on("response", (response) => {
    if (!response.url().includes("/api/")) return;
    void response.text().then(
      (text) => received.push(text),
      () => {},
    );
  });
  g.on("websocket", (socket) => {
    socket.on("framereceived", ({ payload }) =>
      received.push(typeof payload === "string" ? payload : payload.toString("utf8")),
    );
  });
  await g.goto(`/p/${leaf.id}`);
  // Son fil commence à la plus haute page qu'elle voit.
  await expect.poll(() => crumbs(g)).toEqual([shared.title, leaf.title]);
  await expect(breadcrumbOf(g).getByRole("link", { name: shared.title })).toBeVisible();
  // Témoin de l'écoute : ce qu'elle voit est bien passé par là (l'arbre, la page).
  await expect.poll(() => received.some((text) => text.includes(shared.title))).toBe(true);
  // Et le titre de la page de racine n'est nulle part : ni à l'écran, ni dans ce qu'elle a reçu.
  expect(await g.content()).not.toContain(secret);
  expect(received.filter((text) => text.includes(secret))).toEqual([]);
});

test("following_a_crumb_from_the_keyboard_keeps_the_focus_off_the_body", async ({ browser }) => {
  const { aliceApi, bruno } = await ownerAndMember();
  const tag = unique();
  // Six niveaux : la première page, « … », et les deux derniers.
  const pages = [];
  let parentId: string | undefined;
  for (const name of ["Un", "Deux", "Trois", "Quatre", "Cinq", "Six"]) {
    const created = await createPage(aliceApi, { parentId, title: `${name} ${tag}` });
    pages.push(created);
    parentId = created.id;
  }
  const b = await openPage(browser, pages[5].id, bruno);
  await expect.poll(() => crumbs(b)).toEqual([pages[0].title, "…", pages[4].title, pages[5].title]);

  // « … » au clavier : le menu prend le focus, les flèches le parcourent, Entrée choisit.
  const more = breadcrumbOf(b).getByRole("button", { name: "Pages parentes masquées" });
  await more.focus();
  await b.keyboard.press("Enter");
  const menu = b.getByRole("menu", { name: "Pages parentes masquées" });
  await expect(menu.getByRole("menuitem")).toHaveText([
    pages[1].title,
    pages[2].title,
    pages[3].title,
  ]);
  await expect(menu.getByRole("menuitem").first()).toBeFocused();
  await b.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitem").nth(1)).toBeFocused();
  await b.keyboard.press("Enter");
  await expect(b).toHaveURL(new RegExp(`/p/${pages[2].id}$`));
  // La barre du haut est neuve : le focus est sur la page atteinte, dans son fil.
  const current = breadcrumbOf(b).locator('[aria-current="page"]');
  await expect(current).toHaveText(pages[2].title);
  await expect(current).toBeFocused();

  // Un lien du fil, suivi par Entrée : de même.
  await b.keyboard.press("Shift+Tab");
  await expect(breadcrumbOf(b).getByRole("link", { name: pages[1].title })).toBeFocused();
  await b.keyboard.press("Enter");
  await expect(b).toHaveURL(new RegExp(`/p/${pages[1].id}$`));
  await expect(breadcrumbOf(b).locator('[aria-current="page"]')).toBeFocused();
  expect(await b.evaluate(() => document.activeElement === document.body)).toBe(false);
});
