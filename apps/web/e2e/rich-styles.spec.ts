// Nos règles de style dans l'éditeur, telles que Chromium les calcule (R763). La feuille de BlockNote
// arrive avec le code de l'éditeur, après `theme.css` : à spécificité égale, elle l'emporte ; et sa
// règle `.bn-default-styles p, h1…h6, li { margin: 0; padding: 0 }` (0,1,1) passe devant une règle
// d'une seule classe. Les entrées de la table des matières (des `li`) n'étaient donc pas décalées
// par niveau, et le texte d'une table vide (un `p`) perdait sa marge. Ici, la géométrie des entrées,
// une liste fermée de règles des blocs riches, lues dans le navigateur, puis un relevé de toutes nos
// déclarations sur une page qui porte tous les blocs riches : aucune n'est écrasée par BlockNote.
import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, createPage, openPage, putMmd, unique } from "./helpers";

test.afterEach(closeContexts);

/** La fixture MMD des blocs riches (callouts, formules, signet, table des matières, mentions, fichiers). */
const RICH_BLOCKS = readFileSync(new URL("../../../spec/mmd/fixtures/rich-blocks.mmd", import.meta.url), "utf8");

/** Une page de `request`, écrite en MMD, ouverte. */
async function pageWith(browser: Parameters<typeof openPage>[0], request: Parameters<typeof createPage>[0], mmd: string): Promise<Page> {
  const page = await createPage(request, { title: `Styles ${unique()}` });
  await putMmd(request, page.id, mmd);
  return openPage(browser, page.id);
}

test("toc_entries_are_indented_by_their_level", async ({ browser, request }) => {
  const p = await pageWith(browser, request, "<toc/>\n\n# Un\n\n## Deux\n\n### Trois\n\n#### Quatre\n\n## Cinq\n");
  const links = p.locator(".mm-editor .mm-toc").getByRole("link");
  await expect(links).toHaveText(["Un", "Deux", "Trois", "Quatre", "Cinq"]);
  const lefts = await links.evaluateAll((all) => all.map((a) => a.getBoundingClientRect().left));
  // 18 px par niveau sous le premier ; deux entrées du même niveau commencent au même endroit.
  expect(lefts.map((left) => left - lefts[0])).toEqual([0, 18, 36, 54, 18]);
});

/**
 * Ce que Chromium calcule pour chaque règle de la liste : `[sélecteur dans l'éditeur, propriété,
 * valeur voulue]`. Une règle à nous sur un élément que la feuille de BlockNote règle aussi (un `p`,
 * un `li`, un titre, un lien, un bouton dans `.bn-default-styles`) entre ici.
 */
const RULES: [selector: string, property: string, wanted: string][] = [
  [".mm-toc-level-2", "padding-left", "18px"],
  [".mm-toc-level-3", "padding-left", "36px"],
  [".mm-toc-level-4", "padding-left", "54px"],
  [".mm-toc-link", "padding-left", "6px"],
  [".mm-toc-link", "text-decoration-line", "underline"],
  [".mm-callout", "padding-left", "14px"],
  [".mm-callout", "background-color", "rgb(231, 243, 248)"],
  [".mm-callout-glyph", "font-size", "17px"],
  [".mm-bookmark-host", "font-weight", "500"],
  [".mm-bookmark-url", "font-size", "13px"],
  [".mm-page-link-title", "unicode-bidi", "isolate"],
  [".mm-file-download", "margin-left", "10px"],
  [".mm-file-download", "padding-left", "8px"],
  [".mm-equation .mm-tex", "overflow-y", "auto"],
  [".mm-mention", "font-weight", "500"],
  [".mm-mention-link", "text-decoration-line", "none"],
];

/** Ce que Chromium calcule, pour chaque règle, sur le premier élément de l'éditeur qu'elle vise. */
async function computed(p: Page, rules: typeof RULES): Promise<string[]> {
  return p.evaluate(
    (list) =>
      list.map(([selector, property]) => {
        const element = document.querySelector(`.mm-editor ${selector}`);
        return element === null ? `${selector} absent` : getComputedStyle(element).getPropertyValue(property);
      }),
    rules,
  );
}

test("our_rules_win_over_blocknote_in_rich_blocks", async ({ browser, request }) => {
  const target = await createPage(request, { title: `Cible ${unique()}` });
  const p = await pageWith(
    browser,
    request,
    [
      "<toc/>",
      "## Deux",
      "### Trois",
      "#### Quatre",
      '<callout icon="⚠️" color="blue">\nUne idée\n</callout>',
      '<bookmark url="https://exemple.org/page"/>',
      `<page id="${target.id}">Cible</page>`,
      `<mention type="page" id="${target.id}">Cible</mention> et <mention type="date" id="2026-12-01">1 déc. 2026</mention>`,
      '<file src="https://exemple.org/rapport.pdf" name="rapport.pdf"/>',
      "$$\nx^2\n$$",
    ].join("\n\n") + "\n",
  );
  await expect(p.locator(".mm-editor .mm-equation .mm-tex")).toBeVisible();
  expect(await computed(p, RULES)).toEqual(RULES.map(([, , wanted]) => wanted));
});

/**
 * Les déclarations de `theme.css` que la feuille de BlockNote (ou de Mantine) écrase sur un élément
 * de la page : pour chaque élément visé par l'une de nos règles, la valeur calculée de chaque
 * propriété qu'elle déclare, avec puis sans les feuilles de BlockNote ; une différence est une règle
 * à nous qui perd. Ne sont pas jugées les valeurs qui dépendent de ce dont l'élément hérite
 * (`inherit`, `currentcolor`, une longueur en `em` ou en `%`, une hauteur de ligne sans unité), que
 * les feuilles de BlockNote changent sans rien écraser. Les feuilles se reconnaissent à l'attribut
 * que leur pose le serveur de développement de Vite.
 */
async function overridden(p: Page): Promise<string[]> {
  return p.evaluate(() => {
    const id = (sheet: CSSStyleSheet) => (sheet.ownerNode as Element | null)?.getAttribute("data-vite-dev-id") ?? "";
    const sheets = [...document.styleSheets];
    const blocknote = sheets.filter((sheet) => /blocknote|mantine/i.test(id(sheet)));
    const ours = sheets.filter((sheet) => id(sheet).endsWith("/theme.css"));
    if (blocknote.length === 0 || ours.length !== 1) return [`feuilles introuvables : ${blocknote.length} de BlockNote, ${ours.length} à nous`];
    const rules: CSSStyleRule[] = [];
    const collect = (list: CSSRuleList) => {
      for (const rule of list) {
        if (rule instanceof CSSStyleRule) rules.push(rule);
        else if (rule instanceof CSSMediaRule && matchMedia(rule.conditionText).matches) collect(rule.cssRules);
      }
    };
    collect(ours[0].cssRules);
    const hereditary = (property: string, value: string) =>
      /^(inherit|initial|unset|revert)$/.test(value) ||
      /currentcolor/i.test(value) ||
      /\d(r?em|%|ex|ch|lh)\b/.test(value) ||
      (property === "line-height" && /^[\d.]+$/.test(value));
    const judged = new Map<Element, Map<string, string>>();
    const elements = [...document.querySelectorAll(".mm-editor *, .bn-container *")];
    for (const rule of rules) {
      const selector = rule.selectorText.split(",").map((s) => s.trim()).filter((s) => !s.includes("::")).join(", ");
      if (selector === "") continue;
      for (const element of elements) {
        if (!element.matches(selector)) continue;
        const properties = judged.get(element) ?? new Map<string, string>();
        judged.set(element, properties);
        for (let i = 0; i < rule.style.length; i++) {
          const property = rule.style[i];
          if (property.startsWith("--") || hereditary(property, rule.style.getPropertyValue(property).trim())) continue;
          properties.set(property, rule.selectorText);
        }
      }
    }
    const read = () => new Map([...judged].map(([element, properties]) => {
      const style = getComputedStyle(element);
      return [element, new Map([...properties.keys()].map((property) => [property, style.getPropertyValue(property)]))];
    }));
    const withBlocknote = read();
    for (const sheet of blocknote) sheet.disabled = true;
    const without = read();
    for (const sheet of blocknote) sheet.disabled = false;
    const lost = new Set<string>();
    for (const [element, properties] of judged) {
      for (const [property, selector] of properties) {
        const theirs = withBlocknote.get(element)!.get(property);
        const wanted = without.get(element)!.get(property);
        // L'éditeur lui-même n'a pas notre contour de focus : BlockNote le retire, voulu.
        if (selector === ":focus-visible" && element.classList.contains("bn-editor")) continue;
        if (theirs !== wanted) lost.add(`${selector} { ${property}: ${wanted} } donne ${theirs}`);
      }
    }
    return [...lost];
  });
}

test("no_declaration_of_ours_is_overridden_by_blocknote_on_a_page_of_rich_blocks", async ({ browser, request }) => {
  // Tous les blocs riches de la fixture, des titres aux quatre niveaux, et des états à part : un
  // signet, un fichier et une formule refusés, un lien vers une page qu'on ne voit pas.
  const states = [
    '<bookmark url="javascript:alert(1)"/>',
    '<file src="javascript:alert(1)" name="refusé.pdf"/>',
    '<file src="https://exemple.org/rapport.pdf" name="rapport.pdf"/>',
    "$$\n\\frac{\n$$",
  ];
  const headings = "## Formules\n\n### Niveau trois\n\n#### Niveau quatre";
  const mmd = `${RICH_BLOCKS.replace("## Formules", headings)}\n${states.join("\n\n")}\n`;
  const p = await pageWith(browser, request, mmd);
  await expect(p.locator(".mm-editor .mm-toc-level-4")).toHaveCount(1);
  // Une formule ne se met en forme qu'à l'écran : chacune y passe.
  const formulas = p.locator(".mm-editor .mm-equation, .mm-editor .mm-math");
  for (let i = 0; i < (await formulas.count()); i++) await formulas.nth(i).scrollIntoViewIfNeeded();
  await expect(p.locator(".mm-editor .mm-equation .katex").first()).toBeVisible();
  await expect(p.locator(".mm-editor .mm-math .katex").first()).toBeVisible();
  expect(await overridden(p)).toEqual([]);

  const empty = await pageWith(browser, request, "<toc/>\n");
  await expect(empty.locator(".mm-editor .mm-toc-empty")).toBeVisible();
  expect(await overridden(empty)).toEqual([]);
});

test("an_empty_toc_and_the_mention_menu_keep_their_spacing", async ({ browser, request }) => {
  const p = await pageWith(browser, request, "<toc/>\n\n<p/>\n");
  const empty = p.locator(".mm-editor .mm-toc-empty");
  await expect(empty).toHaveText("Aucun titre dans la page.");
  const spacing: typeof RULES = [
    [".mm-toc-empty", "padding-top", "3px"],
    [".mm-toc-empty", "padding-left", "6px"],
  ];
  expect(await computed(p, spacing)).toEqual(spacing.map(([, , wanted]) => wanted));

  // Le menu `@` (des `li` et un `p`) : rendu par BlockNote hors du texte, il garde nos marges.
  await p.locator(".mm-editor .bn-block-content").last().click();
  await p.keyboard.type("@");
  const menu = p.getByRole("listbox", { name: "Mentionner" });
  await expect(menu.getByRole("option").first()).toBeVisible();
  expect(
    await menu.evaluate((list) => {
      const option = list.querySelector(".mm-mention-option")!;
      const name = list.querySelector(".mm-mention-group-name")!;
      return [getComputedStyle(option).paddingLeft, getComputedStyle(name).paddingTop];
    }),
  ).toEqual(["8px", "6px"]);
});
