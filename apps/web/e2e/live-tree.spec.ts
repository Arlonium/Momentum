// La barre latérale en direct (Ruling R133) : ce que d'autres créent, renomment ou déplacent
// dans l'espace arrive par le canal de l'espace (`/events/{id}`), sans attendre le minuteur de
// dix secondes (R128), qui n'est plus qu'un repli. La borne de deux secondes est large : ni le
// minuteur, ni un retour sur l'onglet ne peuvent la faire passer à tort.
import { expect, test, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  closeEventsChannel,
  createPage,
  guestOf,
  newUser,
  openPage,
  ownerAndMember,
  sectionOf,
  setPageLevel,
  shownRows,
  signUpAccount,
  treeRow,
  unique,
} from "./helpers";

test.afterEach(closeContexts);

/** Plus court que le minuteur de repli (dix secondes), plus long que ce que le canal met. */
const AT_ONCE = { timeout: 2_000 };

/**
 * Compte les retours sur l'onglet (focus, visibilité) de la page : le repère d'un onglet qui
 * n'a pas bougé. Une relecture qui vient d'un retour ne prouverait rien du canal.
 */
async function countReturns(p: Page): Promise<() => Promise<number>> {
  await p.evaluate(() => {
    const seen = window as unknown as { returns: number };
    seen.returns = 0;
    window.addEventListener("focus", () => (seen.returns += 1));
    document.addEventListener("visibilitychange", () => (seen.returns += 1));
  });
  return () => p.evaluate(() => (window as unknown as { returns: number }).returns);
}

/**
 * Observe les trames du canal de l'espace : ouvrir avant `goto`, attendre après que la
 * première soit arrivée. Un changement fait avant ne prouverait rien du canal : il serait lu
 * par la première lecture de l'arbre.
 */
function watchChannel(p: Page): () => Promise<void> {
  let frames = 0;
  p.on("websocket", (socket) => {
    if (!socket.url().includes("/events/")) return;
    socket.on("framereceived", () => (frames += 1));
  });
  return async () => {
    await expect.poll(() => frames, { timeout: 10_000 }).toBeGreaterThan(0);
  };
}

test("page_created_by_someone_else_appears_at_once", async ({ browser }) => {
  const { aliceApi, space, bruno } = await ownerAndMember();
  const b = await newUser(browser, bruno);
  const channel = watchChannel(b);
  await b.goto(`/s/${space.id}`);
  await expect(sectionOf(b, "Espace")).toBeVisible();
  await channel();
  const returns = await countReturns(b);

  const title = `Créée ailleurs ${unique()}`;
  const page = await createPage(aliceApi, { title });
  await expect(treeRow(b, page.id)).toBeVisible(AT_ONCE);
  await expect(treeRow(b, page.id).getByRole("link")).toHaveText(title);
  // Ni clic, ni focus, ni changement de visibilité dans la fenêtre de B.
  expect(await returns()).toBe(0);
});

test("rename_and_move_by_someone_else_appear_at_once", async ({ browser }) => {
  const { aliceApi, space, bruno } = await ownerAndMember();
  const a = await createPage(aliceApi, { title: "Alpha" });
  const c = await createPage(aliceApi, { title: "Charlie" });
  const b = await newUser(browser, bruno);
  const channel = watchChannel(b);
  await b.goto(`/s/${space.id}`);
  await expect.poll(() => shownRows(b)).toEqual(["1 Alpha", "1 Charlie"]);
  await channel();
  const returns = await countReturns(b);

  // Renommée.
  const renamed = await aliceApi.patch(`/api/v1/pages/${a.id}`, { data: { title: "Alpha 2" } });
  expect(renamed.status()).toBe(200);
  await expect.poll(() => shownRows(b), AT_ONCE).toEqual(["1 Alpha 2", "1 Charlie"]);

  // Réordonnée chez les mêmes frères.
  const reordered = await aliceApi.patch(`/api/v1/pages/${a.id}`, { data: { afterId: c.id } });
  expect(reordered.status()).toBe(200);
  await expect.poll(() => shownRows(b), AT_ONCE).toEqual(["1 Charlie", "1 Alpha 2"]);

  // Rangée sous une autre page.
  const nested = await aliceApi.patch(`/api/v1/pages/${a.id}`, { data: { parentId: c.id } });
  expect(nested.status()).toBe(200);
  await expect.poll(() => shownRows(b), AT_ONCE).toEqual(["1 Charlie", "2 Alpha 2"]);
  expect(await returns()).toBe(0);
});

test("guest_sees_nothing_new_when_the_space_changes", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const shared = await createPage(aliceApi, { section: "private", title: "Partagée" });
  const gina = await guestOf(alice, shared.id, `Gina ${unique()}`, "read");

  const g = await openPage(browser, shared.id, gina);
  const channel = watchChannel(g);
  await g.reload();
  await expect(treeRow(g, shared.id)).toBeVisible();
  await channel();
  const returns = await countReturns(g);
  const before = await shownRows(g);
  expect(before).toEqual(["1 Partagée"]);

  // Des pages qu'elle ne voit pas apparaissent, changent de nom et de place dans l'espace...
  const secret = await createPage(aliceApi, { title: `Secrète ${unique()}` });
  const other = await createPage(aliceApi, { title: "Autre" });
  await aliceApi.patch(`/api/v1/pages/${secret.id}`, { data: { title: "Secrète 2" } });
  await aliceApi.patch(`/api/v1/pages/${other.id}`, { data: { afterId: secret.id } });
  // ... puis une sous-page de la sienne, qu'elle voit : quand elle arrive, tout ce qui précède
  // est passé par le canal, et la barre latérale n'a rien montré d'autre.
  const child = await createPage(aliceApi, { parentId: shared.id, title: "Sous-page" });
  await expect(treeRow(g, child.id)).toBeVisible(AT_ONCE);
  const rows = await shownRows(g);
  expect(rows.join("|")).not.toMatch(/Secr|Autre/);
  await expect(treeRow(g, secret.id)).toHaveCount(0);
  await expect(treeRow(g, other.id)).toHaveCount(0);
  expect(await returns()).toBe(0);
});

test("tree_falls_back_to_polling_when_the_channel_is_down", async ({ browser }) => {
  const { aliceApi, space, bruno } = await ownerAndMember();
  const b = await newUser(browser, bruno);
  // Le canal se ferme dès qu'il s'ouvre : la barre latérale se rabat sur le minuteur.
  const attempts = await closeEventsChannel(b);
  // L'horloge de la page est la nôtre : on n'attend pas dix secondes pour de bon.
  await b.clock.install();
  await b.goto(`/s/${space.id}`);
  await expect(sectionOf(b, "Espace")).toBeVisible();
  await expect.poll(attempts).toBeGreaterThan(0);

  const page = await createPage(aliceApi, { title: `Par le minuteur ${unique()}` });
  // Pas de canal : rien n'arrive tout de suite.
  await b.waitForTimeout(500);
  await expect(treeRow(b, page.id)).toHaveCount(0);
  // Dix secondes de la page plus tard, le minuteur relit l'arbre.
  await b.clock.fastForward(10_000);
  await expect(treeRow(b, page.id)).toBeVisible({ timeout: 5_000 });
});

test("rights_changed_elsewhere_reach_the_sidebar", async ({ browser }) => {
  // C9-6 : une page partagée avec Bruno paraît dans sa barre latérale, et disparaît quand son
  // accès est retiré, par le canal (sous le minuteur) ; retiré de l'espace, il le voit.
  const { aliceApi, space, bruno, brunoId } = await ownerAndMember();
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Partagée ${unique()}`,
  });
  const witness = await createPage(aliceApi, { spaceId: space.id, title: `Témoin ${unique()}` });
  const b = await newUser(browser, bruno);
  const channel = watchChannel(b);
  await b.goto(`/s/${space.id}`);
  await expect(treeRow(b, witness.id)).toBeVisible();
  await channel();
  await expect(treeRow(b, page.id)).toHaveCount(0);

  await setPageLevel(aliceApi, page.id, brunoId, "read");
  await expect(treeRow(b, page.id)).toBeVisible(AT_ONCE);
  const revoked = await aliceApi.delete(`/api/v1/pages/${page.id}/access/users/${brunoId}`);
  expect([200, 204], await revoked.text()).toContain(revoked.status());
  await expect(treeRow(b, page.id)).toHaveCount(0, AT_ONCE);
  await expect(treeRow(b, witness.id)).toBeVisible();

  // Retiré de l'espace : le serveur ferme le canal (sans trame), la reconnexion est refusée, et le
  // minuteur de repli relit l'arbre, que l'espace refuse aussi.
  const removed = await aliceApi.delete(`/api/v1/spaces/${space.id}/members/${brunoId}`);
  expect(removed.status(), await removed.text()).toBe(204);
  await expect(b.getByRole("heading", { name: "Espace introuvable" })).toBeVisible({
    timeout: 15_000,
  });
});

test("a_silent_channel_is_closed_and_the_sidebar_falls_back_on_its_timer", async ({ browser }) => {
  // C6 (R215) : le canal s'ouvre, puis ne dit plus rien et ne se ferme pas (une connexion à demi
  // ouverte : le navigateur ne voit pas les pings). Tant qu'il est tenu pour vivant, ni le canal
  // ni le minuteur ne relisent l'arbre ; 45 s sans trame, l'onglet le ferme de lui-même, et la
  // barre latérale reprend par le minuteur (le canal ne revient pas : les tentatives suivantes
  // sont refusées). L'horloge de la page est la nôtre : on n'attend pas 45 s pour de bon.
  const { aliceApi, space, bruno } = await ownerAndMember();
  const page = await createPage(aliceApi, { spaceId: space.id, title: `Avant ${unique()}` });
  const b = await newUser(browser, bruno);
  let attempts = 0;
  let closedByPage = 0;
  await b.routeWebSocket(/\/events\//, (ws) => {
    attempts += 1;
    if (attempts > 1) {
      void ws.close();
      return;
    }
    // Ni serveur, ni trame : la connexion reste ouverte, muette.
    ws.onClose(() => (closedByPage += 1));
  });
  await b.clock.install();
  await b.goto(`/s/${space.id}`);
  const link = treeRow(b, page.id).getByRole("link");
  await expect(link).toHaveText(page.title);
  await expect.poll(() => attempts).toBe(1);

  const renamed = await aliceApi.patch(`/api/v1/pages/${page.id}`, {
    data: { title: "Renommée ailleurs" },
  });
  expect(renamed.status(), await renamed.text()).toBe(200);
  // 44 s de la page : le canal est tenu pour vivant, rien n'est relu.
  await b.clock.fastForward(44_000);
  await expect(link).toHaveText(page.title);
  expect(closedByPage).toBe(0);
  // 45 s sans trame : l'onglet ferme le canal de lui-même.
  await b.clock.fastForward(2_000);
  await expect.poll(() => closedByPage).toBe(1);
  // Le canal ne revient pas : le minuteur de dix secondes relit l'arbre.
  await b.clock.fastForward(11_000);
  await expect(link).toHaveText("Renommée ailleurs", { timeout: 5_000 });
  expect(attempts).toBeGreaterThan(1);
});
