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
  defaultAccount,
  expectSignedInAs,
  firstSpace,
  getMmd,
  getPage,
  holdPageReads,
  holdTreeReads,
  inviteToSpace,
  logInWith,
  newUser,
  NO_SESSION,
  openPage,
  ownerAndMember,
  PASSWORD,
  setPageLevel,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
  titleOf,
  trashItem,
  unique,
  waitSynced,
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

test("first_account_screen_gives_way_once_an_account_exists", async ({ page }) => {
  // L'onglet a lu l'instance quand elle n'avait encore aucun compte : la première lecture de
  // `/api/v1/info` dit `setup: true`. Pendant qu'on remplit l'écran, quelqu'un d'autre crée le
  // premier compte (celui des tests, déjà là) ; les lectures suivantes passent au serveur.
  let infoReads = 0;
  await page.route("**/api/v1/info", async (route) => {
    infoReads += 1;
    if (infoReads > 1) return route.continue();
    const response = await route.fetch();
    await route.fulfill({ response, json: { ...(await response.json()), setup: true } });
  });
  const sent: string[] = [];
  page.on("request", (req) => {
    if (new URL(req.url()).pathname.startsWith("/api/v1/auth/")) sent.push(new URL(req.url()).pathname);
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Bienvenue dans Mementum" })).toBeVisible();
  // L'adresse du compte qui existe déjà : le serveur répond `409 email_taken`.
  await page.getByLabel("Nom").fill("Camille");
  await page.getByLabel("Adresse e-mail").fill(defaultAccount().email);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(PASSWORD);
  await password.press("Enter");

  // L'instance est relue, et l'écran devient celui de la connexion, qui dit pourquoi.
  await expect(loginHeading(page)).toBeVisible();
  await expect(page.getByRole("status")).toHaveText(
    "Un compte a déjà été créé sur cette instance. Connecte-toi, ou demande une invitation.",
  );
  expect(infoReads).toBe(2);
  // Ce qui a été tapé n'est parti qu'à l'inscription : aucune connexion tentée, aucun champ
  // rempli, et toujours pas de session.
  expect(sent).toEqual(["/api/v1/auth/signup"]);
  await expect(page.getByLabel("Adresse e-mail")).toHaveValue("");
  await expect(page.getByLabel("Mot de passe", { exact: true })).toHaveValue("");
  await expect(page.getByLabel("Adresse e-mail")).toBeFocused();
  // Le message est relié au champ qui a le focus : un lecteur d'écran le lit avec lui.
  await expect(page.getByLabel("Adresse e-mail")).toHaveAccessibleDescription(
    "Un compte a déjà été créé sur cette instance. Connecte-toi, ou demande une invitation.",
  );
  expect((await page.request.get("/api/v1/me")).status()).toBe(401);
});

test("first_account_screen_enters_the_app_once_another_tab_has_signed_up", async ({ page }) => {
  // Deux onglets du même navigateur sur l'écran du premier compte (ici, la première lecture de
  // `/api/v1/info` de celui-ci dit `setup: true`, comme plus haut).
  let infoReads = 0;
  await page.route("**/api/v1/info", async (route) => {
    infoReads += 1;
    if (infoReads > 1) return route.continue();
    const response = await route.fetch();
    await route.fulfill({ response, json: { ...(await response.json()), setup: true } });
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Bienvenue dans Mementum" })).toBeVisible();

  // L'autre onglet crée son compte : le navigateur porte désormais sa session.
  const name = `Camille ${unique()}`;
  const other = await page.context().newPage();
  await other.goto("/signup");
  await other.getByLabel("Nom").fill(name);
  await other.getByLabel("Adresse e-mail").fill(`camille-${unique()}@exemple.fr`);
  await other.getByLabel("Mot de passe", { exact: true }).fill(PASSWORD);
  await other.getByLabel("Mot de passe", { exact: true }).press("Enter");
  await expect(other.getByRole("heading", { name: "Aucune page ouverte" })).toBeVisible();
  const session = (await meOf(other)).user.id;

  // Ici, l'inscription est refusée (`409 email_taken`), et ne ferme pas cette session : l'onglet
  // entre dans l'application sous ce compte, au lieu de demander une connexion.
  await page.getByLabel("Nom").fill("Dominique");
  await page.getByLabel("Adresse e-mail").fill(defaultAccount().email);
  const password = page.getByLabel("Mot de passe", { exact: true });
  await password.fill(PASSWORD);
  const refused = page.waitForResponse((r) => new URL(r.url()).pathname === "/api/v1/auth/signup");
  await password.press("Enter");
  expect((await refused).status()).toBe(409);
  await expect(page.getByRole("heading", { name: "Aucune page ouverte" })).toBeVisible();
  await expect(loginHeading(page)).toHaveCount(0);
  await expectSignedInAs(page, name);
  // La session de l'autre onglet est toujours la même : rien ne l'a remplacée.
  expect((await meOf(other)).user.id).toBe(session);
  await other.reload();
  await expectSignedInAs(other, name);
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

  // La session prend fin ailleurs (déconnexion depuis un autre onglet du même cookie). Le
  // serveur ferme alors la synchro du compte, et l'onglet, en revenant, relit la page : cette
  // relecture est retenue, pour que ce soit le sélecteur d'espace qui découvre la session
  // perdue (`expired_session_keeps_what_was_typed` laisse la synchro la découvrir).
  // La lecture de l'arbre, que la barre latérale refait seule, l'est aussi : elle découvrirait
  // la session perdue avant le sélecteur, et sa fenêtre recouvrirait le clic.
  const reads = await holdPageReads(p, id);
  const tree = await holdTreeReads(p);
  expect((await api.post("/api/v1/auth/logout")).status()).toBe(204);
  // La synchro, fermée par la déconnexion, revient : sa relecture de la page est là, retenue.
  await reads.asked;

  // Le prochain appel qui demande une session (le sélecteur d'espace, ouvert, relit le
  // compte) répond `401 unauthenticated` : la connexion est demandée par-dessus la page,
  // toujours là.
  await spaceSwitcher(p).click();
  const dialog = p.getByRole("dialog", { name: "Ta session a pris fin" });
  await expect(dialog).toBeVisible();
  // La relecture relâchée répond `401` avant la connexion : plus rien de parti sans session
  // n'est en route quand la session est rouverte. Sinon, relâchée avant d'exister, elle pouvait
  // partir avec le cookie mort juste avant la connexion, et son `401`, revenu après, rouvrait
  // la fenêtre (le défaut est dans l'application, au registre : R337).
  const refused = p.waitForResponse(
    (response) =>
      response.request().method() === "GET" &&
      new URL(response.url()).pathname === `/api/v1/pages/${id}`,
  );
  reads.release();
  tree.release();
  expect((await refused).status()).toBe(401);
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

test("trash_and_settings_are_read_once_logged_in_again", async ({ browser }) => {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const thrown = await createPage(api, { title: `Jetée ${unique()}` });
  expect((await api.post(`/api/v1/pages/${thrown.id}/trash`)).status()).toBe(200);
  const p = await newUser(browser, account);
  await p.goto(`/s/${space.id}/settings`);
  const members = p.getByRole("heading", { name: "Membres" });
  await expect(members).toBeVisible();
  const dialog = p.getByRole("dialog", { name: "Ta session a pris fin" });

  // La session prend fin. La corbeille, ouverte ensuite, ne peut rien lire : la connexion est
  // demandée par-dessus. Reconnecté, on y trouve ses pages, pas « Chargement… ».
  await p.context().clearCookies();
  await sidebarOf(p).getByRole("link", { name: "Corbeille" }).click();
  await expect(dialog).toBeVisible();
  await logInWith(dialog, account.email, account.password);
  await expect(dialog).toHaveCount(0);
  await expect(trashItem(p, thrown.title)).toBeVisible();

  // De même pour les réglages de l'espace, où l'on revient par « Précédent ».
  await p.context().clearCookies();
  await p.goBack();
  await expect(dialog).toBeVisible();
  await expect(members).toHaveCount(0);
  await logInWith(dialog, account.email, account.password);
  await expect(dialog).toHaveCount(0);
  await expect(members).toBeVisible();
});

test("login_as_someone_else_over_the_open_page_starts_afresh", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoApi, brunoId } = await ownerAndMember();
  const shared = await createPage(aliceApi, { section: "private", title: `Commune ${unique()}` });
  // B peut modifier la page : reprise sous son compte, l'application y enverrait sous son nom
  // ce qu'A a tapé.
  await setPageLevel(aliceApi, shared.id, brunoId, "edit");
  const p = await openPage(browser, shared.id, alice);
  const editor = p.locator(".bn-editor");
  const typeAtEnd = async (text: string) => {
    await p.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
    await p.keyboard.press("End");
    await p.keyboard.type(text);
  };
  await editor.click();
  await p.keyboard.type("Écrit par Alice");
  await expect.poll(() => getMmd(aliceApi, shared.id)).toBe("Écrit par Alice\n");
  // Un repère sur la fenêtre : un rechargement de l'onglet l'efface.
  await p.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));
  const dialog = p.getByRole("dialog", { name: "Ta session a pris fin" });

  // La session d'A prend fin, puis la synchro se reconnecte. La relecture de la page est
  // retenue, le temps qu'A écrive encore : dans la page, puis dans son titre.
  await p.context().clearCookies();
  const reads = await holdPageReads(p, shared.id);
  await p.evaluate(() => {
    const { provider } = window.__mementum!;
    provider.disconnect();
    provider.connect();
  });
  await reads.asked;
  await typeAtEnd(", tapé sans session");
  await expect(editor).toContainText("Écrit par Alice, tapé sans session");
  await titleOf(p).fill("Titre tapé sans session");
  await titleOf(p).press("Enter");
  // Le titre est refusé : la connexion est demandée par-dessus la page, où tout cela attend.
  await expect(dialog).toBeVisible();
  reads.release();
  await expect(dialog.getByLabel("Adresse e-mail")).toHaveValue(alice.email);

  // C'est B qui se connecte. L'écran est le sien, avec la page telle que le serveur l'a.
  await logInWith(dialog, bruno.email, bruno.password);
  await expect(dialog).toHaveCount(0);
  await expectSignedInAs(p, bruno.name);
  expect((await meOf(p)).user.email).toBe(bruno.email);
  await expect(titleOf(p)).toHaveValue(shared.title);
  await waitSynced(p);
  await expect(editor).toContainText("Écrit par Alice");
  await expect(editor).not.toContainText("tapé sans session");
  // Rien de ce qu'A avait en attente n'est parti sous le compte de B, ni le texte ni le
  // titre : B écrit, et seul ce qu'il écrit arrive.
  await typeAtEnd(", puis Bruno");
  await expect.poll(() => getMmd(brunoApi, shared.id)).toBe("Écrit par Alice, puis Bruno\n");
  expect((await getPage(brunoApi, shared.id)).title).toBe(shared.title);
  // L'onglet a été rechargé : rien de l'application ouverte par A n'est resté en mémoire.
  expect(await p.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBeUndefined();
});

test("account_changed_in_another_tab_reloads_this_one", async ({ browser }) => {
  const { alice, space, bruno } = await ownerAndMember();
  // Premier onglet : l'application d'A, sans page ouverte. Rien n'y parle au serveur tant
  // qu'on n'y touche pas.
  const first = await newUser(browser, alice);
  await first.goto(`/s/${space.id}`);
  await expectSignedInAs(first, alice.name);
  // Un repère sur la fenêtre : un rechargement de l'onglet l'efface.
  await first.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));

  // Second onglet du même navigateur : A se déconnecte, B se connecte. Le cookie de session
  // est désormais celui de B, pour les deux onglets.
  const second = await first.context().newPage();
  await second.goto("/");
  await spaceSwitcher(second).click();
  await second.getByRole("button", { name: "Se déconnecter" }).click();
  await expect(loginHeading(second)).toBeVisible();
  await logInWith(second, bruno.email, bruno.password);
  await expectSignedInAs(second, bruno.name);

  // Le premier onglet montre toujours A. Dès qu'il relit le compte (le sélecteur d'espace,
  // ouvert), il apprend que ce n'est plus le sien : il se recharge, et l'écran est celui de B.
  const reloaded = first.waitForEvent("load", { timeout: 5_000 });
  await spaceSwitcher(first).click();
  await reloaded;
  await expectSignedInAs(first, bruno.name);
  expect(await first.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBeUndefined();
});

test("account_changed_in_another_tab_reloads_on_return", async ({ browser }) => {
  const { alice, space, bruno } = await ownerAndMember();
  // Premier onglet : l'application d'A, sans page ouverte.
  const first = await newUser(browser, alice);
  // L'horloge de l'onglet est la nôtre : on n'attend pas cinq secondes pour de bon.
  await first.clock.install();
  await first.goto(`/s/${space.id}`);
  await expectSignedInAs(first, alice.name);
  // Un repère sur la fenêtre : un rechargement de l'onglet l'efface.
  await first.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));

  // Second onglet du même navigateur : A se déconnecte, B se connecte. Le cookie de session
  // est désormais celui de B, pour les deux onglets.
  const second = await first.context().newPage();
  await second.goto("/");
  await spaceSwitcher(second).click();
  await second.getByRole("button", { name: "Se déconnecter" }).click();
  await expect(loginHeading(second)).toBeVisible();
  await logInWith(second, bruno.email, bruno.password);
  await expectSignedInAs(second, bruno.name);

  // Ce que le premier onglet demande au serveur d'ici à son rechargement.
  const asked: string[] = [];
  let reloading = false;
  first.on("request", (request) => {
    reloading ||= request.isNavigationRequest();
    const { pathname } = new URL(request.url());
    if (!reloading && pathname.startsWith("/api/")) asked.push(`${request.method()} ${pathname}`);
  });
  // On revient sur le premier onglet, sans rien y toucher : il relit l'arbre et le compte,
  // apprend que ce n'est plus le sien, et se recharge. L'écran est celui de B.
  const reloaded = first.waitForEvent("load", { timeout: 5_000 });
  await first.clock.fastForward(5_500);
  await first.evaluate(() => window.dispatchEvent(new Event("focus")));
  await reloaded;
  await expectSignedInAs(first, bruno.name);
  expect(await first.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBeUndefined();
  // Rien n'est parti sous le mauvais nom : deux lectures, aucune action.
  expect(asked.sort()).toEqual(["GET /api/v1/me", `GET /api/v1/spaces/${space.id}/tree`]);
});

test("session_reopened_in_another_tab_closes_the_login_window_on_return", async ({ browser }) => {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const thrown = await createPage(api, { title: `Jetée ${unique()}` });
  expect((await api.post(`/api/v1/pages/${thrown.id}/trash`)).status()).toBe(200);
  const first = await newUser(browser, account);
  // L'horloge de l'onglet est la nôtre : on n'attend pas cinq secondes pour de bon.
  await first.clock.install();
  await first.goto(`/s/${space.id}`);
  await expectSignedInAs(first, account.name);
  const dialog = first.getByRole("dialog", { name: "Ta session a pris fin" });

  // La session prend fin. Le premier onglet l'apprend en ouvrant la corbeille, qui ne peut
  // rien lire : la connexion est demandée par-dessus.
  await first.context().clearCookies();
  await sidebarOf(first).getByRole("link", { name: "Corbeille" }).click();
  await expect(dialog).toBeVisible();

  // La personne se reconnecte dans un second onglet du même navigateur, sous le même compte.
  const second = await first.context().newPage();
  await second.goto("/");
  await expect(loginHeading(second)).toBeVisible();
  await logInWith(second, account.email, account.password);
  await expectSignedInAs(second, account.name);

  // De retour sur le premier onglet : le compte relu répond, la session est là. La fenêtre se
  // ferme sans mot de passe retapé, et la corbeille, qui attendait, est lue.
  await first.clock.fastForward(5_500);
  await first.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(dialog).toHaveCount(0);
  await expect(trashItem(first, thrown.title)).toBeVisible();
  await expectSignedInAs(first, account.name);
});
