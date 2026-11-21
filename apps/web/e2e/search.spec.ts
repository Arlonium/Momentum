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
  waitFindable,
  waitSynced,
  word,
} from "./helpers";

test.afterEach(closeContexts);

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
/** L'adresse de la page `id`, sans bloc : une page trouvée par son titre, une ligne de l'arbre. */
const pageUrl = (id: string) => new RegExp(`/p/${id}$`);
/** L'adresse de la page `id`, à un bloc : un résultat trouvé par un bloc y mène (Task C2). */
const blockUrl = (id: string) => new RegExp(`/p/${id}#b=.+$`);

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
  // Chaque ligne est un vrai lien, vers la page et le bloc trouvé.
  await expect(found).toHaveAttribute("href", new RegExp(`^/p/${recipes.id}#b=.+$`));

  await found.click();
  await expect(p).toHaveURL(blockUrl(recipes.id));
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
  await expect(p).toHaveURL(blockUrl(notebook.id));
  await expect(win(p)).toHaveCount(0);
  await expect(titleOf(p)).toHaveValue("Carnet");
  expect(await focusOnBody(p)).toBe(false);

  // Une page ouverte vide le champ ; une recherche par le texte, puis Entrée, sans aucune flèche.
  await p.keyboard.press("Control+k");
  await expect(field(p)).toHaveValue("");
  await p.keyboard.type(secret);
  await expect(row(p, recipes.id)).toHaveAttribute("aria-selected", "true");
  await p.keyboard.press("Enter");
  await expect(p).toHaveURL(blockUrl(recipes.id));
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

  // La dernière réponse retire le bouton : le champ reprend le focus, jamais `body`. À sa place, à la
  // même hauteur, une phrase qu'aucun clic ne prend : sous le pointeur resté là, aucune ligne n'est
  // venue, et un second clic n'ouvrirait rien (recette B9).
  await more.click();
  await expect(lines).toHaveCount(45);
  await expect(more).toHaveCount(0);
  await expect(field(p)).toBeFocused();
  const underNow = await p.evaluate(
    ([x, y]) => {
      const element = document.elementFromPoint(x, y);
      return {
        inFoot: element?.closest(".mm-search-foot") != null,
        clickable: element?.closest("a, button, [data-search-page]") != null,
      };
    },
    [before.x + before.width / 2, before.y + before.height / 2],
  );
  expect(underNow).toEqual({ inFoot: true, clickable: false });
  const done = win(p).locator(".mm-search-done");
  await expect(done).toHaveText("Tous les résultats sont affichés.");
  const box = (await done.boundingBox())!;
  expect([box.y, box.height]).toEqual([before.y, before.height]);
  expect(await lines.first().boundingBox()).toEqual(first);
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

// Arriver sur le bloc trouvé (Task C2) : un résultat ouvre la page à son bloc, `/p/<page>#b=<bloc>`.

/** Les paragraphes de la longue page, et les rangs des deux qui portent un mot à chercher. */
const LONG = 1_000;
const FAR = 850;
const MIDDLE = 400;

/**
 * Un compte neuf, une page de mille paragraphes dont deux portent chacun un mot neuf (au rang `FAR`
 * et au rang `MIDDLE`, loin du haut de la page et loin l'un de l'autre), et une page d'accueil. Tout
 * est trouvable avant qu'un onglet s'ouvre.
 */
async function longScene() {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const far = word();
  const middle = word();
  const lines = Array.from({ length: LONG }, (_, i) =>
    i === FAR ? `Le ${far} est loin en bas.` : i === MIDDLE ? `Le ${middle} est au milieu.` : `Ligne ${i} du remplissage.`,
  );
  const long = await createPage(api, { title: "Longue page" });
  await putMmd(api, long.id, `${lines.join("\n\n")}\n`);
  const home = await createPage(api, { title: "Accueil" });
  await putMmd(api, home.id, "Rien ici\n");
  await waitFindable(api, space.id, far, long.id);
  await waitFindable(api, space.id, middle, long.id);
  return { account, api, space, far, middle, long, home };
}

const blocks = (p: Page) => p.locator('.mm-editor [data-node-type="blockOuter"]');
/** Le bloc de la page affichée qui porte `text`. */
const blockWith = (p: Page, text: string) => blocks(p).filter({ hasText: text });
const firstBlock = (p: Page) => blocks(p).first();
/** Où sont le focus et la sélection du document : dans l'éditeur ? dans quel bloc ? */
const cursorAt = (p: Page) =>
  p.evaluate(() => {
    const node = document.getSelection()?.anchorNode ?? null;
    const element = node instanceof Element ? node : (node?.parentElement ?? null);
    return {
      inEditor: document.activeElement?.closest(".mm-editor") != null,
      block: element?.closest('[data-node-type="blockOuter"]')?.getAttribute("data-id") ?? null,
    };
  });
/** Attend que l'adresse soit celle de la page `id` à un bloc ; rend l'identifiant du bloc, décodé. */
async function blockOfUrl(p: Page, id: string): Promise<string> {
  await expect(p).toHaveURL(blockUrl(id));
  return decodeURIComponent(new URL(p.url()).hash.slice("#b=".length));
}
/** Cherche `query` par Ctrl+K, et ouvre par Entrée la page `id`, que le serveur a trouvée. */
async function searchAndEnter(p: Page, query: string, id: string) {
  await p.keyboard.press("Control+k");
  await expect(field(p)).toBeFocused();
  await p.keyboard.type(query);
  await expect(row(p, id)).toHaveAttribute("data-from", "server");
  await expect(row(p, id)).toHaveAttribute("aria-selected", "true");
  await p.keyboard.press("Enter");
  await expect(win(p)).toHaveCount(0);
}

test("a_result_opens_on_its_block_in_a_long_page", async ({ browser }) => {
  const { api, far, long, home, account } = await longScene();
  const p = await openPage(browser, home.id, account);
  await entry(p).click();
  await field(p).fill(far);
  // La ligne est un lien vers la page, à son bloc.
  await expect(row(p, long.id)).toHaveAttribute("href", new RegExp(`^/p/${long.id}#b=.+$`));
  await p.keyboard.press("Enter");
  const blockId = await blockOfUrl(p, long.id);
  await waitSynced(p);
  // Le bloc trouvé est à l'écran, loin du haut de la page, qui n'y est plus.
  const found = blockWith(p, far);
  await expect(found).toHaveAttribute("data-id", blockId);
  await expect(found).toBeInViewport();
  await expect(firstBlock(p)).not.toBeInViewport();
  // Le focus et le curseur y sont : ce qu'on tape y va, et arrive au serveur.
  await expect.poll(() => cursorAt(p)).toEqual({ inEditor: true, block: blockId });
  await p.keyboard.type("Ici : ");
  await expect(found).toHaveText(`Ici : Le ${far} est loin en bas.`);
  await expect.poll(() => getMmd(api, long.id)).toContain(`Ici : Le ${far} est loin en bas.`);
  // Rien n'est dit, rien n'est en erreur.
  await expect(p.locator('[role="alert"]')).toHaveCount(0);

  // L'adresse seule y mène aussi (un lien copié, l'onglet rechargé), pour qui ne fait que lire :
  // la page à la corbeille s'ouvre en lecture seule, à son bloc.
  expect((await api.post(`/api/v1/pages/${long.id}/trash`)).status()).toBe(200);
  const reader = await newUser(browser, account);
  await reader.goto(`/p/${long.id}#b=${encodeURIComponent(blockId)}`);
  await waitSynced(reader);
  await expect(reader.locator(".mm-editor")).toHaveAttribute("data-editable", "false");
  await expect(blockWith(reader, far)).toBeInViewport();
  await expect.poll(() => cursorAt(reader)).toEqual({ inEditor: true, block: blockId });
  // Un bloc qui n'existe pas (ou plus), dans un onglet neuf (dans le même, seul le fragment
  // changerait, sans recharger la page) : la page, en haut, sans rien dire.
  const lost = await newUser(browser, account);
  await lost.goto(`/p/${long.id}#b=${encodeURIComponent('parti "]')}`);
  await waitSynced(lost);
  await expect(lost.locator(".mm-editor")).toContainText(far);
  await expect(firstBlock(lost)).toBeInViewport();
  await expect(blockWith(lost, far)).not.toBeInViewport();
  await expect(lost.locator('[role="alert"]')).toHaveCount(0);
  expect(await lost.evaluate(() => document.activeElement?.closest(".mm-editor") != null)).toBe(false);
});

test("a_result_of_the_open_page_goes_to_its_block", async ({ browser }) => {
  const { far, middle, long, account } = await longScene();
  const p = await openPage(browser, long.id, account);
  // Témoins : l'éditeur et le titre de la page, qui ne doivent pas être remontés.
  const WITNESSES = [".mm-editor", ".mm-title-input"];
  await p.evaluate((selectors) => {
    for (const selector of selectors) {
      (document.querySelector(selector) as unknown as { witness: boolean }).witness = true;
    }
  }, WITNESSES);
  const kept = () =>
    p.evaluate(
      (selectors) =>
        selectors.map(
          (selector) => (document.querySelector(selector) as unknown as { witness?: boolean }).witness === true,
        ),
      WITNESSES,
    );
  await firstBlock(p).locator(".bn-block-content").click();
  await expect(blockWith(p, far)).not.toBeInViewport();

  // La page est déjà ouverte : sans le fragment gardé par le routeur, rien ne se passerait.
  await searchAndEnter(p, far, long.id);
  const farId = await blockOfUrl(p, long.id);
  await expect(blockWith(p, far)).toBeInViewport();
  await expect.poll(() => cursorAt(p)).toEqual({ inEditor: true, block: farId });
  expect(await kept()).toEqual([true, true]);

  // Un autre bloc de la même page : l'adresse change, le bloc est atteint, la page n'est pas remontée.
  await searchAndEnter(p, middle, long.id);
  const middleId = await blockOfUrl(p, long.id);
  expect(middleId).not.toBe(farId);
  await expect(blockWith(p, middle)).toBeInViewport();
  await expect(blockWith(p, far)).not.toBeInViewport();
  await expect.poll(() => cursorAt(p)).toEqual({ inEditor: true, block: middleId });
  expect(await kept()).toEqual([true, true]);

  // Retour : le bloc d'avant.
  await p.goBack();
  expect(await blockOfUrl(p, long.id)).toBe(farId);
  await expect(blockWith(p, far)).toBeInViewport();
  expect(await kept()).toEqual([true, true]);

  // Le même résultat choisi de nouveau, la page défilée entre-temps : on y revient, sans entrée de
  // plus dans l'historique.
  await firstBlock(p).scrollIntoViewIfNeeded();
  await expect(blockWith(p, far)).not.toBeInViewport();
  const length = await p.evaluate(() => history.length);
  await searchAndEnter(p, far, long.id);
  await expect(blockWith(p, far)).toBeInViewport();
  expect(await p.evaluate(() => history.length)).toBe(length);
  expect(await blockOfUrl(p, long.id)).toBe(farId);
  expect(await kept()).toEqual([true, true]);
  await expect(p.locator('[role="alert"]')).toHaveCount(0);
});

/** Cherche `query` par Ctrl+K, et ouvre par Entrée la page `id`, trouvée par son titre dans l'arbre. */
async function titleAndEnter(p: Page, query: string, id: string) {
  await p.keyboard.press("Control+k");
  await expect(field(p)).toBeFocused();
  await p.keyboard.type(query);
  await expect(row(p, id)).toHaveAttribute("data-from", "tree");
  await expect(row(p, id)).toHaveAttribute("aria-selected", "true");
  await p.keyboard.press("Enter");
  await expect(win(p)).toHaveCount(0);
}

/** La page ouverte, dans le fil d'Ariane de la barre du haut. */
const crumb = (p: Page) => p.locator('.mm-breadcrumb [aria-current="page"]');

test("a_page_opened_by_its_title_opens_at_its_top_with_the_focus_on_its_breadcrumb", async ({ browser }) => {
  // Recette C4 et choix 24 : une page trouvée par son titre s'ouvre en haut, depuis une autre page
  // comme depuis elle-même (arrivée en bas par un bloc), sans être remontée ; le focus va à la page,
  // dans le fil d'Ariane, comme à un rétrolien sans bloc (R1128), pas à « Rechercher ».
  const { far, long, home, account } = await longScene();
  const p = await openPage(browser, home.id, account);
  await p.locator(".mm-editor .bn-block-content").first().click();

  // Depuis une autre page.
  await titleAndEnter(p, "longue", long.id);
  await expect(p).toHaveURL(pageUrl(long.id));
  await waitSynced(p);
  await expect(firstBlock(p)).toBeInViewport();
  await expect(crumb(p)).toBeFocused();
  await expect(crumb(p)).toHaveText(long.title);

  // Arrivée en bas, par un bloc ; puis la même page, par son titre : elle remonte en haut.
  const WITNESSES = [".mm-editor", ".mm-title-input"];
  await p.evaluate((selectors) => {
    for (const selector of selectors) {
      (document.querySelector(selector) as unknown as { witness: boolean }).witness = true;
    }
  }, WITNESSES);
  const kept = () =>
    p.evaluate(
      (selectors) =>
        selectors.map(
          (selector) => (document.querySelector(selector) as unknown as { witness?: boolean }).witness === true,
        ),
      WITNESSES,
    );
  await searchAndEnter(p, far, long.id);
  await blockOfUrl(p, long.id);
  await expect(blockWith(p, far)).toBeInViewport();
  await expect(firstBlock(p)).not.toBeInViewport();
  const length = await p.evaluate(() => history.length);
  await titleAndEnter(p, "longue", long.id);
  await expect(p).toHaveURL(pageUrl(long.id));
  expect(await p.evaluate(() => history.length)).toBe(length + 1);
  await expect(firstBlock(p)).toBeInViewport();
  await expect(blockWith(p, far)).not.toBeInViewport();
  await expect(crumb(p)).toBeFocused();
  expect(await kept()).toEqual([true, true]);

  // À l'adresse même de la page, défilée à la main : par son titre, de nouveau en haut, sans entrée
  // de plus dans l'historique.
  await blockWith(p, far).scrollIntoViewIfNeeded();
  await expect(firstBlock(p)).not.toBeInViewport();
  await titleAndEnter(p, "longue", long.id);
  await expect(firstBlock(p)).toBeInViewport();
  await expect(crumb(p)).toBeFocused();
  expect(await p.evaluate(() => history.length)).toBe(length + 1);
  expect(await kept()).toEqual([true, true]);
  expect(await focusOnBody(p)).toBe(false);
});

test("a_found_block_with_many_sub_blocks_shows_its_own_text", async ({ browser }) => {
  // Un bloc dont les sous-blocs dépassent la hauteur de la fenêtre : centré sur son sous-arbre, son
  // texte (ce que la recherche a trouvé) serait au-dessus de l'écran.
  const account = await signUpAccount();
  const api = await apiAs(account);
  const space = await firstSpace(api);
  const parent = word();
  const filler = (from: number) => Array.from({ length: 40 }, (_, i) => `Ligne ${from + i} du remplissage.`).join("\n\n");
  const children = Array.from({ length: 120 }, (_, i) => `  - Sous-bloc ${i}`).join("\n");
  const page = await createPage(api, { title: "Page à sous-blocs" });
  await putMmd(api, page.id, `${filler(0)}\n\n- Le ${parent} a des sous-blocs.\n${children}\n\n${filler(100)}\n`);
  const home = await createPage(api, { title: "Accueil" });
  await putMmd(api, home.id, "Rien ici\n");
  await waitFindable(api, space.id, parent, page.id);

  const p = await openPage(browser, home.id, account);
  await searchAndEnter(p, parent, page.id);
  const blockId = await blockOfUrl(p, page.id);
  await waitSynced(p);
  const found = p.locator(`.mm-editor [data-node-type="blockOuter"][data-id="${blockId}"]`);
  const own = found.locator(":scope > .bn-block > .bn-block-content");
  await expect(own).toHaveText(`Le ${parent} a des sous-blocs.`);
  // Le bloc entier est plus haut que la fenêtre (sans quoi le test ne prouverait rien)…
  const height = (await found.boundingBox())!.height;
  expect(height).toBeGreaterThan(p.viewportSize()!.height);
  // … et c'est son texte qui est à l'écran, entier.
  await expect(own).toBeInViewport({ ratio: 1 });
  await expect.poll(() => cursorAt(p)).toEqual({ inEditor: true, block: blockId });
});
