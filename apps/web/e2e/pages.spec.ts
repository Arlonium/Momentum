// Arbre de pages et persistance, depuis l'interface ; et le fenêtrage des arbres de la barre
// latérale (la page ouverte toujours montrée, les sections qui se suivent).
import { expect, test, type Locator } from "@playwright/test";
import type { ErrorBody, PageNode } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getMmd,
  getPage,
  getTree,
  newUser,
  openPage,
  ownerAndMember,
  pageIdOf,
  putMmd,
  sectionOf,
  sidebarOf,
  signUpAccount,
  titleOf,
  treeRow,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

const UUID = /\/p\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

test("create_page_and_type_persists", async ({ page, request }) => {
  await page.goto("/");
  const sidebar = page.getByRole("navigation", { name: "Barre latérale" });
  await sidebar.getByRole("button", { name: "Nouvelle page" }).click();
  await expect(page).toHaveURL(UUID);
  const id = pageIdOf(page);

  const title = page.getByRole("textbox", { name: "Titre de la page" });
  await title.fill("Essai");
  await title.press("Enter");
  // L'arbre suit le renommage.
  await expect(sidebar.locator(`[data-page-id="${id}"]`).getByRole("link")).toHaveText("Essai");

  await waitSynced(page);
  await page.locator(".bn-editor").click();
  await page.keyboard.type("Bonjour Café 🚀");
  // Rechargement une fois le texte reçu par le serveur (sinon on mesurerait l'envoi).
  await expect.poll(() => getMmd(request, id)).toContain("Bonjour Café 🚀");

  await page.reload();
  await waitSynced(page);
  await expect(title).toHaveValue("Essai");
  await expect(page.locator(".bn-editor")).toContainText("Bonjour Café 🚀");
  expect(await getMmd(request, id)).toContain("Bonjour Café 🚀");
  const node = (await getTree(request)).nodes.find((n) => n.id === id);
  expect(node?.title).toBe("Essai");
});

test("double_click_creates_a_single_page", async ({ browser }) => {
  const account = await signUpAccount();
  const api = await apiAs(account);
  const p = await newUser(browser, account);
  // Les créations de page que l'onglet demande au serveur, comptées dans la page à l'appel
  // même de `fetch` : lu juste après un clic, le compte dit si ce clic a créé, que le serveur
  // ait répondu ou non.
  await p.addInitScript(() => {
    const counted = window as unknown as { __created: number };
    counted.__created = 0;
    const send = window.fetch.bind(window);
    window.fetch = (input, init) => {
      const asked = input instanceof Request ? input : null;
      const { pathname } = new URL(asked ? asked.url : String(input), location.href);
      const method = init?.method ?? asked?.method ?? "GET";
      if (method === "POST" && pathname === "/api/v1/pages") counted.__created += 1;
      return send(input, init);
    };
  });
  const created = () => p.evaluate(() => (window as unknown as { __created: number }).__created);
  await p.goto("/");
  const sidebar = sidebarOf(p);
  /** Les pages déjà ouvertes : la suivante est une autre. */
  const opened: string[] = [];
  const another = async () => {
    await p.waitForURL((url) => UUID.test(url.pathname) && !opened.includes(url.pathname));
    opened.push(new URL(p.url()).pathname);
    return pageIdOf(p);
  };
  /**
   * Un double clic sur un bouton qui reste sous le pointeur, au rythme d'une main : le serveur
   * a répondu au premier clic, et la page créée est ouverte, quand le second arrive. Ce second
   * clic ne crée rien ; le clic simple qui le suit, si.
   */
  const doubleThenSingle = async (button: Locator) => {
    const before = await created();
    await button.click();
    await another();
    expect(await created()).toBe(before + 1);
    // Le second clic du double clic, seul : au même endroit, `detail` vaut 2. (Avec
    // `clickCount: 2`, `click()` enverrait les deux clics, dont un premier qui crée.)
    await button.hover();
    await p.mouse.down({ clickCount: 2 });
    await p.mouse.up({ clickCount: 2 });
    // La création part dans la tâche même du clic : si celui-ci avait créé, c'est déjà compté.
    // Compter après le clic suivant ne suffirait pas : une création encore en route
    // l'avalerait, et le compte final serait le bon.
    expect(await created()).toBe(before + 1);
    await button.click();
    const id = await another();
    expect(await created()).toBe(before + 2);
    return id;
  };

  // « Nouvelle page » du contenu part avec le premier clic : les deux clics se suivent sans
  // attendre, et le second tombe pendant la création.
  await p.getByRole("main").getByRole("button", { name: "Nouvelle page" }).dblclick();
  await another();
  expect(await created()).toBe(1);
  // « Nouvelle page » de la barre latérale, le « + » d'une section et le « + » d'une ligne
  // restent sous le pointeur.
  await doubleThenSingle(sidebar.getByRole("button", { name: "Nouvelle page" }));
  await sectionOf(p, "Espace").hover();
  const inSpace = await doubleThenSingle(
    sidebar.getByRole("button", { name: "Ajouter une page dans « Espace »" }),
  );
  await treeRow(p, inSpace).hover();
  await doubleThenSingle(
    treeRow(p, inSpace).getByRole("button", { name: /^Ajouter une sous-page à / }),
  );
  expect(await created()).toBe(7);
  expect((await getTree(api)).nodes).toHaveLength(7);
});

test("subpage_appears_in_tree", async ({ page, request }) => {
  const parentTitle = `Parent ${unique()}`;
  const parent = await createPage(request, { title: parentTitle });
  await page.goto(`/p/${parent.id}`);
  const sidebar = page.getByRole("navigation", { name: "Barre latérale" });
  // Une ligne par page (`treeitem`), toutes au même niveau du document : l'imbrication se
  // lit dans `aria-level` et dans l'ordre des lignes.
  const parentRow = sidebar.locator(`[role="treeitem"][data-page-id="${parent.id}"]`);
  await expect(parentRow.getByRole("link")).toHaveText(parentTitle);
  await expect(parentRow).toHaveAttribute("aria-level", "1");

  // Le « + » n'apparaît qu'au survol de la page.
  const add = parentRow.getByRole("button", { name: "Ajouter une sous-page" });
  await expect(add).toHaveCSS("opacity", "0");
  await parentRow.hover();
  await expect(add).toHaveCSS("opacity", "1");
  await add.click();
  // La page parente est déjà une `/p/:id` : attendre l'adresse de la sous-page.
  await page.waitForURL((url) => UUID.test(url.pathname) && !url.pathname.endsWith(parent.id));
  const childId = pageIdOf(page);

  const title = page.getByRole("textbox", { name: "Titre de la page" });
  await title.fill("Sous-page");
  await title.press("Enter");

  // Imbriquée sous sa page parente, dans l'interface comme sur le serveur : sa ligne suit
  // celle du parent, un niveau plus bas, et le parent est déplié.
  const child = sidebar.locator(`[data-page-id="${parent.id}"] + [role="treeitem"]`);
  const nested = async () => {
    await expect(child).toHaveAttribute("data-page-id", childId);
    await expect(child).toHaveAttribute("aria-level", "2");
    await expect(child.getByRole("link")).toHaveText("Sous-page");
    await expect(parentRow).toHaveAttribute("aria-expanded", "true");
  };
  await nested();
  const node = (await getTree(request)).nodes.find((n) => n.id === childId);
  expect(node?.parentId).toBe(parent.id);
  expect(node?.title).toBe("Sous-page");

  await page.reload();
  await nested();

  // Un clic dans l'arbre ouvre la page, sans recharger l'application.
  await page.evaluate(() => ((window as unknown as { __loaded: boolean }).__loaded = true));
  await parentRow.getByRole("link").click();
  await expect(page).toHaveURL(new RegExp(`/p/${parent.id}$`));
  await expect(title).toHaveValue(parentTitle);
  expect(await page.evaluate(() => (window as unknown as { __loaded?: boolean }).__loaded)).toBe(true);
});

test("foreign_mmd_block_survives_browser", async ({ page, request }) => {
  // R33 : un bloc étranger écrit par MMD devient un bloc `unknown` de BlockNote, que le
  // navigateur affiche et garde. En élément étranger, y-prosemirror le supprimait chez tous
  // dès l'ouverture de la page.
  const { id } = await createPage(request, { title: `Bloc inconnu ${unique()}` });
  const block =
    '<block type="futureWidget" id="b_future01" ' +
    'data="eyJhdHRycyI6eyJhIjoiMSJ9LCJjb250ZW50IjpbXSwiY2hpbGRyZW4iOltdfQ=="';
  await putMmd(request, id, `Avant\n\n${block} foreign=true/>\n\nAprès\n`);
  const expected = `Avant\n\n${block}/>\n\nAprès\n`;
  expect(await getMmd(request, id)).toBe(expected);

  await page.goto(`/p/${id}`);
  await waitSynced(page);
  await expect(page.locator(".bn-editor")).toContainText("Bloc non pris en charge (futureWidget)");
  await expect(page.locator(".bn-editor")).toContainText("Après");
  expect(await getMmd(request, id)).toBe(expected);
});

test("unregistered_mmd_prop_is_refused", async ({ page, request }) => {
  // R41 : `PUT /mmd` refuse une prop hors registre, sur un bloc comme sur une cellule de
  // tableau. Écrite, y-prosemirror la retirerait du document partagé dès qu'un navigateur
  // modifie le bloc, sans bruit : le serveur répond 422 et la page ne change pas.
  const { id } = await createPage(request, { title: `Prop inconnue ${unique()}` });
  await putMmd(request, id, "Avant\n");
  await page.goto(`/p/${id}`);
  await waitSynced(page);
  const editor = page.locator(".bn-editor");
  await expect(editor).toContainText("Avant");

  for (const [mmd, where] of [
    ["Texte {foo=1}\n", "un bloc « paragraph »"],
    ["| a | Texte {foo=1} |\n| --- | --- |\n", "une cellule d'un bloc « table »"],
  ]) {
    const res = await request.put(`/api/v1/pages/${id}/mmd`, {
      data: mmd,
      headers: { "content-type": "text/markdown" },
    });
    expect(res.status(), await res.text()).toBe(422);
    const body = (await res.json()) as ErrorBody;
    expect(body.error).toBe("invalid_blocks");
    expect(body.message).toContain("« foo »");
    expect(body.message).toContain(where);
    expect(await getMmd(request, id)).toBe("Avant\n");
  }

  // La page ouverte n'a rien reçu, et elle s'édite toujours.
  await page.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
  await page.keyboard.press("End");
  await page.keyboard.type("!");
  await expect.poll(() => getMmd(request, id)).toBe("Avant!\n");
  await expect(editor).toContainText("Avant!");
  await expect(editor).not.toContainText("Texte");
});

test("active_row_is_shown_clear_of_the_sidebar_edges", async ({ browser }) => {
  // La barre latérale défile sous sa tête (le sélecteur d'espace) et son pied (la corbeille),
  // collés à ses bords : la ligne de la page ouverte ne doit jamais rester dessous.
  const account = await signUpAccount();
  const api = await apiAs(account);
  const pages: PageNode[] = [];
  for (let i = 0; i < 40; i++) pages.push(await createPage(api, { title: `Ligne ${i + 1}` }));
  const p = await newUser(browser, account);
  // « Récents », au-dessus des arbres, grandit d'une ligne à chaque page ouverte, jusqu'à cinq
  // (R255), et le navigateur ancre le défilement sur ce qu'il montre : les mesures exactes de
  // ce test veulent une liste repliée, dont la hauteur (son titre) ne bouge pas quand une page
  // s'ouvre. Ce que fait la liste quand elle grandit : `recents.spec.ts`.
  const { id: spaceId } = await firstSpace(api);
  await p.addInitScript(
    (key) => localStorage.setItem(key, JSON.stringify(["recents"])),
    `mementum.sections.${spaceId}`,
  );
  const sidebar = sidebarOf(p);
  /** La ligne est-elle visible en entier, sans rien par-dessus ? */
  const clear = (id: string) =>
    treeRow(p, id).evaluate((row) => {
      const box = row.getBoundingClientRect();
      const at = (y: number) => row.contains(document.elementFromPoint(box.left + box.width / 2, y));
      return at(box.top + 1) && at(box.bottom - 1);
    });
  const scrollTop = () => sidebar.evaluate((el) => el.scrollTop);
  /** Ouvre la page par son lien, sans que Playwright fasse défiler la barre pour cliquer. */
  const open = async (id: string) => {
    await treeRow(p, id).getByRole("link").evaluate((link: HTMLElement) => link.click());
    await expect(p).toHaveURL(new RegExp(`/p/${id}$`));
    await expect(treeRow(p, id).getByRole("link")).toHaveAttribute("aria-current", "page");
  };

  // Lien direct vers la dernière page, loin sous la partie visible : la barre défile
  // jusqu'à elle.
  const last = pages[pages.length - 1];
  await p.goto(`/p/${last.id}`);
  await expect(treeRow(p, last.id)).toBeVisible();
  await expect.poll(() => clear(last.id)).toBe(true);

  // Une ligne coupée par le pied : la barre défile de ce qui manque, sans la centrer.
  await sidebar.evaluate((el) => (el.scrollTop = 0));
  const low = pages[25].id;
  await expect(treeRow(p, low)).toBeVisible();
  await sidebar.evaluate((el, id) => {
    const row = el.querySelector(`[data-page-id="${id}"]`)!.getBoundingClientRect();
    const footer = el.querySelector(".mm-side-footer")!.getBoundingClientRect();
    el.scrollTop += row.bottom - footer.top - 12;
  }, low);
  expect(await clear(low)).toBe(false);
  let before = await scrollTop();
  await open(low);
  await expect.poll(scrollTop).toBe(before + 12);
  expect(await clear(low)).toBe(true);

  // Une ligne coupée par la tête : pareil, vers le haut.
  const high = pages[14].id;
  await sidebar.evaluate((el, id) => {
    const row = el.querySelector(`[data-page-id="${id}"]`)!.getBoundingClientRect();
    const head = el.querySelector(".mm-side-head")!.getBoundingClientRect();
    el.scrollTop += row.top - head.bottom + 10;
  }, high);
  expect(await clear(high)).toBe(false);
  before = await scrollTop();
  await open(high);
  await expect.poll(scrollTop).toBe(before - 10);
  expect(await clear(high)).toBe(true);

  // Une ligne déjà visible en entier : la barre ne bouge pas.
  before = await scrollTop();
  await open(pages[16].id);
  await p.waitForTimeout(200);
  expect(await scrollTop()).toBe(before);

  // Au clavier aussi. La première ligne coupée par le pied, et celle d'avant, visible.
  await sidebar.evaluate((el) => (el.scrollTop = 0));
  const [seen, cut] = await sidebar.evaluate((el) => {
    const footer = el.querySelector(".mm-side-footer")!.getBoundingClientRect();
    const rows = [...el.querySelectorAll<HTMLElement>("[data-page-id]")];
    const at = rows.findIndex((row) => row.getBoundingClientRect().bottom > footer.top);
    return [rows[at - 1].dataset.pageId!, rows[at].dataset.pageId!];
  });
  expect(await clear(seen)).toBe(true);
  expect(await clear(cut)).toBe(false);
  // Tab depuis le lien de la ligne visible : ses deux boutons, puis le lien de la suivante,
  // que la barre fait remonter de sous son pied.
  await treeRow(p, seen).getByRole("link").evaluate((link: HTMLElement) =>
    link.focus({ preventScroll: true }),
  );
  for (let i = 0; i < 3; i++) await p.keyboard.press("Tab");
  await expect(treeRow(p, cut).getByRole("link")).toBeFocused();
  await expect.poll(() => clear(cut)).toBe(true);

  // Et vers le haut : Maj+Tab jusqu'au lien d'une ligne passée sous la tête.
  const [hidden, below] = await sidebar.evaluate((el) => {
    const head = el.querySelector(".mm-side-head")!.getBoundingClientRect();
    const rows = [...el.querySelectorAll<HTMLElement>("[data-page-id]")];
    const target = rows[6];
    el.scrollTop += target.getBoundingClientRect().top - head.bottom + 14;
    return [target.dataset.pageId!, rows[7].dataset.pageId!];
  });
  expect(await clear(hidden)).toBe(false);
  await treeRow(p, below).getByRole("link").evaluate((link: HTMLElement) =>
    link.focus({ preventScroll: true }),
  );
  for (let i = 0; i < 3; i++) await p.keyboard.press("Shift+Tab");
  await expect(treeRow(p, hidden).getByRole("link")).toBeFocused();
  await expect.poll(() => clear(hidden)).toBe(true);

  // La tête et le pied, eux, sont toujours visibles : y poser le focus ne fait pas défiler.
  before = await scrollTop();
  expect(before).toBeGreaterThan(0);
  await sidebar.getByRole("button", { name: "Nouvelle page" }).focus();
  await sidebar.getByRole("link", { name: "Corbeille" }).focus();
  await p.waitForTimeout(200);
  expect(await scrollTop()).toBe(before);
});

test("tree_below_a_long_section_follows_what_happens_above_it", async ({ browser }) => {
  // Chaque section a son arbre, fenêtré : seules les lignes de la partie visible de la barre
  // latérale sont dans le document. Quand une section, au-dessus, change de hauteur, les
  // arbres qui la suivent se déplacent sans que rien ne défile : leur fenêtre doit suivre.
  const account = await signUpAccount();
  const api = await apiAs(account);
  for (let i = 0; i < 70; i++) await createPage(api, { title: `Ligne ${i + 1}` });
  const mine = await createPage(api, { section: "private", title: "Tout en bas" });
  const p = await newUser(browser, account);
  const sidebar = sidebarOf(p);
  await p.goto("/");
  await expect(sectionOf(p, "Espace").getByRole("treeitem").first()).toBeVisible();
  const rows = sidebar.getByRole("treeitem");

  // « Privé » est loin sous la partie visible : sa page n'est pas dans le document, et la
  // longue section « Espace » n'y a que ses premières lignes.
  await expect(treeRow(p, mine.id)).toHaveCount(0);
  expect(await rows.count()).toBeLessThan(50);

  // « Espace » repliée : « Privé » remonte d'un coup dans la partie visible, avec sa page.
  const toggle = sectionOf(p, "Espace").getByRole("button", { name: "Espace", exact: true });
  await toggle.click();
  await expect(treeRow(p, mine.id)).toBeVisible();
  await expect(rows).toHaveCount(1);

  // Dépliée : « Privé » repart sous la partie visible, et sa page quitte le document.
  await toggle.click();
  await expect(treeRow(p, mine.id)).toHaveCount(0);
  expect(await rows.count()).toBeLessThan(50);

  // Tout en bas de la barre latérale, elle y revient.
  await sidebar.evaluate((el) => el.scrollTo(0, el.scrollHeight));
  await expect(treeRow(p, mine.id)).toBeInViewport();
  expect(await rows.count()).toBeLessThan(50);
});

test("rename_elsewhere_while_the_title_has_the_focus_is_kept", async ({ browser }) => {
  // C9-4 (C1, R210) : Alice a cliqué dans le titre sans rien taper ; Bruno renomme la page. Son
  // champ suit le renommage sous le focus, et le quitter n'envoie rien : le titre de Bruno reste.
  const { alice, aliceApi, brunoApi, space } = await ownerAndMember();
  const page = await createPage(aliceApi, { spaceId: space.id, title: `Ancien titre ${unique()}` });
  const a = await openPage(browser, page.id, alice);
  const sent: string[] = [];
  a.on("request", (request) => {
    if (request.method() === "PATCH" && request.url().endsWith(`/api/v1/pages/${page.id}`)) {
      sent.push(request.postData() ?? "");
    }
  });
  const title = titleOf(a);
  await title.click();
  await expect(title).toBeFocused();
  const renamed = await brunoApi.patch(`/api/v1/pages/${page.id}`, {
    data: { title: "Titre de Bruno" },
  });
  expect(renamed.status(), await renamed.text()).toBe(200);
  await expect(title).toHaveValue("Titre de Bruno");
  await expect(title).toBeFocused();
  // Alice quitte le champ et tape dans la page : quand sa frappe est au serveur, le blur est
  // passé depuis longtemps.
  await a.locator(".bn-editor").click();
  await a.keyboard.type("Après le blur");
  await expect.poll(() => getMmd(aliceApi, page.id)).toContain("Après le blur");
  expect(sent).toEqual([]);
  expect((await getPage(aliceApi, page.id)).title).toBe("Titre de Bruno");
});
