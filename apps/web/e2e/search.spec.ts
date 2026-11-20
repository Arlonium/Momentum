// La fenêtre de recherche (lot 4, Task C1) : « Rechercher » dans la barre latérale et Ctrl+K ; les
// pages de l'arbre trouvées par leur titre, tout de suite, puis celles que le serveur trouve par
// leur texte, avec un extrait ; tout au clavier ; rien d'elle sur le chemin de l'ouverture.
//
// L'index suit une écriture après un temps de calme : chaque test attend que ce qu'il cherche soit
// trouvable par une condition (`waitFindable`), jamais par une durée. Un compte neuf par test : son
// espace ne porte que les pages du test.
import { expect, test, type Browser, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getMmd,
  newUser,
  openPage,
  putMmd,
  signUpAccount,
  titleOf,
  unique,
  waitFindable,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

/** Un mot neuf, fait de lettres seules : un seul mot pour l'index, que rien d'autre ne porte. */
function word(): string {
  return `zor${Array.from(unique(), (c) => String.fromCharCode(97 + (c.charCodeAt(0) % 26))).join("")}`;
}

/**
 * Un compte neuf et trois pages de son espace : « Recettes <tag> », dont un paragraphe porte
 * `secret` ; « Accueil <tag> », qui sera ouverte ; « Carnet », dont le texte seul porte `tag`. Tout
 * est trouvable avant qu'un onglet s'ouvre.
 */
async function scene() {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const tag = word();
  const secret = word();
  const recipes = await createPage(api, { title: `Recettes ${tag}` });
  await putMmd(api, recipes.id, `Un paragraphe d'avant.\n\nLe ${secret} se cache ici, au milieu.\n`);
  const home = await createPage(api, { title: `Accueil ${tag}` });
  await putMmd(api, home.id, "Un lien ici\n");
  const notebook = await createPage(api, { title: "Carnet" });
  await putMmd(api, notebook.id, `Une note qui parle de ${tag} sans le titrer.\n`);
  await waitFindable(api, space.id, secret, recipes.id);
  await waitFindable(api, space.id, tag, notebook.id);
  return { account, api, space, tag, secret, recipes, home, notebook };
}

async function opened(browser: Browser) {
  const s = await scene();
  const p = await openPage(browser, s.home.id, s.account);
  return { ...s, p };
}

const entry = (p: Page) => p.locator("[data-search-entry]");
const win = (p: Page) => p.getByRole("dialog", { name: "Rechercher" });
const field = (p: Page) => win(p).getByRole("combobox", { name: "Rechercher dans l'espace" });
const row = (p: Page, id: string) => win(p).locator(`[data-search-page="${id}"]`);
const shown = (p: Page) =>
  win(p)
    .locator("[data-search-page]")
    .evaluateAll((rows) => rows.map((r) => (r as HTMLElement).dataset.searchPage));
const focusOnBody = (p: Page) => p.evaluate(() => document.activeElement === document.body);
const pageUrl = (id: string) => new RegExp(`/p/${id}$`);

test("typed_word_is_found_and_opened", async ({ browser }) => {
  const { p, secret, recipes, home } = await opened(browser);
  // L'entrée de la barre latérale, sous le sélecteur d'espace, avec son raccourci écrit.
  await expect(entry(p)).toContainText("Rechercher");
  await expect(entry(p).locator("kbd")).toHaveText(["Ctrl", "K"]);
  await entry(p).click();
  await expect(field(p)).toBeFocused();
  // Champ vide : les pages récentes, celle qu'on vient d'ouvrir.
  await expect(win(p).locator(".mm-search-heading")).toHaveText("Récents");
  await expect(row(p, home.id)).toBeVisible();

  await field(p).fill(secret);
  // Aucun titre ne porte le mot : c'est le serveur qui trouve la page, par son texte.
  const found = row(p, recipes.id);
  await expect(found).toBeVisible();
  await expect(found).toHaveAttribute("data-from", "server");
  await expect(found.locator(".mm-search-title")).toHaveText(recipes.title);
  await expect(found.locator(".mm-search-excerpt")).toContainText(`Le ${secret} se cache ici`);
  await expect(found.locator("mark")).toHaveText([secret]);
  expect(await shown(p)).toEqual([recipes.id]);
  // Chaque ligne est un vrai lien.
  await expect(found).toHaveAttribute("href", `/p/${recipes.id}`);

  await found.click();
  await expect(p).toHaveURL(pageUrl(recipes.id));
  await expect(win(p)).toHaveCount(0);
  await expect(titleOf(p)).toHaveValue(recipes.title);
  await waitSynced(p);
  await expect(p.locator(".mm-editor")).toContainText(secret);
  // Le focus n'est pas tombé sur `body` avec la fenêtre.
  expect(await focusOnBody(p)).toBe(false);
});

test("searching_at_the_keyboard_only", async ({ browser }) => {
  const { p, tag, secret, recipes, home, notebook } = await opened(browser);
  // Depuis le texte de la page : Ctrl+K ouvre la fenêtre, Échap la ferme, et la frappe reprend là
  // où elle était.
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.press("End");
  await p.keyboard.press("Control+k");
  await expect(field(p)).toBeFocused();
  await p.keyboard.type("un texte tapé");
  await p.keyboard.press("Escape");
  await expect(win(p)).toHaveCount(0);
  await p.keyboard.type(" même");
  await expect(p.locator(".mm-editor .bn-block-content").first()).toHaveText("Un lien ici même");

  // Rouverte, elle rend ce qui était tapé, sélectionné : taper le remplace.
  await p.keyboard.press("Control+k");
  await expect(field(p)).toHaveValue("un texte tapé");
  await p.keyboard.type(tag);
  await expect(field(p)).toHaveValue(tag);
  // Les deux pages dont le titre porte le mot, tout de suite, dans l'ordre de l'arbre ; puis celle
  // que le serveur trouve par son texte, dessous.
  await expect(row(p, notebook.id)).toBeVisible();
  expect(await shown(p)).toEqual([recipes.id, home.id, notebook.id]);
  await expect(row(p, recipes.id)).toHaveAttribute("data-from", "tree");
  await expect(row(p, notebook.id)).toHaveAttribute("data-from", "server");
  await expect(row(p, recipes.id)).toHaveAttribute("aria-selected", "true");
  // Les flèches choisissent, le champ garde le focus.
  await p.keyboard.press("ArrowDown");
  await p.keyboard.press("ArrowDown");
  await expect(row(p, notebook.id)).toHaveAttribute("aria-selected", "true");
  await p.keyboard.press("ArrowUp");
  await p.keyboard.press("ArrowUp");
  await expect(row(p, recipes.id)).toHaveAttribute("aria-selected", "true");
  await expect(field(p)).toBeFocused();

  // Ctrl+Entrée : dans un nouvel onglet, la fenêtre reste.
  const [tab] = await Promise.all([p.context().waitForEvent("page"), p.keyboard.press("Control+Enter")]);
  await expect(tab).toHaveURL(pageUrl(recipes.id));
  await tab.close();
  await expect(field(p)).toBeFocused();
  await expect(p).toHaveURL(pageUrl(home.id));

  // Entrée : la page active s'ouvre dans l'onglet.
  await p.keyboard.press("ArrowDown");
  await p.keyboard.press("ArrowDown");
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(pageUrl(notebook.id));
  await expect(win(p)).toHaveCount(0);
  await expect(titleOf(p)).toHaveValue("Carnet");
  expect(await focusOnBody(p)).toBe(false);

  // Une page ouverte vide le champ ; une recherche par le texte, puis Entrée, sans aucune flèche.
  await p.keyboard.press("Control+k");
  await expect(field(p)).toHaveValue("");
  await p.keyboard.type(secret);
  await expect(row(p, recipes.id)).toHaveAttribute("aria-selected", "true");
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(pageUrl(recipes.id));
  expect(await focusOnBody(p)).toBe(false);
});

test("the_shortcut_in_the_editor_with_a_selection_is_left_to_the_editor", async ({ browser }) => {
  const { p, api, home } = await opened(browser);
  // Ce que devient chaque Ctrl+K une fois passé par l'éditeur et par l'application (écouté sur
  // `window`, après eux), et ce que la sélection de l'éditeur était.
  type Seen = { prevented: boolean; selected: string };
  await p.evaluate(() => {
    const seen: { prevented: boolean; selected: string }[] = [];
    (window as unknown as { ctrlK: typeof seen }).ctrlK = seen;
    window.addEventListener("keydown", (event) => {
      if (event.key !== "k" || !event.ctrlKey) return;
      seen.push({ prevented: event.defaultPrevented, selected: String(document.getSelection()) });
    });
  });
  const seen = () => p.evaluate(() => (window as unknown as { ctrlK: Seen[] }).ctrlK);

  // « lien » sélectionné dans « Un lien ici », le focus dans l'éditeur. La sélection est posée d'un
  // tenant, et reposée tant que l'éditeur ne la tient pas : huit touches de suite (quatre flèches,
  // quatre avec Maj) n'ont laissé que « n » sélectionné une fois sur onze, sans cause trouvée (R1099).
  await p.locator(".mm-editor .bn-block-content").first().click();
  await expect(async () => {
    await p.evaluate(() => {
      const selection = document.getSelection()!;
      if (String(selection) === "lien") return;
      const block = document.querySelector(".mm-editor .bn-block-content")!;
      const text = document.createTreeWalker(block, NodeFilter.SHOW_TEXT).nextNode()!;
      selection.setBaseAndExtent(text, "Un ".length, text, "Un lien".length);
    });
    // BlockNote n'écoute Ctrl+K que sa barre de mise en forme montrée (elle suit la sélection de peu).
    await expect(p.getByRole("button", { name: "Créer un lien" })).toBeVisible({ timeout: 2_000 });
    expect(await p.evaluate(() => String(document.getSelection()))).toBe("lien");
  }).toPass({ timeout: 20_000 });
  await p.keyboard.press("Control+k");
  // L'éditeur en fait un lien à créer : son champ d'adresse s'ouvre, pas la fenêtre de recherche.
  const linkField = p.getByPlaceholder("Modifier l'URL");
  await expect(linkField).toBeVisible();
  await expect(win(p)).toHaveCount(0);
  expect(await seen()).toEqual([{ prevented: true, selected: "lien" }]);
  await p.keyboard.press("Escape");
  await expect(linkField).toHaveCount(0);

  // Témoin : sans sélection, au même endroit, la fenêtre s'ouvre, et aucun lien n'est créé ni
  // proposé.
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.press("End");
  await p.keyboard.press("Control+k");
  await expect(field(p)).toBeFocused();
  await expect(linkField).toHaveCount(0);
  expect(await seen()).toEqual([
    { prevented: true, selected: "lien" },
    { prevented: true, selected: "" },
  ]);
  await p.keyboard.press("Escape");
  await expect(win(p)).toHaveCount(0);
  await expect(p.locator(".mm-editor a[href]")).toHaveCount(0);
  expect(await getMmd(api, home.id)).toBe("Un lien ici\n");
});

test("opening_the_application_asks_nothing_of_search", async ({ browser }) => {
  const { account, home, secret, recipes } = await scene();
  const p = await newUser(browser, account);
  const urls: string[] = [];
  p.on("request", (request) => urls.push(request.url()));
  /** Ce qui est demandé de la recherche : sa route, et le code de sa fenêtre. */
  const asked = (what: RegExp) => urls.filter((url) => what.test(new URL(url).pathname));
  const ROUTE = /^\/api\/v1\/spaces\/[^/]+\/search$/;
  const WINDOW = /SearchDialog|useSearch|search\/(marks|rows)/;

  // L'application ouverte sur une page, jusqu'à ce que tout ce qu'une ouverture lit soit lu : la
  // synchro, l'arbre, puis les favoris, demandés après lui.
  const favorites = p.waitForResponse((response) => /\/favorites$/.test(new URL(response.url()).pathname));
  await p.goto(`/p/${home.id}`);
  await waitSynced(p);
  await favorites;
  expect(asked(ROUTE)).toEqual([]);
  expect(asked(WINDOW)).toEqual([]);
  // Témoin : le relevé voit passer ce qu'une ouverture demande.
  expect(asked(/\/tree$/).length).toBeGreaterThan(0);

  // Témoin : la fenêtre ouverte, son code est demandé ; la recherche, à la première frappe seulement.
  await entry(p).click();
  await expect(field(p)).toBeFocused();
  expect(asked(WINDOW).length).toBeGreaterThan(0);
  expect(asked(ROUTE)).toEqual([]);
  await field(p).fill(secret);
  await expect(row(p, recipes.id)).toBeVisible();
  expect(asked(ROUTE).length).toBeGreaterThan(0);
  // La recherche ne voyage dans aucune adresse.
  expect(urls.filter((url) => url.includes(secret))).toEqual([]);
});

test("more_results_arrive_without_moving_their_button_under_the_pointer", async ({ browser }) => {
  // Quarante-cinq pages dont le texte seul porte le mot : trois réponses du serveur (20, 20, 5).
  const account = await signUpAccount();
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const secret = word();
  const home = await createPage(api, { title: "Accueil" });
  for (let i = 0; i < 45; i += 1) {
    const page = await createPage(api, { title: `Note ${i}` });
    await putMmd(api, page.id, `La note ${i} parle du ${secret}, comme les autres.\n`);
  }
  // Toutes trouvables : la troisième réponse porte ses cinq pages.
  await expect
    .poll(
      async () => {
        const res = await api.post(`/api/v1/spaces/${space.id}/search`, { data: { q: secret, after: 40 } });
        return res.status() === 200 ? ((await res.json()) as { results: unknown[] }).results.length : -1;
      },
      { timeout: 60_000 },
    )
    .toBe(5);
  const p = await openPage(browser, home.id, account);

  // Un double clic sur « Rechercher » : le second clic tombe sur le voile de la fenêtre que le premier
  // vient d'ouvrir, et ne la ferme pas.
  await entry(p).dblclick();
  await expect(field(p)).toBeFocused();
  await field(p).fill(secret);
  const lines = win(p).locator("[data-search-page]");
  await expect(lines).toHaveCount(20);
  const more = win(p).getByRole("button", { name: "Afficher plus de résultats" });
  const before = (await more.boundingBox())!;
  const first = (await lines.first().boundingBox())!;

  await more.click();
  await expect(lines).toHaveCount(40);
  // Le bouton est où il était, sous le pointeur resté là : un second clic ne tombe sur aucune ligne.
  expect(await more.boundingBox()).toEqual(before);
  const under = await p.evaluate(
    ([x, y]) => document.elementFromPoint(x, y)?.textContent,
    [before.x + before.width / 2, before.y + before.height / 2],
  );
  expect(under).toBe("Afficher plus de résultats");
  // Les lignes déjà montrées n'ont pas bougé non plus.
  expect(await lines.first().boundingBox()).toEqual(first);

  // La dernière réponse retire le bouton : le champ reprend le focus, jamais `body`.
  await more.click();
  await expect(lines).toHaveCount(45);
  await expect(more).toHaveCount(0);
  await expect(field(p)).toBeFocused();
});

test("a_line_shown_does_not_move_when_results_arrive", async ({ browser }) => {
  const { p, tag, recipes, home, notebook } = await opened(browser);
  // La réponse du serveur est retenue : les lignes de l'arbre sont montrées seules.
  let release!: () => void;
  const held = new Promise<void>((done) => {
    release = done;
  });
  let asked = 0;
  await p.route(/\/api\/v1\/spaces\/[^/]+\/search$/, async (route) => {
    asked += 1;
    await held;
    await route.continue();
  });
  await entry(p).click();
  await field(p).fill(tag);
  await expect(row(p, home.id)).toBeVisible();
  await expect.poll(() => asked).toBeGreaterThan(0);
  expect(await shown(p)).toEqual([recipes.id, home.id]);
  const boxes = async () => [
    await field(p).boundingBox(),
    await row(p, recipes.id).boundingBox(),
    await row(p, home.id).boundingBox(),
  ];
  const before = await boxes();
  await expect(win(p).getByRole("status")).toHaveText("Recherche…");

  release();
  await expect(row(p, notebook.id)).toBeVisible();
  expect(await shown(p)).toEqual([recipes.id, home.id, notebook.id]);
  // Le champ et les lignes déjà montrées sont où ils étaient, au pixel près.
  expect(await boxes()).toEqual(before);
  // La ligne ajoutée est dessous.
  const added = (await row(p, notebook.id).boundingBox())!;
  expect(added.y).toBeGreaterThanOrEqual(before[2]!.y + before[2]!.height);
});
