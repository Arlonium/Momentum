// Les équations de la partie B du lot 3 (Task C2) : en bloc et en ligne, posées par le menu « / »,
// écrites dans leur fenêtre, vues par l'autre personne de la page et relues en MMD. Et ce qu'une
// formule venue d'ailleurs ne peut pas faire à l'onglet de qui ouvre la page : une formule
// imbriquée le faisait planter (la mise en page de Chromium déborde sa pile sur des tables
// imbriquées), une définition le figer.
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, createPage, getMmd, openPage, ownerAndMember, putMmd, unique } from "./helpers";

test.afterEach(closeContexts);

async function duo(browser: Parameters<typeof openPage>[0], title: string) {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `${title} ${unique()}` });
  const mona = await openPage(browser, page.id, team.alice);
  const tom = await openPage(browser, page.id, team.bruno);
  return { ...team, page, mona, tom };
}

async function slash(p: Page, query: string, title: string): Promise<void> {
  await p.keyboard.type(`/${query}`);
  await expect(p.locator(".bn-suggestion-menu-item").first()).toContainText(title);
  await p.keyboard.press("Enter");
}

function latexField(p: Page, label: string) {
  return p.getByRole("dialog", { name: label }).getByRole("textbox", { name: "Formule (LaTeX)" });
}

test("equation_block_is_typed_and_read_back", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Équation");
  await mona.locator(".mm-editor .bn-block-content").first().click();
  await slash(mona, "équation", "Équation");
  // Insérée vide, elle ouvre son champ ; la frappe va au champ, pas au document.
  const field = latexField(mona, "Formule de l'équation");
  await expect(field).toBeFocused();
  await mona.keyboard.type("E = mc^2");
  await expect(mona.getByRole("dialog", { name: "Formule de l'équation" }).locator(".mm-latex-preview .katex")).toBeVisible();
  // Rien n'est écrit dans la page tant qu'on tape.
  expect(await getMmd(aliceApi, page.id)).not.toContain("mc^2");
  await mona.keyboard.press("Control+Enter");
  await expect(field).toBeHidden();
  // Posée depuis le texte : la frappe reprend dans le paragraphe qui la suit.
  await expect(mona.locator(".mm-editor .bn-editor")).toBeFocused();
  await mona.keyboard.type("Suite");
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("$$\nE = mc^2\n$$\n\nSuite\n");
  await expect(tom.locator(".mm-editor .mm-equation .katex")).toBeVisible();
});

test("inline_math_is_typed_and_read_back", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "En ligne");
  await mona.locator(".mm-editor .bn-block-content").first().click();
  await mona.keyboard.type("Masse et énergie : ");
  await slash(mona, "équation en ligne", "Équation en ligne");
  const field = latexField(mona, "Formule en ligne");
  await expect(field).toBeFocused();
  await mona.keyboard.type("E = mc^2");
  await mona.keyboard.press("Control+Enter");
  await expect(field).toBeHidden();
  await mona.keyboard.press("End");
  await mona.keyboard.type(" fin");
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("Masse et énergie : $E = mc^2$ fin\n");
  await expect(tom.locator(".mm-editor .mm-math .katex")).toBeVisible();
});

test("formula_that_dollars_cannot_carry_is_read_back", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Barre finale ${unique()}` });
  await putMmd(request, page.id, "Avant $a$ après\n");
  const p = await openPage(browser, page.id);
  await p.getByRole("button", { name: "Modifier la formule" }).click();
  const field = latexField(p, "Formule en ligne");
  await expect(field).toBeFocused();
  await field.fill("x\\");
  await p.keyboard.press("Control+Enter");
  await expect(field).toBeHidden();
  await expect.poll(() => getMmd(request, page.id)).toBe('Avant <math latex="x\\"/> après\n');
});

test("typing_right_after_choosing_an_equation_goes_to_its_field", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Aussitôt ${unique()}` });
  const p = await openPage(browser, page.id);
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.type("/équation");
  await expect(p.locator(".bn-suggestion-menu-item").first()).toContainText("Équation");
  // Entrée, puis la formule aussitôt, sans attendre la fenêtre.
  await p.keyboard.press("Enter");
  await p.keyboard.type("\\alpha");
  await expect(latexField(p, "Formule de l'équation")).toHaveValue("\\alpha");
  // Rien n'est entré dans la page.
  expect(await getMmd(request, page.id)).not.toContain("alpha");
});

test("a_long_inline_formula_scrolls_instead_of_being_cut", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Longue ${unique()}` });
  // Sans point de coupure (ni opérateur ni relation) : KaTeX ne la coupe pas en lignes.
  await putMmd(request, page.id, `Une formule $${"x_{1}".repeat(200)}$ dans la ligne\n`);
  const p = await openPage(browser, page.id);
  const formula = p.locator(".mm-editor .mm-math");
  await expect(formula.locator(".katex")).toBeVisible();
  const box = await formula.evaluate((element) => ({
    overflow: getComputedStyle(element).overflowX,
    scroll: element.scrollWidth,
    client: element.clientWidth,
    editor: document.querySelector(".mm-editor .bn-editor")!.clientWidth,
  }));
  // Plus large que la ligne : elle défile en dedans, sans dépasser l'éditeur.
  expect(box.scroll).toBeGreaterThan(box.client);
  expect(box.overflow).toBe("auto");
  expect(box.client).toBeLessThanOrEqual(box.editor);
});

test("a_formula_of_huge_sizes_leaves_the_rest_of_the_page_within_reach", async ({ browser, request }) => {
  // Une formule de 24 caractères faisait un bloc de 33 millions de pixels, et la suite de la page
  // devenait inaccessible (relecture de C2, I3) : les tailles écrites sont bornées, et une hauteur
  // négative, que KaTeX ne borne pas, défile dans la formule.
  const page = await createPage(request, { title: `Tailles ${unique()}` });
  await putMmd(
    request,
    page.id,
    `$$\n\\rule{1em}{2000000em}\n$$\n\nAvant $\\raisebox{-100000em}{x}$ milieu $\\rule{1em}{100000em}$ après\n\n$$\n\\frac{a}{b} + \\sqrt{2}\n$$\n\nOrdinaire $a^2 + \\frac{1}{2}$ ici\n\nFin\n`,
  );
  const p = await openPage(browser, page.id);
  await expect(p.locator(".mm-editor .mm-equation .katex")).toHaveCount(2);
  await expect(p.locator(".mm-editor .mm-math .katex")).toHaveCount(3);
  const end = p.locator(".mm-editor .bn-block-content").filter({ hasText: /^Fin$/ });
  const top = await end.evaluate(
    (element) => element.getBoundingClientRect().top - document.querySelector(".mm-editor")!.getBoundingClientRect().top,
  );
  expect(top).toBeLessThan(2_000);
  // Plus hautes que leur borne, elles défilent en dedans, en bloc comme en ligne.
  for (const tall of [p.locator(".mm-editor .mm-equation .mm-tex").first(), p.locator(".mm-editor .mm-math").first()]) {
    const box = await tall.evaluate((element) => ({ scroll: element.scrollHeight, client: element.clientHeight }));
    expect(box.scroll).toBeGreaterThan(box.client);
    expect(box.client).toBeLessThan(800);
  }
  // Une formule ordinaire ne défile pas : rien ne dépasse de sa hauteur, en bloc ni en ligne.
  for (const ordinary of [p.locator(".mm-editor .mm-equation .mm-tex").last(), p.locator(".mm-editor .mm-math").last()]) {
    const box = await ordinary.evaluate((element) => ({ scroll: element.scrollHeight, client: element.clientHeight }));
    expect(box.scroll).toBeLessThanOrEqual(box.client);
  }
});

/** Ouvre la page et dit si l'onglet a planté avant la fin de la première synchro. */
async function openWatchingCrash(browser: Parameters<typeof openPage>[0], id: string) {
  const context = await browser.newContext({ storageState: test.info().project.use.storageState as string });
  const p = await context.newPage();
  let crashed = false;
  p.on("crash", () => {
    crashed = true;
  });
  await p.goto(`/p/${id}`);
  await p.waitForFunction(() => document.querySelector(".mm-editor")?.getAttribute("data-synced") === "true", null, {
    timeout: 20_000,
  });
  return { p, crashed: () => crashed, close: () => context.close() };
}

const frac = (n: number) => "\\frac{1}{".repeat(n) + "x" + "}".repeat(n);
const sqrt = (n: number) => "\\sqrt{".repeat(n) + "x" + "}".repeat(n);

test("a_nested_formula_never_crashes_the_tab", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Imbriquée ${unique()}` });
  // Sans macro : 150 racines (1 051 caractères), 200 fractions, et 13 fractions dans une liste
  // imbriquée de dix niveaux ; puis une formule ordinaire, qui doit s'afficher.
  const list = Array.from({ length: 10 }, (_, i) => `${"  ".repeat(i)}- niveau ${i}`).join("\n");
  await putMmd(
    request,
    page.id,
    `$$\n${sqrt(150)}\n$$\n\n$$\n${frac(200)}\n$$\n\n${list}\n${"  ".repeat(10)}- en ligne $${frac(13)}$\n\n$$\n\\frac{a}{b} + \\sqrt{2}\n$$\n`,
  );
  const opened = await openWatchingCrash(browser, page.id);
  const { p } = opened;
  await expect(p.locator(".mm-editor .mm-equation").last().locator(".katex")).toBeVisible();
  await expect(p.locator(".mm-editor .mm-tex-why")).toHaveCount(3);
  await expect(p.locator(".mm-editor .mm-tex-why").first()).toHaveText("Formule trop imbriquée pour être affichée ici.");
  expect(opened.crashed()).toBe(false);
  // La page reste vivante : on y tape.
  await p.locator(".mm-editor .mm-equation").last().click();
  await expect(p.getByRole("dialog", { name: "Formule de l'équation" })).toBeVisible();
  await opened.close();
});

const B = String.fromCharCode(92);
/** `@` pour la barre oblique inverse. */
const tex = (s: string) => s.replace(/@/g, B);
const nest = (open: string, close: string, n: number, core = "x") => tex(open).repeat(n) + tex(core) + tex(close).repeat(n);
/** Une liste imbriquée de `levels` niveaux, puis un élément au fond qui porte `latex` en ligne. */
const deepList = (levels: number, latex: string) =>
  `${Array.from({ length: levels }, (_, i) => `${"  ".repeat(i)}- niveau ${i}`).join("\n")}\n${"  ".repeat(levels)}- en ligne $${latex}$\n`;

/**
 * Des formules imbriquées sans table (ou presque) : la garde ne comptait que les tables de KaTeX, et
 * chacune faisait planter l'onglet de qui ouvrait la page (relecture de C2, C1). Chacune est prise un
 * peu au-dessus du seuil de plantage mesuré dans le Chromium de Playwright (registre, R525), à sa place.
 */
const CRASHING: [string, string][] = [
  // 86 caractères au fond d'une liste de 25 niveaux.
  ["mathllap_deep_in_a_list", deepList(25, nest("@mathllap{", "}", 8))],
  ["mathllap_inline", `Avant $${nest("@mathllap{", "}", 16)}$ après\n`],
  ["braces", `$$\n${nest("{", "a}", 300)}\n$$\n`],
  ["smash_inline", `Avant $${nest("a@smash{", "}", 100)}$ après\n`],
  ["vphantom", `$$\n${nest("a@vphantom{", "}", 38)}\n$$\n`],
  ["mathreflectbox", `$$\n${nest("@mathreflectbox{", "}", 34)}\n$$\n`],
  ["color", `$$\n${nest("@color{red}{a", "}", 145)}\n$$\n`],
];

for (const [form, mmd] of CRASHING) {
  test(`a_formula_nested_without_tables_never_crashes_the_tab__${form}`, async ({ browser, request }) => {
    const page = await createPage(request, { title: `Imbriquée ${form} ${unique()}` });
    // Une formule ordinaire d'abord, qui doit s'afficher : la page est vivante, et la garde ne
    // refuse que la formule trop profonde.
    await putMmd(request, page.id, `$$\n\\frac{a}{b} + \\sqrt{2}\n$$\n\n${mmd}`);
    const opened = await openWatchingCrash(browser, page.id);
    const { p } = opened;
    // Le dernier bloc de la page d'abord, qui existe toujours : une formule n'est mise en forme
    // qu'à portée de l'écran (400 px), et `.mm-tex-why` n'existe qu'une fois qu'elle l'est. Au
    // fond de la liste, elle était à 54 px de ne jamais y venir, et l'attente durait jusqu'au
    // délai du test (Ruling R1203).
    await p.locator(".mm-editor .bn-block-content").last().scrollIntoViewIfNeeded();
    await p.locator(".mm-editor .mm-tex-why").scrollIntoViewIfNeeded();
    await expect(p.locator(".mm-editor .mm-tex-why")).toHaveText("Formule trop imbriquée pour être affichée ici.");
    await expect(p.locator(".mm-editor .mm-equation").first().locator(".katex")).toBeVisible();
    expect(opened.crashed()).toBe(false);
    await opened.close();
  });
}

test("a_formula_allowed_at_the_top_is_text_deep_in_a_list", async ({ browser, request }) => {
  // La part « profondeur de la page » de la garde (relecture de C2, I2) : la même formule est mise en
  // forme en haut de la page, et montrée en texte au fond d'une liste de 25 niveaux, où elle ferait
  // planter l'onglet si la profondeur de la page n'était pas comptée.
  const formula = nest("@mathreflectbox{", "}", 18);
  const page = await createPage(request, { title: `Profondeur ${unique()}` });
  await putMmd(request, page.id, `$$\n${formula}\n$$\n\n${deepList(25, formula)}`);
  const opened = await openWatchingCrash(browser, page.id);
  const { p } = opened;
  await expect(p.locator(".mm-editor .mm-equation .katex")).toBeVisible();
  const deep = p.locator(".mm-editor .mm-math");
  await deep.scrollIntoViewIfNeeded();
  await expect(deep.locator(".mm-tex-why")).toHaveText("Formule trop imbriquée pour être affichée ici.");
  // Elle est bien au fond de la page : plus de 90 éléments au-dessus d'elle.
  const depth = await deep.locator(".mm-tex-refused").evaluate((element) => {
    let count = 0;
    for (let at: Element | null = element; at !== null; at = at.parentElement) count += 1;
    return count;
  });
  expect(depth).toBeGreaterThan(90);
  expect(opened.crashed()).toBe(false);
  await opened.close();
});

test("six_nested_fractions_are_typeset_at_the_top_of_a_page", async ({ browser, request }) => {
  // Les fractions coûtent moins que les boîtes de `\mathreflectbox` (R551) : six fractions
  // imbriquées, qui s'affichaient avant la Task C2 (le plantage est à treize), s'affichent en haut
  // d'une page, en bloc et en ligne.
  const page = await createPage(request, { title: `Six fractions ${unique()}` });
  await putMmd(request, page.id, `$$\n${frac(6)}\n$$\n\nEn ligne $${frac(6)}$ fin\n`);
  const opened = await openWatchingCrash(browser, page.id);
  const { p } = opened;
  await expect(p.locator(".mm-editor .mm-equation .katex")).toBeVisible();
  await expect(p.locator(".mm-editor .mm-math .katex")).toBeVisible();
  await expect(p.locator(".mm-editor .mm-tex-why")).toHaveCount(0);
  expect(opened.crashed()).toBe(false);
  await opened.close();
});

test("formulas_never_shown_are_printed_typeset", async ({ browser, request }) => {
  // Une formule n'est mise en forme qu'à l'écran : à l'impression, celles qui n'y étaient jamais
  // venues s'imprimaient en texte source (re-relecture de C2, N3).
  const page = await createPage(request, { title: `Impression ${unique()}` });
  const formulas = Array.from({ length: 40 }, (_, i) => `$$\n\\sum_{k=1}^{${i + 1}} k^2\n$$`);
  await putMmd(request, page.id, `${formulas.join("\n\n")}\n`);
  const p = await openPage(browser, page.id);
  await expect(p.locator(".mm-editor .mm-equation .katex").first()).toBeVisible();
  expect(await p.locator(".mm-editor .mm-tex-pending").count()).toBeGreaterThan(0);
  await p.emulateMedia({ media: "print" });
  await expect(p.locator(".mm-editor .mm-tex-pending")).toHaveCount(0);
  await expect(p.locator(".mm-editor .mm-equation .katex")).toHaveCount(40);
});

test("a_formula_that_defines_never_freezes_the_tab", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Définitions ${unique()}` });
  // Dix fois dix appels d'une macro qui tape 3 000 caractères : 300 000 lettres à mettre en page ; et
  // la même cachée derrière `\verb`.
  const bomb = `\\def\\z{${"a".repeat(3000)}}\\def\\y{${"\\z".repeat(10)}}\\def\\x{${"\\y".repeat(10)}}\\x`;
  await putMmd(request, page.id, `$$\n${bomb}\n$$\n\n$$\n\\verb|x|${bomb}\n$$\n\nAprès\n`);
  const started = Date.now();
  const opened = await openWatchingCrash(browser, page.id);
  const { p } = opened;
  await expect(p.locator(".mm-editor .mm-tex-why")).toHaveCount(2);
  await expect(p.locator(".mm-editor .mm-tex-why").first()).toHaveText(
    "Les définitions (\\def, \\newcommand…) ne sont pas permises dans une formule.",
  );
  expect(Date.now() - started).toBeLessThan(10_000);
  // L'onglet répond : un aller-retour de la page prend moins d'une seconde.
  const t0 = Date.now();
  await p.evaluate(() => new Promise((done) => requestAnimationFrame(() => done(null))));
  expect(Date.now() - t0).toBeLessThan(1_000);
  await opened.close();
});
