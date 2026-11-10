// L'écran revient à l'état du serveur : après une relecture qui échoue, rien de ce qui est
// affiché ne se perd ; après un refus, la barre latérale et la page ouverte sont relues ; ce
// qui change ailleurs finit par apparaître ici.
import { expect, test, type APIRequestContext, type Page } from "@playwright/test";
import type { ErrorBody } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getMmd,
  newUser,
  NO_SESSION,
  openPage,
  openRowMenu,
  sectionOf,
  shownRows,
  sidebarOf,
  signUpAccount,
  titleOf,
  trashBanner,
  trashItem,
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

/** Supprime pour de bon la page `id`, comme le ferait quelqu'un d'autre ailleurs. */
async function deleteElsewhere(api: APIRequestContext, id: string): Promise<void> {
  expect((await api.post(`/api/v1/pages/${id}/trash`)).status()).toBe(200);
  expect((await api.delete(`/api/v1/pages/${id}`)).status()).toBe(204);
}

test("refused_trash_and_creation_reload_the_sidebar", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  await createPage(api, { title: "Charlie" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "1 Charlie"]);
  const alert = sidebarOf(p).getByRole("alert");

  // « Alpha » est supprimée ailleurs ; ici, sa ligne est toujours là. La mettre à la corbeille
  // est refusé : le message du serveur, et la barre latérale telle qu'il la voit.
  await deleteElsewhere(api, a.id);
  const gone = await api.post(`/api/v1/pages/${a.id}/trash`);
  expect(gone.status()).toBe(404);
  const body = (await gone.json()) as ErrorBody;
  await (await openRowMenu(p, a.id, "Alpha"))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(alert).toContainText("La page n'a pas pu être mise à la corbeille");
  await expect(alert).toContainText(body.message, { ignoreCase: true });
  await expect.poll(() => shownRows(p)).toEqual(["1 Bravo", "1 Charlie"]);

  // « Bravo » aussi : y ajouter une sous-page est refusé, et sa ligne s'en va.
  await deleteElsewhere(api, b.id);
  await treeRow(p, b.id).hover();
  await treeRow(p, b.id).getByRole("button", { name: "Ajouter une sous-page à « Bravo »" }).click();
  await expect(alert).toContainText("La page n'a pas pu être créée");
  // Dit pour la personne, sans les noms de champs de l'API.
  await expect(alert).not.toContainText("parentId");
  await expect.poll(() => shownRows(p)).toEqual(["1 Charlie"]);
  expect(new URL(p.url()).pathname).toBe(`/s/${space.id}`);
});

test("trash_view_follows_the_sidebar", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}/trash`);
  await expect(p.getByText("La corbeille est vide.")).toBeVisible();
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo"]);

  // Une page jetée depuis la barre latérale arrive dans la corbeille ouverte.
  await (await openRowMenu(p, a.id, "Alpha"))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(trashItem(p, "Alpha")).toBeVisible();
  await expect(p.getByText("La corbeille est vide.")).toHaveCount(0);
  // Une seconde aussi, la plus récente d'abord.
  await (await openRowMenu(p, b.id, "Bravo"))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(
    p.getByRole("list", { name: "Pages à la corbeille" }).getByRole("listitem"),
  ).toHaveText([/Bravo/, /Alpha/]);
  expect(new URL(p.url()).pathname).toBe(`/s/${space.id}/trash`);
});

test("failed_trash_reload_keeps_the_list", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  expect((await api.post(`/api/v1/pages/${a.id}/trash`)).status()).toBe(200);

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}/trash`);
  await expect(trashItem(p, "Alpha")).toBeVisible();
  const main = p.getByRole("main");

  // Le serveur ne répond plus pour la corbeille. Une page est jetée depuis la barre
  // latérale : la corbeille ouverte est relue, en vain.
  await p.route("**/api/v1/spaces/*/trash", (route) => route.abort());
  await (await openRowMenu(p, b.id, "Bravo"))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(treeRow(p, b.id)).toHaveCount(0);
  await expect(main.getByRole("alert")).toContainText("La corbeille n'a pas pu être relue.");
  // La liste déjà lue reste à l'écran, avec ses actions.
  await expect(trashItem(p, "Alpha")).toBeVisible();
  await expect(trashItem(p, "Alpha").getByRole("button", { name: "Restaurer" })).toBeVisible();

  // Le serveur revient : « Réessayer » relit, le message part, et la page jetée entre-temps
  // est là.
  await p.unroute("**/api/v1/spaces/*/trash");
  await main.getByRole("button", { name: "Réessayer" }).click();
  await expect(main.getByRole("alert")).toHaveCount(0);
  await expect(
    p.getByRole("list", { name: "Pages à la corbeille" }).getByRole("listitem"),
  ).toHaveText([/Bravo/, /Alpha/]);
});

test("refused_restore_reads_the_page_again", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  for (const id of [a.id, b.id]) {
    expect((await api.post(`/api/v1/pages/${id}/trash`)).status()).toBe(200);
  }

  // « Alpha », à la corbeille, est ouverte ; ailleurs, on la supprime pour de bon.
  const p = await newUser(browser, alice);
  await p.goto(`/p/${a.id}`);
  await expect(trashBanner(p)).toBeVisible();
  expect((await api.delete(`/api/v1/pages/${a.id}`)).status()).toBe(204);
  // « Restaurer » est refusé : la page est relue, et elle n'est plus là.
  await trashBanner(p).getByRole("button", { name: "Restaurer" }).click();
  await expect(p.getByRole("heading", { name: "Page introuvable" })).toBeVisible();
  await expect(trashBanner(p)).toHaveCount(0);

  // « Bravo », à la corbeille, est ouverte ; ailleurs, on la restaure. Ici, le bandeau est
  // toujours là : « Restaurer » la montre telle qu'elle est, et sa ligne revient.
  await p.goto(`/p/${b.id}`);
  await expect(trashBanner(p)).toBeVisible();
  await expect(treeRow(p, b.id)).toHaveCount(0);
  expect((await api.post(`/api/v1/pages/${b.id}/restore`)).status()).toBe(200);
  await trashBanner(p).getByRole("button", { name: "Restaurer" }).click();
  await expect(trashBanner(p)).toHaveCount(0);
  await expect(treeRow(p, b.id).getByRole("link")).toHaveAttribute("aria-current", "page");
  await expect(titleOf(p)).not.toHaveAttribute("readonly", "");
});

test("focus_follows_a_page_removed_elsewhere", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  await createPage(api, { parentId: b.id, title: "Bravo bis" });
  const c = await createPage(api, { title: "Charlie" });

  const p = await newUser(browser, alice);
  // L'horloge de la page est la nôtre : on n'attend pas cinq secondes pour de bon.
  await p.clock.install();
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "2 Bravo bis", "1 Charlie"]);
  const comeBack = async () => {
    await p.clock.fastForward(5_500);
    await p.evaluate(() => window.dispatchEvent(new Event("focus")));
  };

  // Le focus est sur la ligne de « Bravo ». Ailleurs, elle est supprimée avec sa sous-page ;
  // de retour sur l'onglet, l'arbre est relu, sa ligne part, et le focus va à celle qui prend
  // sa place, comme après une mise à la corbeille d'ici.
  await treeRow(p, b.id).getByRole("link").focus();
  await deleteElsewhere(api, b.id);
  await comeBack();
  await expect(treeRow(p, b.id)).toHaveCount(0);
  await expect(treeRow(p, c.id).getByRole("link")).toBeFocused();
  // La dernière ligne part : le focus va à celle d'avant.
  await deleteElsewhere(api, c.id);
  await comeBack();
  await expect(treeRow(p, c.id)).toHaveCount(0);
  await expect(treeRow(p, a.id).getByRole("link")).toBeFocused();

  // Une ligne qui n'a pas le focus part sans le déplacer.
  const d = await createPage(api, { title: "Delta" });
  await comeBack();
  await expect(treeRow(p, d.id)).toBeVisible();
  await deleteElsewhere(api, d.id);
  await comeBack();
  await expect(treeRow(p, d.id)).toHaveCount(0);
  await expect(treeRow(p, a.id).getByRole("link")).toBeFocused();
  // La seule ligne de la section part : le focus va au titre de la section.
  await deleteElsewhere(api, a.id);
  await comeBack();
  await expect(treeRow(p, a.id)).toHaveCount(0);
  await expect(sectionOf(p, "Espace").getByRole("button", { name: "Espace", exact: true })).toBeFocused();
});

test("tree_is_read_again_when_the_tab_comes_back", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });

  const p = await newUser(browser, alice);
  // L'horloge de la page est la nôtre : on n'attend pas cinq secondes pour de bon.
  await p.clock.install();
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha"]);
  let reads = 0;
  p.on("request", (request) => {
    if (request.url().endsWith(`/spaces/${space.id}/tree`)) reads += 1;
  });
  const comeBack = () => p.evaluate(() => window.dispatchEvent(new Event("focus")));

  // Ailleurs, une page est créée et une autre renommée ; ici, rien ne bouge.
  const b = await createPage(api, { title: "Bravo" });
  const renamed = await api.patch(`/api/v1/pages/${a.id}`, { data: { title: "Alpha, renommée" } });
  expect(renamed.status()).toBe(200);
  // Retour sur l'onglet juste après son chargement : l'arbre vient d'être lu.
  await comeBack();
  await p.waitForTimeout(300);
  expect(reads).toBe(0);
  expect(await shownRows(p)).toEqual(["1 Alpha"]);

  // Plus de cinq secondes après : il est relu, et ce qui a changé ailleurs arrive.
  await p.clock.fastForward(5_500);
  await comeBack();
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha, renommée", "1 Bravo"]);
  await expect(treeRow(p, b.id)).toBeVisible();
  expect(reads).toBe(1);
  // Pas deux fois de suite.
  await comeBack();
  await p.waitForTimeout(300);
  expect(reads).toBe(1);
});
