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
