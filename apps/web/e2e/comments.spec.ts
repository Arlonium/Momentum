// Les commentaires d'une page (R134) : le panneau, les pastilles de la marge, « Commenter » dans
// le menu d'un bloc, et ce que chaque niveau y peut faire. Rien n'est écrit dans le document :
// le MMD de la page ne change pas, et quelqu'un qui peut commenter sans pouvoir modifier
// commente.
import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";
import type { Thread } from "@mementum/protocol";
import {
  closeContexts,
  createPage,
  getMmd,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  setPageLevel,
  trashBanner,
  unique,
  watchWrites,
} from "./helpers";

test.afterEach(closeContexts);

/** Le contenu des pages de ces tests : trois blocs qu'on retrouve par leur texte. */
const CONTENT = "Premier bloc\n\nDeuxième bloc\n\nTroisième bloc\n";

/**
 * Une page de trois blocs, qu'Alice possède ; Bruno, membre, y a le niveau `level`. Elle est
 * privée : dans « Espace », l'accès général donnerait plus à tout membre.
 */
async function scene(level: "read" | "comment" | "edit") {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, {
    spaceId: team.space.id,
    section: "private",
    title: `Commentaires ${unique()}`,
  });
  await putMmd(team.aliceApi, page.id, CONTENT);
  await setPageLevel(team.aliceApi, page.id, team.brunoId, level);
  return { ...team, page };
}

/** Le bouton « Commentaires » de la barre du haut ; son nom dit combien de fils sont ouverts. */
function commentsButton(p: Page): Locator {
  return p.getByRole("button", { name: /^Commentaires/ });
}

function commentsPanel(p: Page): Locator {
  return p.getByRole("complementary", { name: "Commentaires" });
}

async function openComments(p: Page): Promise<Locator> {
  await commentsButton(p).click();
  const panel = commentsPanel(p);
  await expect(panel).toBeVisible();
  return panel;
}

/** Un bloc de l'éditeur, par le texte qui lui est propre. */
function blockOf(p: Page, text: string): Locator {
  return p
    .locator('.bn-editor [data-node-type="blockOuter"]')
    .filter({ has: p.locator(":scope > .bn-block > .bn-block-content", { hasText: text }) });
}

async function blockIdOf(p: Page, text: string): Promise<string> {
  const id = await blockOf(p, text).getAttribute("data-id");
  if (!id) throw new Error(`pas de bloc « ${text} »`);
  return id;
}

/** La pastille de la marge d'un bloc commenté. */
function markOf(p: Page, text: string): Locator {
  return p.getByRole("list", { name: "Blocs commentés" }).getByRole("button", {
    name: new RegExp(`sur « ${text} »$`),
  });
}

/** Un fil du panneau, par le texte d'un de ses messages. */
function threadOf(scope: Locator, body: string): Locator {
  return scope.getByRole("article").filter({ hasText: body });
}

/** Ouvre le menu de la poignée d'un bloc et y choisit `item`. */
async function blockMenu(p: Page, text: string, item: string): Promise<void> {
  await blockOf(p, text).locator(".bn-block-content").first().hover();
  await p.getByRole("button", { name: "Ouvrir le menu du bloc" }).click();
  await p.getByRole("menuitem", { name: item }).click();
}

/** Ouvre un fil par l'API (`POST …/comments`). */
async function openThread(
  api: APIRequestContext,
  pageId: string,
  blockId: string | null,
  body: string,
): Promise<Thread> {
  const res = await api.post(`/api/v1/pages/${pageId}/comments`, { data: { blockId, body } });
  expect(res.status(), await res.text()).toBe(201);
  return (await res.json()) as Thread;
}

test("comment_on_a_block_reaches_the_other_reader_at_once", async ({ browser }) => {
  const { alice, aliceApi, bruno, page } = await scene("read");
  const before = await getMmd(aliceApi, page.id);
  const writer = await openPage(browser, page.id, alice);
  const reader = await openPage(browser, page.id, bruno);
  await expect(commentsButton(reader)).toHaveAccessibleName("Commentaires, aucun fil ouvert");

  // Alice commente le deuxième bloc depuis son menu.
  await blockMenu(writer, "Deuxième bloc", "Commenter");
  const panel = commentsPanel(writer);
  const field = panel.getByRole("textbox", { name: "Commenter le bloc" });
  await expect(field).toBeFocused();
  await expect(panel).toContainText("Deuxième bloc");
  await field.fill("À revoir avant vendredi");
  await panel.getByRole("button", { name: "Commenter le bloc" }).click();
  await expect(threadOf(panel, "À revoir avant vendredi")).toBeVisible();
  await expect(markOf(writer, "Deuxième bloc")).toBeVisible();

  // Bruno, qui ne fait que lire, l'apprend tout de suite : la trame `comments` de la page.
  await expect(commentsButton(reader)).toHaveAccessibleName("Commentaires, 1 fil ouvert", {
    timeout: 3_000,
  });
  await expect(markOf(reader, "Deuxième bloc")).toBeVisible();
  // La pastille ouvre le panneau sur le fil, qui prend le focus.
  await markOf(reader, "Deuxième bloc").click();
  const thread = threadOf(commentsPanel(reader), "À revoir avant vendredi");
  await expect(thread).toBeFocused();
  await expect(thread).toContainText(alice.name);

  // Rien n'est écrit dans le document : le MMD du serveur est le même.
  expect(await getMmd(aliceApi, page.id)).toBe(before);
});

test("commenter_who_cannot_edit_comments", async ({ browser }) => {
  const { alice, aliceApi, bruno, page } = await scene("comment");
  const before = await getMmd(aliceApi, page.id);
  const commenter = await openPage(browser, page.id, bruno);
  const writes = watchWrites(commenter);
  // L'éditeur est en lecture seule.
  await expect(readerBanner(commenter)).toBeVisible();
  await expect(commenter.locator(".mm-editor")).toHaveAttribute("data-editable", "false");

  // Un bloc : la poignée de BlockNote n'est pas là, la marge offre « Commenter ce bloc ».
  await blockOf(commenter, "Premier bloc").locator(".bn-block-content").first().hover();
  await commenter.getByRole("button", { name: "Commenter ce bloc" }).click();
  const panel = commentsPanel(commenter);
  const field = panel.getByRole("textbox", { name: "Commenter le bloc" });
  await expect(field).toBeFocused();
  await field.fill("Une remarque sur le premier bloc");
  await panel.getByRole("button", { name: "Commenter le bloc" }).click();
  await expect(threadOf(panel, "Une remarque sur le premier bloc")).toBeVisible();
  await expect(markOf(commenter, "Premier bloc")).toBeVisible();

  // Et la page.
  await panel.getByRole("textbox", { name: "Commenter la page" }).fill("Et une sur la page");
  await panel.getByRole("button", { name: "Commenter", exact: true }).click();
  await expect(threadOf(panel, "Et une sur la page")).toContainText("Sur la page");

  // Alice les lit.
  const owner = await openPage(browser, page.id, alice);
  const ownerPanel = await openComments(owner);
  await expect(threadOf(ownerPanel, "Une remarque sur le premier bloc")).toBeVisible();
  await expect(threadOf(ownerPanel, "Et une sur la page")).toBeVisible();
  // Rien n'est parti dans le document.
  expect(writes).toEqual([]);
  expect(await getMmd(aliceApi, page.id)).toBe(before);
});

test("commenter_comments_the_selected_block_with_ctrl_alt_m", async ({ browser }) => {
  const { aliceApi, bruno, page } = await scene("comment");
  const before = await getMmd(aliceApi, page.id);
  const commenter = await openPage(browser, page.id, bruno);
  await expect(commenter.locator(".mm-editor")).toHaveAttribute("data-editable", "false");
  // Au clavier, sans poignée ni survol : la sélection du document dit le bloc (R176).
  await blockOf(commenter, "Deuxième bloc").locator(".bn-inline-content").first().selectText();
  await commenter.keyboard.press("Control+Alt+m");
  const panel = commentsPanel(commenter);
  const field = panel.getByRole("textbox", { name: "Commenter le bloc" });
  await expect(field).toBeFocused();
  await expect(panel).toContainText("Deuxième bloc");
  await commenter.keyboard.type("Choisi au clavier");
  await commenter.keyboard.press("Control+Enter");
  await expect(threadOf(panel, "Choisi au clavier")).toBeVisible();
  await expect(markOf(commenter, "Deuxième bloc")).toBeVisible();
  expect(await getMmd(aliceApi, page.id)).toBe(before);
});

test("reader_reads_comments_and_cannot_write_one", async ({ browser }) => {
  const { alice, aliceApi, bruno, page } = await scene("read");
  const owner = await openPage(browser, page.id, alice);
  const blockId = await blockIdOf(owner, "Deuxième bloc");
  const thread = await openThread(aliceApi, page.id, blockId, "Un fil à lire");
  const res = await aliceApi.put(
    `/api/v1/pages/${page.id}/comments/${thread.id}/messages/${thread.comments[0].id}/reactions/${encodeURIComponent("🎉")}`,
  );
  expect(res.status(), await res.text()).toBe(200);

  const reader = await openPage(browser, page.id, bruno);
  await expect(markOf(reader, "Deuxième bloc")).toBeVisible();
  const panel = await openComments(reader);
  const shown = threadOf(panel, "Un fil à lire");
  await expect(shown).toBeVisible();
  await expect(shown.getByRole("list", { name: "Réactions" })).toContainText("🎉");
  // Rien ne s'écrit : ni zone de rédaction, ni réponse, ni résolution, ni réaction.
  await expect(panel.getByRole("textbox")).toHaveCount(0);
  for (const name of ["Répondre", "Résoudre", "Ajouter une réaction", "Supprimer ce message"]) {
    await expect(panel.getByRole("button", { name })).toHaveCount(0);
  }
  await expect(shown.getByRole("list", { name: "Réactions" }).getByRole("button")).toHaveCount(0);
  // Ni « Commenter ce bloc » dans la marge.
  await blockOf(reader, "Premier bloc").locator(".bn-block-content").first().hover();
  await expect(reader.getByRole("button", { name: "Commenter ce bloc" })).toHaveCount(0);
});

test("resolve_hides_the_thread_and_reopen_brings_it_back", async ({ browser }) => {
  const { alice, aliceApi, bruno, page } = await scene("read");
  const owner = await openPage(browser, page.id, alice);
  await openThread(aliceApi, page.id, await blockIdOf(owner, "Troisième bloc"), "À résoudre");
  const reader = await openPage(browser, page.id, bruno);
  await expect(markOf(reader, "Troisième bloc")).toBeVisible();
  await expect(markOf(owner, "Troisième bloc")).toBeVisible();

  const panel = await openComments(owner);
  await threadOf(panel, "À résoudre").getByRole("button", { name: "Résoudre" }).click();
  // Caché, sans pastille, et compté à part.
  await expect(threadOf(panel, "À résoudre")).toHaveCount(0);
  await expect(markOf(owner, "Troisième bloc")).toHaveCount(0);
  await expect(commentsButton(owner)).toHaveAccessibleName("Commentaires, aucun fil ouvert");
  // Chez l'autre aussi.
  await expect(markOf(reader, "Troisième bloc")).toHaveCount(0, { timeout: 3_000 });

  const resolved = panel.getByRole("button", { name: "Fils résolus (1)" });
  await expect(resolved).toHaveAttribute("aria-expanded", "false");
  await resolved.click();
  const back = threadOf(panel.getByRole("list", { name: "Fils résolus" }), "À résoudre");
  await expect(back).toContainText(`Résolu par ${alice.name}`);
  await back.getByRole("button", { name: "Rouvrir" }).click();
  await expect(threadOf(panel.getByRole("list", { name: "Fils ouverts" }), "À résoudre")).toBeVisible();
  await expect(markOf(owner, "Troisième bloc")).toBeVisible();
  await expect(markOf(reader, "Troisième bloc")).toBeVisible({ timeout: 3_000 });
});

test("comment_of_a_deleted_block_is_kept_apart", async ({ browser }) => {
  const { alice, aliceApi, page } = await scene("read");
  const owner = await openPage(browser, page.id, alice);
  await openThread(aliceApi, page.id, await blockIdOf(owner, "Deuxième bloc"), "Sur un bloc qui part");
  await openThread(aliceApi, page.id, await blockIdOf(owner, "Premier bloc"), "Sur un bloc qui reste");
  const panel = await openComments(owner);
  const open = panel.getByRole("list", { name: "Fils ouverts" });
  // Dans l'ordre des blocs de la page, pas dans celui de leur ouverture.
  await expect(open.getByRole("article")).toHaveText([/Sur un bloc qui reste/, /Sur un bloc qui part/]);

  // Le bloc est supprimé dans l'éditeur.
  await blockMenu(owner, "Deuxième bloc", "Supprimer");
  await expect(blockOf(owner, "Deuxième bloc")).toHaveCount(0);
  const deleted = panel.getByRole("list", { name: "Fils d'un bloc supprimé" });
  await expect(panel.getByRole("heading", { name: "Bloc supprimé" })).toBeVisible();
  const kept = threadOf(deleted, "Sur un bloc qui part");
  await expect(kept).toBeVisible();
  await expect(open.getByRole("article")).toHaveText([/Sur un bloc qui reste/]);
  await expect(markOf(owner, "Deuxième bloc")).toHaveCount(0);
  // Toujours lisible : on y répond, et on le résout.
  await kept.getByRole("button", { name: "Répondre" }).click();
  await kept.getByRole("textbox", { name: "Répondre au fil" }).fill("Réponse après coup");
  await kept.getByRole("button", { name: "Envoyer la réponse" }).click();
  await expect(kept).toContainText("Réponse après coup");
  await kept.getByRole("button", { name: "Résoudre" }).click();
  await expect(deleted).toHaveCount(0);
  await expect(panel.getByRole("button", { name: "Fils résolus (1)" })).toBeVisible();
});

test("comments_are_read_only_in_the_trash", async ({ browser }) => {
  const { alice, aliceApi, page } = await scene("read");
  await openThread(aliceApi, page.id, null, "Écrit avant la corbeille");
  const res = await aliceApi.post(`/api/v1/pages/${page.id}/trash`);
  expect(res.status(), await res.text()).toBe(200);

  const owner = await openPage(browser, page.id, alice);
  await expect(trashBanner(owner)).toBeVisible();
  const panel = await openComments(owner);
  // Tout se lit, rien ne s'écrit, même avec l'accès complet.
  await expect(threadOf(panel, "Écrit avant la corbeille")).toBeVisible();
  await expect(panel.getByRole("textbox")).toHaveCount(0);
  for (const name of [
    "Répondre",
    "Résoudre",
    "Ajouter une réaction",
    "Modifier ce message",
    "Supprimer ce message",
  ]) {
    await expect(panel.getByRole("button", { name })).toHaveCount(0);
  }
});

test("the_panel_closes_with_escape_and_gives_the_focus_back", async ({ browser }) => {
  const { alice, page } = await scene("read");
  const owner = await openPage(browser, page.id, alice);
  const button = commentsButton(owner);
  await button.focus();
  await owner.keyboard.press("Enter");
  const panel = commentsPanel(owner);
  await expect(panel).toBeFocused();
  // Au clavier, jusqu'au commentaire de la page, envoyé par Ctrl+Entrée.
  await owner.keyboard.press("Tab");
  await owner.keyboard.press("Tab");
  await expect(panel.getByRole("textbox", { name: "Commenter la page" })).toBeFocused();
  await owner.keyboard.type("Écrit au clavier");
  await owner.keyboard.press("Control+Enter");
  await expect(threadOf(panel, "Écrit au clavier")).toBeVisible();
  await owner.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect(button).toHaveAccessibleName("Commentaires, 1 fil ouvert");
});

test("ctrl_alt_m_comments_the_block_of_the_cursor", async ({ browser }) => {
  const { alice, page } = await scene("read");
  const owner = await openPage(browser, page.id, alice);
  await blockOf(owner, "Troisième bloc").locator(".bn-block-content").first().click();
  await owner.keyboard.press("Control+Alt+KeyM");
  const panel = commentsPanel(owner);
  const field = panel.getByRole("textbox", { name: "Commenter le bloc" });
  await expect(field).toBeFocused();
  await expect(panel).toContainText("Troisième bloc");
  await owner.keyboard.type("Au clavier, sur le troisième");
  await owner.keyboard.press("Control+Enter");
  await expect(markOf(owner, "Troisième bloc")).toBeVisible();
  // Échap ferme le panneau, et le focus revient dans le texte, où il était.
  await owner.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
  await expect(owner.locator(".bn-editor")).toBeFocused();
});
