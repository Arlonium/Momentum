// Les mots de BlockNote dans la page, tels que l'application les montre (R766) : « Envoyer » pour un
// fichier qui part du poste, un seul groupe « Médias » dans le menu « / ». Le dictionnaire entier est
// relu par `src/editor/dictionary.test.ts` ; ici, qu'il est bien celui de l'éditeur de la page.
import { expect, test } from "@playwright/test";
import { closeContexts, createPage, openPage, unique } from "./helpers";

test.afterEach(closeContexts);

test("the file panel says envoyer and the slash menu has one media group", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Mots ${unique()}` });
  const p = await openPage(browser, page.id);
  await p.locator(".mm-editor .bn-block-content").first().click();

  // Le menu « / » : les blocs de fichier sous un seul titre de groupe.
  await p.keyboard.type("/");
  const menu = p.locator(".bn-suggestion-menu");
  await expect(menu.locator(".bn-suggestion-menu-item").first()).toBeVisible();
  const groups = await menu.locator(".bn-suggestion-menu-label").allTextContents();
  expect(groups.filter((group) => group.startsWith("Média"))).toEqual(["Médias"]);

  // « Image » : le panneau du bloc s'ouvre ; on y envoie, on n'y télécharge pas.
  await p.keyboard.type("image");
  await expect(menu.locator(".bn-suggestion-menu-item").first()).toContainText("Image");
  await p.keyboard.press("Enter");
  const panel = p.locator(".bn-panel");
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("tab")).toHaveText(["Envoyer", "Intégrer"]);
  await expect(panel.getByText("Envoyer une image")).toBeVisible();
  await expect(panel).not.toContainText("Télécharger");
});
