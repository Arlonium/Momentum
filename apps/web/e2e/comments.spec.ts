// Les commentaires d'une page (R134) : le panneau, les pastilles de la marge, « Commenter » dans
// le menu d'un bloc, et ce que chaque niveau y peut faire. Rien n'est écrit dans le document :
// le MMD de la page ne change pas, et quelqu'un qui peut commenter sans pouvoir modifier
// commente.
import { expect, test, type APIRequestContext, type Locator, type Page } from "@playwright/test";
import type { ErrorBody, Thread } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  firstSpace,
  getMmd,
  guestOf,
  holdPageReads,
  holdTreeReads,
  newUser,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  setPageLevel,
  signUpAccount,
  trashBanner,
  unique,
  versionsOf,
  waitSynced,
  watchWrites,
  writeAfterTheInterval,
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

/**
 * Le contenu propre d'un bloc : sous son conteneur, ou dans le cadre du repli d'un bloc
 * repliable (où sont aussi ses enfants).
 */
const OWN_CONTENT =
  ":scope > .bn-block > .bn-block-content, " +
  ":scope > .bn-block > .bn-toggle-frame > .bn-toggle-slot > .bn-block-content";

/** Un bloc de l'éditeur, par le texte qui lui est propre. */
function blockOf(p: Page, text: string): Locator {
  return p
    .locator('.bn-editor [data-node-type="blockOuter"]')
    .filter({ has: p.locator(OWN_CONTENT, { hasText: text }) });
}

async function blockIdOf(p: Page, text: string): Promise<string> {
  const id = await blockOf(p, text).getAttribute("data-id");
  if (!id) throw new Error(`pas de bloc « ${text} »`);
  return id;
}

/** La pastille de la marge d'un bloc commenté (le texte du bloc y est isolé, U+2068 et U+2069). */
function markOf(p: Page, text: string): Locator {
  return p.getByRole("list", { name: "Blocs commentés" }).getByRole("button", {
    name: new RegExp(`sur « ⁨${text}⁩ »$`),
  });
}

/** Un fil du panneau, par le texte d'un de ses messages. */
function threadOf(scope: Locator, body: string): Locator {
  return scope.getByRole("article").filter({ hasText: body });
}

/**
 * Survole `target`, et attend que la page l'ait vu (`:hover`) puis ait rendu deux images : ce qui
 * réagit au survol a eu le temps de paraître. Avant d'affirmer qu'un bouton n'est pas là.
 */
async function hoverAndSettle(p: Page, target: Locator): Promise<void> {
  await target.hover();
  await expect.poll(() => target.evaluate((element) => element.matches(":hover"))).toBe(true);
  await p.evaluate(
    () => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))),
  );
}

/** Ouvre le menu de la poignée d'un bloc et y choisit `item`. */
async function blockMenu(p: Page, text: string, item: string): Promise<void> {
  await blockOf(p, text).locator(OWN_CONTENT).hover();
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
  // L'éditeur est en lecture seule.
  await expect(readerBanner(commenter)).toBeVisible();
  await expect(commenter.locator(".mm-editor")).toHaveAttribute("data-editable", "false");

  // Un bloc : la poignée de BlockNote n'est pas là, la marge offre « Commenter ce bloc ».
  await blockOf(commenter, "Premier bloc").locator(OWN_CONTENT).hover();
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
  // Rien n'a changé dans le document. (Une session en lecture seule n'écrit rien de toute façon :
  // l'écriture se surveille dans l'éditeur modifiable, `ctrl_alt_m_comments_the_block_of_the_cursor`.)
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
  const { alice, aliceApi, bruno, brunoId, page } = await scene("read");
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
  // Ni « Commenter ce bloc » dans la marge : le survol a été vu, et le bouton n'est pas venu.
  const premier = blockOf(reader, "Premier bloc").locator(OWN_CONTENT);
  await hoverAndSettle(reader, premier);
  await expect(reader.getByRole("button", { name: "Commenter ce bloc" })).toHaveCount(0);
  // Contrôle : au niveau « Peut commenter », le même survol le fait paraître.
  await setPageLevel(aliceApi, page.id, brunoId, "comment");
  await expect(panel.getByRole("textbox", { name: "Commenter la page" })).toBeVisible({
    timeout: 10_000,
  });
  await reader.mouse.move(1, 1);
  await hoverAndSettle(reader, premier);
  await expect(reader.getByRole("button", { name: "Commenter ce bloc" })).toBeVisible();
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
    "Supprimer le fil",
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
  const { alice, aliceApi, page } = await scene("read");
  const before = await getMmd(aliceApi, page.id);
  // L'éditeur se modifie : c'est là qu'une écriture pourrait partir. On écoute ce qu'envoie sa
  // connexion de synchro dès l'ouverture.
  const owner = await newUser(browser, alice);
  const writes = watchWrites(owner);
  await owner.goto(`/p/${page.id}`);
  await waitSynced(owner);
  await expect(owner.locator(".mm-editor")).toHaveAttribute("data-editable", "true");
  // Par le menu de la poignée.
  await blockMenu(owner, "Premier bloc", "Commenter");
  await commentsPanel(owner).getByRole("textbox", { name: "Commenter le bloc" }).fill("Par le menu");
  await commentsPanel(owner).getByRole("button", { name: "Commenter le bloc" }).click();
  await expect(markOf(owner, "Premier bloc")).toBeVisible();
  await owner.keyboard.press("Escape");
  await expect(commentsPanel(owner)).toHaveCount(0);
  // Par le raccourci, la lettre « m » tapée.
  await blockOf(owner, "Troisième bloc").locator(OWN_CONTENT).click();
  await owner.keyboard.press("Control+Alt+m");
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
  // Rien n'est parti dans le document, ni par le menu ni par le raccourci.
  expect(writes).toEqual([]);
  expect(await getMmd(aliceApi, page.id)).toBe(before);
});

/** `POST …/comments/{threadId}/replies` par l'API. */
async function replyByApi(api: APIRequestContext, pageId: string, threadId: string, body: string) {
  return api.post(`/api/v1/pages/${pageId}/comments/${threadId}/replies`, { data: { body } });
}

/** Les fils de la page, tels que l'API les rend. */
async function threadsOf(api: APIRequestContext, pageId: string): Promise<Thread[]> {
  const res = await api.get(`/api/v1/pages/${pageId}/comments`);
  expect(res.status(), await res.text()).toBe(200);
  return (await res.json()) as Thread[];
}

/** L'encart des textes qu'on ne peut plus envoyer (R211). */
function unsentOf(panel: Locator): Locator {
  return panel.getByRole("region", { name: "Textes non envoyés" });
}

test("message_limit_is_read_and_the_reply_stays", async ({ browser }) => {
  // Le plafond le moins cher à atteindre qui touche un texte tapé : 500 messages dans un fil (S4).
  test.setTimeout(120_000);
  const { alice, aliceApi, page } = await scene("comment");
  const thread = await openThread(aliceApi, page.id, null, "Message 1");
  for (let from = 2; from <= 500; from += 25) {
    const batch = [];
    for (let n = from; n < from + 25 && n <= 500; n += 1) {
      batch.push(replyByApi(aliceApi, page.id, thread.id, `Message ${n}`));
    }
    for (const res of await Promise.all(batch)) expect(res.status()).toBe(201);
  }
  // Témoin : le serveur refuse le 501e, et dit pourquoi.
  const over = await replyByApi(aliceApi, page.id, thread.id, "Par l'API, de trop");
  expect(over.status()).toBe(409);
  const refusal = (await over.json()) as ErrorBody;
  expect(refusal.error).toBe("message_limit");

  const p = await openPage(browser, page.id, alice);
  const panel = await openComments(p);
  const shown = threadOf(panel, "Message 1");
  await expect(shown.getByRole("listitem").filter({ hasText: "Message 500" })).toHaveCount(1);
  await shown.getByRole("button", { name: "Répondre" }).click();
  const field = panel.getByRole("textbox", { name: "Répondre au fil" });
  await field.fill("Une réponse de trop");
  await panel.getByRole("button", { name: "Envoyer la réponse" }).click();
  // Le refus tel que le serveur le dit, et le texte reste dans son champ.
  await expect(panel.getByRole("alert")).toHaveText(refusal.message);
  await expect(field).toHaveValue("Une réponse de trop");
  // Fermé puis rouvert, le panneau le retrouve ; rien n'est parti.
  await commentsButton(p).click();
  await expect(panel).toBeHidden();
  await openComments(p);
  await expect(panel.getByRole("textbox", { name: "Répondre au fil" })).toHaveValue(
    "Une réponse de trop",
  );
  const [stored] = await threadsOf(aliceApi, page.id);
  expect(stored.comments).toHaveLength(500);
});

test("agent_rewrite_keeps_the_thread_on_its_block", async ({ browser }) => {
  // C9-1 : un agent relit la page (`GET /mmd`, sans ancres) et la récrit par `PUT /mmd`, pendant
  // qu'un fil est ouvert sur un bloc. Un bloc inchangé garde son identifiant (S3, R201) : le fil
  // reste sur son bloc, et la pastille aussi.
  const { alice, aliceApi, page } = await scene("comment");
  const a = await openPage(browser, page.id, alice);
  const second = await blockIdOf(a, "Deuxième bloc");
  await openThread(aliceApi, page.id, second, "Sur le deuxième bloc");
  const panel = await openComments(a);
  const thread = threadOf(panel, "Sur le deuxième bloc");
  await expect(thread.getByRole("button", { name: "Aller au bloc" })).toBeVisible();
  await expect(markOf(a, "Deuxième bloc")).toBeVisible();

  // Récrite telle quelle, puis avec un bloc de plus : quand celui-ci paraît, la récriture
  // identique, partie avant lui, est arrivée aussi.
  const read = await getMmd(aliceApi, page.id);
  expect(read).toBe(CONTENT);
  await putMmd(aliceApi, page.id, read);
  await putMmd(aliceApi, page.id, `${CONTENT}\nQuatrième bloc\n`);
  await expect(blockOf(a, "Quatrième bloc")).toHaveCount(1);
  expect(await blockIdOf(a, "Deuxième bloc")).toBe(second);
  await expect(markOf(a, "Deuxième bloc")).toBeVisible();
  await expect(thread.getByRole("button", { name: "Aller au bloc" })).toBeVisible();
  await expect(panel.getByRole("heading", { name: "Bloc supprimé" })).toHaveCount(0);

  // L'agent modifie le bloc commenté lui-même. Il reste à sa place, entre deux blocs gardés, et
  // garde son type : il garde aussi son identifiant (R201), et le fil le suit.
  await putMmd(
    aliceApi,
    page.id,
    "Premier bloc\n\nDeuxième bloc, revu par l'agent\n\nTroisième bloc\n\nQuatrième bloc\n",
  );
  await expect(blockOf(a, "Deuxième bloc, revu par l'agent")).toHaveCount(1);
  expect(await blockIdOf(a, "Deuxième bloc, revu par l'agent")).toBe(second);
  await expect(markOf(a, "Deuxième bloc, revu par l'agent")).toBeVisible();
  await expect(thread.getByRole("button", { name: "Aller au bloc" })).toBeVisible();
  await expect(panel.getByRole("heading", { name: "Bloc supprimé" })).toHaveCount(0);
});

test("a_draft_outlives_a_lowered_level_and_comes_back_with_it", async ({ browser }) => {
  // C9-2 : un commentateur rédige ; son niveau tombe à « Peut lire ». Son texte reste montré
  // (« Textes non envoyés », R211), et retrouve son champ quand le droit revient, sans partir.
  const { aliceApi, bruno, brunoId, page } = await scene("comment");
  const b = await openPage(browser, page.id, bruno);
  // Les fils que l'onglet ouvre (`POST …/comments`), envoyés par lui ou partis d'eux-mêmes.
  const sends: string[] = [];
  b.on("request", (request) => {
    const { pathname } = new URL(request.url());
    if (request.method() === "POST" && pathname === `/api/v1/pages/${page.id}/comments`) {
      sends.push(request.postData() ?? "");
    }
  });
  const panel = await openComments(b);
  await panel.getByRole("textbox", { name: "Commenter la page" }).fill("Mon brouillon");
  await setPageLevel(aliceApi, page.id, brunoId, "read");
  const unsent = unsentOf(panel);
  await expect(unsent).toContainText("Mon brouillon");
  await expect(unsent).toContainText("Tu ne peux plus écrire ici.");
  await expect(panel.getByRole("textbox")).toHaveCount(0);
  // Le droit revient : le texte retrouve son champ, et rien n'est parti tout seul.
  await setPageLevel(aliceApi, page.id, brunoId, "comment");
  await expect(panel.getByRole("textbox", { name: "Commenter la page" })).toHaveValue(
    "Mon brouillon",
  );
  await expect(unsent).toHaveCount(0);
  expect(await threadsOf(aliceApi, page.id)).toEqual([]);
  // Témoin : c'est Bruno qui envoie. Son envoi part après le retour du champ ; un envoi parti
  // de lui-même à ce retour l'aurait précédé (la requête précède sa réponse, et le fil
  // affiché la suit), et serait compté avec le sien.
  const send = panel.getByRole("button", { name: "Commenter", exact: true });
  await expect(send).toBeEnabled();
  await send.click();
  await expect(threadOf(panel, "Mon brouillon")).toBeVisible();
  expect(sends, "fils ouverts par l'onglet").toHaveLength(1);
  const threads = await threadsOf(aliceApi, page.id);
  expect(threads.map((thread) => thread.comments.map((comment) => comment.body))).toEqual([
    ["Mon brouillon"],
  ]);
});

test("a_draft_refused_by_a_trash_shows_the_server_message", async ({ browser }) => {
  // C9-2 : la page part à la corbeille pendant qu'il rédige ; son onglet ne le sait pas encore
  // (lectures de la page et de l'arbre retenues). Il envoie : le serveur refuse, et quand la page
  // relue dit la corbeille, le texte reste montré avec le refus du serveur tel quel.
  const { aliceApi, bruno, page } = await scene("comment");
  const b = await openPage(browser, page.id, bruno);
  const panel = await openComments(b);
  const field = panel.getByRole("textbox", { name: "Commenter la page" });
  await field.fill("Envoyé trop tard");
  const pageReads = await holdPageReads(b, page.id);
  const treeReads = await holdTreeReads(b);
  const trashed = await aliceApi.post(`/api/v1/pages/${page.id}/trash`);
  expect(trashed.status()).toBe(200);
  await panel.getByRole("button", { name: "Commenter", exact: true }).click();
  const refusal = panel.getByRole("alert");
  await expect(refusal).toContainText("la page est à la corbeille");
  const said = await refusal.textContent();
  pageReads.release();
  treeReads.release();
  await expect(trashBanner(b)).toBeVisible();
  const unsent = unsentOf(panel);
  await expect(unsent).toContainText("Envoyé trop tard");
  await expect(unsent).toContainText(said!);
  await expect(panel.getByRole("textbox")).toHaveCount(0);
  expect(await threadsOf(aliceApi, page.id)).toEqual([]);
});

test("a_restore_without_the_commented_block_moves_its_thread_apart_and_back", async ({ browser }) => {
  // C9-3 : Bruno restaure une version où le bloc commenté n'existe pas, pendant qu'Alice a le
  // panneau ouvert : le fil passe sous « Bloc supprimé ». Bruno restaure ensuite la version
  // « avant une restauration » : le bloc revient avec son identifiant, et le fil sur lui.
  const { alice, aliceApi, brunoApi, brunoId, space } = await ownerAndMember();
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Restaurations ${unique()}`,
  });
  await setPageLevel(aliceApi, page.id, brunoId, "edit");
  await putMmd(aliceApi, page.id, "Premier bloc\n\nDeuxième bloc\n");
  const [withoutThird] = await writeAfterTheInterval(aliceApi, page.id, CONTENT);

  const a = await openPage(browser, page.id, alice);
  const third = await blockIdOf(a, "Troisième bloc");
  await openThread(aliceApi, page.id, third, "Sur le troisième bloc");
  const panel = await openComments(a);
  const open = panel.getByRole("list", { name: "Fils ouverts" });
  await expect(threadOf(open, "Sur le troisième bloc")).toBeVisible();
  await expect(markOf(a, "Troisième bloc")).toBeVisible();

  const restore = (versionId: string) =>
    brunoApi.post(`/api/v1/pages/${page.id}/versions/${versionId}/restore`);
  expect((await restore(withoutThird.id)).status()).toBe(204);
  await expect(blockOf(a, "Troisième bloc")).toHaveCount(0);
  const deleted = panel.getByRole("list", { name: "Fils d'un bloc supprimé" });
  await expect(threadOf(deleted, "Sur le troisième bloc")).toBeVisible();
  await expect(markOf(a, "Troisième bloc")).toHaveCount(0);

  // La restauration a gardé ce qu'elle remplaçait : la version « avant une restauration ».
  const [beforeRestore] = await versionsOf(aliceApi, page.id);
  expect(beforeRestore.reason).toBe("before_restore");
  expect((await restore(beforeRestore.id)).status()).toBe(204);
  await expect(blockOf(a, "Troisième bloc")).toHaveCount(1);
  expect(await blockIdOf(a, "Troisième bloc")).toBe(third);
  await expect(threadOf(open, "Sur le troisième bloc")).toBeVisible();
  await expect(deleted).toHaveCount(0);
  await expect(markOf(a, "Troisième bloc")).toBeVisible();
});

/**
 * Une page de trois blocs, qu'Alice possède, et Carla, invitée de l'espace sur cette page seule
 * avec « Peut commenter ». Les droits sont posés avant qu'aucun onglet ne s'ouvre.
 */
async function guestScene() {
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const space = await firstSpace(aliceApi);
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Fils ${unique()}`,
  });
  await putMmd(aliceApi, page.id, CONTENT);
  const carla = await guestOf(alice, page.id, `Carla ${unique()}`, "comment");
  return { alice, aliceApi, carla, carlaApi: await apiAs(carla), page };
}

test("full_access_deletes_a_whole_thread_for_everyone", async ({ browser }) => {
  // R352 : la propriétaire supprime d'un coup le fil qu'une invitée a ouvert et où elle a répondu.
  const { alice, aliceApi, carla, carlaApi, page } = await guestScene();
  const owner = await openPage(browser, page.id, alice);
  const second = await blockIdOf(owner, "Deuxième bloc");
  const gone = await openThread(carlaApi, page.id, second, "Fil de Carla");
  const reply = await replyByApi(carlaApi, page.id, gone.id, "Et sa réponse");
  expect(reply.status(), await reply.text()).toBe(201);
  // Témoin : un autre fil, qui doit rester.
  await openThread(aliceApi, page.id, await blockIdOf(owner, "Premier bloc"), "Fil qui reste");

  const guest = await openPage(browser, page.id, carla);
  const guestPanel = await openComments(guest);
  await expect(threadOf(guestPanel, "Fil de Carla")).toContainText("Et sa réponse");
  await expect(markOf(guest, "Deuxième bloc")).toBeVisible();
  // Ce que porte la fenêtre de Carla survivra à la suppression : elle n'est pas rechargée.
  await guest.evaluate(() => Object.assign(window, { notReloaded: true }));

  const ownerPanel = await openComments(owner);
  const doomed = threadOf(ownerPanel, "Fil de Carla");
  await doomed.getByRole("button", { name: "Supprimer le fil" }).click();
  const confirm = owner.getByRole("alertdialog", { name: "Supprimer ce fil ?" });
  await expect(confirm).toContainText("ses 2 messages");
  await expect(confirm).toContainText("Cela ne se défait pas.");
  await confirm.getByRole("button", { name: "Supprimer", exact: true }).click();
  await expect(confirm).toHaveCount(0);
  await expect(doomed).toHaveCount(0);
  // Le focus va au fil voisin.
  await expect(threadOf(ownerPanel, "Fil qui reste")).toBeFocused();
  await expect(markOf(owner, "Deuxième bloc")).toHaveCount(0);

  // Chez Carla, panneau ouvert : le fil et sa pastille partent, sans recharger.
  await expect(threadOf(guestPanel, "Fil de Carla")).toHaveCount(0, { timeout: 3_000 });
  await expect(markOf(guest, "Deuxième bloc")).toHaveCount(0);
  await expect(threadOf(guestPanel, "Fil qui reste")).toBeVisible();
  await expect(markOf(guest, "Premier bloc")).toBeVisible();
  expect(await guest.evaluate(() => (window as { notReloaded?: boolean }).notReloaded)).toBe(true);
  // Sur le serveur, il ne reste que le témoin.
  const left = await threadsOf(aliceApi, page.id);
  expect(left.map((t) => t.comments.map((c) => c.body))).toEqual([["Fil qui reste"]]);
});

test("a_commenter_is_not_offered_to_delete_a_thread", async ({ browser }) => {
  // Même le fil qu'elle a ouvert : supprimer tout un fil demande l'accès complet (R352).
  const { alice, aliceApi, carla, carlaApi, page } = await guestScene();
  const own = await openThread(carlaApi, page.id, null, "Fil ouvert par Carla");

  const guest = await openPage(browser, page.id, carla);
  const guestPanel = await openComments(guest);
  const shown = threadOf(guestPanel, "Fil ouvert par Carla");
  // Le fil est rendu avec ses actions : elle y répond, et le résout.
  await expect(shown.getByRole("button", { name: "Répondre" })).toBeVisible();
  await expect(shown.getByRole("button", { name: "Résoudre" })).toBeVisible();
  await expect(guestPanel.getByRole("button", { name: "Supprimer le fil" })).toHaveCount(0);
  // Témoin : la propriétaire a le bouton, sur le même fil.
  const owner = await openPage(browser, page.id, alice);
  const ownerPanel = await openComments(owner);
  await expect(
    threadOf(ownerPanel, "Fil ouvert par Carla").getByRole("button", { name: "Supprimer le fil" }),
  ).toBeVisible();

  // L'appel direct, par son compte : refusé, tel que le serveur le dit, et rien n'est parti.
  const res = await carlaApi.delete(`/api/v1/pages/${page.id}/comments/${own.id}`);
  expect(res.status()).toBe(403);
  const refusal = (await res.json()) as ErrorBody;
  expect(refusal.error).toBe("forbidden");
  expect(refusal.message).toBe(
    "niveau insuffisant sur cette page : il faut « Accès complet », tu as « Peut commenter »",
  );
  expect((await threadsOf(aliceApi, page.id)).map((t) => t.id)).toEqual([own.id]);
});
