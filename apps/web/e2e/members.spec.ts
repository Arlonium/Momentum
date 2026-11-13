// Réglages d'un espace et invitations par lien, depuis l'interface : inviter, copier le
// lien, l'ouvrir sans compte ou avec, changer un rôle, retirer quelqu'un, révoquer, partir.
// Le serveur fait foi pour les droits : ses messages s'affichent tels quels.
import { expect, test, type Page } from "@playwright/test";
import type { ErrorBody } from "@mementum/protocol";
import {
  acceptInvitation,
  apiAs,
  closeContexts,
  createPage,
  expectSignedInAs,
  expectUnder,
  firstSpace,
  forbidNativeDialogs,
  getMe,
  getMembers,
  getSpaceInvitations,
  inviteToPage,
  inviteToSpace,
  joinSpace,
  logInWith,
  newUser,
  NO_SESSION,
  openShareMenu,
  PASSWORD,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  unique,
  UNKNOWN_ID,
} from "./helpers";
import { BASE_URL } from "./servers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

/** La ligne d'une personne dans la liste des membres. */
function memberRow(p: Page, email: string) {
  return p.getByRole("list", { name: "Membres" }).getByRole("listitem").filter({ hasText: email });
}

/** La ligne d'une invitation qui attend. */
function pendingRow(p: Page, email: string) {
  return p
    .getByRole("list", { name: "Invitations en attente" })
    .getByRole("listitem")
    .filter({ hasText: email });
}

/** La section des invitations des réglages. */
function invitationsOf(p: Page) {
  return p.getByRole("region", { name: "Invitations" });
}

/** Le titre de la page d'invitation d'un espace. */
function invitedHeading(p: Page, by: string, spaceName: string) {
  return p.getByRole("heading", { name: `${by} t'invite dans l'espace « ${spaceName} »` });
}

const settingsPath = (spaceId: string) => `/s/${spaceId}/settings`;

test("invite_to_space_by_link_then_signup", async ({ browser, page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const space = await firstSpace(await apiAs(alice));
  const a = await newUser(browser, alice);
  await a.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: BASE_URL });

  // Le sélecteur d'espace mène aux réglages de l'espace.
  await a.goto("/");
  await spaceSwitcher(a).click();
  await sidebarOf(a).getByRole("link", { name: "Réglages de l'espace" }).click();
  await expect(a).toHaveURL(`${BASE_URL}${settingsPath(space.id)}`);
  await expect(a.getByRole("heading", { name: "Réglages de l'espace", level: 1 })).toBeVisible();

  // A invite une adresse comme membre, puis copie le lien, montré cette fois seulement.
  const email = `bruno-${unique()}@exemple.fr`;
  const invitations = invitationsOf(a);
  await invitations.getByLabel("Adresse e-mail").fill(email);
  await invitations.getByLabel("Rôle").selectOption("member");
  await invitations.getByRole("button", { name: "Inviter", exact: true }).click();
  await expect(
    invitations.getByText("Ce lien ne sera plus affiché : copie-le maintenant"),
  ).toBeVisible();
  // Préparé pour cette adresse, sans la lui réserver : c'est le lien qui fait foi.
  await expect(invitations).toContainText(`Lien d'invitation préparé pour ${email}`);
  const shown = await invitations.getByRole("textbox", { name: "Lien d'invitation" }).inputValue();
  await invitations.getByRole("button", { name: "Copier le lien" }).click();
  await expect(invitations.getByRole("button", { name: "Lien copié" })).toBeVisible();
  const link = await a.evaluate(() => navigator.clipboard.readText());
  expect(link).toBe(shown);
  const token = new URL(link).hash.slice(1);
  expect(link).toBe(`${BASE_URL}/invite#${token}`);
  // L'invitation attend, sans son jeton.
  await expect(pendingRow(a, email)).toContainText("Membre");
  await expect(pendingRow(a, email)).not.toContainText(token);

  // B, dans un navigateur sans session, ouvre le lien : le jeton quitte aussitôt l'adresse.
  await page.goto(link);
  await expect(invitedHeading(page, alice.name, space.name)).toBeVisible();
  expect(page.url()).toBe(`${BASE_URL}/invite`);
  // L'adresse n'est pas vérifiée : l'écran ne la donne pas pour une condition.
  await expect(page.getByRole("main")).toContainText(
    "Tu peux l'accepter avec le compte de ton choix",
  );
  await page.getByRole("button", { name: "Créer mon compte" }).click();
  await expect(page.getByRole("heading", { name: "Créer un compte" })).toBeVisible();
  // L'adresse de l'invitation est préremplie.
  await expect(page.getByLabel("Adresse e-mail")).toHaveValue(email);
  const name = `Bruno ${unique()}`;
  await page.getByLabel("Nom").fill(name);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(PASSWORD);
  await password.press("Enter");

  // Dans l'application, sous son compte, dans l'espace rejoint : le seul qu'il ait.
  await expect(page).toHaveURL(`${BASE_URL}/s/${space.id}`);
  await expect(spaceSwitcher(page, space.name)).toBeVisible();
  await expectSignedInAs(page, name);
  const me = await getMe(page.request);
  expect(me.spaces).toContainEqual({ id: space.id, name: space.name, role: "member" });

  // B apparaît dans les réglages de A, et l'invitation n'attend plus.
  await a.reload();
  await expect(memberRow(a, email)).toContainText(name);
  await expect(memberRow(a, email)).toContainText("Membre");
  await expect(pendingRow(a, email)).toHaveCount(0);

  // Ni l'adresse ni l'historique de B ne gardent le jeton.
  expect(page.url()).not.toContain(token);
  await page.goBack();
  await expect(page).toHaveURL(`${BASE_URL}/invite`);
  expect(page.url()).not.toContain(token);
});

test("existing_account_accepts_an_invitation", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const { token } = await inviteToSpace(aliceApi, space.id, { email: bruno.email, role: "admin" });

  // B a déjà un compte, et une session dans ce navigateur.
  const b = await newUser(browser, bruno);
  await b.goto(`/invite#${token}`);
  await expect(invitedHeading(b, alice.name, space.name)).toBeVisible();
  expect(b.url()).toBe(`${BASE_URL}/invite`);
  // L'écran dit avec quel compte il la rejoindra.
  await expect(b.getByRole("main")).toContainText(bruno.email);
  await b.getByRole("button", { name: "Accepter" }).click();

  // L'espace rejoint (et non le sien), avec le rôle de l'invitation.
  await expect(b).toHaveURL(`${BASE_URL}/s/${space.id}`);
  await expect(spaceSwitcher(b, space.name)).toBeVisible();
  await expectSignedInAs(b, bruno.name);
  const me = await getMe(b.request);
  expect(me.spaces).toContainEqual({ id: space.id, name: space.name, role: "admin" });
  expect(await getMembers(aliceApi, space.id)).toContainEqual(
    expect.objectContaining({ email: bruno.email, role: "admin" }),
  );
});

test("existing_account_logs_in_then_accepts", async ({ page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const { token } = await inviteToSpace(aliceApi, space.id, {
    email: bruno.email,
    role: "member",
  });

  // Sans session : « J'ai déjà un compte », la connexion, puis l'acceptation.
  await page.goto(`/invite#${token}`);
  await expect(invitedHeading(page, alice.name, space.name)).toBeVisible();
  await page.getByRole("button", { name: "J'ai déjà un compte" }).click();
  await expect(page.getByRole("heading", { name: "Se connecter" })).toBeVisible();
  await expect(page.getByLabel("Adresse e-mail")).toHaveValue(bruno.email);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(bruno.password);
  await password.press("Enter");

  await expect(invitedHeading(page, alice.name, space.name)).toBeVisible();
  expect(page.url()).toBe(`${BASE_URL}/invite`);
  await page.getByRole("button", { name: "Accepter" }).click();
  await expect(page).toHaveURL(`${BASE_URL}/s/${space.id}`);
  await expectSignedInAs(page, bruno.name);
  expect((await getMe(page.request)).spaces).toContainEqual({
    id: space.id,
    name: space.name,
    role: "member",
  });
});

test("owner_changes_a_role_and_removes_a_member", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const chloe = await signUpAccount(`Chloé ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "member");
  await joinSpace(aliceApi, space.id, chloe, "member");
  const roleOf = async (email: string) =>
    (await getMembers(aliceApi, space.id)).find((m) => m.email === email)?.role;

  const a = await newUser(browser, alice);
  const nativeDialogs = forbidNativeDialogs(a);
  await a.goto(settingsPath(space.id));
  await expect(memberRow(a, alice.email)).toContainText("Propriétaire");
  await expect(memberRow(a, bruno.email)).toContainText(bruno.name);

  // Le rôle se change par une liste.
  await memberRow(a, bruno.email)
    .getByRole("combobox", { name: `Rôle de ${bruno.name}` })
    .selectOption("admin");
  await expect.poll(() => roleOf(bruno.email)).toBe("admin");

  // « Retirer » demande une confirmation dans la page.
  const remove = memberRow(a, chloe.email).getByRole("button", { name: "Retirer" });
  const confirm = a.getByRole("alertdialog", { name: `Retirer ${chloe.name} de l'espace ?` });
  await remove.click();
  await expect(confirm).toBeVisible();
  // Le focus est sur « Annuler » : Entrée ne retire personne.
  await expect(confirm.getByRole("button", { name: "Annuler" })).toBeFocused();
  await a.keyboard.press("Enter");
  await expect(confirm).toHaveCount(0);
  // Échap annule aussi.
  await remove.click();
  await expect(confirm).toBeVisible();
  await a.keyboard.press("Escape");
  await expect(confirm).toHaveCount(0);
  expect(await roleOf(chloe.email)).toBe("member");

  // Le focus reste dans la fenêtre, à l'aller comme au retour.
  await remove.click();
  await expect(confirm).toBeVisible();
  for (const key of ["Tab", "Tab", "Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab"]) {
    await a.keyboard.press(key);
    expect(await confirm.evaluate((dialog) => dialog.contains(document.activeElement))).toBe(true);
  }
  await confirm.getByRole("button", { name: "Retirer" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(memberRow(a, chloe.email)).toHaveCount(0);
  // Le bouton qui a ouvert la fenêtre est parti avec sa ligne : le focus va au titre de la liste.
  await expect(a.getByRole("heading", { name: "Membres", level: 2 })).toBeFocused();
  expect(await roleOf(chloe.email)).toBeUndefined();
  // Chloé n'a plus l'espace.
  expect((await getMe(await apiAs(chloe))).spaces.map((s) => s.id)).not.toContain(space.id);
  // Bruno est toujours là, admin.
  await expect(memberRow(a, bruno.email)).toBeVisible();
  expect(nativeDialogs).toEqual([]);
});

test("owner_renames_the_space", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const a = await newUser(browser, alice);
  await a.goto(settingsPath(space.id));
  const field = a.getByRole("textbox", { name: "Nom de l'espace" });
  await expect(field).toHaveValue(space.name);
  const renamed = `Pro ${unique()}`;
  await field.fill(renamed);
  await field.press("Enter");
  await expect.poll(async () => (await firstSpace(aliceApi)).name).toBe(renamed);
  await a.reload();
  await expect(a.getByRole("textbox", { name: "Nom de l'espace" })).toHaveValue(renamed);
});

test("revoked_invitation_link_says_so", async ({ browser, page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const email = `revoquee-${unique()}@exemple.fr`;
  const revoked = await inviteToSpace(aliceApi, space.id, { email, role: "member" });

  // A révoque l'invitation depuis les réglages.
  const a = await newUser(browser, alice);
  await a.goto(settingsPath(space.id));
  await expect(pendingRow(a, email)).toContainText("Membre");
  await pendingRow(a, email).getByRole("button", { name: "Révoquer" }).click();
  await expect(pendingRow(a, email)).toHaveCount(0);
  expect((await getSpaceInvitations(aliceApi, space.id)).map((i) => i.id)).not.toContain(
    revoked.id,
  );

  // Le lien révoqué.
  const gone = (p: Page) => p.getByRole("heading", { name: "Cette invitation n'est plus valable" });
  await page.goto(`/invite#${revoked.token}`);
  await expect(gone(page)).toBeVisible();
  expect(page.url()).toBe(`${BASE_URL}/invite`);
  const revokedScreen = await page.getByRole("main").innerText();
  await expect(page.getByRole("button", { name: "Créer mon compte" })).toHaveCount(0);

  // Le même lien qu'un autre a déjà utilisé : le même message, sans autre détail.
  const used = await inviteToSpace(aliceApi, space.id, { email: bruno.email, role: "member" });
  await acceptInvitation(await apiAs(bruno), used.token);
  const other = await page.context().newPage();
  await other.goto(`/invite#${used.token}`);
  await expect(gone(other)).toBeVisible();
  expect(await other.getByRole("main").innerText()).toBe(revokedScreen);
});

test("member_sees_members_without_actions", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "member");
  await inviteToSpace(aliceApi, space.id, { email: `attente-${unique()}@exemple.fr`, role: "admin" });

  // Un simple membre arrive aux réglages par le sélecteur d'espace, comme les autres.
  const b = await newUser(browser, bruno);
  await b.goto(`/s/${space.id}`);
  await spaceSwitcher(b, space.name).click();
  await sidebarOf(b).getByRole("link", { name: "Réglages de l'espace" }).click();
  await expect(b).toHaveURL(`${BASE_URL}${settingsPath(space.id)}`);
  await expect(b.getByRole("heading", { name: "Réglages de l'espace", level: 1 })).toBeVisible();
  // La liste des membres : nom, adresse, rôle.
  await expect(memberRow(b, alice.email)).toContainText(alice.name);
  await expect(memberRow(b, alice.email)).toContainText("Propriétaire");
  await expect(memberRow(b, bruno.email)).toContainText(bruno.name);
  await expect(memberRow(b, bruno.email)).toContainText("Membre");
  // Sans aucune action : ni rôle à changer, ni retrait, ni invitation, ni nom à modifier.
  const members = b.getByRole("list", { name: "Membres" });
  await expect(members.getByRole("combobox")).toHaveCount(0);
  await expect(members.getByRole("button")).toHaveCount(0);
  await expect(invitationsOf(b)).toHaveCount(0);
  await expect(b.getByRole("button", { name: "Inviter", exact: true })).toHaveCount(0);
  await expect(b.getByRole("button", { name: "Révoquer" })).toHaveCount(0);
  await expect(b.getByRole("textbox", { name: "Nom de l'espace" })).toHaveCount(0);
  await expect(b.getByRole("main")).toContainText(space.name);
  // Partir reste possible : c'est une action pour soi.
  await expect(b.getByRole("button", { name: "Quitter l'espace" })).toBeVisible();
});

test("pending_page_invitation_shows_the_page_title", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const title = `Plan ${unique()}`;
  const plan = await createPage(aliceApi, { section: "private", title });
  const onPage = `page-${unique()}@exemple.fr`;
  const inSpace = `espace-${unique()}@exemple.fr`;
  await inviteToPage(aliceApi, plan.id, { email: onPage, level: "edit" });
  await inviteToSpace(aliceApi, space.id, { email: inSpace, role: "member" });

  // Dans les réglages de l'espace, l'invitation de page nomme sa page ; celle de l'espace dit
  // son rôle, comme avant.
  const a = await newUser(browser, alice);
  await a.goto(settingsPath(space.id));
  await expect(pendingRow(a, onPage)).toContainText(`Peut modifier sur « ${title} » · expire le`);
  await expect(pendingRow(a, inSpace)).toContainText("Membre · expire le");

  // Dans le menu Partager de la page, de même.
  await a.goto(`/p/${plan.id}`);
  const menu = await openShareMenu(a);
  await expect(
    menu
      .getByRole("list", { name: "Invitations en attente" })
      .getByRole("listitem")
      .filter({ hasText: onPage }),
  ).toContainText(`Peut modifier sur « ${title} » · expire le`);
});

test("guest_is_sent_home", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "member");
  // A fait de B un invité de l'espace.
  const { user } = await getMe(await apiAs(bruno));
  const res = await aliceApi.patch(`/api/v1/spaces/${space.id}/members/${user.id}`, {
    data: { role: "guest" },
  });
  expect(res.status(), await res.text()).toBe(200);

  // Renvoyé à l'accueil de l'espace, où il reste invité.
  const b = await newUser(browser, bruno);
  await b.goto(settingsPath(space.id));
  await expect(b).toHaveURL(`${BASE_URL}/s/${space.id}`);
  await expect(spaceSwitcher(b, space.name)).toBeVisible();
  await expect(b.getByRole("list", { name: "Membres" })).toHaveCount(0);
});

test("space_without_a_role_is_not_found", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const space = await firstSpace(await apiAs(alice));

  const b = await newUser(browser, bruno);
  await b.goto(settingsPath(space.id));
  await expect(b.getByRole("heading", { name: "Espace introuvable", level: 1 })).toBeVisible();
  // Rien de l'espace : ni son nom, ni ses membres.
  await expect(b.getByRole("list", { name: "Membres" })).toHaveCount(0);
  await expect(b.getByRole("main")).not.toContainText(space.name);
  await expect(b.getByRole("main")).not.toContainText(alice.email);
});

test("second_invitation_does_not_claim_it_was_copied", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const space = await firstSpace(await apiAs(alice));
  const a = await newUser(browser, alice);
  await a.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: BASE_URL });
  await a.goto(settingsPath(space.id));
  const invitations = invitationsOf(a);
  const link = invitations.getByRole("textbox", { name: "Lien d'invitation" });
  const clipboard = () => a.evaluate(() => navigator.clipboard.readText());
  const invite = async (email: string) => {
    await invitations.getByLabel("Adresse e-mail").fill(email);
    await invitations.getByRole("button", { name: "Inviter", exact: true }).click();
  };

  // Le lien d'une première invitation, copié.
  await invite(`premiere-${unique()}@exemple.fr`);
  await expect(link).toBeVisible();
  const first = await link.inputValue();
  await invitations.getByRole("button", { name: "Copier le lien" }).click();
  await expect(invitations.getByRole("button", { name: "Lien copié" })).toBeVisible();
  expect(await clipboard()).toBe(first);

  // Une seconde : son lien n'est pas copié, et l'écran ne le prétend pas.
  await invite(`seconde-${unique()}@exemple.fr`);
  await expect(link).not.toHaveValue(first);
  const second = await link.inputValue();
  await expect(invitations.getByRole("button", { name: "Copier le lien" })).toBeVisible();
  await expect(invitations.getByRole("button", { name: "Lien copié" })).toHaveCount(0);
  expect(await clipboard()).toBe(first);
  await invitations.getByRole("button", { name: "Copier le lien" }).click();
  await expect(invitations.getByRole("button", { name: "Lien copié" })).toBeVisible();
  expect(await clipboard()).toBe(second);
});

test("refused_action_reloads_the_server_state", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const chloe = await signUpAccount(`Chloé ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "admin");
  await joinSpace(aliceApi, space.id, chloe, "member");
  const idOf = async (email: string) =>
    (await getMembers(aliceApi, space.id)).find((m) => m.email === email)!.userId;

  // B, admin, ouvre les réglages : il peut changer le rôle de C.
  const b = await newUser(browser, bruno);
  await b.goto(settingsPath(space.id));
  const chloeRole = memberRow(b, chloe.email).getByRole("combobox", { name: `Rôle de ${chloe.name}` });
  await expect(chloeRole).toBeVisible();

  // Pendant que l'écran est ouvert, A fait de B un simple membre.
  const demoted = await aliceApi.patch(`/api/v1/spaces/${space.id}/members/${await idOf(bruno.email)}`, {
    data: { role: "member" },
  });
  expect(demoted.status(), await demoted.text()).toBe(200);
  // Ce que le serveur répond désormais à B : un refus, sans effet.
  const refused = await (await apiAs(bruno)).patch(
    `/api/v1/spaces/${space.id}/members/${await idOf(chloe.email)}`,
    { data: { role: "admin" } },
  );
  expect(refused.status()).toBe(403);
  const body = (await refused.json()) as ErrorBody;

  // L'action refusée : le message du serveur, puis l'écran tel que le serveur le voit.
  await chloeRole.selectOption("admin");
  await expect(b.getByRole("alert")).toHaveText(body.message);
  await expect(memberRow(b, bruno.email)).toContainText("Membre");
  const members = b.getByRole("list", { name: "Membres" });
  await expect(members.getByRole("combobox")).toHaveCount(0);
  await expect(members.getByRole("button", { name: "Retirer" })).toHaveCount(0);
  await expect(invitationsOf(b)).toHaveCount(0);
  await expect(memberRow(b, chloe.email)).toContainText("Membre");
  expect((await getMembers(aliceApi, space.id)).find((m) => m.email === chloe.email)?.role).toBe(
    "member",
  );
});

test("last_owner_cannot_leave", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const { user } = await getMe(aliceApi);
  // Ce que le serveur répond au dernier propriétaire qui part : un refus, sans effet.
  const refused = await aliceApi.delete(`/api/v1/spaces/${space.id}/members/${user.id}`);
  expect(refused.status()).toBe(409);
  const body = (await refused.json()) as ErrorBody;
  expect(body.error).toBe("last_owner");

  const a = await newUser(browser, alice);
  const nativeDialogs = forbidNativeDialogs(a);
  await a.goto(settingsPath(space.id));
  await a.getByRole("button", { name: "Quitter l'espace" }).click();
  const confirm = a.getByRole("alertdialog", { name: `Quitter l'espace « ${space.name} » ?` });
  await expect(confirm).toBeVisible();
  await confirm.getByRole("button", { name: "Quitter l'espace" }).click();
  await expect(confirm).toHaveCount(0);
  // Le message du serveur, tel quel ; A est toujours propriétaire, sur ses réglages.
  await expect(a.getByRole("alert")).toHaveText(body.message);
  expect(a.url()).toBe(`${BASE_URL}${settingsPath(space.id)}`);
  expect((await getMe(aliceApi)).spaces).toContainEqual({ ...space, role: "owner" });
  expect(nativeDialogs).toEqual([]);
});

test("failed_reload_keeps_the_settings_and_the_invitation_link", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "member");

  const a = await newUser(browser, alice);
  await a.goto(settingsPath(space.id));
  const invitations = invitationsOf(a);
  const link = invitations.getByRole("textbox", { name: "Lien d'invitation" });
  // Une invitation créée : son lien n'est montré que cette fois.
  const email = `invitee-${unique()}@exemple.fr`;
  await invitations.getByLabel("Adresse e-mail").fill(email);
  await invitations.getByRole("button", { name: "Inviter", exact: true }).click();
  await expect(link).toHaveValue(/\/invite#/);
  await expect(pendingRow(a, email)).toBeVisible();
  const shown = await link.inputValue();

  // Le serveur ne répond plus aux lectures des membres. Un rôle est changé : le changement
  // passe, la relecture qui le suit échoue.
  await a.route("**/api/v1/spaces/*/members", (route) =>
    route.request().method() === "GET" ? route.abort() : route.continue(),
  );
  const role = memberRow(a, bruno.email).getByRole("combobox", { name: `Rôle de ${bruno.name}` });
  await role.selectOption("admin");
  await expect(a.getByRole("alert")).toContainText("Les réglages n'ont pas pu être relus.");
  // De quoi la refaire, sous le message, comme dans la corbeille.
  await expectUnder(a.getByRole("alert"), a.getByRole("button", { name: "Réessayer" }));
  // Rien de ce qui était affiché ne se perd : ni les réglages, ni le lien de l'invitation,
  // que le serveur ne redonnera jamais.
  await expect(a.getByRole("heading", { name: "Réglages de l'espace", level: 1 })).toBeVisible();
  await expect(link).toHaveValue(shown);
  await expect(memberRow(a, bruno.email)).toBeVisible();
  await expect(pendingRow(a, email)).toBeVisible();
  expect((await getMembers(aliceApi, space.id)).find((m) => m.email === bruno.email)?.role).toBe(
    "admin",
  );

  // Le serveur revient : « Réessayer » relit, le message part, et l'écran montre son état.
  await a.unroute("**/api/v1/spaces/*/members");
  await a.getByRole("button", { name: "Réessayer" }).click();
  await expect(a.getByRole("alert")).toHaveCount(0);
  await expect(role).toHaveValue("admin");
  await expect(link).toHaveValue(shown);
});

test("refusal_stays_when_its_section_goes", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "admin");
  const brunoId = (await getMembers(aliceApi, space.id)).find((m) => m.email === bruno.email)!.userId;

  // B, admin, a les réglages ouverts : il invite, et renomme l'espace.
  const b = await newUser(browser, bruno);
  await b.goto(settingsPath(space.id));
  const invitations = invitationsOf(b);
  await expect(invitations).toBeVisible();
  const demote = async () => {
    const res = await aliceApi.patch(`/api/v1/spaces/${space.id}/members/${brunoId}`, {
      data: { role: "member" },
    });
    expect(res.status(), await res.text()).toBe(200);
  };
  const promote = async () => {
    const res = await aliceApi.patch(`/api/v1/spaces/${space.id}/members/${brunoId}`, {
      data: { role: "admin" },
    });
    expect(res.status(), await res.text()).toBe(200);
  };

  // Pendant ce temps, A en fait un simple membre. Ce que le serveur lui répond désormais.
  await demote();
  const refused = await (await apiAs(bruno)).post(`/api/v1/spaces/${space.id}/invitations`, {
    data: { email: "x@exemple.fr", role: "member" },
  });
  expect(refused.status()).toBe(403);
  const body = (await refused.json()) as ErrorBody;

  // Son invitation est refusée, et la section des invitations, qui n'est plus pour lui,
  // disparaît : le refus du serveur reste à l'écran.
  await invitations.getByLabel("Adresse e-mail").fill(`ami-${unique()}@exemple.fr`);
  await invitations.getByRole("button", { name: "Inviter", exact: true }).click();
  await expect(invitationsOf(b)).toHaveCount(0);
  await expect(b.getByRole("alert")).toHaveText(body.message);
  await expect(memberRow(b, bruno.email)).toContainText("Membre");

  // De même pour le nom de l'espace, dont le champ disparaît avec le rôle.
  await promote();
  await b.reload();
  const field = b.getByRole("textbox", { name: "Nom de l'espace" });
  await expect(field).toHaveValue(space.name);
  await expect(b.getByRole("alert")).toHaveCount(0);
  await demote();
  await field.fill(`Renommé ${unique()}`);
  await field.press("Enter");
  await expect(field).toHaveCount(0);
  await expect(b.getByRole("alert")).not.toHaveText("");
  await expect(b.getByRole("main")).toContainText(space.name);
  expect((await firstSpace(aliceApi)).name).toBe(space.name);
});

test("space_left_meanwhile_is_the_same_not_found_screen", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "admin");
  const brunoId = (await getMembers(aliceApi, space.id)).find((m) => m.email === bruno.email)!.userId;

  // B a les réglages de l'espace de A ouverts ; A l'en retire. Sa prochaine action est
  // refusée, et l'espace n'est plus là pour lui : l'écran d'un espace introuvable, avec son
  // bouton, à la même adresse.
  const b = await newUser(browser, bruno);
  const missing = b.getByRole("heading", { name: "Espace introuvable", level: 1 });
  await b.goto(settingsPath(space.id));
  const field = b.getByRole("textbox", { name: "Nom de l'espace" });
  await expect(field).toHaveValue(space.name);
  expect((await aliceApi.delete(`/api/v1/spaces/${space.id}/members/${brunoId}`)).status()).toBe(204);
  const own = await firstSpace(await apiAs(bruno));

  // L'écran d'un espace qui n'existe pas (R96 : il n'y en a qu'un), lu d'abord dans un autre
  // onglet du même compte, à une adresse inconnue, où rien ne le précède.
  const probe = await b.context().newPage();
  await probe.goto(`/s/${UNKNOWN_ID}/settings`);
  await expect(probe.getByRole("heading", { name: "Espace introuvable", level: 1 })).toBeVisible();
  const unknownScreen = await probe.getByRole("main").innerText();
  await probe.close();
  // Les guillemets de l'écran portent des espaces insécables.
  expect(unknownScreen.replace(/\u00a0/g, " ")).toContain(`Ouvrir « ${own.name} »`);
  expect(unknownScreen).not.toContain(space.name);

  await field.fill(`Renommé ${unique()}`);
  await field.press("Enter");
  await expect(missing).toBeVisible();
  await expect(b).toHaveURL(`${BASE_URL}${settingsPath(space.id)}`);
  // Mot pour mot le même écran. Il repasse un instant par « Chargement… », le temps que la
  // liste des espaces du compte soit relue : lire le texte une seule fois pouvait tomber
  // dessus (vu en CI) ; on attend donc l'écran final.
  await expect.poll(() => b.getByRole("main").innerText()).toBe(unknownScreen);
  await b.getByRole("main").getByRole("button", { name: `Ouvrir « ${own.name} »` }).click();
  await expect(b).toHaveURL(`${BASE_URL}/s/${own.id}`);
});

test("lost_session_in_the_settings_only_asks_to_log_in", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const bruno = await signUpAccount(`Bruno ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  await joinSpace(aliceApi, space.id, bruno, "member");
  const waiting = `attente-${unique()}@exemple.fr`;
  await inviteToSpace(aliceApi, space.id, { email: waiting, role: "member" });

  const a = await newUser(browser, alice);
  await a.goto(settingsPath(space.id));
  await expect(memberRow(a, bruno.email)).toBeVisible();
  await expect(pendingRow(a, waiting)).toBeVisible();
  const dialog = a.getByRole("dialog", { name: "Ta session a pris fin" });
  const alerts = a.getByRole("main").getByRole("alert");
  /**
   * La session prend fin, puis `act` demande quelque chose au serveur : la connexion est
   * demandée par-dessus les réglages, qui n'en disent rien de plus, ni alors ni une fois le
   * compte revenu.
   */
  const lostThen = async (act: () => Promise<unknown>) => {
    await a.context().clearCookies();
    await act();
    await expect(dialog).toBeVisible();
    await logInWith(dialog, alice.email, alice.password);
    await expect(dialog).toHaveCount(0);
    await expect(memberRow(a, bruno.email)).toBeVisible();
    await expect(alerts).toHaveCount(0);
  };

  // Renommer l'espace.
  const name = a.getByRole("textbox", { name: "Nom de l'espace" });
  await lostThen(async () => {
    await name.fill(`Pro ${unique()}`);
    await name.press("Enter");
  });
  // Changer un rôle.
  await lostThen(() =>
    memberRow(a, bruno.email)
      .getByRole("combobox", { name: `Rôle de ${bruno.name}` })
      .selectOption("admin"),
  );
  // Inviter.
  const invitations = invitationsOf(a);
  await lostThen(async () => {
    await invitations.getByLabel("Adresse e-mail").fill(`ami-${unique()}@exemple.fr`);
    await invitations.getByRole("button", { name: "Inviter", exact: true }).click();
  });
  // Révoquer une invitation.
  await lostThen(() => pendingRow(a, waiting).getByRole("button", { name: "Révoquer" }).click());
  // Retirer un membre, puis quitter l'espace : la confirmation se ferme, la connexion reste.
  await lostThen(async () => {
    await memberRow(a, bruno.email).getByRole("button", { name: "Retirer" }).click();
    await a
      .getByRole("alertdialog", { name: `Retirer ${bruno.name} de l'espace ?` })
      .getByRole("button", { name: "Retirer" })
      .click();
  });
  await lostThen(async () => {
    await a.getByRole("button", { name: "Quitter l'espace" }).click();
    await a
      .getByRole("alertdialog", { name: `Quitter l'espace « ${space.name} » ?` })
      .getByRole("button", { name: "Quitter l'espace" })
      .click();
  });

  // Le serveur a tout refusé : rien de cela n'est fait.
  expect((await firstSpace(aliceApi)).name).toBe(space.name);
  expect((await getMembers(aliceApi, space.id)).map((m) => [m.email, m.role]).sort()).toEqual(
    [
      [alice.email, "owner"],
      [bruno.email, "member"],
    ].sort(),
  );
  expect((await getSpaceInvitations(aliceApi, space.id)).map((i) => i.email)).toEqual([waiting]);
});

test("reloaded_invitation_page_asks_to_open_the_link_again", async ({ page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const { token } = await inviteToSpace(aliceApi, space.id, {
    email: `invitee-${unique()}@exemple.fr`,
    role: "member",
  });
  await page.goto(`/invite#${token}`);
  await expect(invitedHeading(page, alice.name, space.name)).toBeVisible();
  expect(page.url()).toBe(`${BASE_URL}/invite`);

  // La page rechargée : l'adresse ne porte plus le jeton. L'écran ne dit pas l'invitation
  // périmée, ce qu'il ne sait pas, et ne demande rien au serveur à son sujet.
  const asked: string[] = [];
  page.on("request", (request) => {
    const { pathname } = new URL(request.url());
    if (pathname.startsWith("/api/v1/invitations")) asked.push(pathname);
  });
  await page.reload();
  await expect(page.getByRole("heading", { name: "Ce lien d'invitation est incomplet" })).toBeVisible();
  await expect(page.getByRole("main")).toContainText("Rouvre le lien que tu as reçu.");
  await expect(page.getByRole("main")).not.toContainText("n'est plus valable");
  expect(asked).toEqual([]);

  // Le lien, rouvert, vaut toujours.
  await page.goto(`/invite#${token}`);
  await expect(invitedHeading(page, alice.name, space.name)).toBeVisible();
  expect(asked).toEqual(["/api/v1/invitations/lookup"]);
});
