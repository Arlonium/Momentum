// Arbre de pages et persistance, depuis l'interface.
import { expect, test } from "@playwright/test";
import { createPage, getMmd, getTree, pageIdOf, unique, waitSynced } from "./helpers";

const UUID = /\/p\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

test("create_page_and_type_persists", async ({ page, request }) => {
  await page.goto("/");
  const sidebar = page.getByRole("navigation", { name: "Barre latérale" });
  await sidebar.getByRole("button", { name: "Nouvelle page" }).click();
  await expect(page).toHaveURL(UUID);
  const id = pageIdOf(page);

  const title = page.getByRole("textbox", { name: "Titre de la page" });
  await title.fill("Essai");
  await title.press("Enter");
  // L'arbre suit le renommage.
  await expect(sidebar.locator(`[data-page-id="${id}"]`).getByRole("link")).toHaveText("Essai");

  await waitSynced(page);
  await page.locator(".bn-editor").click();
  await page.keyboard.type("Bonjour Café 🚀");
  // Rechargement une fois le texte reçu par le serveur (sinon on mesurerait l'envoi).
  await expect.poll(() => getMmd(request, id)).toContain("Bonjour Café 🚀");

  await page.reload();
  await waitSynced(page);
  await expect(title).toHaveValue("Essai");
  await expect(page.locator(".bn-editor")).toContainText("Bonjour Café 🚀");
  expect(await getMmd(request, id)).toContain("Bonjour Café 🚀");
  const node = (await getTree(request)).nodes.find((n) => n.id === id);
  expect(node?.title).toBe("Essai");
});

test("subpage_appears_in_tree", async ({ page, request }) => {
  const parentTitle = `Parent ${unique()}`;
  const parent = await createPage(request, { title: parentTitle });
  await page.goto(`/p/${parent.id}`);
  const sidebar = page.getByRole("navigation", { name: "Barre latérale" });
  const parentItem = sidebar.locator(`[data-page-id="${parent.id}"]`);
  await expect(parentItem.getByRole("link").first()).toHaveText(parentTitle);

  // Le « + » n'apparaît qu'au survol de la page.
  const row = parentItem.locator(".tree-row").first();
  const add = row.getByRole("button", { name: "Ajouter une sous-page" });
  await expect(add).toHaveCSS("opacity", "0");
  await row.hover();
  await expect(add).toHaveCSS("opacity", "1");
  await add.click();
  // La page parente est déjà une `/p/:id` : attendre l'adresse de la sous-page.
  await page.waitForURL((url) => UUID.test(url.pathname) && !url.pathname.endsWith(parent.id));
  const childId = pageIdOf(page);

  const title = page.getByRole("textbox", { name: "Titre de la page" });
  await title.fill("Sous-page");
  await title.press("Enter");

  // Imbriquée sous sa page parente, dans l'interface comme sur le serveur.
  const child = parentItem.locator(`[data-page-id="${childId}"]`);
  await expect(child.getByRole("link")).toHaveText("Sous-page");
  const node = (await getTree(request)).nodes.find((n) => n.id === childId);
  expect(node?.parentId).toBe(parent.id);
  expect(node?.title).toBe("Sous-page");

  await page.reload();
  await expect(child.getByRole("link")).toHaveText("Sous-page");

  // Un clic dans l'arbre ouvre la page, sans recharger l'application.
  await page.evaluate(() => ((window as unknown as { __loaded: boolean }).__loaded = true));
  await parentItem.getByRole("link").first().click();
  await expect(page).toHaveURL(new RegExp(`/p/${parent.id}$`));
  await expect(title).toHaveValue(parentTitle);
  expect(await page.evaluate(() => (window as unknown as { __loaded?: boolean }).__loaded)).toBe(true);
});
