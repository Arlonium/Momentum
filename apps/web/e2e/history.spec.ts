// L'historique d'une page (R135) : le menu « … » de la barre du haut ouvre la fenêtre
// « Historique », à partir de « Peut modifier » (R216, S5), à la corbeille aussi. Elle liste les
// versions (« Version actuelle », puis la plus récente d'abord, avec leurs auteurs) et montre
// celle qu'on choisit dans le même éditeur que la page, en lecture seule. L'aperçu n'écrit
// jamais : ni dans le document de la page, ni sur le réseau. « Restaurer
// cette version », dès « Peut modifier », remplace le contenu pour tout le monde ; ce qui est
// remplacé reste dans l'historique.
//
// Le serveur des e2e coupe une version quand une écriture suit de plus d'une seconde la première
// écriture non versionnée (`E2E_VERSION_EVERY_SECS`, `playwright.config.ts`).
import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  closeContexts,
  createPage,
  defaultAccount,
  getMmd,
  injectForeignBlock,
  newUser,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  setPageLevel,
  trashBanner,
  unique,
  versionsOf,
  waitSynced,
  watchWrites,
  writeAfterTheInterval,
} from "./helpers";

test.afterEach(closeContexts);

const OLD = "Le texte d'hier";
const NEW = "Le texte d'aujourd'hui";
const REPLACED = "Le contenu actuel sera remplacé. Il restera dans l'historique.";
const OTHERS = "D'autres personnes ont cette page ouverte";

function menuButton(p: Page): Locator {
  return p.getByRole("button", { name: "Actions de la page", exact: true });
}

function versionList(dialog: Locator): Locator {
  return dialog.getByRole("list", { name: "Versions de la page" });
}

/** Les lignes de la liste : « Version actuelle », puis chaque version. */
function versionItems(dialog: Locator): Locator {
  return versionList(dialog).getByRole("listitem");
}

/** L'éditeur de l'aperçu, dans la fenêtre. */
function preview(dialog: Locator): Locator {
  return dialog.locator(".mm-preview-editor .bn-editor");
}

/** L'éditeur de la page, derrière la fenêtre. */
function pageEditor(p: Page): Locator {
  return p.locator(".mm-editor .bn-editor");
}

/** Ouvre l'historique par le menu « … », et attend que la liste soit lue. */
async function openHistory(p: Page): Promise<Locator> {
  // Un menu sans entrée n'est pas là : sans cette attente, le clic attendrait le délai du test.
  await expect(menuButton(p), "le menu « … » de la page, qui porte « Historique »").toBeVisible();
  await menuButton(p).click();
  await p.getByRole("menuitem", { name: "Historique" }).click();
  const dialog = p.getByRole("dialog", { name: "Historique" });
  await expect(dialog).toBeVisible();
  await expect(versionList(dialog)).toHaveAttribute("aria-busy", "false");
  return dialog;
}

/** Restaure la version choisie dans la fenêtre, et attend qu'elle se ferme. */
async function restoreChosen(p: Page, dialog: Locator, { othersWarned }: { othersWarned: boolean }) {
  await dialog.getByRole("button", { name: "Restaurer cette version" }).click();
  const confirm = p.getByRole("alertdialog", { name: /^Restaurer cette version/ });
  await expect(confirm).toContainText(REPLACED);
  if (othersWarned) await expect(confirm).toContainText(OTHERS);
  else await expect(confirm).not.toContainText(OTHERS);
  await confirm.getByRole("button", { name: "Restaurer", exact: true }).click();
  await expect(dialog).toBeHidden();
  // Le focus revient au bouton du menu, et la page l'annonce.
  await expect(menuButton(p)).toBeFocused();
  await expect(p.getByRole("status").filter({ hasText: "Version restaurée." })).toHaveCount(1);
}

test("version_is_listed_and_previewed", async ({ browser, request, baseURL }) => {
  const { id } = await createPage(request, { title: `Historique ${unique()}` });
  await putMmd(request, id, `${OLD}\n`);
  // Un bloc d'une version plus récente de Mementum : y-prosemirror le retire de tout document
  // qu'il affiche. La version le garde ; l'aperçu ne doit le retirer de rien d'autre que du sien.
  await injectForeignBlock(request, baseURL!, id);
  const versions = await writeAfterTheInterval(request, id, `${NEW}\n`);
  const state = await request.get(`/api/v1/pages/${id}/versions/${versions[0].id}/state`);
  expect(state.status()).toBe(200);
  expect(Buffer.from(await state.body()).includes("futureWidget")).toBe(true);
  const contentBefore = await getMmd(request, id);

  const p = await newUser(browser);
  const writes = watchWrites(p);
  const sockets: string[] = [];
  /** Les messages de synchro (type 0 : étapes et mises à jour) envoyés pendant l'historique. */
  const documentFrames: number[][] = [];
  let watching = false;
  p.on("websocket", (ws) => {
    sockets.push(ws.url());
    if (!ws.url().includes("/sync/")) return;
    ws.on("framesent", ({ payload }) => {
      const bytes = [...(typeof payload === "string" ? Buffer.from(payload) : payload)];
      if (watching && bytes[0] === 0) documentFrames.push(bytes);
    });
  });
  // Les canaux entre onglets que la page ouvre (y-websocket en ouvrirait un sans `disableBc`).
  await p.addInitScript(() => {
    const Native = window.BroadcastChannel;
    const counted = window as unknown as { channelsOpened: number };
    counted.channelsOpened = 0;
    window.BroadcastChannel = class extends Native {
      constructor(name: string) {
        super(name);
        counted.channelsOpened += 1;
      }
    };
  });
  const channelsOpened = () =>
    p.evaluate(() => (window as unknown as { channelsOpened: number }).channelsOpened);
  await p.goto(`/p/${id}`);
  await waitSynced(p);
  await expect(pageEditor(p)).toContainText(NEW);
  const channelsBefore = await channelsOpened();
  // À partir d'ici, rien ne doit arriver dans le document de la page ouverte.
  await p.evaluate(() => {
    const counter = window as unknown as { updatesDuringHistory: number };
    counter.updatesDuringHistory = 0;
    window.__mementum!.doc.on("update", () => {
      counter.updatesDuringHistory += 1;
    });
  });
  const socketsBefore = sockets.length;
  watching = true;

  const dialog = await openHistory(p);
  const items = versionItems(dialog);
  await expect(items).toHaveCount(versions.length + 1);
  await expect(items.first()).toContainText("Version actuelle");
  // La version actuelle est choisie à l'ouverture : la page telle qu'elle est.
  await expect(preview(dialog)).toContainText(NEW);
  const latest = items.nth(1);
  await expect(latest).toContainText(defaultAccount().name);
  await expect(latest.locator("time")).toHaveAttribute(
    "datetime",
    new Date(versions[0].createdAt).toISOString(),
  );

  await latest.getByRole("button").click();
  await expect(preview(dialog)).toContainText(OLD);
  await expect(preview(dialog)).not.toContainText(NEW);
  await expect(preview(dialog)).toHaveAttribute("contenteditable", "false");
  // Derrière, la page n'a pas changé.
  await expect(pageEditor(p)).toContainText(NEW);
  await expect(pageEditor(p)).not.toContainText(OLD);
  // Une frappe dans l'aperçu ne fait rien.
  await preview(dialog).click();
  await p.keyboard.type("frappe perdue");
  await expect(preview(dialog)).not.toContainText("frappe perdue");

  // Ce qui partirait, partirait aussitôt : deux images et un tour de boucle suffisent à le voir.
  await p.evaluate(
    () => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 200)))),
  );
  expect(await p.evaluate(() => (window as unknown as { updatesDuringHistory: number }).updatesDuringHistory)).toBe(0);
  // Aucune trame de document n'est partie (la présence de la session, type 1, n'en porte pas,
  // et y-protocols la renouvelle d'elle-même toutes les quinze secondes) ; aucune connexion de
  // plus ; et la page, sur le serveur, n'a pas bougé.
  expect(documentFrames).toEqual([]);
  expect(writes).toEqual([]);
  expect(sockets.slice(socketsBefore)).toEqual([]);
  expect(await channelsOpened()).toBe(channelsBefore);
  expect(await getMmd(request, id)).toBe(contentBefore);

  // Échap ferme la fenêtre, et le focus revient au bouton du menu.
  await p.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(menuButton(p)).toBeFocused();

  // Le document de la page n'est pas bloqué : une frappe faite après l'historique part au
  // serveur (un document resté bloqué dans une transaction n'émettrait plus rien).
  await pageEditor(p).getByText(NEW).click();
  await p.keyboard.press("End");
  await p.keyboard.type(" et après");
  await expect.poll(() => getMmd(request, id)).toContain(`${NEW} et après`);
  expect(await p.evaluate(() => (window as unknown as { updatesDuringHistory: number }).updatesDuringHistory)).toBeGreaterThan(0);
});

test("restore_brings_the_old_content_back_for_everyone", async ({ browser }) => {
  const { alice, aliceApi, bruno, brunoId, space } = await ownerAndMember();
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Restauration ${unique()}`,
  });
  // Les droits avant d'ouvrir les pages : un changement d'accès ferme les connexions de synchro.
  await setPageLevel(aliceApi, page.id, brunoId, "edit");
  await putMmd(aliceApi, page.id, `${OLD}\n`);
  await writeAfterTheInterval(aliceApi, page.id, `${NEW}\n`);

  const a = await openPage(browser, page.id, alice);
  const b = await openPage(browser, page.id, bruno);
  for (const p of [a, b]) await expect(pageEditor(p)).toContainText(NEW);
  // Alice sait que Bruno a la page ouverte.
  await expect(a.getByRole("list", { name: "Présents sur la page" })).toBeVisible();

  const dialog = await openHistory(a);
  await versionItems(dialog).nth(1).getByRole("button").click();
  await expect(preview(dialog)).toContainText(OLD);
  await restoreChosen(a, dialog, { othersWarned: true });

  for (const p of [a, b]) {
    await expect(pageEditor(p)).toContainText(OLD);
    await expect(pageEditor(p)).not.toContainText(NEW);
  }
  const mmd = await getMmd(aliceApi, page.id);
  expect(mmd).toContain(OLD);
  expect(mmd).not.toContain(NEW);
});

test("restored_content_is_itself_in_the_history", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Aller-retour ${unique()}` });
  await putMmd(request, id, `${OLD}\n`);
  await writeAfterTheInterval(request, id, `${NEW}\n`);
  const p = await openPage(browser, id);

  // Restaurer la version d'hier, seul sur la page : pas de mise en garde de plus.
  let dialog = await openHistory(p);
  await versionItems(dialog).nth(1).getByRole("button").click();
  await expect(preview(dialog)).toContainText(OLD);
  await restoreChosen(p, dialog, { othersWarned: false });
  await expect(pageEditor(p)).toContainText(OLD);
  await expect(pageEditor(p)).not.toContainText(NEW);

  // Ce qui a été remplacé est resté dans l'historique : « avant une restauration ».
  const afterFirst = await versionsOf(request, id);
  expect(afterFirst.map((v) => v.reason)).toEqual(["before_restore", "periodic"]);
  dialog = await openHistory(p);
  await expect(versionItems(dialog)).toHaveCount(afterFirst.length + 1);
  const replaced = versionItems(dialog).nth(1);
  await expect(replaced).toContainText("avant une restauration");
  await replaced.getByRole("button").click();
  await expect(preview(dialog)).toContainText(NEW);
  await expect(preview(dialog)).not.toContainText(OLD);
  // Il se restaure à son tour.
  await restoreChosen(p, dialog, { othersWarned: false });
  await expect(pageEditor(p)).toContainText(NEW);
  await expect(pageEditor(p)).not.toContainText(OLD);

  // Le contenu restauré la première fois est lui-même une version gardée (celle d'hier) : la
  // seconde restauration ne coupe rien de plus (R190), et il se retrouve dans la liste.
  expect((await versionsOf(request, id)).map((v) => v.id)).toEqual(afterFirst.map((v) => v.id));
  dialog = await openHistory(p);
  await expect(versionItems(dialog)).toHaveCount(afterFirst.length + 1);
  const yesterday = versionItems(dialog).nth(2);
  await expect(yesterday).not.toContainText("avant une restauration");
  await yesterday.getByRole("button").click();
  await expect(preview(dialog)).toContainText(OLD);
  await expect(preview(dialog)).not.toContainText(NEW);
  await expect(dialog).toBeVisible();
});

test("reader_is_not_offered_the_history_and_cannot_restore", async ({ browser }) => {
  // Sous « Peut modifier », l'historique n'est pas proposé (R216) : un lecteur ne lit pas ce qui a
  // été effacé avant qu'on partage la page avec lui. Son menu « … » n'a aucune entrée : il n'est
  // pas là.
  const { aliceApi, bruno, brunoApi, brunoId, space } = await ownerAndMember();
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Lecture ${unique()}`,
  });
  await setPageLevel(aliceApi, page.id, brunoId, "read");
  await putMmd(aliceApi, page.id, `${OLD}\n`);
  const versions = await writeAfterTheInterval(aliceApi, page.id, `${NEW}\n`);

  const b = await openPage(browser, page.id, bruno);
  await expect(readerBanner(b)).toBeVisible();
  // La barre du haut est là (les commentaires se lisent à tout niveau), sans menu « … ».
  await expect(b.getByRole("button", { name: /^Commentaires/ })).toBeVisible();
  await expect(menuButton(b)).toHaveCount(0);
  // Le serveur lui refuse la liste et l'état des versions (S5), et la restauration.
  const list = await brunoApi.get(`/api/v1/pages/${page.id}/versions`);
  expect(list.status(), await list.text()).toBe(403);
  const state = await brunoApi.get(`/api/v1/pages/${page.id}/versions/${versions[0].id}/state`);
  expect(state.status(), await state.text()).toBe(403);
  const res = await brunoApi.post(`/api/v1/pages/${page.id}/versions/${versions[0].id}/restore`);
  expect(res.status()).toBe(403);
  expect(await getMmd(aliceApi, page.id)).toContain(NEW);
});

test("editor_reads_the_history_of_a_trashed_page_without_restoring", async ({ browser }) => {
  // « Peut modifier », page à la corbeille : l'historique se lit, rien ne se restaure.
  const { aliceApi, bruno, brunoId, space } = await ownerAndMember();
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Corbeille ${unique()}`,
  });
  await setPageLevel(aliceApi, page.id, brunoId, "edit");
  await putMmd(aliceApi, page.id, `${OLD}\n`);
  const versions = await writeAfterTheInterval(aliceApi, page.id, `${NEW}\n`);
  expect((await aliceApi.post(`/api/v1/pages/${page.id}/trash`)).status()).toBe(200);

  const b = await openPage(browser, page.id, bruno);
  await expect(trashBanner(b)).toBeVisible();
  const dialog = await openHistory(b);
  await expect(versionItems(dialog)).toHaveCount(versions.length + 1);
  await versionItems(dialog).nth(1).getByRole("button").click();
  await expect(preview(dialog)).toContainText(OLD);
  await expect(dialog.getByRole("button", { name: "Restaurer cette version" })).toHaveCount(0);
  await expect(dialog).toContainText("La page est à la corbeille : aucune version ne s'y restaure.");
  // Fermée, la fenêtre rend le focus au bouton du menu.
  await dialog.getByRole("button", { name: "Fermer l'historique" }).click();
  await expect(dialog).toBeHidden();
  await expect(menuButton(b)).toBeFocused();
});

test("a_new_version_reaches_an_open_history_without_a_click", async ({ browser }) => {
  // C9-5 : Alice a l'historique ouvert pendant que Bruno écrit ; la version neuve arrive dans la
  // liste sans qu'elle fasse rien (trame `versions`, R185).
  const { alice, aliceApi, brunoApi, brunoId, space } = await ownerAndMember();
  const page = await createPage(aliceApi, {
    spaceId: space.id,
    section: "private",
    title: `Suivi ${unique()}`,
  });
  await setPageLevel(aliceApi, page.id, brunoId, "edit");
  await putMmd(aliceApi, page.id, `${OLD}\n`);
  const first = await writeAfterTheInterval(aliceApi, page.id, `${NEW}\n`);

  const a = await openPage(browser, page.id, alice);
  const dialog = await openHistory(a);
  await expect(versionItems(dialog)).toHaveCount(first.length + 1);
  const next = await writeAfterTheInterval(brunoApi, page.id, `${NEW}, puis Bruno\n`);
  expect(next.length).toBeGreaterThan(first.length);
  await expect(versionItems(dialog)).toHaveCount(next.length + 1, { timeout: 5_000 });
});
