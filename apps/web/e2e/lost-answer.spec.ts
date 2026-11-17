// Une réponse perdue après que le serveur a écrit (relecture finale de la partie B, I1, R757 à R760) :
// un mandataire qui coupe (`502` sans corps de l'API) rend la réponse d'une écriture que le serveur a
// faite. L'écran ne dit pas qu'elle a été refusée, n'invite pas à la refaire sans regarder, et relit ce
// qui peut l'être. Témoin dans chaque test : la même coupure quand le serveur n'a rien reçu.
import { expect, test, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getMmd,
  getTree,
  newUser,
  sidebarOf,
  signUpAccount,
  treeRow,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

const BAD_GATEWAY = { status: 502, contentType: "text/html", body: "<h1>502 Bad Gateway</h1>" };

/**
 * La prochaine écriture `method` vers `url` répond `502` d'un mandataire : après que le serveur l'a
 * faite (`reached`), ou sans qu'il l'ait reçue. Les suivantes passent.
 */
async function loseNextAnswer(p: Page, url: string | RegExp, method: string, reached: boolean): Promise<void> {
  let lost = false;
  await p.route(url, async (route) => {
    if (route.request().method() !== method || lost) return route.fallback();
    lost = true;
    if (reached) await route.fetch();
    await route.fulfill(BAD_GATEWAY);
  });
}

/** Un compte neuf, son espace, et une page ouverte dans un onglet. */
async function opened(browser: Parameters<typeof newUser>[0], title: string) {
  const account = await signUpAccount(`Mona ${unique()}`);
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const page = await createPage(api, { spaceId: space.id, title });
  const p = await newUser(browser, account);
  await p.goto(`/p/${page.id}`);
  await waitSynced(p);
  return { api, space, page, p };
}

const favoritesOf = (p: Page) => sidebarOf(p).getByRole("group", { name: "Favoris", exact: true });

async function toggleFavorite(p: Page, entry: string): Promise<void> {
  await p.getByRole("button", { name: "Actions de la page", exact: true }).click();
  await p.getByRole("menuitem", { name: entry }).click();
  // Hors de la barre latérale : « Favoris » n'y est pas gelé (R411).
  await p.mouse.move(900, 400);
}

test("a_favorite_whose_answer_is_lost_is_read_again_and_no_refusal_is_said_when_it_was_done", async ({ browser }) => {
  const { api, space, page, p } = await opened(browser, `Favori ${unique()}`);
  const stored = async () =>
    ((await (await api.get(`/api/v1/spaces/${space.id}/favorites`)).json()) as { pages: string[] }).pages;
  const notice = sidebarOf(p).getByRole("alert");

  // Le serveur pose le favori, sa réponse se perd : la liste relue le montre, et rien n'est dit.
  await loseNextAnswer(p, /\/favorite$/, "PUT", true);
  const reread = p.waitForResponse((r) => r.url().endsWith("/favorites") && r.request().method() === "GET");
  await toggleFavorite(p, "Ajouter aux favoris");
  await reread;
  await expect(favoritesOf(p).getByRole("link")).toHaveText([page.title]);
  expect(await stored()).toEqual([page.id]);
  // Barrière : le menu, rouvert, offre de le retirer ; aucun message de refus n'a paru.
  await p.getByRole("button", { name: "Actions de la page", exact: true }).click();
  await expect(p.getByRole("menuitem", { name: "Retirer des favoris" })).toBeVisible();
  await p.keyboard.press("Escape");
  await expect(notice).toHaveCount(0);

  // Témoin : le serveur n'a rien reçu. Le favori n'est pas retiré, et c'est dit comme un refus, ce qui est vrai.
  await loseNextAnswer(p, /\/favorite$/, "DELETE", false);
  await toggleFavorite(p, "Retirer des favoris");
  await expect(sidebarOf(p).getByRole("alert")).toHaveText(
    "Le favori n'a pas pu être retiré. Le serveur n'a pas répondu, et le favori n'a pas changé. Réessaie.",
  );
  await expect(favoritesOf(p).getByRole("link")).toHaveText([page.title]);
  expect(await stored()).toEqual([page.id]);
});

test("a_page_created_by_brackets_whose_answer_is_lost_is_said_seen_and_never_said_refused", async ({ browser }) => {
  const { api, space, page, p } = await opened(browser, `Crochets ${unique()}`);
  const made = async (title: string) => (await getTree(api, space.id)).nodes.filter((n) => n.title === title);
  const ask = async (title: string) => {
    await p.locator(".mm-editor .bn-block-content").first().click();
    await p.keyboard.press("Control+End");
    await p.keyboard.type(` [[${title}`);
    await expect(p.getByRole("listbox", { name: "Lier une page" }).getByRole("option")).toHaveText([
      `Créer la page « ${title} »`,
    ]);
    await p.keyboard.press("Enter");
  };

  // Le serveur crée la page, sa réponse se perd.
  const seen = `Perdue ${unique()}`;
  await loseNextAnswer(p, "**/api/v1/pages", "POST", true);
  await ask(seen);
  const alert = p.getByRole("alert").filter({ hasText: "La réponse du serveur" });
  await expect(alert).toContainText(
    `La réponse du serveur n'est pas arrivée, mais une page « ${seen} » vient d'apparaître sous celle-ci : elle a sans doute été créée. Sa mention n'est pas posée : tape [[ pour la lier, sans la créer de nouveau.`,
  );
  await expect(alert).not.toContainText("n'a pas pu");
  // Une seule page, sous celle-ci ; la barre latérale la reçoit ; aucune mention n'est posée.
  const pages = await made(seen);
  expect(pages.map((n) => n.parentId)).toEqual([page.id]);
  // La ligne de la page gagne sa sous-page (le canal de l'espace l'apporte) ; dépliée s'il le faut.
  const row = treeRow(p, page.id);
  await expect(row).toHaveAttribute("aria-expanded", /true|false/);
  if ((await row.getAttribute("aria-expanded")) === "false") await row.getByRole("button", { name: "Déplier" }).click();
  await expect(treeRow(p, pages[0].id)).toBeVisible();
  expect(await getMmd(api, page.id)).not.toContain("<mention");
  // Elle se lie par `[[`, sans être créée de nouveau.
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.press("Control+End");
  await p.keyboard.type(` [[${seen}`);
  await p.getByRole("listbox", { name: "Lier une page" }).getByRole("option", { name: seen, exact: true }).click();
  await expect.poll(() => getMmd(api, page.id)).toContain(`id="${pages[0].id}"`);
  expect(await made(seen)).toHaveLength(1);

  // Témoin : le serveur n'a rien reçu. Rien n'a paru : le message dit le doute, jamais un refus.
  const unseen = `Jamais ${unique()}`;
  await loseNextAnswer(p, "**/api/v1/pages", "POST", false);
  await ask(unseen);
  await expect(alert).toContainText(
    `La réponse du serveur n'est pas arrivée : la page « ${unseen} » a peut-être été créée. Regarde dans la barre latérale avant de réessayer.`,
  );
  await expect(alert).not.toContainText("n'a pas pu");
  expect(await made(unseen)).toHaveLength(0);
});

test("a_page_created_from_the_sidebar_whose_answer_is_lost_is_listed_and_not_said_refused", async ({ browser }) => {
  const { api, space, p } = await opened(browser, `Barre ${unique()}`);
  const before = (await getTree(api, space.id)).nodes.length;
  await loseNextAnswer(p, "**/api/v1/pages", "POST", true);
  await sidebarOf(p).getByRole("button", { name: "Nouvelle page" }).click();
  await expect(sidebarOf(p).getByRole("alert")).toHaveText(
    "La réponse du serveur n'est pas arrivée : la page a peut-être été créée. Regarde dans la barre latérale avant de réessayer.",
  );
  // La barre latérale, relue, montre la page que le serveur a créée : une seule.
  const nodes = (await getTree(api, space.id)).nodes;
  expect(nodes).toHaveLength(before + 1);
  const created = nodes.find((n) => n.title === "Sans titre")!;
  await expect(treeRow(p, created.id)).toBeVisible();
});
