// Le partage d'une page, depuis l'interface : le menu Partager (inviter par lien, changer ou
// retirer un accès, l'accès général), la lecture seule par droit, et ce que voit quelqu'un dont
// l'accès change pendant qu'il a la page ouverte. Le serveur fait foi : chaque test relit aussi
// ce qu'il répond, et ses refus s'affichent tels quels.
import { expect, test, type Page } from "@playwright/test";
import type { ErrorBody } from "@mementum/protocol";
import {
  accessLost,
  accessRow,
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  forbidNativeDialogs,
  getAccess,
  getMe,
  getMmd,
  getPage,
  getTree,
  guestOf,
  holdPageReads,
  inviteToPage,
  inviteToSpace,
  logInWith,
  newUser,
  NO_SESSION,
  openPage,
  openShareMenu,
  ownerAndMember,
  PASSWORD,
  pendingRow,
  putMmd,
  READ_ONLY_BANNER,
  readerBanner,
  sectionOf,
  setGeneralAccess,
  setPageLevel,
  shareButton,
  shareMenu,
  sidebarOf,
  signUpAccount,
  titleOf,
  trashBanner,
  treeRow,
  unique,
  waitSynced,
  watchWrites,
} from "./helpers";
import { BASE_URL } from "./servers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

/** Le bandeau d'une page ouverte en lecture seule dont le niveau vient de monter. */
function raisedBanner(p: Page) {
  return p.getByRole("status").filter({ hasText: "Tu peux maintenant modifier cette page" });
}

/** Ce que le menu Partager dit du lien d'une invitation tout juste créée. */
const LINK_WARNING =
  "Copie ce lien maintenant : il ne sera plus affiché une fois la page fermée.";

/** Le premier paragraphe de la page ouverte. */
function firstParagraph(p: Page) {
  return p.locator('[data-content-type="paragraph"] .bn-inline-content').first();
}

/** Tape `text` à la fin du premier paragraphe. */
async function typeAtEnd(p: Page, text: string): Promise<void> {
  await firstParagraph(p).click();
  await p.keyboard.press("End");
  await p.keyboard.type(text);
}

/** La liste de niveaux d'une personne, dans le menu Partager. */
function levelOf(menu: ReturnType<typeof shareMenu>, account: { email: string; name: string }) {
  return accessRow(menu, account.email).getByRole("combobox", { name: `Niveau de ${account.name}` });
}

test("invite_by_link_then_edit_together", async ({ browser, page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const title = `Cahier ${unique()}`;
  const shared = await createPage(aliceApi, { section: "private", title });
  // Une page de la section « Espace » : un invité ne la voit pas.
  const forAll = await createPage(aliceApi, { title: `Pour l'espace ${unique()}` });
  const a = await openPage(browser, shared.id, alice);
  await a.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: BASE_URL });

  // A invite une adresse sur la page, en modification, puis copie le lien.
  const email = `bruno-${unique()}@exemple.fr`;
  const menu = await openShareMenu(a);
  await menu.getByLabel("Adresse e-mail").fill(email);
  await menu.getByLabel("Niveau de l'invitation").selectOption("edit");
  await menu.getByRole("button", { name: "Inviter", exact: true }).click();
  await expect(menu.getByText(LINK_WARNING)).toBeVisible();
  const shown = await menu.getByRole("textbox", { name: "Lien d'invitation" }).inputValue();
  await menu.getByRole("button", { name: "Copier le lien d'invitation" }).click();
  await expect(menu.getByRole("button", { name: "Lien copié" })).toBeVisible();
  const link = await a.evaluate(() => navigator.clipboard.readText());
  expect(link).toBe(shown);
  const token = new URL(link).hash.slice(1);
  expect(link).toBe(`${BASE_URL}/invite#${token}`);
  // L'invitation attend, avec son niveau et sans son jeton.
  await expect(pendingRow(menu, email)).toContainText("Peut modifier");
  await expect(pendingRow(menu, email)).not.toContainText(token);

  // B, dans un navigateur sans session, ouvre le lien : le jeton quitte aussitôt l'adresse.
  await page.goto(link);
  await expect(
    page.getByRole("heading", { name: `${alice.name} t'invite sur la page « ${title} »` }),
  ).toBeVisible();
  expect(page.url()).toBe(`${BASE_URL}/invite`);
  await page.getByRole("button", { name: "Créer mon compte" }).click();
  await expect(page.getByLabel("Adresse e-mail")).toHaveValue(email);
  const name = `Bruno ${unique()}`;
  await page.getByLabel("Nom").fill(name);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(PASSWORD);
  await password.press("Enter");

  // Son compte créé, il arrive sur la page, invité de l'espace.
  await expect(page).toHaveURL(`${BASE_URL}/p/${shared.id}`);
  await expect(titleOf(page)).toHaveValue(title);
  await waitSynced(page);
  expect((await getMe(page.request)).spaces).toEqual([
    { id: space.id, name: space.name, role: "guest" },
  ]);
  // Sa barre latérale n'a que « Partagé », avec cette page.
  await expect(sectionOf(page, "Partagé").locator(`[data-page-id="${shared.id}"]`)).toBeVisible();
  await expect(sectionOf(page, "Espace")).toHaveCount(0);
  await expect(sectionOf(page, "Privé")).toHaveCount(0);
  await expect(sidebarOf(page).getByRole("treeitem")).toHaveCount(1);
  await expect(sidebarOf(page).getByText(forAll.title)).toHaveCount(0);
  // En modification, il n'a pas le menu Partager.
  await expect(shareButton(page)).toHaveCount(0);

  // B tape ; A voit le texte.
  await page.locator(".bn-editor").click();
  await page.keyboard.type("Écrit par l'invité");
  await expect(a.locator(".bn-editor")).toContainText("Écrit par l'invité");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par l'invité\n");

  // Le menu de A, rouvert : B y est, en modification ; l'invitation n'attend plus, et son
  // lien n'est plus montré.
  await a.keyboard.press("Escape");
  await expect(shareMenu(a)).toHaveCount(0);
  const reopened = await openShareMenu(a);
  await expect(accessRow(reopened, email)).toContainText(name);
  await expect(levelOf(reopened, { email, name })).toHaveValue("edit");
  await expect(pendingRow(reopened, email)).toHaveCount(0);
  await expect(reopened.getByRole("textbox", { name: "Lien d'invitation" })).toHaveCount(0);

  // Ni l'adresse ni l'historique de B ne gardent le jeton.
  expect(page.url()).not.toContain(token);
  await page.goBack();
  expect(page.url()).not.toContain(token);
});

test("reader_cannot_type", async ({ browser }) => {
  const { aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `À lire ${unique()}` });
  await putMmd(aliceApi, shared.id, "Écrit par Alice\n");
  await setPageLevel(aliceApi, shared.id, brunoId, "read");

  const b = await newUser(browser, bruno);
  const writes = watchWrites(b);
  await b.goto(`/p/${shared.id}`);
  // Le bandeau dit pourquoi ; ce n'est pas celui d'un schéma périmé.
  await expect(readerBanner(b)).toBeVisible();
  await expect(b.getByText(READ_ONLY_BANNER)).toHaveCount(0);
  await waitSynced(b);
  const editor = b.locator(".bn-editor");
  await expect(editor).toContainText("Écrit par Alice");
  await expect(editor).toHaveAttribute("contenteditable", "false");
  expect(await b.evaluate(() => window.__mementum!.readOnly)).toBe(true);
  // Le titre ne se modifie pas non plus, et le menu Partager n'est pas pour lui.
  await expect(titleOf(b)).toHaveValue(shared.title);
  await expect(titleOf(b)).toHaveAttribute("readonly", "");
  await expect(shareButton(b)).toHaveCount(0);

  await editor.click({ force: true });
  await b.keyboard.type("frappe interdite");
  // L'onglet reçoit toujours tout, en direct : quand l'écriture d'Alice s'affiche, ce que B
  // aurait envoyé avant elle serait déjà parti.
  await putMmd(aliceApi, shared.id, "Écrit par Alice\n\nAjout en direct\n");
  await expect(editor).toContainText("Ajout en direct");
  expect(writes).toEqual([]);
  expect(await getMmd(aliceApi, shared.id)).toBe("Écrit par Alice\n\nAjout en direct\n");
});

test("revoked_user_loses_the_page", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Projet ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  const a = await openPage(browser, shared.id, alice);
  const nativeDialogs = forbidNativeDialogs(a);
  const b = await openPage(browser, shared.id, bruno);
  const editor = b.locator(".bn-editor");
  await editor.click();
  await b.keyboard.type("Avant le retrait");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Avant le retrait\n");
  await expect(treeRow(b, shared.id)).toBeVisible();

  // Les lectures de la page par l'onglet de B sont retenues : le temps de taper entre le
  // retrait et l'écran qui l'annonce.
  const reads = await holdPageReads(b, shared.id);
  // A retire l'accès de B, par le menu Partager ; une fenêtre de la page le fait confirmer.
  const menu = await openShareMenu(a);
  await expect(levelOf(menu, bruno)).toHaveValue("edit");
  const start = Date.now();
  await accessRow(menu, bruno.email).getByRole("button", { name: "Retirer" }).click();
  const confirm = a.getByRole("alertdialog", { name: `Retirer l'accès de ${bruno.name} ?` });
  await expect(confirm.getByRole("button", { name: "Annuler" })).toBeFocused();
  await confirm.getByRole("button", { name: "Retirer" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(accessRow(menu, bruno.email)).toHaveCount(0);
  // Le menu est toujours ouvert, et le focus n'est pas parti dans le vide.
  await expect(menu).toBeVisible();
  expect(await a.evaluate(() => document.activeElement !== document.body)).toBe(true);

  // Le serveur a fermé la synchro de B, qui revient : avant de se reconnecter, son onglet
  // relit la page.
  await reads.asked;
  const reconnecting = Date.now() - start;
  // Entre-temps, B tape encore.
  await typeAtEnd(b, ", et après");
  await expect(editor).toContainText("Avant le retrait, et après");
  const released = Date.now();
  reads.release();
  // En moins de 5 s (sans compter le temps de la frappe retenue), B sait qu'il a perdu la page.
  await expect(accessLost(b)).toBeVisible({ timeout: 5_000 });
  expect(reconnecting + (Date.now() - released)).toBeLessThan(5_000);
  await expect(editor).toHaveCount(0);
  // Sa ligne quitte la barre latérale, et rien de ce qu'il a tapé depuis n'est arrivé.
  await expect(treeRow(b, shared.id)).toHaveCount(0);
  expect(await getMmd(aliceApi, shared.id)).toBe("Avant le retrait\n");

  // A, elle, écrit toujours : sa synchro est revenue.
  await typeAtEnd(a, ", puis Alice");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Avant le retrait, puis Alice\n");
  // Rechargée, la page de B est celle d'une page qui n'existe pas.
  await b.reload();
  await expect(b.getByRole("heading", { name: "Page introuvable" })).toBeVisible();
  expect(nativeDialogs).toEqual([]);
});

test("downgrade_to_read_makes_the_open_editor_read_only", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Notes ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  const a = await openPage(browser, shared.id, alice);
  const b = await openPage(browser, shared.id, bruno);
  const editor = b.locator(".bn-editor");
  await editor.click();
  await b.keyboard.type("Écrit par Bruno");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par Bruno\n");
  expect(await b.evaluate(() => window.__mementum!.readOnly)).toBe(false);
  // Sa ligne de la barre latérale offre ses actions.
  const row = treeRow(b, shared.id);
  await row.hover();
  await expect(row.getByRole("button", { name: /^Actions de/ })).toBeVisible();

  // A le fait passer en lecture, par le menu Partager.
  const writes = watchWrites(b);
  const menu = await openShareMenu(a);
  await levelOf(menu, bruno).selectOption("read");
  await expect(levelOf(menu, bruno)).toHaveValue("read");
  expect((await getPage(await apiAs(bruno), shared.id)).level).toBe("read");

  // En moins de 5 s, la page ouverte chez B est en lecture seule, et le dit.
  await expect(readerBanner(b)).toBeVisible({ timeout: 5_000 });
  await expect.poll(() => b.evaluate(() => window.__mementum?.readOnly)).toBe(true);
  await waitSynced(b);
  await expect(editor).toHaveAttribute("contenteditable", "false");
  await expect(editor).toContainText("Écrit par Bruno");
  await expect(titleOf(b)).toHaveAttribute("readonly", "");
  // La ligne de la barre latérale suit : plus d'actions.
  await row.hover();
  await expect(row.getByRole("button", { name: /^Actions de/ })).toHaveCount(0);
  await expect(row).not.toHaveAttribute("draggable", "true");

  // Ce que B tape ne part plus ; ce qu'écrit A lui arrive toujours.
  await editor.click({ force: true });
  await b.keyboard.type("perdu en silence");
  await typeAtEnd(a, ", puis par Alice");
  await expect(editor).toContainText("Écrit par Bruno, puis par Alice");
  expect(writes).toEqual([]);
  expect(await getMmd(aliceApi, shared.id)).toBe("Écrit par Bruno, puis par Alice\n");
});

test("two_tabs_of_one_account_exchange_only_through_the_server", async ({ browser }) => {
  const { aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Onglets ${unique()}` });
  await putMmd(aliceApi, shared.id, "Écrit par Alice\n");
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  // B a la page ouverte dans deux onglets du même navigateur.
  const first = await openPage(browser, shared.id, bruno);
  const second = await first.context().newPage();
  await second.goto(`/p/${shared.id}`);
  await waitSynced(second);
  /** L'onglet est passé en lecture seule, et montre ce que le serveur a, sans la frappe. */
  const readsWhatTheServerHas = async (tab: Page) => {
    await expect(readerBanner(tab)).toBeVisible({ timeout: 5_000 });
    await expect.poll(() => tab.evaluate(() => window.__mementum?.readOnly)).toBe(true);
    await waitSynced(tab);
    await expect(tab.locator(".bn-editor")).toContainText("Écrit par Alice");
    await expect(tab.locator(".bn-editor")).not.toContainText("frappe sans le droit");
  };

  // A le passe en lecture. Les relectures de la page sont retenues dans les deux onglets : le
  // temps de taper dans le premier, qui n'a plus de connexion et se croit encore en écriture.
  const firstReads = await holdPageReads(first, shared.id);
  const secondReads = await holdPageReads(second, shared.id);
  await setPageLevel(aliceApi, shared.id, brunoId, "read");
  await Promise.all([firstReads.asked, secondReads.asked]);
  await typeAtEnd(first, ", frappe sans le droit");
  await expect(first.locator(".bn-editor")).toContainText("frappe sans le droit");

  // Le premier onglet apprend la baisse : ses documents neufs n'ont que ce que le serveur a.
  // Le second, qui n'a encore rien appris, n'a pas reçu la frappe pour la lui rendre.
  firstReads.release();
  await readsWhatTheServerHas(first);
  await expect(second.locator(".bn-editor")).not.toContainText("frappe sans le droit");
  secondReads.release();
  await readsWhatTheServerHas(second);

  // Le droit revenu et la page rouverte, la frappe ne revient pas non plus de l'autre onglet.
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  await raisedBanner(first).getByRole("button", { name: "Rouvrir la page" }).click();
  await expect(raisedBanner(first)).toHaveCount(0);
  await waitSynced(first);
  await expect(first.locator(".bn-editor")).toHaveAttribute("contenteditable", "true");
  await typeAtEnd(first, ", puis avec le droit");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par Alice, puis avec le droit\n");
  await expect(second.locator(".bn-editor")).toContainText("Écrit par Alice, puis avec le droit");
});

test("upgrade_to_edit_takes_effect_when_the_page_is_reopened", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Brouillon ${unique()}` });
  const other = await createPage(aliceApi, { section: "private", title: `Autre ${unique()}` });
  await putMmd(aliceApi, shared.id, "Écrit par Alice\n");
  await setPageLevel(aliceApi, shared.id, brunoId, "read");
  await setPageLevel(aliceApi, other.id, brunoId, "read");
  const a = await openPage(browser, shared.id, alice);
  const b = await openPage(browser, shared.id, bruno);
  const editor = b.locator(".bn-editor");
  await expect(readerBanner(b)).toBeVisible();

  // A le fait passer en modification : la page ouverte chez B reste en lecture seule, et un
  // bandeau lui dit de la rouvrir.
  const menu = await openShareMenu(a);
  await levelOf(menu, bruno).selectOption("edit");
  await expect(raisedBanner(b)).toBeVisible({ timeout: 5_000 });
  await expect(readerBanner(b)).toHaveCount(0);
  await expect(editor).toHaveAttribute("contenteditable", "false");
  await expect(titleOf(b)).toHaveAttribute("readonly", "");

  // Rouverte depuis la barre latérale, elle se modifie.
  await treeRow(b, other.id).getByRole("link").click();
  await expect(titleOf(b)).toHaveValue(other.title);
  await treeRow(b, shared.id).getByRole("link").click();
  await expect(titleOf(b)).toHaveValue(shared.title);
  await waitSynced(b);
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(raisedBanner(b)).toHaveCount(0);
  await expect(titleOf(b)).not.toHaveAttribute("readonly", "");
  await typeAtEnd(b, ", puis par Bruno");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par Alice, puis par Bruno\n");

  // Baissée puis remontée pendant qu'elle est ouverte : le bouton du bandeau la rouvre.
  await levelOf(menu, bruno).selectOption("read");
  await expect(readerBanner(b)).toBeVisible({ timeout: 5_000 });
  await levelOf(menu, bruno).selectOption("edit");
  await expect(raisedBanner(b)).toBeVisible({ timeout: 5_000 });
  await raisedBanner(b).getByRole("button", { name: "Rouvrir la page" }).click();
  await expect(raisedBanner(b)).toHaveCount(0);
  await waitSynced(b);
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await typeAtEnd(b, ", deux fois");
  await expect
    .poll(() => getMmd(aliceApi, shared.id))
    .toBe("Écrit par Alice, puis par Bruno, deux fois\n");
});

test("page_raised_before_it_is_opened_opens_for_editing", async ({ browser }) => {
  const { aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Relevée ${unique()}` });
  const other = await createPage(aliceApi, { section: "private", title: `Autre ${unique()}` });
  await putMmd(aliceApi, shared.id, "Écrit par Alice\n");
  await setPageLevel(aliceApi, shared.id, brunoId, "read");
  await setPageLevel(aliceApi, other.id, brunoId, "read");
  // B a l'application ouverte sur une autre page ; sa barre latérale porte la page, en lecture.
  const b = await openPage(browser, other.id, bruno);
  const row = treeRow(b, shared.id);
  await expect(row).toBeVisible();

  // A lui donne « Peut modifier ». Rien ne le dit à l'onglet de B, qui n'a pas la page ouverte :
  // sa barre latérale date d'avant, et la ligne n'offre toujours aucune action.
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  await row.hover();
  await expect(row.getByRole("button", { name: /^Actions de/ })).toHaveCount(0);

  // B ouvre la page. C'est une ouverture : elle se modifie d'emblée, sans demander à être
  // rouverte.
  await row.getByRole("link").click();
  await expect(titleOf(b)).toHaveValue(shared.title);
  await waitSynced(b);
  await expect(b.locator(".bn-editor")).toHaveAttribute("contenteditable", "true");
  await expect(raisedBanner(b)).toHaveCount(0);
  await expect(readerBanner(b)).toHaveCount(0);
  await expect(titleOf(b)).not.toHaveAttribute("readonly", "");
  await typeAtEnd(b, ", puis par Bruno");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par Alice, puis par Bruno\n");
  // La barre latérale a suivi.
  await row.hover();
  await expect(row.getByRole("button", { name: /^Actions de/ })).toBeVisible();
});

test("member_invited_to_the_space_sees_space_pages", async ({ browser, page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const forAll = await createPage(aliceApi, { title: `Feuille de route ${unique()}` });
  const mine = await createPage(aliceApi, { section: "private", title: `Journal ${unique()}` });
  const a = await openPage(browser, forAll.id, alice);

  // A invite une adresse dans l'espace ; B ouvre le lien et crée son compte.
  const email = `bruno-${unique()}@exemple.fr`;
  const { token } = await inviteToSpace(aliceApi, space.id, { email, role: "member" });
  await page.goto(`/invite#${token}`);
  await page.getByRole("button", { name: "Créer mon compte" }).click();
  await page.getByLabel("Nom").fill(`Bruno ${unique()}`);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(PASSWORD);
  await password.press("Enter");

  // Membre de l'espace : il en voit la section « Espace » et ses pages, pas la page privée de A.
  await expect(page).toHaveURL(`${BASE_URL}/s/${space.id}`);
  const row = sectionOf(page, "Espace").locator(`[data-page-id="${forAll.id}"]`);
  await expect(row.getByRole("link")).toHaveText(forAll.title);
  await expect(sectionOf(page, "Privé")).toBeVisible();
  await expect(sidebarOf(page).getByText(mine.title)).toHaveCount(0);
  // Il ouvre la page, y écrit, et A le voit.
  await row.getByRole("link").click();
  await waitSynced(page);
  await page.locator(".bn-editor").click();
  await page.keyboard.type("Écrit par le nouveau membre");
  await expect(a.locator(".bn-editor")).toContainText("Écrit par le nouveau membre");
  // Une page d'équipe donne l'accès complet à ses membres : lui aussi peut la partager.
  await expect(shareButton(page)).toBeVisible();
});

test("general_access_makes_a_private_page_a_space_page", async ({ browser }) => {
  const { alice, aliceApi, space, bruno, brunoApi } = await ownerAndMember();
  const title = `Compte rendu ${unique()}`;
  const mine = await createPage(aliceApi, { section: "private", title });
  const seenByBruno = async () =>
    (await getTree(brunoApi, space.id)).nodes
      .filter((n) => n.id === mine.id)
      .map((n) => [n.section, n.level]);
  expect(await seenByBruno()).toEqual([]);

  const a = await openPage(browser, mine.id, alice);
  await expect(sectionOf(a, "Privé").locator(`[data-page-id="${mine.id}"]`)).toBeVisible();
  const menu = await openShareMenu(a);
  const general = menu.getByRole("radiogroup", { name: "Accès général" });
  const invited = general.getByRole("radio", { name: "Seulement les invités" });
  const members = general.getByRole("radio", { name: "Tous les membres de l'espace" });
  const level = menu.getByRole("combobox", { name: "Niveau des membres de l'espace" });
  await expect(invited).toBeChecked();
  await expect(level).toHaveCount(0);

  // « Tous les membres de l'espace » : en lecture d'abord. La page passe dans « Espace », chez
  // A comme chez B.
  await members.click();
  await expect(members).toBeChecked();
  await expect(level).toHaveValue("read");
  await expect(sectionOf(a, "Espace").locator(`[data-page-id="${mine.id}"]`)).toBeVisible();
  await expect(sectionOf(a, "Privé").locator(`[data-page-id="${mine.id}"]`)).toHaveCount(0);
  expect(await seenByBruno()).toEqual([["space", "read"]]);
  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  const row = sectionOf(b, "Espace").locator(`[data-page-id="${mine.id}"]`);
  await expect(row.getByRole("link")).toHaveText(title);
  await row.getByRole("link").click();
  await expect(readerBanner(b)).toBeVisible();
  await waitSynced(b);

  // Le niveau des membres se change dans la même ligne.
  await level.selectOption("edit");
  await expect(level).toHaveValue("edit");
  expect(await seenByBruno()).toEqual([["space", "edit"]]);
  expect((await getAccess(aliceApi, mine.id)).general).toBe("edit");

  // « Seulement les invités » : la page redevient privée, et B, qui l'a ouverte, la perd.
  await invited.click();
  await expect(invited).toBeChecked();
  await expect(level).toHaveCount(0);
  await expect(sectionOf(a, "Privé").locator(`[data-page-id="${mine.id}"]`)).toBeVisible();
  expect(await seenByBruno()).toEqual([]);
  await expect(accessLost(b)).toBeVisible({ timeout: 5_000 });
  await expect(treeRow(b, mine.id)).toHaveCount(0);
});

test("inherited_access_is_shown_and_locked", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId } = await ownerAndMember();
  const parent = await createPage(aliceApi, { section: "private", title: `Projet ${unique()}` });
  const child = await createPage(aliceApi, { parentId: parent.id, title: `Chapitre ${unique()}` });
  await setPageLevel(aliceApi, parent.id, brunoId, "edit");

  // Le menu de la sous-page montre l'accès posé sur sa page parente, marqué « hérité » : il
  // ne s'y change pas, ni ne s'y retire.
  const a = await openPage(browser, child.id, alice);
  const menu = await openShareMenu(a);
  const row = accessRow(menu, bruno.email);
  await expect(row).toContainText(bruno.name);
  await expect(row).toContainText("Peut modifier");
  await expect(row).toContainText("hérité");
  await expect(row.getByRole("combobox")).toHaveCount(0);
  await expect(row.getByRole("button")).toHaveCount(0);
  // Celui de A aussi, qu'elle tient de la page parente.
  const own = accessRow(menu, alice.email);
  await expect(own).toContainText("Accès complet");
  await expect(own).toContainText("hérité");
  await expect(own.getByRole("combobox")).toHaveCount(0);
  await expect(menu).toContainText("se change sur la page parente");

  // Sur la page parente, le même accès se change et se retire.
  await a.keyboard.press("Escape");
  await treeRow(a, parent.id).getByRole("link").click();
  await expect(titleOf(a)).toHaveValue(parent.title);
  const parentMenu = await openShareMenu(a);
  await expect(accessRow(parentMenu, bruno.email)).not.toContainText("hérité");
  await expect(levelOf(parentMenu, bruno)).toHaveValue("edit");
  await expect(accessRow(parentMenu, bruno.email).getByRole("button", { name: "Retirer" })).toBeVisible();
});

test("expired_session_keeps_what_was_typed", async ({ browser }) => {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const { id } = await createPage(api, { title: `Session échue ${unique()}` });
  const p = await openPage(browser, id, account);
  const editor = p.locator(".bn-editor");
  await editor.click();
  await p.keyboard.type("Avant la coupure");
  await expect.poll(() => getMmd(api, id)).toBe("Avant la coupure\n");
  // Un repère sur le conteneur de l'éditeur : s'il était démonté, le repère disparaîtrait.
  const marked = () =>
    p.locator(".mm-editor").evaluate((el) => (el as unknown as { mark?: number }).mark === 1);
  await p.locator(".mm-editor").evaluate((el) => ((el as unknown as { mark: number }).mark = 1));
  const dialog = p.getByRole("dialog", { name: "Ta session a pris fin" });

  // La session prend fin (le cookie n'est plus là), puis la synchro se reconnecte. La relecture
  // de la page qui précède la connexion est retenue, le temps de taper.
  await p.context().clearCookies();
  const reads = await holdPageReads(p, id);
  await p.evaluate(() => {
    const { provider } = window.__mementum!;
    provider.disconnect();
    provider.connect();
  });
  await reads.asked;
  await typeAtEnd(p, ", et pendant");
  await expect(editor).toContainText("Avant la coupure, et pendant");
  reads.release();

  // La connexion est demandée par-dessus la page, qui n'est pas démontée.
  await expect(dialog).toBeVisible();
  await expect(editor).toContainText("Avant la coupure, et pendant");
  expect(await marked()).toBe(true);
  expect(await getMmd(api, id)).toBe("Avant la coupure\n");
  await logInWith(dialog, account.email, account.password);
  await expect(dialog).toHaveCount(0);
  // La synchro reprend : ce qui a été tapé part au serveur, depuis le même éditeur.
  await expect.poll(() => getMmd(api, id)).toBe("Avant la coupure, et pendant\n");
  expect(await marked()).toBe(true);
  await expect(editor).toHaveAttribute("contenteditable", "true");

  // De même pour le titre : tapé sans session, il est refusé, puis repart après la reconnexion.
  await p.unroute(`**/api/v1/pages/${id}`);
  await p.context().clearCookies();
  await titleOf(p).fill("Titre tapé sans session");
  await titleOf(p).press("Enter");
  await expect(dialog).toBeVisible();
  await expect(titleOf(p)).toHaveValue("Titre tapé sans session");
  await logInWith(dialog, account.email, account.password);
  await expect(dialog).toHaveCount(0);
  await expect.poll(async () => (await getPage(api, id)).title).toBe("Titre tapé sans session");
  await expect(treeRow(p, id).getByRole("link")).toHaveText("Titre tapé sans session");
  await expect(p.getByRole("main").getByRole("alert")).toHaveCount(0);
});

test("share_menu_by_keyboard", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const { id } = await createPage(await apiAs(alice), { section: "private", title: "Au clavier" });
  const a = await openPage(browser, id, alice);
  const button = shareButton(a);
  const menu = shareMenu(a);

  // Ouvert par Entrée : le focus va au champ d'invitation.
  await button.focus();
  await a.keyboard.press("Enter");
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(menu).toHaveAttribute("aria-busy", "false");
  await expect(menu.getByLabel("Adresse e-mail")).toBeFocused();
  // Tab passe d'un réglage au suivant, sans sortir du menu ni le fermer.
  for (const next of [
    menu.getByLabel("Niveau de l'invitation"),
    menu.getByRole("button", { name: "Inviter", exact: true }),
    menu.getByRole("combobox", { name: `Niveau de ${alice.name}` }),
    accessRow(menu, alice.email).getByRole("button", { name: "Retirer" }),
    menu.getByRole("radio", { name: "Seulement les invités" }),
    menu.getByRole("button", { name: "Copier le lien", exact: true }),
  ]) {
    await a.keyboard.press("Tab");
    await expect(next).toBeFocused();
  }
  // L'accès général se choisit aux flèches, comme tout groupe de boutons radio.
  await menu.getByRole("radio", { name: "Seulement les invités" }).focus();
  await a.keyboard.press("ArrowDown");
  await expect(menu.getByRole("radio", { name: "Tous les membres de l'espace" })).toBeChecked();
  await expect(menu.getByRole("combobox", { name: "Niveau des membres de l'espace" })).toHaveValue(
    "read",
  );
  // Échap ferme le menu et rend le focus au bouton.
  await a.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-expanded", "false");

  // Un clic dehors le ferme aussi ; le focus revient au bouton, sauf si le clic l'a posé ailleurs.
  await button.click();
  await expect(menu).toBeVisible();
  await a.locator(".mm-page").click({ position: { x: 5, y: 5 } });
  await expect(menu).toHaveCount(0);
  await expect(button).toBeFocused();
  await button.click();
  await expect(menu).toBeVisible();
  await titleOf(a).click();
  await expect(menu).toHaveCount(0);
  await expect(titleOf(a)).toBeFocused();
  // Le focus qui sort du menu par Tab le ferme.
  await button.click();
  await expect(menu).toHaveAttribute("aria-busy", "false");
  await menu.getByRole("button", { name: "Copier le lien", exact: true }).focus();
  await a.keyboard.press("Tab");
  await expect(menu).toHaveCount(0);
});

test("copy_link_copies_the_page_address", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const { id } = await createPage(await apiAs(alice), { title: `Lien ${unique()}` });
  const a = await openPage(browser, id, alice);
  await a.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: BASE_URL });
  const menu = await openShareMenu(a);
  await menu.getByRole("button", { name: "Copier le lien", exact: true }).click();
  await expect(menu.getByRole("button", { name: "Lien copié" })).toBeVisible();
  expect(await a.evaluate(() => navigator.clipboard.readText())).toBe(`${BASE_URL}/p/${id}`);
});

test("refused_change_shows_the_server_message", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId } = await ownerAndMember();
  const { user } = await getMe(aliceApi);
  const shared = await createPage(aliceApi, { section: "private", title: `Seule ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "read");
  // Ce que le serveur répond à la seule personne en accès complet qui se baisse : un refus.
  const refused = await aliceApi.put(`/api/v1/pages/${shared.id}/access/users/${user.id}`, {
    data: { level: "edit" },
  });
  expect(refused.status()).toBe(409);
  const body = (await refused.json()) as ErrorBody;
  expect(body.error).toBe("last_full_access");

  const a = await openPage(browser, shared.id, alice);
  const menu = await openShareMenu(a);
  await expect(accessRow(menu, alice.email)).toContainText("(toi)");
  // Le message du serveur, tel quel, et la liste telle qu'il la voit : A a toujours l'accès
  // complet.
  await levelOf(menu, alice).selectOption("edit");
  await expect(menu.getByRole("alert")).toHaveText(body.message);
  await expect(levelOf(menu, alice)).toHaveValue("full");
  expect((await getAccess(aliceApi, shared.id)).entries.map((e) => [e.email, e.level])).toEqual([
    [alice.email, "full"],
    [bruno.email, "read"],
  ]);
  // De même pour son retrait.
  await accessRow(menu, alice.email).getByRole("button", { name: "Retirer" }).click();
  const confirm = a.getByRole("alertdialog", { name: "Retirer ton accès à cette page ?" });
  await confirm.getByRole("button", { name: "Retirer" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(menu.getByRole("alert")).toHaveText(body.message);
  await expect(accessRow(menu, alice.email)).toBeVisible();
  // Le refus part au changement suivant, quand il passe.
  await levelOf(menu, bruno).selectOption("comment");
  await expect(levelOf(menu, bruno)).toHaveValue("comment");
  await expect(menu.getByRole("alert")).toHaveCount(0);
});

test("losing_full_access_closes_the_menu", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `À deux ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "full");
  const leaving = await createPage(aliceApi, { section: "private", title: `Quittée ${unique()}` });
  await setPageLevel(aliceApi, leaving.id, brunoId, "full");

  // B, en accès complet, se baisse lui-même en lecture : le serveur ne lui rend plus les
  // accès. Le menu se ferme, et la page passe en lecture seule.
  const b = await openPage(browser, shared.id, bruno);
  const menu = await openShareMenu(b);
  await levelOf(menu, bruno).selectOption("read");
  await expect(shareMenu(b)).toHaveCount(0);
  await expect(shareButton(b)).toHaveCount(0);
  await expect(readerBanner(b)).toBeVisible();
  await expect(b.locator(".bn-editor")).toHaveAttribute("contenteditable", "false");
  expect((await getAccess(aliceApi, shared.id)).entries.map((e) => [e.email, e.level])).toEqual([
    [alice.email, "full"],
    [bruno.email, "read"],
  ]);

  // Il se retire d'une autre page : il n'y a plus accès, et l'écran le dit.
  await treeRow(b, leaving.id).getByRole("link").click();
  await expect(titleOf(b)).toHaveValue(leaving.title);
  const other = await openShareMenu(b);
  await accessRow(other, bruno.email).getByRole("button", { name: "Retirer" }).click();
  await b
    .getByRole("alertdialog", { name: "Retirer ton accès à cette page ?" })
    .getByRole("button", { name: "Retirer" })
    .click();
  await expect(accessLost(b)).toBeVisible();
  await expect(shareMenu(b)).toHaveCount(0);
  await expect(treeRow(b, leaving.id)).toHaveCount(0);
  await expect(treeRow(b, shared.id)).toBeVisible();
});

test("guest_with_full_access_shares_but_leaves_general_access", async ({ browser }) => {
  const { alice, aliceApi, bruno } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Dossier ${unique()}` });
  await setGeneralAccess(aliceApi, shared.id, "read");
  const gina = await guestOf(alice, shared.id, `Gina ${unique()}`, "full");
  const { user: brunoUser } = await getMe(await apiAs(bruno));

  // L'invitée a l'accès complet : elle a le menu Partager.
  const g = await openPage(browser, shared.id, gina);
  const menu = await openShareMenu(g);
  // L'accès général se lit, sans se régler : ni choix, ni liste de niveaux (R85).
  await expect(menu).toContainText("Tous les membres de l'espace");
  await expect(menu).toContainText("Peut lire");
  await expect(menu).toContainText("Seuls les membres de l'espace règlent l'accès général");
  await expect(menu.getByRole("radio")).toHaveCount(0);
  await expect(menu.getByRole("combobox", { name: "Niveau des membres de l'espace" })).toHaveCount(0);
  // Le reste marche : elle change l'accès d'une personne, et invite (R86).
  await setPageLevel(aliceApi, shared.id, brunoUser.id, "read");
  await g.keyboard.press("Escape");
  const reopened = await openShareMenu(g);
  await levelOf(reopened, bruno).selectOption("edit");
  await expect(levelOf(reopened, bruno)).toHaveValue("edit");
  const email = `ami-${unique()}@exemple.fr`;
  await reopened.getByLabel("Adresse e-mail").fill(email);
  await reopened.getByRole("button", { name: "Inviter", exact: true }).click();
  await expect(pendingRow(reopened, email)).toBeVisible();
  const access = await getAccess(aliceApi, shared.id);
  expect(access.general).toBe("read");
  expect(access.entries.find((e) => e.email === bruno.email)?.level).toBe("edit");
  expect(access.invitations.map((i) => i.email)).toEqual([email]);
});

test("inherited_general_access_is_a_floor", async ({ browser }) => {
  const { alice, aliceApi, space, brunoApi } = await ownerAndMember();
  const parent = await createPage(aliceApi, { title: `Équipe ${unique()}` });
  const child = await createPage(aliceApi, { parentId: parent.id, title: `Sous-page ${unique()}` });
  await setGeneralAccess(aliceApi, parent.id, "edit");
  const seenByBruno = async () =>
    (await getTree(brunoApi, space.id)).nodes.find((n) => n.id === child.id)?.level;
  // Ce que le serveur répond à qui baisse l'accès général sous celui de la page parente.
  const refused = await aliceApi.put(`/api/v1/pages/${child.id}/access/general`, {
    data: { level: "read" },
  });
  expect(refused.status()).toBe(409);
  expect(((await refused.json()) as ErrorBody).error).toBe("inherited_access");

  const a = await openPage(browser, child.id, alice);
  const menu = await openShareMenu(a);
  const general = menu.getByRole("radiogroup", { name: "Accès général" });
  const invited = general.getByRole("radio", { name: "Seulement les invités" });
  const members = general.getByRole("radio", { name: "Tous les membres de l'espace" });
  const level = menu.getByRole("combobox", { name: "Niveau des membres de l'espace" });
  const optionDisabled = (value: string) =>
    level.locator(`option[value="${value}"]`).evaluate((o) => (o as HTMLOptionElement).disabled);

  // L'accès de la page parente est le plancher : marqué « hérité », il ne se baisse ni ne se
  // retire ici.
  await expect(members).toBeChecked();
  await expect(level).toHaveValue("edit");
  await expect(general).toContainText("hérité");
  await expect(invited).toBeDisabled();
  expect(await optionDisabled("read")).toBe(true);
  expect(await optionDisabled("comment")).toBe(true);
  expect(await optionDisabled("edit")).toBe(false);
  expect(await optionDisabled("full")).toBe(false);

  // Relevé : la sous-page a son propre accès, plus ouvert que celui de sa page parente.
  await level.selectOption("full");
  await expect(level).toHaveValue("full");
  await expect(general).not.toContainText("hérité");
  expect(await seenByBruno()).toBe("full");
  expect(await getAccess(aliceApi, child.id)).toMatchObject({
    general: "full",
    inheritedGeneral: "edit",
  });
  // Ramené au niveau de la page parente : de nouveau hérité.
  await level.selectOption("edit");
  await expect(level).toHaveValue("edit");
  await expect(general).toContainText("hérité");
  expect(await seenByBruno()).toBe("edit");
  await expect(menu.getByRole("alert")).toHaveCount(0);
});

test("pending_page_invitation_is_revoked_from_the_menu", async ({ browser, page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const shared = await createPage(aliceApi, { section: "private", title: `Plan ${unique()}` });
  const email = `attente-${unique()}@exemple.fr`;
  const { token } = await inviteToPage(aliceApi, shared.id, { email, level: "read" });

  const a = await openPage(browser, shared.id, alice);
  const menu = await openShareMenu(a);
  await expect(pendingRow(menu, email)).toContainText("Peut lire");
  await expect(pendingRow(menu, email)).toContainText("expire le");
  await pendingRow(menu, email).getByRole("button", { name: "Révoquer" }).click();
  await expect(pendingRow(menu, email)).toHaveCount(0);
  expect((await getAccess(aliceApi, shared.id)).invitations).toEqual([]);

  // Le lien ne vaut plus rien.
  await page.goto(`/invite#${token}`);
  await expect(page.getByRole("heading", { name: "Cette invitation n'est plus valable" })).toBeVisible();
});

test("invitation_link_is_shown_until_the_page_is_closed", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const shared = await createPage(aliceApi, { section: "private", title: `Plan ${unique()}` });
  const other = await createPage(aliceApi, { section: "private", title: `Autre ${unique()}` });
  const a = await openPage(browser, shared.id, alice);
  const email = `attente-${unique()}@exemple.fr`;
  const linkOf = (menu: ReturnType<typeof shareMenu>) =>
    menu.getByRole("textbox", { name: "Lien d'invitation" });

  const menu = await openShareMenu(a);
  await menu.getByLabel("Adresse e-mail").fill(email);
  await menu.getByRole("button", { name: "Inviter", exact: true }).click();
  await expect(menu.getByText(LINK_WARNING)).toBeVisible();
  const link = await linkOf(menu).inputValue();

  // Le menu fermé puis rouvert (un clic à côté par mégarde) : l'invitation attend, et son lien
  // est encore là, comme le dit la phrase.
  await a.keyboard.press("Escape");
  await expect(shareMenu(a)).toHaveCount(0);
  const reopened = await openShareMenu(a);
  await expect(linkOf(reopened)).toHaveValue(link);
  await expect(reopened.getByText(LINK_WARNING)).toBeVisible();
  await a.keyboard.press("Escape");

  // La page fermée, puis rouverte : l'invitation attend toujours, son lien n'est plus affiché.
  await treeRow(a, other.id).getByRole("link").click();
  await expect(titleOf(a)).toHaveValue(other.title);
  await treeRow(a, shared.id).getByRole("link").click();
  await expect(titleOf(a)).toHaveValue(shared.title);
  const again = await openShareMenu(a);
  await expect(pendingRow(again, email)).toBeVisible();
  await expect(linkOf(again)).toHaveCount(0);
});

test("page_trashed_or_deleted_elsewhere_is_seen_while_open", async ({ browser }) => {
  const { aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Chantier ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  const b = await openPage(browser, shared.id, bruno);
  const editor = b.locator(".bn-editor");
  await editor.click();
  await b.keyboard.type("Écrit par Bruno");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par Bruno\n");

  // A met la page à la corbeille : chez B, elle passe en lecture seule, le bandeau le dit, et
  // sa ligne quitte la barre latérale.
  expect((await aliceApi.post(`/api/v1/pages/${shared.id}/trash`)).status()).toBe(200);
  await expect(trashBanner(b)).toBeVisible({ timeout: 5_000 });
  await waitSynced(b);
  await expect(editor).toHaveAttribute("contenteditable", "false");
  await expect(editor).toContainText("Écrit par Bruno");
  await expect(treeRow(b, shared.id)).toHaveCount(0);

  // A la restaure : B la retrouve, modifiable, et dans sa barre latérale.
  expect((await aliceApi.post(`/api/v1/pages/${shared.id}/restore`)).status()).toBe(200);
  await expect(trashBanner(b)).toHaveCount(0, { timeout: 5_000 });
  await waitSynced(b);
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await expect(treeRow(b, shared.id)).toBeVisible();
  await typeAtEnd(b, ", restaurée");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par Bruno, restaurée\n");

  // A la supprime pour de bon : B n'y a plus accès, et l'écran le dit.
  expect((await aliceApi.post(`/api/v1/pages/${shared.id}/trash`)).status()).toBe(200);
  expect((await aliceApi.delete(`/api/v1/pages/${shared.id}`)).status()).toBe(204);
  await expect(accessLost(b)).toBeVisible({ timeout: 5_000 });
  await expect(treeRow(b, shared.id)).toHaveCount(0);
});

test("change_refused_after_full_access_was_lost_elsewhere", async ({ browser }) => {
  const { aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Commune ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "full");
  const b = await openPage(browser, shared.id, bruno);
  const menu = await openShareMenu(b);
  await expect(levelOf(menu, bruno)).toHaveValue("full");

  // Pendant que le menu de B est ouvert, A le fait passer en modification. La relecture de la
  // page par l'onglet de B est retenue : son écran croit encore qu'il a l'accès complet.
  const reads = await holdPageReads(b, shared.id);
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  const refused = await (await apiAs(bruno)).get(`/api/v1/pages/${shared.id}/access`);
  expect(refused.status()).toBe(403);
  const body = (await refused.json()) as ErrorBody;

  // Son changement est refusé, et le serveur ne lui rend plus les accès : le menu se ferme, et
  // le refus reste dit, en pied de barre latérale.
  await menu.getByLabel("Adresse e-mail").fill(`ami-${unique()}@exemple.fr`);
  await menu.getByRole("button", { name: "Inviter", exact: true }).click();
  await expect(shareMenu(b)).toHaveCount(0);
  const alert = sidebarOf(b).getByRole("alert");
  await expect(alert).toContainText("Le partage n'a pas pu être modifié.");
  await expect(alert).toContainText(body.message, { ignoreCase: true });
  expect((await getAccess(aliceApi, shared.id)).invitations).toEqual([]);

  // La page relue, il n'a plus le bouton Partager ; il la modifie toujours.
  reads.release();
  await expect(shareButton(b)).toHaveCount(0);
  await waitSynced(b);
  await expect(b.locator(".bn-editor")).toHaveAttribute("contenteditable", "true");
  await expect(readerBanner(b)).toHaveCount(0);
});

test("revocation_refused_after_full_access_was_lost_elsewhere", async ({ browser }) => {
  const { aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Liste ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "full");
  const email = `attente-${unique()}@exemple.fr`;
  const { id } = await inviteToPage(aliceApi, shared.id, { email, level: "read" });
  const b = await openPage(browser, shared.id, bruno);
  const menu = await openShareMenu(b);
  await expect(pendingRow(menu, email)).toBeVisible();

  // Pendant que le menu de B est ouvert, A le fait passer en modification ; la relecture de la
  // page par son onglet est retenue.
  const reads = await holdPageReads(b, shared.id);
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  const refused = await (await apiAs(bruno)).get(`/api/v1/pages/${shared.id}/access`);
  expect(refused.status()).toBe(403);
  const body = (await refused.json()) as ErrorBody;

  // « Révoquer » lui est refusé comme si l'invitation n'existait pas : le menu se ferme, et ne
  // laisse pas croire que c'est fait.
  await pendingRow(menu, email).getByRole("button", { name: "Révoquer" }).click();
  await expect(shareMenu(b)).toHaveCount(0);
  const alert = sidebarOf(b).getByRole("alert");
  await expect(alert).toContainText("Le partage n'a pas pu être modifié.");
  await expect(alert).toContainText(body.message, { ignoreCase: true });
  expect((await getAccess(aliceApi, shared.id)).invitations.map((i) => i.id)).toEqual([id]);

  reads.release();
  await expect(shareButton(b)).toHaveCount(0);
});

test("rights_change_does_not_say_offline", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Suivi ${unique()}` });
  await setPageLevel(aliceApi, shared.id, brunoId, "read");
  const a = await openPage(browser, shared.id, alice);
  const status = a.locator(".mm-topbar-status");
  await expect(status).toHaveText("");
  // Tout ce que dit l'état de la connexion à partir d'ici, et ce que fait la synchro.
  await a.evaluate(() => {
    const probe = window as unknown as { said: string[]; sync: string[] };
    probe.said = [];
    probe.sync = [];
    const bar = document.querySelector(".mm-topbar")!;
    const read = () => bar.querySelector(".mm-topbar-status")?.textContent ?? "";
    new MutationObserver(() => probe.said.push(read())).observe(bar, {
      childList: true,
      characterData: true,
      subtree: true,
    });
    window.__mementum!.provider.on("status", ({ status }) => probe.sync.push(status));
  });
  const probe = () =>
    a.evaluate(() => {
      const { said, sync } = window as unknown as { said: string[]; sync: string[] };
      return { said, sync };
    });

  // A change un accès : le serveur ferme les connexions de synchro de la page, la sienne
  // comprise, et son onglet revient aussitôt. Ce n'est pas une panne : rien ne le dit.
  const menu = await openShareMenu(a);
  await levelOf(menu, bruno).selectOption("edit");
  await expect(levelOf(menu, bruno)).toHaveValue("edit");
  await expect.poll(async () => (await probe()).sync.join(" ")).toMatch(/disconnected.* connected/);
  expect((await probe()).said.filter((text) => text.includes("Hors ligne"))).toEqual([]);
  await expect(status).toHaveText("");

  // Une coupure qui dure, elle, se dit ; et le message part quand la synchro revient.
  await a.evaluate(() => window.__mementum!.provider.disconnect());
  await expect(status).toHaveText("Hors ligne : tes modifications partiront au retour du réseau");
  await a.evaluate(() => window.__mementum!.provider.connect());
  await expect(status).toHaveText("");
});

test("top_bar_keeps_its_height_with_or_without_the_share_button", async ({ browser }) => {
  const { aliceApi, bruno, brunoId } = await ownerAndMember();
  const full = await createPage(aliceApi, { title: `Équipe ${unique()}` });
  const read = await createPage(aliceApi, { section: "private", title: `À lire ${unique()}` });
  await setPageLevel(aliceApi, read.id, brunoId, "read");
  const b = await openPage(browser, full.id, bruno);
  const bar = b.locator(".mm-topbar");
  // Avec le bouton « Partager », puis sans lui : la barre du haut garde sa hauteur, et la page
  // ne saute pas d'une page à l'autre.
  await expect(shareButton(b)).toBeVisible();
  const withButton = (await bar.boundingBox())!.height;
  await treeRow(b, read.id).getByRole("link").click();
  await expect(titleOf(b)).toHaveValue(read.title);
  await expect(shareButton(b)).toHaveCount(0);
  expect((await bar.boundingBox())!.height).toBe(withButton);
});
