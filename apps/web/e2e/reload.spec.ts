// L'écran revient à l'état du serveur : après une relecture qui échoue, rien de ce qui est
// affiché ne se perd ; après un refus, la barre latérale et la page ouverte sont relues ; ce
// qui change ailleurs finit par apparaître ici.
import { expect, test, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getMmd,
  NO_SESSION,
  openPage,
  openRowMenu,
  shownRows,
  sidebarOf,
  signUpAccount,
  titleOf,
  treeRow,
  unique,
} from "./helpers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

/** Un repère sur le conteneur de l'éditeur : démonté, il le perdrait. */
async function markEditor(p: Page): Promise<() => Promise<boolean>> {
  await p.locator(".mm-editor").evaluate((el) => ((el as unknown as { mark: number }).mark = 1));
  return () =>
    p.locator(".mm-editor").evaluate((el) => (el as unknown as { mark?: number }).mark === 1);
}

test("failed_reload_keeps_the_open_page_and_what_was_typed", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });

  const p = await openPage(browser, a.id, alice);
  const editor = p.locator(".bn-editor");
  await editor.click();
  await p.keyboard.type("Avant la panne");
  await expect.poll(() => getMmd(api, a.id)).toBe("Avant la panne\n");
  const marked = await markEditor(p);

  // Le serveur ne répond plus, ni à la synchro, ni aux appels ; on écrit quand même.
  await p.context().setOffline(true);
  await p.evaluate(() => window.__mementum!.provider.disconnect());
  await p.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
  await p.keyboard.press("End");
  await p.keyboard.type(", pendant la panne");

  // « Déplacer vers le haut » échoue ; la barre latérale est relue, et cela échoue aussi.
  const reread = p.waitForEvent("requestfailed", (request) => request.url().endsWith("/tree"));
  await (await openRowMenu(p, b.id, "Bravo"))
    .getByRole("menuitem", { name: "Déplacer vers le haut" })
    .click();
  await reread;
  await expect(sidebarOf(p).getByRole("alert")).toContainText("La page n'a pas pu être déplacée");

  // Rien de ce qui est affiché ne se perd : ni la page, ni son éditeur (le même, avec ce qui
  // y a été tapé), ni la barre latérale.
  await expect(p.getByRole("heading", { name: "Impossible de charger Mementum" })).toHaveCount(0);
  await expect(titleOf(p)).toHaveValue("Alpha");
  expect(await marked()).toBe(true);
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(editor).toContainText("Avant la panne, pendant la panne");
  expect(await shownRows(p)).toEqual(["1 Alpha", "1 Bravo"]);
  await p.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
  await p.keyboard.press("End");
  await p.keyboard.type(", et encore");

  // Pendant la panne, une page est créée ailleurs.
  const c = await createPage(api, { title: "Charlie" });

  // Le serveur revient : ce qui a été tapé part ; la barre latérale est relue d'elle-même.
  await p.context().setOffline(false);
  await p.evaluate(() => window.__mementum!.provider.connect());
  await expect
    .poll(() => getMmd(api, a.id), { timeout: 15_000 })
    .toBe("Avant la panne, pendant la panne, et encore\n");
  expect(await marked()).toBe(true);
  await expect(treeRow(p, c.id)).toBeVisible();
  // Le refus du déplacement reste dit jusqu'à la navigation suivante ; plus rien ensuite.
  await treeRow(p, b.id).getByRole("link").click();
  await expect(titleOf(p)).toHaveValue("Bravo");
  await expect(sidebarOf(p).getByRole("alert")).toHaveCount(0);
  expect((await api.get(`/api/v1/spaces/${space.id}/tree`)).status()).toBe(200);
});
