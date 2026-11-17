// Nos règles de style dans l'éditeur, telles que Chromium les calcule (R763). La feuille de BlockNote
// arrive avec le code de l'éditeur, après `theme.css` : à spécificité égale, elle l'emporte ; et sa
// règle `.bn-default-styles p, h1…h6, li { margin: 0; padding: 0 }` (0,1,1) passe devant une règle
// d'une seule classe. Les entrées de la table des matières (des `li`) n'étaient donc pas décalées
// par niveau, et le texte d'une table vide (un `p`) perdait sa marge. Ici, la géométrie des entrées,
// puis une liste fermée de règles des blocs riches, lues dans le navigateur.
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, createPage, openPage, putMmd, unique } from "./helpers";

test.afterEach(closeContexts);

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
