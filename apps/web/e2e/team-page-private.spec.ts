// Une page née dans la section « Espace » redevient privée par le menu Partager (Ruling R122).
// Elle ne porte que l'accès général des membres : son auteur n'y est pas nommé. Qui coche
// « Seulement les invités » y garde pourtant l'accès complet : le serveur pose sa ligne avec
// le changement. Avant, le choix était refusé sans issue (`409 last_full_access`), ou retirait
// la page à celui qui cliquait. Le serveur fait foi : le test relit aussi ce qu'il répond.
import { expect, test, type Page } from "@playwright/test";
import {
  accessLost,
  accessRow,
  apiAs,
  closeContexts,
  getAccess,
  getMe,
  getTree,
  memberOf,
  newUser,
  NO_SESSION,
  openShareMenu,
  ownerAndMember,
  pageIdOf,
  sectionOf,
  shareMenu,
  sidebarOf,
  titleOf,
  treeRow,
  unique,
  UNKNOWN_ID,
  waitSynced,
} from "./helpers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

/** Crée une page par le « + » de la section « Espace », la nomme, et rend son id. */
async function addTeamPage(p: Page, title: string): Promise<string> {
  const before = new URL(p.url()).pathname;
  await sectionOf(p, "Espace")
    .getByRole("button", { name: "Ajouter une page dans « Espace »" })
    .click();
  await p.waitForURL((url) => url.pathname.startsWith("/p/") && url.pathname !== before);
  const id = pageIdOf(p);
  await titleOf(p).fill(title);
  await titleOf(p).press("Enter");
  await expect(treeRow(p, id).getByRole("link")).toHaveText(title);
  return id;
}

test("team_page_becomes_private_from_the_share_menu", async ({ browser }) => {
  // Deux membres simples du même espace : Bruno, qui crée la page, et Chloé.
  const { alice, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const chloe = await memberOf(alice, space.id, `Chloé ${unique()}`);
  const chloeApi = await apiAs(chloe);
  expect((await getMe(brunoApi)).spaces).toEqual([{ id: space.id, name: space.name, role: "member" }]);

  // Bruno crée une page par le « + » de la section « Espace ».
  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  const title = `Notes d'équipe ${unique()}`;
  const id = await addTeamPage(b, title);
  await waitSynced(b);
  await expect(sectionOf(b, "Espace").locator(`[data-page-id="${id}"]`)).toBeVisible();
  const seenBy = async (api: typeof brunoApi) =>
    (await getTree(api, space.id)).nodes.filter((n) => n.id === id).map((n) => [n.section, n.level]);
  expect(await seenBy(brunoApi)).toEqual([["space", "full"]]);

  // Chloé la voit, dans « Espace », sur sa fenêtre. L'horloge de sa page est la nôtre : on
  // n'attendra pas cinq secondes pour de bon avant que son arbre se relise (R97).
  const c = await newUser(browser, chloe);
  await c.clock.install();
  await c.goto(`/s/${space.id}`);
  await expect(sectionOf(c, "Espace").locator(`[data-page-id="${id}"]`)).toBeVisible();
  expect(await seenBy(chloeApi)).toEqual([["space", "full"]]);

  // Bruno ouvre « Partager » : personne n'est nommé sur la page, ouverte à tous les membres.
  const menu = await openShareMenu(b);
  await expect(menu).toContainText("Personne n'a d'accès à son nom sur cette page.");
  const general = menu.getByRole("radiogroup", { name: "Accès général" });
  const invited = general.getByRole("radio", { name: "Seulement les invités" });
  const members = general.getByRole("radio", { name: "Tous les membres de l'espace" });
  await expect(members).toBeChecked();
  await expect(invited).toBeEnabled();

  // Il coche « Seulement les invités » : aucun message d'erreur, le choix reste coché, et le
  // menu reste ouvert sur la page.
  await invited.click();
  await expect(invited).toBeChecked();
  await expect(menu.getByRole("combobox", { name: "Niveau des membres de l'espace" })).toHaveCount(0);
  await expect(b.getByRole("alert")).toHaveCount(0);
  await expect(accessLost(b)).toHaveCount(0);
  await expect(shareMenu(b)).toBeVisible();

  // Le menu le montre, lui, en « Accès complet », à son nom.
  const own = accessRow(menu, bruno.email);
  await expect(own).toContainText(bruno.name);
  const level = own.getByRole("combobox", { name: `Niveau de ${bruno.name}` });
  await expect(level).toHaveValue("full");
  await expect(level.locator("option:checked")).toHaveText("Accès complet");
  await expect(
    menu.getByRole("list", { name: "Personnes qui ont accès" }).getByRole("listitem"),
  ).toHaveCount(1);

  // La page est passée dans « Privé », et il peut toujours y écrire.
  await expect(sectionOf(b, "Privé").locator(`[data-page-id="${id}"]`)).toBeVisible();
  await expect(sectionOf(b, "Espace").locator(`[data-page-id="${id}"]`)).toHaveCount(0);
  await expect(sidebarOf(b).getByRole("alert")).toHaveCount(0);
  await expect(titleOf(b)).toHaveValue(title);
  // Le serveur dit la même chose.
  expect(await seenBy(brunoApi)).toEqual([["private", "full"]]);
  const access = await getAccess(brunoApi, id);
  expect(access.general).toBeNull();
  expect(access.entries.map((e) => [e.userId, e.level, e.inherited])).toEqual([[brunoId, "full", false]]);

  // Chloé, de retour sur sa fenêtre, ne la voit plus ; pour elle, la page n'existe plus :
  // la réponse du serveur est celle d'une page inconnue.
  await c.clock.fastForward(5_500);
  await c.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(treeRow(c, id)).toHaveCount(0);
  await expect(sidebarOf(c).getByText(title)).toHaveCount(0);
  expect(await seenBy(chloeApi)).toEqual([]);
  const gone = await chloeApi.get(`/api/v1/pages/${id}`);
  const unknown = await chloeApi.get(`/api/v1/pages/${UNKNOWN_ID}`);
  expect(gone.status()).toBe(404);
  expect([gone.status(), gone.headers()["content-type"], await gone.text()]).toEqual([
    unknown.status(),
    unknown.headers()["content-type"],
    await unknown.text(),
  ]);
});
