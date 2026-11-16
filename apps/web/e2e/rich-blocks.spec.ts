// Les blocs riches de la partie B du lot 3 (Task C1b) : le callout et le signet, posés par le menu
// « / », réglés dans leur fenêtre (tenue par l'id du bloc, rendue hors du document), vus par
// l'autre personne de la page, et relus en MMD par le serveur. Un lecteur les voit sans pouvoir
// les régler. Puis (Task C3) la table des matières, qui suit les titres tapés par l'autre, et le
// lien vers une page, posé par un sélecteur, dont le titre suit la page pour qui la voit, et reste
// le titre copié pour qui ne la voit pas.
import { expect, test, type Page } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  getMmd,
  guestOf,
  newUser,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  setPageLevel,
  signUpAccount,
  titleOf,
  unique,
  waitSynced,
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

test("typing_after_a_click_under_the_icon_of_a_callout_is_never_lost", async ({ browser, request }) => {
  // L'icône d'un callout est un îlot non modifiable (R653). Modifiable, un clic sous elle y posait le
  // curseur du navigateur : la frappe qui suivait n'arrivait pas tout entière au document (« K5 »
  // tapé, le « K » écrit nulle part).
  const page = await createPage(request, { title: `Icône ${unique()}` });
  await putMmd(request, page.id, 'Avant\n\n<callout icon="⚠️" color="blue">\nAstuce du jour\n</callout>\n\nAprès\n');
  const p = await openPage(browser, page.id);
  const callout = (await p.locator(".mm-editor .mm-callout").boundingBox())!;
  const icon = (await p.locator(".mm-editor .mm-callout-icon").boundingBox())!;
  await p.mouse.click(icon.x + icon.width / 2, callout.y + callout.height - 2);
  await p.keyboard.type("K5");
  await expect.poll(() => getMmd(request, page.id)).toContain("K5");
  // Rien n'est montré hors du texte du callout, et l'icône est restée elle-même.
  await expect(p.locator(".mm-editor .mm-callout-icon")).toHaveText("⚠️");
  const stray = await p.evaluate(() => {
    const copy = document.querySelector(".mm-editor .mm-callout")!.cloneNode(true) as HTMLElement;
    copy.querySelector(".mm-callout-icon")?.remove();
    copy.querySelector(".mm-callout-content")?.remove();
    return copy.textContent;
  });
  expect(stray).toBe("");
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

// Task C3 : table des matières et lien vers une page.

/** Les entrées de la table des matières de la page ouverte. */
function tocEntries(p: Page) {
  return p.locator(".mm-editor").getByRole("navigation", { name: "Table des matières" }).getByRole("link");
}

/** Le lien de page de l'éditeur. */
function pageLinkOf(p: Page) {
  return p.locator(".mm-editor .mm-page-link");
}

test("toc_follows_headings_typed_by_someone_else", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Sommaire");
  await putMmd(aliceApi, page.id, "<toc/>\n\n# Introduction\n\n<p/>\n");
  await expect(tocEntries(mona)).toHaveText(["Introduction"]);
  await expect(tocEntries(tom)).toHaveText(["Introduction"]);

  // Tom tape un titre de niveau 2 sous l'introduction : la table de Mona le suit, à sa profondeur.
  await tom.locator(".mm-editor .bn-block-content").last().click();
  await tom.keyboard.type("## Contexte");
  await expect(tocEntries(mona)).toHaveText(["Introduction", "Contexte"]);
  const levels = mona.locator(".mm-editor .mm-toc li");
  expect(await levels.evaluateAll((items) => items.map((li) => (li as HTMLElement).dataset.level))).toEqual(["1", "2"]);

  // Un lien de la table met le curseur au début de son titre : la frappe y entre.
  await tocEntries(mona).filter({ hasText: "Contexte" }).click();
  await expect(mona.locator(".mm-editor .bn-editor")).toBeFocused();
  await mona.keyboard.type("Le ");
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("<toc/>\n\n# Introduction\n\n## Le Contexte\n");
  await expect(tocEntries(tom)).toHaveText(["Introduction", "Le Contexte"]);
});

test("page_link_opens_the_page_and_reads_back", async ({ browser, request }) => {
  const tag = unique();
  const plan = await createPage(request, { title: `Plan ${tag}` });
  const notes = await createPage(request, { title: `Notes ${tag}` });
  const p = await openPage(browser, notes.id);
  await typeInFirstBlock(p, "");
  await slash(p, "lien vers", "Lien vers une page");

  // Le sélecteur prend le focus ; rien n'est écrit tant qu'on cherche.
  const picker = blockWindow(p, "Lien vers une page");
  const field = picker.getByRole("combobox", { name: "Chercher une page" });
  await expect(field).toBeFocused();
  await p.keyboard.type(`Plan ${tag}`);
  await expect(picker.getByRole("option")).toHaveText([`Plan ${tag}`]);
  expect(await getMmd(request, notes.id)).toBe("<p/>\n");
  await p.keyboard.press("Enter");
  await expect(picker).toBeHidden();

  // Le lien est posé, avec l'id de la page et son titre ; la frappe reprend dans le texte qui suit.
  await expect(pageLinkOf(p).getByRole("link")).toHaveText(`Plan ${tag}`);
  await expect(p.locator(".mm-editor .bn-editor")).toBeFocused();
  await p.keyboard.type("La suite");
  await expect.poll(() => getMmd(request, notes.id)).toBe(`<page id="${plan.id}">Plan ${tag}</page>\n\nLa suite\n`);

  // Un clic ouvre la page, sans recharger l'application.
  await p.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));
  await pageLinkOf(p).getByRole("link").click();
  await expect(p).toHaveURL(new RegExp(`/p/${plan.id}$`));
  await expect(titleOf(p)).toHaveValue(`Plan ${tag}`);
  expect(await p.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBe(true);
});

test("renamed_page_shows_its_new_title_in_the_link", async ({ browser, request }) => {
  const tag = unique();
  const plan = await createPage(request, { title: `Plan ${tag}` });
  const notes = await createPage(request, { title: `Notes ${tag}` });
  await putMmd(request, notes.id, `<page id="${plan.id}">Plan ${tag}</page>\n`);
  const p = await openPage(browser, notes.id);
  await expect(pageLinkOf(p).getByRole("link")).toHaveText(`Plan ${tag}`);
  // Renommée ailleurs : le lien suit (l'arbre relu par le canal de l'espace) ; le document garde le
  // titre copié.
  const res = await request.patch(`/api/v1/pages/${plan.id}`, { data: { title: `Plan 2027 ${tag}` } });
  expect(res.status(), await res.text()).toBe(200);
  await expect(pageLinkOf(p).getByRole("link")).toHaveText(`Plan 2027 ${tag}`);
  expect(await getMmd(request, notes.id)).toBe(`<page id="${plan.id}">Plan ${tag}</page>\n`);
});

test("guest_reads_the_link_label_of_a_page_he_cannot_see", async ({ browser }) => {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const tag = unique();
  const plan = await createPage(aliceApi, { title: `Plan ${tag}` });
  const notes = await createPage(aliceApi, { title: `Notes ${tag}` });
  await putMmd(aliceApi, notes.id, `Voir :\n\n<page id="${plan.id}">Plan ${tag}</page>\n`);
  // Gina n'est invitée que sur « Notes » ; « Plan » est ensuite renommé.
  const gina = await guestOf(alice, notes.id, `Gina ${unique()}`, "read");
  const secret = `Plan secret ${tag}`;
  const renamed = await aliceApi.patch(`/api/v1/pages/${plan.id}`, { data: { title: secret } });
  expect(renamed.status(), await renamed.text()).toBe(200);

  /** Tout ce que reçoit un onglet : les réponses de l'API et les trames de ses WebSocket. */
  const listen = (p: Page) => {
    const received: string[] = [];
    p.on("response", (response) => {
      if (!response.url().includes("/api/")) return;
      void response.text().then(
        (text) => received.push(text),
        () => {},
      );
    });
    p.on("websocket", (socket) => {
      socket.on("framereceived", ({ payload }) =>
        received.push(typeof payload === "string" ? payload : payload.toString("utf8")),
      );
    });
    return received;
  };

  // Témoin : Alice, qui voit « Plan », lit son titre à jour dans le lien, et l'a reçu.
  const a = await newUser(browser, alice);
  const toAlice = listen(a);
  await a.goto(`/p/${notes.id}`);
  await waitSynced(a);
  await expect(pageLinkOf(a).getByRole("link")).toHaveText(secret);
  await expect.poll(() => toAlice.some((text) => text.includes(secret))).toBe(true);

  // Gina lit le libellé copié, et rien de ce qu'elle reçoit ne porte le titre à jour.
  const g = await newUser(browser, gina);
  const toGina = listen(g);
  await g.goto(`/p/${notes.id}`);
  await waitSynced(g);
  await expect(pageLinkOf(g).getByRole("link")).toHaveText(`Plan ${tag}`);
  // Témoin de l'écoute : ce qu'elle voit est bien passé par là (le libellé copié, dans le document).
  await expect.poll(() => toGina.some((text) => text.includes(`Plan ${tag}`))).toBe(true);
  // Le lien mène à « Page introuvable » : la page ne lui est pas montrée.
  await pageLinkOf(g).getByRole("link").click();
  await expect(g).toHaveURL(new RegExp(`/p/${plan.id}$`));
  await expect(g.getByRole("heading", { name: "Page introuvable" })).toBeVisible();
  expect(await g.content()).not.toContain(secret);
  expect(toGina.filter((text) => text.includes(secret))).toEqual([]);
});
