// Réglages d'un espace et invitations par lien, depuis l'interface : inviter, copier le
// lien, l'ouvrir sans compte ou avec, changer un rôle, retirer quelqu'un, révoquer, partir.
// Le serveur fait foi pour les droits : ses messages s'affichent tels quels.
import { expect, test, type Page } from "@playwright/test";
import type { ErrorBody } from "@mementum/protocol";
import {
  acceptInvitation,
  apiAs,
  closeContexts,
  firstSpace,
  getMe,
  getMembers,
  getSpaceInvitations,
  inviteToSpace,
  joinSpace,
  newUser,
  NO_SESSION,
  PASSWORD,
  signUpAccount,
  unique,
} from "./helpers";
import { BASE_URL } from "./servers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

function sidebarOf(p: Page) {
  return p.getByRole("navigation", { name: "Barre latérale" });
}

/** Le bouton du menu du compte : on est dans l'application, sous ce compte. */
function accountButton(p: Page, name: string) {
  return sidebarOf(p).getByRole("button", { name: `${name}, menu du compte`, exact: true });
}

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

/** Échoue si la page ouvre une fenêtre du navigateur (`window.confirm`, `alert`…). */
function forbidNativeDialogs(p: Page): string[] {
  const opened: string[] = [];
  p.on("dialog", (dialog) => {
    opened.push(dialog.message());
    void dialog.dismiss();
  });
  return opened;
}

test("invite_to_space_by_link_then_signup", async ({ browser, page }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const space = await firstSpace(await apiAs(alice));
  const a = await newUser(browser, alice);
  await a.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: BASE_URL });

  // Le lien de la barre latérale mène aux réglages de l'espace.
  await a.goto("/");
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
  await page.getByRole("button", { name: "Créer mon compte" }).click();
  await expect(page.getByRole("heading", { name: "Créer un compte" })).toBeVisible();
  // L'adresse de l'invitation est préremplie.
  await expect(page.getByLabel("Adresse e-mail")).toHaveValue(email);
  const name = `Bruno ${unique()}`;
  await page.getByLabel("Nom").fill(name);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(PASSWORD);
  await password.press("Enter");

  // Dans l'application, à l'accueil, son compte en bas de la barre latérale.
  await expect(accountButton(page, name)).toBeVisible();
  expect(page.url()).toBe(`${BASE_URL}/`);
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

  // L'espace, avec le rôle de l'invitation.
  await expect(accountButton(b, bruno.name)).toBeVisible();
  expect(b.url()).toBe(`${BASE_URL}/`);
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
  await expect(accountButton(page, bruno.name)).toBeVisible();
  expect(page.url()).toBe(`${BASE_URL}/`);
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

  const b = await newUser(browser, bruno);
  await b.goto(settingsPath(space.id));
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
