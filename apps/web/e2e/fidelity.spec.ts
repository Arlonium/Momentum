// Fidélité de bout en bout entre le vrai BlockNote et le serveur (Task 8) :
// - un document BlockNote (tous les types de blocs, puis des props hors défaut) se relit
//   sur le serveur en MMD exact ;
// - le même MMD écrit au serveur s'ouvre dans le navigateur en exactement le même document ;
// - une frappe dans le navigateur, et des caractères spéciaux tapés (Review Focus 3), font
//   l'aller-retour sans rien changer ;
// - les tableaux fusionnés que le format admet ne sont pas réécrits par le navigateur.
//
// Fixtures : `apps/web/src/editor/fixtures/<nom>.input.json` (l'entrée, chargée par
// `/p/<id>?fixture=<nom>` en développement), puis dans `spec/mmd/fixtures/` le MMD attendu
// (`<nom>.mmd`, écrit à la main), l'état Yjs du navigateur (`yjs/<nom>.bin`, relu par
// `crates/mementum-crdt/tests/blocknote_fixtures.rs`) et `editor.document`
// (`blocknote/<nom>.json`). Les `.bin` et `.json` ne s'écrivent qu'avec `EXPORT_FIXTURES=1` ;
// sans cette variable, le test compare.
import { expect, test, type Browser, type Page } from "@playwright/test";
import { readFileSync, writeFileSync } from "node:fs";
import {
  closeContexts,
  createPage,
  getMmd,
  newUser,
  openPage,
  putMmd,
  unique,
  waitSynced,
  watchWrites,
} from "./helpers";

test.afterEach(closeContexts);

const FIXTURES = ["all-blocks", "non-default"] as const;
type Fixture = (typeof FIXTURES)[number];

const EXPORT = process.env.EXPORT_FIXTURES === "1";
const SPEC = new URL("../../../spec/mmd/fixtures/", import.meta.url);

/** Texte du premier paragraphe de chaque fixture, tel que le MMD l'écrit. */
const FIRST_PARAGRAPH: Record<Fixture, string> = {
  "all-blocks": "Chaque type de bloc, une fois.",
  "non-default": "Paragraphe coloré et centré",
};

function expectedMmd(name: Fixture): string {
  return readFileSync(new URL(`${name}.mmd`, SPEC), "utf8");
}

function committedDocument(name: Fixture): Block[] {
  return JSON.parse(readFileSync(new URL(`blocknote/${name}.json`, SPEC), "utf8")) as Block[];
}

/** Un bloc de `editor.document`, après un aller-retour JSON. */
type Block = { id?: string; children: Block[]; [key: string]: unknown };

/** Les blocs sans leurs ids (ceux des colonnes compris). */
function withoutIds(blocks: Block[]): unknown[] {
  return blocks.map(({ id: _id, children, ...rest }) => ({ ...rest, children: withoutIds(children) }));
}

/** `editor.document` après un aller-retour JSON (`undefined` devient `null`, etc.). */
async function documentOf(p: Page): Promise<Block[]> {
  return JSON.parse(await p.evaluate(() => JSON.stringify(window.__mementum!.editor.document))) as Block[];
}

/** Ouvre une page neuve et y charge la fixture `name`, par `?fixture=<nom>`. */
async function openFixture(browser: Browser, id: string, name: Fixture): Promise<Page> {
  const p = await newUser(browser);
  await p.goto(`/p/${id}?fixture=${name}`);
  await p.waitForFunction(() => window.__mementum?.fixture !== undefined, null, { timeout: 30_000 });
  expect(await p.evaluate(() => window.__mementum!.fixture)).toBe("loaded");
  return p;
}

for (const name of FIXTURES) {
  test(`blocknote_output_matches_committed: ${name}`, async ({ browser, request }) => {
    const { id } = await createPage(request, { title: `Fixture ${name} ${unique()}` });
    const p = await openFixture(browser, id, name);
    const document = await documentOf(p);

    if (EXPORT) {
      const state = await p.evaluate(() => Array.from(window.__mementum!.encodeState()));
      writeFileSync(new URL(`yjs/${name}.bin`, SPEC), Buffer.from(state));
      writeFileSync(new URL(`blocknote/${name}.json`, SPEC), JSON.stringify(document, null, 2) + "\n");
      return;
    }
    expect(withoutIds(document)).toEqual(withoutIds(committedDocument(name)));
    // Le serveur, qui a reçu les mises à jour du navigateur, relit le MMD attendu.
    await expect.poll(() => getMmd(request, id)).toBe(expectedMmd(name));
  });

  test(`server_written_doc_opens_identical: ${name}`, async ({ browser, request }) => {
    const mmd = expectedMmd(name);
    const { id } = await createPage(request, { title: `MMD ${name} ${unique()}` });
    await putMmd(request, id, mmd);
    expect(await getMmd(request, id)).toBe(mmd);

    const p = await newUser(browser);
    const writes = watchWrites(p);
    await p.goto(`/p/${id}`);
    await waitSynced(p);
    const expected = withoutIds(committedDocument(name));
    await expect.poll(async () => withoutIds(await documentOf(p))).toEqual(expected);
    // Ouvrir la page n'y change rien : le navigateur n'écrit pas (y-prosemirror ne
    // supprime ni ne corrige aucun nœud écrit par le serveur).
    await p.waitForTimeout(500);
    expect(writes).toEqual([]);
    expect(await getMmd(request, id)).toBe(mmd);
  });

  test(`browser_edit_reads_back_as_mmd: ${name}`, async ({ browser, request }) => {
    const mmd = expectedMmd(name);
    const paragraph = FIRST_PARAGRAPH[name];
    expect(mmd.split(paragraph), "texte du premier paragraphe unique dans le MMD").toHaveLength(2);
    const { id } = await createPage(request, { title: `Édition ${name} ${unique()}` });
    await putMmd(request, id, mmd);
    const p = await openPage(browser, id);

    // Clic sur le texte du paragraphe, puis `End` : un clic sous le contenu perdrait le
    // premier espace tapé (comportement de BlockNote).
    await p.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
    await p.keyboard.press("End");
    await p.keyboard.type("!");
    await expect.poll(() => getMmd(request, id)).toBe(mmd.replace(paragraph, `${paragraph}!`));
  });
}

/**
 * Tableaux limites que le format admet (Ruling R37) : fusion au bord droit, sur toute la
 * ligne, plus bas que la première ligne ; `rowspan` jusqu'à la dernière ligne ; `colspan`
 * et `rowspan` sur une même cellule ; fusions en cascade ; avec largeurs ou en-têtes.
 */
const EDGE_TABLES =
  [
    "Tableaux limites",
    "| a | B {colspan=2} |\n| --- | --- |\n| c | d | e |",
    "| T {colspan=3} |\n| --- |\n| a | b | c |",
    "| a | B {rowspan=3} |\n| --- | --- |\n| c |\n| d |",
    "| A {colspan=2 rowspan=2} | b |\n| --- | --- |\n| c |\n| d | e | f |",
    "| a | b | c |\n| --- | --- | --- |\n| D {colspan=2} | e |",
    '| A {rowspan=2} | b | c |\n| --- | --- | --- |\n| d | e |\n{widths="120,,80"}',
    "| A {colspan=2} |\n| --- |",
    "| A {colspan=2} | b |\n| --- | --- |\n| C {rowspan=2} | d | e |\n| f | g |\n{header-rows=0}",
    "| A {rowspan=2} | b |\n| --- | --- |\n| c |\n{header-rows=2}",
    "| A {rowspan=2} | B {colspan=2} |\n| --- | --- |\n| C {rowspan=2} | d |\n| e | f |",
  ].join("\n\n") + "\n";

test("edge_tables_survive_an_edit", async ({ browser, request }) => {
  // Une correction de prosemirror-tables (`fixTables`) faite pendant la synchro ne part
  // qu'avec la première édition locale (y-prosemirror) : on tape, puis on relit.
  const { id } = await createPage(request, { title: `Tableaux ${unique()}` });
  await putMmd(request, id, EDGE_TABLES);
  expect(await getMmd(request, id)).toBe(EDGE_TABLES);
  const p = await openPage(browser, id);
  await p.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
  await p.keyboard.press("End");
  await p.keyboard.type("!");
  const expected = EDGE_TABLES.replace("Tableaux limites\n", "Tableaux limites!\n");
  await expect.poll(() => getMmd(request, id)).toBe(expected);
  await p.waitForTimeout(500);
  expect(await getMmd(request, id)).toBe(expected);
});

/** Caractères qui piègent le Markdown, emoji et texte de droite à gauche (Review Focus 3). */
const SPECIAL = "a*b_c \\ <d> & {e} $f$ | ^g [h] ~i~ 🚀 שלום";

/** Le MMD de ce texte, d'après les échappements du §6.2. */
const SPECIAL_MMD = "a\\*b\\_c \\\\ &lt;d> &amp; \\{e\\} \\$f\\$ \\| \\^g \\[h\\] \\~i\\~ 🚀 שלום\n";

/** Texte affiché du premier bloc, et son id. */
async function firstBlock(p: Page): Promise<{ id: string; text: string | null; blocks: number }> {
  return p.evaluate(() => {
    const { editor } = window.__mementum!;
    const inline = document.querySelector(".bn-editor .bn-inline-content");
    return { id: editor.document[0].id, text: inline?.textContent ?? null, blocks: editor.document.length };
  });
}

test("typed_special_characters_roundtrip", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Caractères ${unique()}` });
  const p = await openPage(browser, id);
  await p.locator('[data-content-type="paragraph"] .bn-inline-content').first().click();
  await p.keyboard.press("End");
  await p.keyboard.type(SPECIAL);
  await expect.poll(async () => (await firstBlock(p)).text).toBe(SPECIAL);
  await expect.poll(() => getMmd(request, id)).toBe(SPECIAL_MMD);

  // Le MMD relu (`GET`) puis réécrit tel quel (`PUT`, qui le relit par `parse`) : le
  // serveur écrit un bloc neuf (nouvel id), et rien d'autre ne change.
  const before = await firstBlock(p);
  const mmd = await getMmd(request, id);
  await putMmd(request, id, mmd);
  await expect.poll(async () => (await firstBlock(p)).id).not.toBe(before.id);
  expect(await firstBlock(p)).toMatchObject({ text: SPECIAL, blocks: 1 });
  await expect(p.locator(".bn-editor")).toContainText(SPECIAL);
  expect(await getMmd(request, id)).toBe(mmd);
});
