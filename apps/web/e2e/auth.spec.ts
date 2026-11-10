// Comptes, depuis l'interface : inscription, connexion et déconnexion, mot de passe faux ou
// changé, menu du compte au clavier (il est dans le sélecteur d'espace, en tête de barre
// latérale), présence au nom du compte, et session perdue pendant qu'une page est ouverte.
import { expect, request as apiRequest, test, type Page } from "@playwright/test";
import type { ErrorBody, Me } from "@mementum/protocol";
import { colorOf } from "../src/session/session";
import {
  apiAs,
  closeContexts,
  createPage,
  expectSignedInAs,
  firstSpace,
  getMmd,
  inviteToSpace,
  newUser,
  NO_SESSION,
  openPage,
  PASSWORD,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  unique,
} from "./helpers";
import { BASE_URL } from "./servers";

// La page de chaque test arrive sans session ; `newUser(browser, compte)` en donne une.
test.use({ storageState: NO_SESSION });
test.afterEach(closeContexts);

/** Un autre mot de passe de test, manifestement faux lui aussi. */
const NEW_PASSWORD = "nouveau-mot-de-passe-de-test";

const loginHeading = (p: Page) => p.getByRole("heading", { name: "Se connecter" });

/** Le compte de la session de la page (`GET /api/v1/me`). */
async function meOf(p: Page): Promise<Me> {
  const res = await p.request.get("/api/v1/me");
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as Me;
}

/** Remplit le formulaire de connexion de `scope` et l'envoie par Entrée. */
async function logInWith(scope: Page | ReturnType<Page["getByRole"]>, email: string, password: string) {
  await scope.getByLabel("Adresse e-mail").fill(email);
  const field = scope.getByLabel("Mot de passe", { exact: true });
  await field.fill(password);
  await field.press("Enter");
}

test("signup_then_lands_in_the_app", async ({ page }) => {
  const name = `Camille ${unique()}`;
  const email = `camille-${unique()}@exemple.fr`;
  await page.goto("/");
  await expect(loginHeading(page)).toBeVisible();
  await page.getByRole("link", { name: "Créer un compte" }).click();
  await expect(page).toHaveURL(/\/signup$/);
  await expect(page.getByRole("heading", { name: "Créer un compte" })).toBeVisible();
  await page.getByLabel("Nom").fill(name);
  await page.getByLabel("Adresse e-mail").fill(email);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(PASSWORD);
  await password.press("Enter");

  // Dans l'application, à l'accueil de son espace, sous son compte.
  await expect(page.getByRole("heading", { name: "Aucune page ouverte" })).toBeVisible();
  await expect(spaceSwitcher(page, `Espace de ${name}`)).toBeVisible();
  await expectSignedInAs(page, name);
  const me = await meOf(page);
  expect(me.user).toMatchObject({ name, email, instanceAdmin: false });
  expect(me.spaces).toEqual([expect.objectContaining({ name: `Espace de ${name}`, role: "owner" })]);
  expect(new URL(page.url()).pathname).toBe(`/s/${me.spaces[0].id}`);

  // La session tient au rechargement.
  await page.reload();
  await expectSignedInAs(page, name);
});

test("login_logout_roundtrip", async ({ page }) => {
  const account = await signUpAccount();
  const title = `Connexion ${unique()}`;
  const { id } = await createPage(await apiAs(account), { title });

  // Sans session, l'adresse d'une page mène à la connexion, puis à cette page.
  await page.goto(`/p/${id}`);
  await expect(loginHeading(page)).toBeVisible();
  await logInWith(page, account.email, account.password);
  await expect(page.getByRole("textbox", { name: "Titre de la page" })).toHaveValue(title);
  expect(new URL(page.url()).pathname).toBe(`/p/${id}`);
  expect((await meOf(page)).user.email).toBe(account.email);

  const session = await page.context().storageState();
  await spaceSwitcher(page).click();
  await page.getByRole("button", { name: "Se déconnecter" }).click();
  await expect(loginHeading(page)).toBeVisible();
  expect(new URL(page.url()).pathname).toBe("/login");
  // La session est supprimée sur le serveur, pas seulement oubliée par l'onglet.
  const old = await apiRequest.newContext({ baseURL: BASE_URL, storageState: session });
  try {
    const res = await old.get("/api/v1/me");
    expect(res.status()).toBe(401);
    expect(((await res.json()) as ErrorBody).error).toBe("unauthenticated");
  } finally {
    await old.dispose();
  }
  await page.reload();
  await expect(loginHeading(page)).toBeVisible();
});

test("back_after_logout_leaves_the_app", async ({ page }) => {
  const account = await signUpAccount();
  const { id } = await createPage(await apiAs(account), { title: `Retour ${unique()}` });
  // Une page de l'application, ouverte puis connectée dans le même onglet.
  await page.goto(`/p/${id}`);
  await logInWith(page, account.email, account.password);
  await expect(page.getByRole("textbox", { name: "Titre de la page" })).toBeVisible();
  await spaceSwitcher(page).click();
  await page.getByRole("button", { name: "Se déconnecter" }).click();
  await expect(loginHeading(page)).toBeVisible();
  // La déconnexion a remplacé l'adresse de la page : Précédent ne revient pas sur elle.
  await page.goBack();
  expect(page.url()).not.toContain(`/p/${id}`);
  expect(page.url().startsWith(BASE_URL) && new URL(page.url()).pathname !== "/login").toBe(false);
});

test("wrong_password_shows_the_server_message", async ({ page, request }) => {
  const account = await signUpAccount();
  // Ce que le serveur répond à ce mot de passe.
  const res = await request.post("/api/v1/auth/login", {
    data: { email: account.email, password: "mauvais-mot-de-passe" },
  });
  expect(res.status()).toBe(401);
  const body = (await res.json()) as ErrorBody;
  expect(body.error).toBe("invalid_credentials");

  await page.goto("/login");
  await logInWith(page, account.email, "mauvais-mot-de-passe");
  await expect(page.getByRole("alert")).toHaveText(body.message);
  // Toujours sans session, sur la connexion, l'adresse gardée.
  await expect(loginHeading(page)).toBeVisible();
  await expect(page.getByLabel("Adresse e-mail")).toHaveValue(account.email);
  expect((await page.request.get("/api/v1/me")).status()).toBe(401);
});

test("change_password_then_old_one_fails", async ({ browser, page, request }) => {
  const account = await signUpAccount();
  // Une autre session du même compte (un autre appareil).
  const other = await apiAs(account);
  // Ce que le serveur répond à un mot de passe actuel faux.
  const refused = await other.post("/api/v1/auth/password", {
    data: { current: "pas-le-bon-mot-de-passe", new: NEW_PASSWORD },
  });
  expect(refused.status()).toBe(401);
  const body = (await refused.json()) as ErrorBody;
  expect(body.error).toBe("invalid_credentials");

  const p = await newUser(browser, account);
  await p.goto("/");
  await spaceSwitcher(p).click();
  await p.getByRole("button", { name: "Changer le mot de passe" }).click();
  const dialog = p.getByRole("dialog", { name: "Changer le mot de passe" });
  await expect(dialog).toBeVisible();

  // Mot de passe actuel faux : le message du serveur ; ce `401` n'est pas une session perdue.
  // Le focus revient au champ à reprendre.
  const current = dialog.getByLabel("Mot de passe actuel");
  await current.fill("pas-le-bon-mot-de-passe");
  await dialog.getByLabel("Nouveau mot de passe").fill(NEW_PASSWORD);
  await dialog.getByLabel("Nouveau mot de passe").press("Enter");
  await expect(dialog.getByRole("alert")).toHaveText(body.message);
  await expect(current).toBeFocused();
  await expect(p.getByRole("dialog", { name: "Ta session a pris fin" })).toHaveCount(0);

  await current.fill(account.password);
  await dialog.getByLabel("Nouveau mot de passe").press("Enter");
  await expect(dialog.getByText("Mot de passe changé.")).toBeVisible();
  await dialog.getByRole("button", { name: "Fermer" }).click();
  await expect(dialog).toHaveCount(0);
  // Le menu d'où venait la fenêtre est fermé : le focus revient au sélecteur d'espace.
  await expect(spaceSwitcher(p)).toBeFocused();
  await expect(spaceSwitcher(p)).toHaveAttribute("aria-expanded", "false");

  // L'onglet garde sa session (le serveur lui a donné un cookie neuf) ; l'autre est fermée.
  await p.reload();
  await expectSignedInAs(p, account.name);
  expect((await meOf(p)).user.email).toBe(account.email);
  expect((await other.get("/api/v1/me")).status()).toBe(401);

  // L'ancien mot de passe ne connecte plus, avec le message de la connexion refusée ; le
  // nouveau, si.
  const loginRefused = await request.post("/api/v1/auth/login", {
    data: { email: account.email, password: account.password },
  });
  expect(loginRefused.status()).toBe(401);
  const loginBody = (await loginRefused.json()) as ErrorBody;
  expect(loginBody.error).toBe("invalid_credentials");
  await page.goto("/login");
  await logInWith(page, account.email, account.password);
  await expect(page.getByRole("alert")).toHaveText(loginBody.message);
  await logInWith(page, account.email, NEW_PASSWORD);
  await expectSignedInAs(page, account.name);
});

test("account_menu_by_keyboard", async ({ browser }) => {
  const account = await signUpAccount();
  const space = await firstSpace(await apiAs(account));
  const p = await newUser(browser, account);
  await p.goto("/");
  // Le menu du compte est dans le sélecteur d'espace : ses espaces, puis son compte.
  const button = spaceSwitcher(p, space.name);
  const menu = sidebarOf(p);
  const current = menu.getByRole("list", { name: "Espaces" }).getByRole("link", { name: space.name });
  await button.focus();
  // Ouvert par Entrée : son premier élément, l'espace ouvert, a le focus.
  await p.keyboard.press("Enter");
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(current).toBeFocused();
  // Tab passe d'une action à la suivante, menu ouvert, jusqu'à la déconnexion.
  for (const next of [
    menu.getByRole("button", { name: "Créer un espace" }),
    menu.getByRole("link", { name: "Réglages de l'espace" }),
    menu.getByRole("button", { name: "Changer le mot de passe" }),
    menu.getByRole("button", { name: "Se déconnecter" }),
  ]) {
    await p.keyboard.press("Tab");
    await expect(next).toBeFocused();
    await expect(button).toHaveAttribute("aria-expanded", "true");
  }
  // Échap le ferme et rend le focus au bouton.
  await p.keyboard.press("Escape");
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(button).toBeFocused();
  // Depuis le bouton, Tab entre dans le menu ouvert à la souris, sans le fermer.
  await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await p.keyboard.press("Tab");
  await expect(current).toBeFocused();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  // Le focus qui en sort le ferme.
  await p.keyboard.press("Shift+Tab");
  await p.keyboard.press("Shift+Tab");
  await expect(button).toHaveAttribute("aria-expanded", "false");
});

test("presence_carries_the_account_name", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  // B rejoint l'espace de A par une invitation : il voit les pages de sa section « Espace ».
  const space = await firstSpace(aliceApi);
  const { token } = await inviteToSpace(aliceApi, space.id, {
    email: `bruno-${unique()}@exemple.fr`,
    role: "member",
  });
  const bruno = await signUpAccount(`Bruno ${unique()}`, token);
  const { id } = await createPage(aliceApi, { title: `Présence ${unique()}` });
  const a = await openPage(browser, id, alice);
  const b = await openPage(browser, id, bruno);
  const aliceId = (await meOf(a)).user.id;
  const brunoId = (await meOf(b)).user.id;

  type User = { name: string; color: string } | undefined;
  // L'awareness de A, telle que la synchro la lui apporte : B y porte son nom et sa couleur.
  const othersSeenBy = (p: Page) =>
    p.evaluate(() => {
      const { awareness } = window.__mementum!.provider;
      return [...awareness.getStates()]
        .filter(([client]) => client !== awareness.clientID)
        .map(([, state]) => state.user as User);
    });
  await expect.poll(() => othersSeenBy(a)).toEqual([{ name: bruno.name, color: colorOf(brunoId) }]);
  await expect.poll(() => othersSeenBy(b)).toEqual([{ name: alice.name, color: colorOf(aliceId) }]);
  // Plus d'« Invité » tiré au hasard : chacun s'annonce sous son compte.
  const own = await a.evaluate(
    () => window.__mementum!.provider.awareness.getLocalState()?.user as User,
  );
  expect(own).toEqual({ name: alice.name, color: colorOf(aliceId) });
});

test("lost_session_asks_login_over_the_open_page", async ({ browser }) => {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const { id } = await createPage(api, { title: `Session perdue ${unique()}` });
  const p = await openPage(browser, id, account);
  const editor = p.locator(".bn-editor");
  await editor.click();
  await p.keyboard.type("Écrit avant la coupure");
  await expect.poll(() => getMmd(api, id)).toBe("Écrit avant la coupure\n");
  // Un repère sur le conteneur de l'éditeur : s'il était démonté, le repère disparaîtrait.
  const marked = () =>
    p.locator(".mm-editor").evaluate((el) => (el as unknown as { mark?: number }).mark === 1);
  await p.locator(".mm-editor").evaluate((el) => ((el as unknown as { mark: number }).mark = 1));

  // La session prend fin ailleurs (déconnexion depuis un autre onglet du même cookie).
  expect((await api.post("/api/v1/auth/logout")).status()).toBe(204);

  // Le prochain appel qui demande une session (le sélecteur d'espace, ouvert, relit le
  // compte) répond `401 unauthenticated` : la connexion est demandée par-dessus la page,
  // toujours là.
  await spaceSwitcher(p).click();
  const dialog = p.getByRole("dialog", { name: "Ta session a pris fin" });
  await expect(dialog).toBeVisible();
  await expect(editor).toContainText("Écrit avant la coupure");
  expect(await marked()).toBe(true);
  // L'adresse est déjà là : le mot de passe a le focus.
  await expect(dialog.getByLabel("Adresse e-mail")).toHaveValue(account.email);
  await expect(dialog.getByLabel("Mot de passe", { exact: true })).toBeFocused();

  await logInWith(dialog, account.email, account.password);
  await expect(dialog).toHaveCount(0);
  // Le focus revient où il était avant la fenêtre : le sélecteur d'espace.
  await expect(spaceSwitcher(p)).toBeFocused();
  // La même page, le même éditeur, qui s'édite toujours.
  expect(await marked()).toBe(true);
  await expect(editor).toContainText("Écrit avant la coupure");
  await p.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
  await p.keyboard.press("End");
  await p.keyboard.type(", et après");
  // Par la session rouverte dans l'onglet : celle de `api` a pris fin, et les pages en
  // demandent une.
  await expect.poll(() => getMmd(p.request, id)).toBe("Écrit avant la coupure, et après\n");
  expect((await meOf(p)).user.email).toBe(account.email);
});
