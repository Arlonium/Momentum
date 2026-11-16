// Les blocs riches de la partie B du lot 3 (Task C1b) : le callout et le signet, posés par le menu
// « / », réglés dans leur fenêtre (tenue par l'id du bloc, rendue hors du document), vus par
// l'autre personne de la page, et relus en MMD par le serveur. Un lecteur les voit sans pouvoir
// les régler.
import { expect, test, type Page } from "@playwright/test";
import {
  closeContexts,
  createPage,
  getMmd,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  setPageLevel,
  unique,
} from "./helpers";

test.afterEach(closeContexts);

/** Mona (propriétaire de l'espace) et Tom (membre), et une page de « Espace », vide. */
async function duo(browser: Parameters<typeof openPage>[0], title: string) {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `${title} ${unique()}` });
  const mona = await openPage(browser, page.id, team.alice);
  const tom = await openPage(browser, page.id, team.bruno);
  return { ...team, page, mona, tom };
}

/** Le premier bloc de l'éditeur, où l'on tape. */
async function typeInFirstBlock(p: Page, text: string): Promise<void> {
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.type(text);
}

/** Choisit, par le menu « / », l'élément `title` (tapé, puis Entrée). */
async function slash(p: Page, query: string, title: string): Promise<void> {
  await p.keyboard.type(`/${query}`);
  await expect(p.locator(".bn-suggestion-menu-item").first()).toContainText(title);
  await p.keyboard.press("Enter");
}

function blockWindow(p: Page, label: string) {
  return p.getByRole("dialog", { name: label });
}

test("callout_is_inserted_set_and_read_back", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Callout");
  await typeInFirstBlock(mona, "");
  await slash(mona, "callout", "Callout");
  await mona.keyboard.type("Lire avant de commencer");

  // L'icône, puis la couleur, dans la fenêtre du callout : au clavier.
  const iconButton = mona.getByRole("button", { name: "Icône et couleur du callout" });
  await iconButton.click();
  const settings = blockWindow(mona, "Icône et couleur du callout");
  await expect(settings).toBeVisible();
  await expect(settings.getByRole("option", { name: "💡" })).toBeFocused();
  await mona.keyboard.press("ArrowRight");
  await expect(settings.getByRole("option", { name: "⚠️" })).toBeFocused();
  await mona.keyboard.press("Enter");
  await settings.getByRole("option", { name: "Bleu" }).click();
  await mona.keyboard.press("Escape");
  await expect(settings).toBeHidden();
  await expect(iconButton).toBeFocused();

  // Tom les voit.
  const seen = tom.locator(".mm-editor .mm-callout");
  await expect(seen).toHaveClass(/mm-callout-blue/);
  await expect(seen.locator(".mm-callout-glyph")).toHaveText("⚠️");
  await expect(seen).toContainText("Lire avant de commencer");

  // Le serveur relit le callout en MMD, exactement ; récrit tel quel, il laisse la page telle quelle.
  const expected = '<callout icon="⚠️" color="blue">\nLire avant de commencer\n</callout>\n';
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe(expected);
  await putMmd(aliceApi, page.id, expected);
  expect(await getMmd(aliceApi, page.id)).toBe(expected);
  await expect(seen).toHaveClass(/mm-callout-blue/);
  await expect(mona.locator(".mm-editor .mm-callout .mm-callout-glyph")).toHaveText("⚠️");
  await expect(mona.locator(".mm-editor .mm-callout")).toContainText("Lire avant de commencer");
});

test("bookmark_is_inserted_and_read_back", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Signet");
  await typeInFirstBlock(mona, "");
  await slash(mona, "signet", "Signet");

  // Inséré vide, il ouvre sa fenêtre sur son champ d'adresse.
  const settings = blockWindow(mona, "Adresse du signet");
  const field = settings.getByRole("textbox", { name: "Adresse de la page" });
  await expect(field).toBeFocused();
  await mona.keyboard.type("javascript:alert(1)");
  await mona.keyboard.press("Enter");
  await expect(settings.getByRole("alert")).toHaveText("Une adresse commence par http:// ou https://");
  await field.fill("https://mementum.dev");
  await mona.keyboard.press("Enter");
  await expect(settings).toBeHidden();
  // Posé depuis le texte, le signet y rend le focus (R474) : un bloc sans texte posé par « / » est
  // suivi d'un paragraphe vide, où passe le curseur (R308), et la frappe y reprend.
  await expect(mona.locator(".mm-editor .bn-editor")).toBeFocused();
  await mona.keyboard.type("La suite");
  await expect
    .poll(() => getMmd(aliceApi, page.id))
    .toBe('<bookmark url="https://mementum.dev/"/>\n\nLa suite\n');
  const card = tom.locator(".mm-editor .mm-bookmark");
  await expect(card.locator(".mm-bookmark-host")).toHaveText("mementum.dev");
  await expect(card.getByRole("link")).toHaveAttribute("href", "https://mementum.dev/");
  await expect(card.getByRole("link")).toHaveAttribute("rel", "noopener noreferrer");
});

test("rich_blocks_are_read_only_for_a_reader", async ({ browser }) => {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, {
    spaceId: team.space.id,
    section: "private",
    title: `Lecture ${unique()}`,
  });
  await putMmd(
    team.aliceApi,
    page.id,
    '<callout icon="⚠️" color="blue">\nÀ lire\n</callout>\n\n<bookmark url="https://mementum.dev/"/>\n',
  );
  await setPageLevel(team.aliceApi, page.id, team.brunoId, "read");
  const reader = await openPage(browser, page.id, team.bruno);
  await expect(reader.locator(".mm-editor .mm-callout .mm-callout-glyph")).toHaveText("⚠️");
  await expect(reader.locator(".mm-editor .mm-bookmark-host")).toHaveText("mementum.dev");
  await expect(reader.getByRole("button", { name: "Icône et couleur du callout" })).toHaveCount(0);
  await expect(reader.getByRole("button", { name: "Modifier le signet" })).toHaveCount(0);
  // Ctrl+Alt+Entrée n'ouvre rien non plus.
  await reader.locator(".mm-editor .mm-callout-content").click();
  await reader.keyboard.press("Control+Alt+Enter");
  await expect(reader.getByRole("dialog")).toHaveCount(0);
  // Témoin : la propriétaire, sur la même page, a les deux boutons.
  const owner = await openPage(browser, page.id, team.alice);
  await expect(owner.getByRole("button", { name: "Icône et couleur du callout" })).toBeVisible();
  await expect(owner.getByRole("button", { name: "Modifier le signet" })).toBeVisible();
});

test("ctrl_alt_enter_sets_the_rich_block_at_the_cursor", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Clavier ${unique()}` });
  await putMmd(request, page.id, '<callout icon="💡" color="default">\nAstuce\n</callout>\n\nAprès\n');
  const p = await openPage(browser, page.id);
  await p.locator(".mm-editor .mm-callout-content").click();
  await p.keyboard.press("End");
  await p.keyboard.press("Control+Alt+Enter");
  const settings = blockWindow(p, "Icône et couleur du callout");
  await expect(settings.getByRole("option", { name: "💡" })).toBeFocused();
  await p.keyboard.press("Tab");
  await expect(settings.getByRole("option", { name: "Par défaut" })).toBeFocused();
  await p.keyboard.press("ArrowDown");
  await p.keyboard.press("Enter");
  await expect(p.locator(".mm-editor .mm-callout")).toHaveClass(/mm-callout-gray/);
  // Échap rend le focus au texte du callout : la frappe y reprend.
  await p.keyboard.press("Escape");
  await expect(settings).toBeHidden();
  await p.keyboard.type(" du jour");
  await expect(p.locator(".mm-editor .mm-callout")).toContainText("Astuce du jour");
});

test("block_window_stays_inside_the_screen", async ({ browser, request }) => {
  const page = await createPage(request, { title: `Écran ${unique()}` });
  const filler = Array.from({ length: 40 }, (_, i) => `Paragraphe ${i + 1}`).join("\n\n");
  await putMmd(request, page.id, `${filler}\n\n<bookmark url="https://mementum.dev/"/>\n\nFin\n`);
  const p = await openPage(browser, page.id);
  const modify = p.getByRole("button", { name: "Modifier le signet" });
  // Le signet en bas de l'écran : la fenêtre n'a pas la place au-dessous.
  await modify.evaluate((button) => button.scrollIntoView({ block: "end" }));
  await modify.click();
  const settings = blockWindow(p, "Adresse du signet");
  await expect(settings).toBeVisible();
  const box = (await settings.boundingBox())!;
  const viewport = p.viewportSize()!;
  expect(box.y).toBeGreaterThanOrEqual(0);
  expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
  expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
  // Elle passe au-dessus du bouton, qui reste visible.
  const button = (await modify.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(button.y);
});

test("block_window_stays_open_when_someone_else_moves_its_block", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Déplacé");
  await putMmd(aliceApi, page.id, 'Avant\n\n<bookmark url="https://mementum.dev/"/>\n\nAprès\n');
  await expect(mona.getByRole("button", { name: "Modifier le signet" })).toBeVisible();
  await expect(tom.locator(".mm-editor .mm-bookmark")).toBeVisible();
  await mona.getByRole("button", { name: "Modifier le signet" }).click();
  const settings = blockWindow(mona, "Adresse du signet");
  const field = settings.getByRole("textbox", { name: "Adresse de la page" });
  await expect(field).toBeFocused();
  await mona.keyboard.press("End");
  await mona.keyboard.type("do");

  // Tom descend le signet d'une place (supprimé et récrit au même id) ; Mona le voit bouger.
  await tom.evaluate(() => {
    const editor = window.__mementum!.editor;
    const bookmark = editor.document.find((b) => b.type === "bookmark")!;
    editor.setTextCursorPosition(bookmark);
    editor.moveBlocksDown();
  });
  await expect
    .poll(() => mona.evaluate(() => window.__mementum!.editor.document.map((b) => b.type)))
    .toEqual(["paragraph", "paragraph", "bookmark"]);

  // La fenêtre est restée, le focus aussi ; la frappe va au champ, pas à la page.
  await expect(field).toBeFocused();
  await mona.keyboard.type("c");
  await expect(field).toHaveValue("https://mementum.dev/doc");
  await mona.keyboard.press("Enter");
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe(
    'Avant\n\nAprès\n\n<bookmark url="https://mementum.dev/doc"/>\n',
  );
});

/** Ce qui a le focus dans l'onglet : `BODY`, ou l'élément et sa classe. */
function focused(p: Page): Promise<string> {
  return p.evaluate(() => {
    const active = document.activeElement;
    return active === null || active === document.body ? "BODY" : `${active.tagName}.${active.className}`;
  });
}

test("block_window_and_its_text_survive_the_page_turning_read_only", async ({ browser }) => {
  // Bruno remplit la fenêtre d'un signet ; Alice le baisse à « Peut lire » : la session de son onglet
  // est recréée, avec un éditeur neuf. La fenêtre reste, avec son texte, en lecture seule ; le droit
  // revenu, le texte s'enregistre (R471, R472).
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, {
    spaceId: team.space.id,
    section: "private",
    title: `Lecture seule ${unique()}`,
  });
  await putMmd(team.aliceApi, page.id, 'Avant\n\n<bookmark url="https://mementum.dev/"/>\n\nAprès\n');
  await setPageLevel(team.aliceApi, page.id, team.brunoId, "edit");
  const bruno = await openPage(browser, page.id, team.bruno);
  await bruno.getByRole("button", { name: "Modifier le signet" }).click();
  const settings = blockWindow(bruno, "Adresse du signet");
  const field = settings.getByRole("textbox", { name: "Adresse de la page" });
  await expect(field).toBeFocused();
  await bruno.keyboard.press("End");
  await bruno.keyboard.type("doc-en-cours");

  await setPageLevel(team.aliceApi, page.id, team.brunoId, "read");
  await expect(readerBanner(bruno)).toBeVisible();
  await expect(settings).toBeVisible();
  await expect(field).toHaveValue("https://mementum.dev/doc-en-cours");
  await expect(field).toHaveAttribute("readonly", "");
  await expect(settings.getByRole("status")).toHaveText(
    "Tu ne peux pas modifier cette page pour l'instant : ton texte reste ici, à copier.",
  );
  await expect.poll(() => focused(bruno)).toContain("INPUT");
  // Rien ne part : ni la frappe, ni Entrée.
  await bruno.keyboard.type("xyz");
  await bruno.keyboard.press("Enter");
  await expect(field).toHaveValue("https://mementum.dev/doc-en-cours");
  // Échap : le focus va au bouton du signet, qui reste pour le lecteur tant qu'un texte non
  // enregistré l'attend, et rouvre la fenêtre, le texte à copier (R472).
  await bruno.keyboard.press("Escape");
  await expect(settings).toBeHidden();
  const unsaved = bruno.getByRole("button", { name: "Texte non enregistré du signet" });
  await expect(unsaved).toBeFocused();
  await bruno.keyboard.press("Enter");
  await expect(field).toHaveValue("https://mementum.dev/doc-en-cours");
  await expect(field).toHaveAttribute("readonly", "");
  await bruno.keyboard.press("Escape");
  await expect(settings).toBeHidden();
  expect(await getMmd(team.aliceApi, page.id)).toBe('Avant\n\n<bookmark url="https://mementum.dev/"/>\n\nAprès\n');

  // Le droit revient ; Bruno rouvre la page (le bandeau le lui offre) : la fenêtre rouverte retrouve
  // le texte tapé, qui s'enregistre.
  await setPageLevel(team.aliceApi, page.id, team.brunoId, "edit");
  await bruno
    .getByRole("status")
    .filter({ hasText: "Tu peux maintenant modifier cette page" })
    .getByRole("button", { name: "Rouvrir la page" })
    .click();
  const modify = bruno.getByRole("button", { name: "Modifier le signet" });
  await expect(modify).toBeVisible();
  await modify.click();
  await expect(field).toHaveValue("https://mementum.dev/doc-en-cours");
  await bruno.keyboard.press("Enter");
  await expect
    .poll(() => getMmd(team.aliceApi, page.id))
    .toBe('Avant\n\n<bookmark url="https://mementum.dev/doc-en-cours"/>\n\nAprès\n');
});

test("escape_after_someone_else_changed_the_block_gives_the_focus_to_the_text", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Changé");
  await putMmd(aliceApi, page.id, 'Avant\n\n<bookmark url="https://mementum.dev/"/>\n\nAprès\n');
  await expect(tom.locator(".mm-editor .mm-bookmark")).toBeVisible();
  await mona.getByRole("button", { name: "Modifier le signet" }).click();
  const settings = blockWindow(mona, "Adresse du signet");
  await expect(settings.getByRole("textbox", { name: "Adresse de la page" })).toBeFocused();
  // Tom change le signet en paragraphe : son bouton part.
  await tom.evaluate(() => {
    const editor = window.__mementum!.editor;
    const bookmark = editor.document.find((b) => b.type === "bookmark")!;
    editor.updateBlock(bookmark, { type: "paragraph" });
  });
  await expect(settings.getByRole("status")).toContainText("n'est plus dans la page");
  await mona.keyboard.press("Escape");
  await expect(settings).toBeHidden();
  // Le focus est dans l'éditeur, le curseur à la fin du bloc changé (pas à la dernière sélection,
  // ici le haut de la page), et la frappe y entre.
  expect(await focused(mona)).toContain("bn-editor");
  await mona.keyboard.type("repris");
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("Avant\n\nrepris\n\nAprès\n");
});
