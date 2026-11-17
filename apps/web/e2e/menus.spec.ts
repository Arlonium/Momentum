// Les menus « … » (`RowMenu` : celui de la barre du haut d'une page, celui d'une ligne de la barre
// latérale) et le défilement (R764). Un menu se ferme quand ce qui porte son bouton défile, et rend
// alors le focus à son bouton, sans défiler jusqu'à lui ; un défilement d'autre chose, ou un
// défilement fait juste avant l'ouverture dont l'événement n'arrive qu'à l'image suivante, le
// laisse ouvert.
//
// Le cas de la recette (relecture finale de la partie B, défaut D-2) : on tape à la fin d'une longue
// page, le document défile jusqu'en bas, et la barre du haut (qui défile avec lui) sort de l'écran ;
// le clic sur « … » le fait d'abord revenir en haut, puis ouvre le menu dans la même tâche, et
// l'événement `scroll` de ce retour, distribué à l'image suivante, fermait le menu : le focus tombait
// sur `body`. Une main fait de même en cliquant pendant la fin d'un défilement à l'inertie.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { closeContexts, createPage, openPage, openRowMenu, putMmd, sidebarOf, unique } from "./helpers";

test.afterEach(closeContexts);

/** Une longue page de `request`, ouverte. */
async function longPage(browser: Parameters<typeof openPage>[0], request: Parameters<typeof createPage>[0]) {
  const page = await createPage(request, { title: `Menus ${unique()}` });
  await putMmd(request, page.id, Array.from({ length: 80 }, (_, i) => `Paragraphe ${i}`).join("\n\n") + "\n");
  const p = await openPage(browser, page.id);
  return { page, p };
}

function pageMenuButton(p: Page): Locator {
  return p.locator("header").getByRole("button", { name: "Actions de la page", exact: true });
}

/** Deux images : ce que le défilement fait distribuer l'a été. */
async function twoFrames(p: Page): Promise<void> {
  await p.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
}

const scrollY = (p: Page) => p.evaluate(() => window.scrollY);

test("page_menu_opens_at_the_first_click_after_typing_at_the_end_of_a_long_page", async ({ browser, request }) => {
  const { p } = await longPage(browser, request);
  await p.locator(".mm-editor .bn-block-content").last().click();
  await p.keyboard.press("End");
  await p.keyboard.type(" et la suite");
  await twoFrames(p);
  // La frappe a fait défiler le document : la barre du haut n'est plus à l'écran.
  expect(await scrollY(p)).toBeGreaterThan(200);

  const button = pageMenuButton(p);
  await button.click();
  await twoFrames(p);
  const menu = p.getByRole("menu", { name: "Actions de la page" });
  await expect(menu).toBeVisible();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(menu.getByRole("menuitem").first()).toBeFocused();
  // Le clic a ramené la barre du haut à l'écran, avant d'ouvrir le menu.
  expect(await scrollY(p)).toBe(0);
});

test("page_menu_opened_from_the_keyboard_on_a_scrolled_page_takes_the_focus", async ({ browser, request }) => {
  const { p } = await longPage(browser, request);
  await p.locator(".mm-editor .bn-block-content").last().click();
  await p.keyboard.press("End");
  await p.keyboard.type(" et la suite");
  await twoFrames(p);
  expect(await scrollY(p)).toBeGreaterThan(200);
  // Le focus porté au bouton le fait revenir à l'écran, comme Maj+Tab ; Entrée ouvre le menu.
  const button = pageMenuButton(p);
  await button.focus();
  await p.keyboard.press("Enter");
  await twoFrames(p);
  const menu = p.getByRole("menu", { name: "Actions de la page" });
  await expect(menu.getByRole("menuitem").first()).toBeFocused();
  await p.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(button).toBeFocused();
});

test("a_scroll_of_what_holds_the_button_closes_the_menu_and_gives_the_focus_back", async ({ browser, request }) => {
  const { p } = await longPage(browser, request);
  const button = pageMenuButton(p);
  await button.click();
  const menu = p.getByRole("menu", { name: "Actions de la page" });
  await expect(menu.getByRole("menuitem").first()).toBeFocused();

  // Le document défile, et la barre du haut avec lui : le menu ne reste pas en l'air.
  await p.evaluate(() => window.scrollBy(0, 300));
  await expect(menu).toBeHidden();
  await expect(button).toHaveAttribute("aria-expanded", "false");
  // Le focus revient au bouton, sans faire revenir le document jusqu'à lui.
  await expect(button).toBeFocused();
  await twoFrames(p);
  expect(await scrollY(p)).toBe(300);
});

test("a_scroll_of_something_else_leaves_the_menu_open", async ({ browser, request }) => {
  const { page, p } = await longPage(browser, request);

  // Le menu d'une ligne de la barre latérale (qui reste collée au haut de la fenêtre) : le document
  // qui défile ne déplace pas son bouton.
  const rowMenu = await openRowMenu(p, page.id, page.title);
  await expect(rowMenu.getByRole("menuitem").first()).toBeFocused();
  await p.evaluate(() => window.scrollBy(0, 300));
  await twoFrames(p);
  expect(await scrollY(p)).toBe(300);
  await expect(rowMenu).toBeVisible();
  await expect(rowMenu.getByRole("menuitem").first()).toBeFocused();
  await p.keyboard.press("Escape");
  await expect(rowMenu).toBeHidden();

  // Le menu de la page, quand la barre latérale défile.
  await p.evaluate(() => window.scrollTo(0, 0));
  await p.setViewportSize({ width: 1280, height: 260 });
  const sidebar = sidebarOf(p);
  await expect.poll(() => sidebar.evaluate((e) => e.scrollHeight - e.clientHeight)).toBeGreaterThan(40);
  const button = pageMenuButton(p);
  await button.click();
  const menu = p.getByRole("menu", { name: "Actions de la page" });
  await expect(menu.getByRole("menuitem").first()).toBeFocused();
  await sidebar.evaluate((e) => e.scrollBy(0, 40));
  await twoFrames(p);
  expect(await sidebar.evaluate((e) => e.scrollTop)).toBeGreaterThan(0);
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem").first()).toBeFocused();
});

test("a_row_menu_closes_when_the_sidebar_scrolls_and_gives_the_focus_back", async ({ browser, request }) => {
  const { page, p } = await longPage(browser, request);
  await p.setViewportSize({ width: 1280, height: 260 });
  const sidebar = sidebarOf(p);
  await expect.poll(() => sidebar.evaluate((e) => e.scrollHeight - e.clientHeight)).toBeGreaterThan(40);
  const rowMenu = await openRowMenu(p, page.id, page.title);
  await expect(rowMenu.getByRole("menuitem").first()).toBeFocused();
  const rowButton = p.getByRole("button", { name: `Actions de « ${page.title} »` });
  const before = await sidebar.evaluate((e) => e.scrollTop);
  // La ligne remonte avec la barre latérale (vers le haut ou vers le bas, selon où elle en est).
  await sidebar.evaluate((e) => (e.scrollTop > 20 ? e.scrollBy(0, -20) : e.scrollBy(0, 20)));
  await expect(rowMenu).toBeHidden();
  await expect(rowButton).toBeFocused();
  await twoFrames(p);
  expect(await sidebar.evaluate((e) => e.scrollTop)).not.toBe(before);
});
