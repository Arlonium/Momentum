// Garde des attributs (Rulings R695, R705) : rien de ce que l'éditeur écrit en collant n'est refusé,
// ni par son greffon (`attrBound.ts`), ni par le serveur. Le tour de correction 1 a trouvé ici le
// collage d'une cellule fusionnée (`colwidth = [NaN, NaN]`) ; ces tests collent, dans un vrai
// navigateur, les fixtures de BlockNote (le HTML que l'éditeur met au presse-papiers) et
// des HTML de tableur et de traitement de texte, et affirment : aucun refus du greffon, et la
// frappe qui suit arrive au serveur (une connexion fermée en boucle la retiendrait).
import { expect, test, type Page } from "@playwright/test";
import type {} from "../src/editor/debug";
import { apiAs, closeContexts, createPage, defaultAccount, getMmd, openPage, unique, waitSynced } from "./helpers";

test.afterEach(closeContexts);

/** Colle `data` (types du presse-papiers) à la fin du premier bloc de la page. */
async function paste(p: Page, data: Record<string, string>): Promise<void> {
  await p.evaluate((data) => {
    const editor = window.__mementum!.editor as unknown as {
      document: { id: string }[];
      setTextCursorPosition(id: string, at: "end"): void;
    };
    editor.setTextCursorPosition(editor.document[0].id, "end");
    const transfer = new DataTransfer();
    for (const [type, value] of Object.entries(data)) transfer.setData(type, value);
    document
      .querySelector(".bn-editor")!
      .dispatchEvent(new ClipboardEvent("paste", { clipboardData: transfer, bubbles: true, cancelable: true }));
  }, data);
}

/** Aucun refus du greffon, et ce qu'on tape ensuite arrive au serveur. */
async function nothingRefused(p: Page, api: Parameters<typeof getMmd>[0], pageId: string, what: string) {
  await p.waitForTimeout(500);
  expect(await p.locator(".mm-attr-refused").allTextContents(), what).toEqual([]);
  const mark = `vivant${unique()}`;
  await p.evaluate(() => {
    const editor = window.__mementum!.editor as unknown as {
      document: { id: string }[];
      setTextCursorPosition(id: string, at: "start"): void;
      focus(): void;
    };
    editor.focus();
    editor.setTextCursorPosition(editor.document[0].id, "start");
  });
  await p.keyboard.type(mark);
  await expect.poll(() => getMmd(api, pageId), { message: what, timeout: 10_000 }).toContain(mark);
}

for (const fixture of ["all-blocks", "non-default", "rich-blocks"]) {
  test(`a_pasted_fixture_is_not_refused: ${fixture}`, async ({ browser }) => {
    const account = defaultAccount();
    const api = await apiAs(account);
    const source = await createPage(api, { title: `Source ${unique()}` });
    const target = await createPage(api, { title: `Collage ${unique()}` });
    const p = await openPage(browser, target.id, account);
    const errors: string[] = [];
    p.on("pageerror", (error) => errors.push(String(error)));

    // La fixture, chargée dans une page, puis copiée en entier par le presse-papiers de l'éditeur.
    const s = await p.context().newPage();
    await s.goto(`/p/${source.id}?fixture=${fixture}`);
    await waitSynced(s);
    await expect.poll(() => s.evaluate(() => window.__mementum!.editor.document.length)).toBeGreaterThan(3);
    // (Une sélection ne finit pas sur un bloc sans texte : le HTML du presse-papiers de
    // BlockNote est demandé à l'éditeur, pour tous les blocs.)
    const copied = await s.evaluate(async () => {
      const editor = window.__mementum!.editor as unknown as {
        document: unknown[];
        blocksToFullHTML(blocks: unknown[]): string | Promise<string>;
      };
      return { "blocknote/html": await editor.blocksToFullHTML(editor.document) };
    });
    expect(copied["blocknote/html"].length).toBeGreaterThan(500);
    await s.close();

    const before = await p.evaluate(() => window.__mementum!.editor.document.length);
    await paste(p, copied);
    await expect.poll(() => p.evaluate(() => window.__mementum!.editor.document.length)).toBeGreaterThan(before + 2);
    await nothingRefused(p, api, target.id, fixture);
    expect(errors).toEqual([]);
  });
}

const HTML: [string, string][] = [
  [
    "un tableau de tableur aux cellules fusionnées et aux largeurs",
    '<table><colgroup><col width="120"><col width="80"><col></colgroup><tbody>' +
      '<tr><td colspan="2" rowspan="2" width="200" style="width:200px">A</td><td>B</td></tr>' +
      "<tr><td>C</td></tr><tr><td>D</td><td>E</td><td>F</td></tr></tbody></table>",
  ],
  [
    "un tableau de traitement de texte, en-têtes et fusion de lignes",
    '<table border="1"><thead><tr><th colspan="3">Titre</th></tr></thead><tbody>' +
      '<tr><td rowspan="2">a</td><td>b</td><td>c</td></tr><tr><td>d</td><td>e</td></tr></tbody></table>',
  ],
  [
    "une image à dimensions",
    '<p>Avant</p><img src="https://exemple.invalid/a.png" width="320" height="200" alt="Une image"><p>Après</p>',
  ],
  ["une liste numérotée qui commence à 5", '<ol start="5"><li>cinq</li><li>six</li></ol>'],
  [
    "des titres et une case cochée",
    '<h2>Deux</h2><h6>Six</h6><ul><li><input type="checkbox" checked>fait</li></ul>',
  ],
];

test("pasted_html_from_a_spreadsheet_or_a_word_processor_is_not_refused", async ({ browser }) => {
  const account = defaultAccount();
  const api = await apiAs(account);
  for (const [what, html] of HTML) {
    const page = await createPage(api, { title: `HTML ${unique()}` });
    const p = await openPage(browser, page.id, account);
    const errors: string[] = [];
    p.on("pageerror", (error) => errors.push(String(error)));
    const before = await getMmd(api, page.id);
    await paste(p, { "text/html": html });
    await expect.poll(() => getMmd(api, page.id), { message: what }).not.toBe(before);
    await nothingRefused(p, api, page.id, what);
    expect(errors, what).toEqual([]);
    await p.close();
  }
});
