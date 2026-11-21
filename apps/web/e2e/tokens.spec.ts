// L'écran « Jetons d'API » du compte (lot 5, Task C1), dans un vrai navigateur : un jeton créé,
// copié, qui sert à un vrai appel, puis révoqué et refusé dès l'appel suivant ; et un changement de
// mot de passe qui les révoque tous, que la fenêtre dit avant et après, et que la liste montre.
//
// L'appel fait avec le jeton passe par une route REST ouverte aux jetons (`GET …/tree`) : `/mcp` n'est
// pas encore servi (Task M2) ; la Task X1 branche un vrai client MCP. Les jetons sont créés par le
// test, jamais écrits ailleurs que dans sa mémoire.
import { expect, request, test, type APIRequestContext } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  firstSpace,
  NO_SESSION,
  newUser,
  PASSWORD,
  sidebarOf,
  signUpAccount,
  spaceSwitcher,
} from "./helpers";
import { BASE_URL } from "./servers";

const agents: APIRequestContext[] = [];

test.afterEach(async () => {
  await Promise.all(agents.splice(0).map((agent) => agent.dispose()));
  await closeContexts();
});

/** Un agent : un client sans session, qui ne s'annonce que par son jeton. */
async function agent(): Promise<APIRequestContext> {
  const client = await request.newContext({ baseURL: BASE_URL, storageState: NO_SESSION });
  agents.push(client);
  return client;
}

/** L'arbre de l'espace `spaceId`, lu avec le jeton `token` : le statut, et le code d'un refus. */
async function readTree(client: APIRequestContext, spaceId: string, token: string) {
  const res = await client.get(`/api/v1/spaces/${spaceId}/tree`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  const body = (await res.json()) as { error?: string };
  return { status: res.status(), error: body.error ?? null };
}

const NEW_PASSWORD = "un-autre-mot-de-passe-de-test";

test("a_token_is_created_copied_used_and_revoked", async ({ browser }) => {
  const account = await signUpAccount();
  const spaceId = (await firstSpace(await apiAs(account))).id;
  const p = await newUser(browser, account, { permissions: ["clipboard-read", "clipboard-write"] });
  await p.goto(`/s/${spaceId}`);

  // Depuis le menu du compte.
  await spaceSwitcher(p).click();
  await sidebarOf(p).getByRole("link", { name: "Jetons d'API" }).click();
  await expect(p).toHaveURL(/\/account\/tokens$/);
  // Le lien suivi part avec le menu : le titre de l'écran prend le focus (R1621).
  await expect(p.getByRole("heading", { level: 1, name: "Jetons d'API" })).toBeFocused();
  await expect(p.getByText("Aucun jeton pour l'instant.")).toBeVisible();

  await p.getByRole("button", { name: "Créer un jeton" }).click();
  const form = p.getByRole("dialog", { name: "Créer un jeton" });
  await form.getByLabel("Nom du jeton").fill("Claude Code");
  // La portée par défaut : l'espace ouvert. Le niveau par défaut : « Lecture ».
  await form.getByLabel("Mot de passe actuel").fill(PASSWORD);
  await form.getByRole("button", { name: "Créer le jeton" }).click();

  // Montré une fois, à copier.
  const shown = p.getByRole("dialog", { name: "Jeton créé" });
  const field = shown.getByLabel("Jeton d'API");
  await expect(field).toHaveValue(/^mmt_[A-Za-z0-9_-]{43}$/);
  // Le premier geste a le focus : copier la ligne qui attend le jeton (R1462).
  await expect(shown.getByRole("button", { name: "Copier la ligne 1", exact: true })).toBeFocused();
  await shown.getByRole("button", { name: "Copier le jeton" }).click();
  await expect(shown.getByRole("button", { name: "Jeton copié" })).toBeVisible();
  const token = await p.evaluate(() => navigator.clipboard.readText());
  expect(token).toBe(await field.inputValue());

  // La commande prête à coller porte l'adresse que le navigateur connaît, et jamais le jeton.
  const origin = new URL(p.url()).origin;
  for (const shell of ["bash ou zsh", "PowerShell", "cmd"]) {
    await shown.getByRole("radio", { name: shell }).check();
    const lines = await shown.locator("code.mm-token-command").allTextContents();
    expect(lines.some((line) => line.includes(`${origin}/mcp`)), shell).toBe(true);
    for (const line of lines) expect(line, shell).not.toContain(token);
  }

  // Le jeton copié sert à un vrai appel.
  const client = await agent();
  expect(await readTree(client, spaceId, token)).toEqual({ status: 200, error: null });
  // L'adresse de la commande mène au serveur, pas à l'application : derrière Vite, `/mcp` passe
  // par son mandataire (avant la Task M2, le serveur y répond sans page HTML).
  const mcp = await client.get(`${origin}/mcp`, { headers: { Authorization: `Bearer ${token}` } });
  expect(mcp.headers()["content-type"] ?? "").not.toContain("text/html");

  // Fermée, la fenêtre l'emporte : la liste ne le montre que par sa fin.
  await shown.getByRole("button", { name: "Fermer" }).click();
  await expect(shown).toHaveCount(0);
  await expect(p.getByRole("button", { name: "Créer un jeton" })).toBeFocused();
  const line = p.locator("li[data-token-id]").filter({ hasText: "Claude Code" });
  await expect(line).toContainText(`…${token.slice(-4)}`);
  await expect(line).toContainText("Lecture");
  expect(await p.locator("body").innerHTML()).not.toContain(token);

  // Révoqué, après confirmation : refusé dès l'appel suivant.
  await line.getByRole("button", { name: "Révoquer" }).click();
  const confirm = p.getByRole("alertdialog");
  await expect(confirm).toContainText("Claude Code");
  await confirm.getByRole("button", { name: "Révoquer" }).click();
  await expect(confirm).toHaveCount(0);
  await expect(line).toContainText("Révoqué");
  await expect(line.getByRole("button", { name: "Révoquer" })).toHaveCount(0);
  expect(await p.evaluate(() => document.activeElement !== document.body)).toBe(true);
  expect(await readTree(client, spaceId, token)).toEqual({ status: 401, error: "invalid_token" });
});

test("changing_the_password_revokes_the_tokens", async ({ browser }) => {
  // Un compte neuf : son mot de passe change, et toutes ses sessions avec (R120, R1221).
  const account = await signUpAccount();
  const api = await apiAs(account);
  const spaceId = (await firstSpace(api)).id;
  const created = await api.post("/api/v1/tokens", {
    data: { name: "Agent du soir", level: "read", scope: "all", canTrash: false, password: PASSWORD },
  });
  expect(created.status(), await created.text()).toBe(201);
  const { token } = (await created.json()) as { token: string };
  const client = await agent();
  expect(await readTree(client, spaceId, token)).toEqual({ status: 200, error: null });

  const p = await newUser(browser, account);
  await p.goto("/account/tokens");
  const line = p.locator("li[data-token-id]").filter({ hasText: "Agent du soir" });
  await expect(line.getByRole("button", { name: "Révoquer" })).toBeVisible();

  // La fenêtre le dit avant de valider, puis dit combien en sont partis.
  await spaceSwitcher(p).click();
  await p.getByRole("button", { name: "Changer le mot de passe" }).click();
  const dialog = p.getByRole("dialog", { name: "Changer le mot de passe" });
  await expect(dialog).toContainText("Tes jetons d'API seront tous révoqués");
  await dialog.getByLabel("Mot de passe actuel").fill(PASSWORD);
  await dialog.getByLabel("Nouveau mot de passe").fill(NEW_PASSWORD);
  await dialog.getByLabel("Nouveau mot de passe").press("Enter");
  await expect(dialog).toContainText("Mot de passe changé.");
  await expect(dialog).toContainText("1 jeton d'API révoqué");

  // La liste, ouverte derrière la fenêtre, le montre ; le jeton ne sert plus.
  await dialog.getByRole("button", { name: "Fermer" }).click();
  await expect(line).toContainText("Révoqué");
  await expect(line.getByRole("button", { name: "Révoquer" })).toHaveCount(0);
  expect(await readTree(client, spaceId, token)).toEqual({ status: 401, error: "invalid_token" });
});

test("going_back_with_a_shown_token_asks_first", async ({ browser }) => {
  // Précédent, le jeton montré : l'adresse reste, la fenêtre demande, et « Rester » garde le jeton
  // (R1460). Le focus n'est jamais sur `body`.
  const account = await signUpAccount();
  const spaceId = (await firstSpace(await apiAs(account))).id;
  const p = await newUser(browser, account);
  await p.goto(`/s/${spaceId}`);
  await spaceSwitcher(p).click();
  await sidebarOf(p).getByRole("link", { name: "Jetons d'API" }).click();
  await p.getByRole("button", { name: "Créer un jeton" }).click();
  const form = p.getByRole("dialog", { name: "Créer un jeton" });
  await form.getByLabel("Nom du jeton").fill("Agent");
  await form.getByLabel("Mot de passe actuel").fill(PASSWORD);
  await form.getByRole("button", { name: "Créer le jeton" }).click();
  const shown = p.getByRole("dialog", { name: "Jeton créé" });
  const field = shown.getByLabel("Jeton d'API");
  await expect(field).toHaveValue(/^mmt_/);
  const token = await field.inputValue();

  await p.goBack();
  const confirm = p.getByRole("alertdialog", { name: /Fermer la fenêtre du jeton/ });
  await expect(confirm).toBeVisible();
  await expect(p).toHaveURL(/\/account\/tokens$/);
  await expect(confirm.getByRole("button", { name: "Rester" })).toBeFocused();
  await confirm.getByRole("button", { name: "Rester" }).click();
  await expect(confirm).toHaveCount(0);
  expect(await p.evaluate(() => document.activeElement !== document.body)).toBe(true);
  await expect(field).toHaveValue(token);
  await expect(p).toHaveURL(/\/account\/tokens$/);
});
