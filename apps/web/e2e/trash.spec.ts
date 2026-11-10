// La corbeille, depuis l'interface : le menu « … » d'une ligne, la vue de la corbeille
// (« Restaurer », « Supprimer définitivement »), le bandeau d'une page jetée, et le focus qui
// suit une page qui part. Le serveur fait foi pour les droits : chaque test relit aussi ce
// qu'il répond.
import { expect, test } from "@playwright/test";
import type { ErrorBody, PageNode } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  forbidNativeDialogs,
  getTree,
  newUser,
  NO_SESSION,
  openRowMenu,
  ownerAndMember,
  READ_ONLY_BANNER,
  sectionOf,
  serverSchemaBumped,
  setPageLevel,
  shownRows,
  sidebarOf,
  signUpAccount,
  titleOf,
  trashBanner,
  trashItem,
  treeRow,
  unique,
  waitSynced,
} from "./helpers";
import { BASE_URL } from "./servers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

test("trash_and_restore_from_the_sidebar", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const parent = await createPage(api, { title: `Projet ${unique()}` });
  const child = await createPage(api, { parentId: parent.id, title: `Notes ${unique()}` });
  const other = await createPage(api, { title: `Autre ${unique()}` });
  const visible = async () => (await getTree(api, space.id)).nodes.map((n) => n.id).sort();
  const trashedAt = async (id: string) => {
    const res = await api.get(`/api/v1/pages/${id}`);
    expect(res.status(), await res.text()).toBe(200);
    return ((await res.json()) as PageNode).trashedAt;
  };

  const a = await newUser(browser, alice);
  const nativeDialogs = forbidNativeDialogs(a);
  await a.goto(`/p/${parent.id}`);
  await waitSynced(a);
  const editor = a.locator(".bn-editor");
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(treeRow(a, child.id)).toBeVisible();

  // « Mettre à la corbeille », dans le menu « … » de la ligne : la page part avec sa sous-page.
  const menu = await openRowMenu(a, parent.id, parent.title);
  await menu.getByRole("menuitem", { name: "Mettre à la corbeille" }).click();
  await expect(menu).toHaveCount(0);
  await expect(treeRow(a, parent.id)).toHaveCount(0);
  await expect(treeRow(a, child.id)).toHaveCount(0);
  await expect(treeRow(a, other.id)).toBeVisible();
  expect(await visible()).toEqual([other.id]);
  expect(await trashedAt(parent.id)).not.toBeNull();
  // La page ouverte le dit, et ne s'écrit plus.
  await expect(trashBanner(a)).toBeVisible();
  await expect(titleOf(a)).toHaveValue(parent.title);
  await expect(titleOf(a)).toHaveAttribute("readonly", "");
  await expect(editor).toHaveAttribute("contenteditable", "false");

  // « Corbeille », en bas de la barre latérale : la page jetée y est, pas sa sous-page, qui
  // n'y est que par elle.
  await sidebarOf(a).getByRole("link", { name: "Corbeille" }).click();
  await expect(a).toHaveURL(`${BASE_URL}/s/${space.id}/trash`);
  await expect(a.getByRole("heading", { name: "Corbeille", level: 1 })).toBeVisible();
  await expect(trashItem(a, parent.title)).toBeVisible();
  await expect(trashItem(a, child.title)).toHaveCount(0);

  // « Restaurer » : elle revient dans la barre latérale, avec sa sous-page.
  await trashItem(a, parent.title).getByRole("button", { name: "Restaurer" }).click();
  await expect(trashItem(a, parent.title)).toHaveCount(0);
  await expect(a.getByText("La corbeille est vide.")).toBeVisible();
  await expect(treeRow(a, parent.id).getByRole("link")).toHaveText(parent.title);
  await expect(treeRow(a, child.id)).toHaveAttribute("aria-level", "2");
  expect(await visible()).toEqual([parent.id, child.id, other.id].sort());
  expect(await trashedAt(parent.id)).toBeNull();

  // Une sous-page partie avec sa page parente le dit aussi ; c'est la page parente qu'on
  // restaure, et le serveur le dit à qui essaie sur la sous-page.
  await treeRow(a, child.id).getByRole("link").click();
  await waitSynced(a);
  await (await openRowMenu(a, parent.id, parent.title))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(trashBanner(a)).toBeVisible();
  await expect(titleOf(a)).toHaveValue(child.title);
  await expect(editor).toHaveAttribute("contenteditable", "false");
  expect(await trashedAt(child.id)).not.toBeNull();
  const refused = await api.post(`/api/v1/pages/${child.id}/restore`);
  expect(refused.status()).toBe(409);
  const body = (await refused.json()) as ErrorBody;
  expect(body.error).toBe("parent_trashed");
  await trashBanner(a).getByRole("button", { name: "Restaurer" }).click();
  await expect(a.getByRole("main").getByRole("alert")).toHaveText(body.message);
  await expect(trashBanner(a)).toBeVisible();

  // La page parente, ouverte depuis la corbeille, se restaure par son bandeau.
  await sidebarOf(a).getByRole("link", { name: "Corbeille" }).click();
  await trashItem(a, parent.title).getByRole("link", { name: parent.title }).click();
  await expect(a).toHaveURL(`${BASE_URL}/p/${parent.id}`);
  await trashBanner(a).getByRole("button", { name: "Restaurer" }).click();
  await expect(trashBanner(a)).toHaveCount(0);
  await expect(treeRow(a, parent.id).getByRole("link")).toHaveAttribute("aria-current", "page");
  await expect(treeRow(a, child.id)).toBeVisible();
  await waitSynced(a);
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(titleOf(a)).not.toHaveAttribute("readonly", "");
  expect(await visible()).toEqual([parent.id, child.id, other.id].sort());
  expect(nativeDialogs).toEqual([]);
});

test("trash_view_is_read_only_under_another_schema", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  expect((await api.post(`/api/v1/pages/${a.id}/trash`)).status()).toBe(200);

  // Le serveur a été mis à jour : cet onglet n'écrit plus rien (R35), la corbeille comprise.
  const p = await newUser(browser, alice);
  await serverSchemaBumped(p);
  await p.goto(`/s/${space.id}/trash`);
  // La liste se lit, avec le lien vers la page ; le bandeau dit pourquoi rien ne s'y fait.
  await expect(trashItem(p, "Alpha").getByRole("link", { name: "Alpha" })).toBeVisible();
  await expect(p.getByRole("main").getByText(READ_ONLY_BANNER)).toBeVisible();
  // Ni « Restaurer », ni « Supprimer définitivement ».
  await expect(trashItem(p, "Alpha").getByRole("button")).toHaveCount(0);
  expect((await getTree(api, space.id)).nodes).toEqual([]);
});

test("delete_forever_asks_first", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const parent = await createPage(api, { title: `Brouillon ${unique()}` });
  const child = await createPage(api, { parentId: parent.id, title: `Annexe ${unique()}` });
  const kept = await createPage(api, { title: `À garder ${unique()}` });
  for (const id of [parent.id, kept.id]) {
    expect((await api.post(`/api/v1/pages/${id}/trash`)).status()).toBe(200);
  }
  const status = async (id: string) => (await api.get(`/api/v1/pages/${id}`)).status();

  const a = await newUser(browser, alice);
  const nativeDialogs = forbidNativeDialogs(a);
  await a.goto(`/s/${space.id}/trash`);
  // La plus récente d'abord.
  await expect(
    a.getByRole("list", { name: "Pages à la corbeille" }).getByRole("listitem"),
  ).toHaveText([new RegExp(kept.title), new RegExp(parent.title)]);
  const remove = trashItem(a, parent.title).getByRole("button", { name: "Supprimer définitivement" });
  const confirm = a.getByRole("alertdialog", {
    name: `Supprimer définitivement « ${parent.title} » ?`,
  });

  // La suppression demande une confirmation dans la page. Le focus est sur « Annuler » :
  // Entrée ne supprime rien.
  await remove.click();
  await expect(confirm).toBeVisible();
  await expect(confirm.getByRole("button", { name: "Annuler" })).toBeFocused();
  await a.keyboard.press("Enter");
  await expect(confirm).toHaveCount(0);
  // Échap annule aussi.
  await remove.click();
  await expect(confirm).toBeVisible();
  await a.keyboard.press("Escape");
  await expect(confirm).toHaveCount(0);
  await expect(trashItem(a, parent.title)).toBeVisible();
  expect(await status(parent.id)).toBe(200);
  expect(await status(child.id)).toBe(200);

  // Confirmée : la page et sa sous-page n'existent plus ; l'autre est toujours à la corbeille.
  await remove.click();
  await confirm.getByRole("button", { name: "Supprimer définitivement" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(trashItem(a, parent.title)).toHaveCount(0);
  await expect(trashItem(a, kept.title)).toBeVisible();
  // Le bouton qui a ouvert la fenêtre est parti avec sa ligne : le focus va au titre.
  await expect(a.getByRole("heading", { name: "Corbeille", level: 1 })).toBeFocused();
  expect(await status(parent.id)).toBe(404);
  expect(await status(child.id)).toBe(404);
  expect(await status(kept.id)).toBe(200);
  const trash = await api.get(`/api/v1/spaces/${space.id}/trash`);
  expect(((await trash.json()) as PageNode[]).map((n) => n.id)).toEqual([kept.id]);
  expect(nativeDialogs).toEqual([]);
});

test("delete_forever_needs_full_access", async ({ browser }) => {
  const { alice, aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: "Brouillon commun" });
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");

  // B, qui peut la modifier, la met à la corbeille depuis sa barre latérale.
  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  await (await openRowMenu(b, shared.id, "Brouillon commun"))
    .getByRole("menuitem", { name: "Mettre à la corbeille" })
    .click();
  await expect(treeRow(b, shared.id)).toHaveCount(0);

  // Dans sa corbeille : « Restaurer », pas « Supprimer définitivement », que le serveur lui
  // refuserait.
  await sidebarOf(b).getByRole("link", { name: "Corbeille" }).click();
  const mine = trashItem(b, "Brouillon commun");
  await expect(mine.getByRole("button", { name: "Restaurer" })).toBeVisible();
  await expect(mine.getByRole("button", { name: "Supprimer définitivement" })).toHaveCount(0);
  await expect(mine.getByRole("button")).toHaveCount(1);
  const refused = await brunoApi.delete(`/api/v1/pages/${shared.id}`);
  expect(refused.status()).toBe(403);

  // A, en accès complet, a les deux.
  const a = await newUser(browser, alice);
  await a.goto(`/s/${space.id}/trash`);
  const hers = trashItem(a, "Brouillon commun");
  await expect(hers.getByRole("button", { name: "Restaurer" })).toBeVisible();
  await expect(hers.getByRole("button", { name: "Supprimer définitivement" })).toBeVisible();
});

test("focus_follows_a_page_that_leaves", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(alice);
  const space = await firstSpace(api);
  const a = await createPage(api, { title: "Alpha" });
  const b = await createPage(api, { title: "Bravo" });
  await createPage(api, { parentId: b.id, title: "Bravo bis" });
  const c = await createPage(api, { title: "Charlie" });

  const p = await newUser(browser, alice);
  await p.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(p)).toEqual(["1 Alpha", "1 Bravo", "2 Bravo bis", "1 Charlie"]);
  /** Met à la corbeille, au clavier, la page de la ligne `id`. */
  const trashByKeyboard = async (id: string, title: string) => {
    await treeRow(p, id).getByRole("button", { name: `Actions de « ${title} »` }).focus();
    await p.keyboard.press("Enter");
    await p.keyboard.press("End");
    await expect(
      p.getByRole("menu").getByRole("menuitem", { name: "Mettre à la corbeille" }),
    ).toBeFocused();
    await p.keyboard.press("Enter");
    await expect(treeRow(p, id)).toHaveCount(0);
  };

  // « Bravo » part avec sa sous-page : le focus va à la ligne qui prend sa place.
  await trashByKeyboard(b.id, "Bravo");
  await expect(treeRow(p, c.id).getByRole("link")).toBeFocused();
  // La dernière ligne part : le focus va à celle d'avant.
  await trashByKeyboard(c.id, "Charlie");
  await expect(treeRow(p, a.id).getByRole("link")).toBeFocused();
  // La seule ligne de la section part : le focus va au titre de la section.
  await trashByKeyboard(a.id, "Alpha");
  await expect(
    sectionOf(p, "Espace").getByRole("button", { name: "Espace", exact: true }),
  ).toBeFocused();

  // Dans la corbeille, « Restaurer » retire la ligne : le focus va au titre de la vue.
  await sidebarOf(p).getByRole("link", { name: "Corbeille" }).click();
  const restore = trashItem(p, "Charlie").getByRole("button", { name: "Restaurer" });
  await restore.focus();
  await p.keyboard.press("Enter");
  await expect(trashItem(p, "Charlie")).toHaveCount(0);
  await expect(p.getByRole("heading", { name: "Corbeille", level: 1 })).toBeFocused();
});
