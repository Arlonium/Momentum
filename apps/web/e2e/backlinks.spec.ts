// « Pages qui mènent ici » (lot 4, Task C3) : sous le texte d'une page, les pages du même espace
// que le compte voit et qui portent un lien vers elle. Rien ne pousse un changement de rétroliens :
// un lien posé ailleurs paraît à la lecture suivante (le retour sur l'onglet), une fois sa page
// indexée. La liste n'est jamais demandée avant la première synchro de la page.
import { expect, test, type Page } from "@playwright/test";
import type { Backlinks } from "@mementum/protocol";
import {
  apiAs,
  closeContexts,
  createPage,
  guestOf,
  mention,
  newUser,
  openPage,
  ownerAndMember,
  putMmd,
  signUpAccount,
  treeRow,
  unique,
  waitLinked,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

const section = (p: Page) => p.getByRole("navigation", { name: "Pages qui mènent ici" });
const row = (p: Page, id: string) => section(p).locator(`[data-backlink-page="${id}"]`);
const BACKLINKS = /\/api\/v1\/pages\/[^/]+\/backlinks$/;

test("a_mention_makes_a_backlink", async ({ browser }) => {
  const tag = unique();
  const { alice, aliceApi, space, bruno } = await ownerAndMember();
  const target = await createPage(aliceApi, { spaceId: space.id, title: `Cible ${tag}` });
  const source = await createPage(aliceApi, { spaceId: space.id, title: `Source ${tag}` });

  // Tom a la page visée ouverte. Son horloge est la nôtre : on n'attend pas pour de bon les cinq
  // secondes qui séparent deux lectures au retour sur l'onglet.
  const tom = await newUser(browser, bruno);
  await tom.clock.install();
  const first = tom.waitForResponse((res) => BACKLINKS.test(res.url()));
  await tom.goto(`/p/${target.id}`);
  await waitSynced(tom);
  // La première lecture est revenue, vide : pas de section, pas même son titre.
  expect(((await (await first).json()) as Backlinks).pages).toEqual([]);
  await expect(tom.getByText("Pages qui mènent ici")).toHaveCount(0);

  // Mona mentionne la page visée dans une autre page.
  const mona = await openPage(browser, source.id, alice);
  await mona.locator(".mm-editor .bn-block-content").first().click();
  await mona.keyboard.type(`Voir @Cible ${tag}`);
  await expect(mona.getByRole("listbox", { name: "Mentionner" }).getByRole("option")).toHaveText([
    `Cible ${tag}`,
  ]);
  await mona.keyboard.press("Enter");
  await expect(mona.locator(".mm-editor .mm-mention")).toHaveText([`@Cible ${tag}`]);
  await waitLinked(aliceApi, target.id, [source.id]);

  // Rien ne l'a poussé à l'onglet de Tom : c'est son retour sur l'onglet qui relit. Derrière une
  // barrière : une page créée après que le serveur a rendu le lien est arrivée dans l'arbre de Tom
  // par le canal de l'espace ; ce qui aurait été poussé avec le lien serait arrivé avant elle.
  const barrier = await createPage(aliceApi, { spaceId: space.id, title: `Barrière ${tag}` });
  await expect(treeRow(tom, barrier.id)).toBeVisible();
  await expect(section(tom)).toHaveCount(0);
  const reread = tom.waitForResponse((res) => BACKLINKS.test(res.url()));
  await tom.clock.fastForward(5_500);
  await tom.evaluate(() => window.dispatchEvent(new Event("focus")));
  await reread;
  await expect(row(tom, source.id)).toBeVisible();
  // Le titre de l'arbre, et l'extrait du bloc qui mentionne.
  await expect(row(tom, source.id).locator(".mm-backlink-name")).toHaveText(`Source ${tag}`);
  await expect(row(tom, source.id).locator(".mm-backlink-excerpt")).toContainText("Voir");
  await expect(section(tom).getByRole("listitem")).toHaveCount(1);
  // Sous le texte de la page.
  const editor = (await tom.locator(".mm-editor").boundingBox())!;
  const below = (await section(tom).boundingBox())!;
  expect(below.y).toBeGreaterThanOrEqual(editor.y + editor.height);

  // Le lien mène à la page source, sans recharger l'application ; le lien suivi part avec la page
  // quittée, et le focus ne tombe pas sur `body`.
  await tom.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));
  await row(tom, source.id).click();
  await expect(tom).toHaveURL(new RegExp(`/p/${source.id}`));
  await waitSynced(tom);
  expect(await tom.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBe(true);
  // Le bloc de la mention prend le focus une fois la page synchronisée et l'éditeur monté.
  await expect
    .poll(() => tom.evaluate(() => document.activeElement?.closest(".mm-editor") != null))
    .toBe(true);
  // La mention de la page source est là.
  await expect(tom.locator(".mm-editor .mm-mention")).toHaveText([`@Cible ${tag}`]);
});

test("a_backlink_opens_the_source_page_at_the_block_that_links", async ({ browser }) => {
  const tag = unique();
  const account = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(account);
  const target = await createPage(api, { title: `Cible ${tag}` });
  // Une longue page source : le bloc qui mentionne la page visée est loin sous la fenêtre.
  const source = await createPage(api, { title: `Source ${tag}` });
  const lines = Array.from({ length: 120 }, (_, i) => `Paragraphe ${i + 1} de la source.`);
  lines[99] = `Le lien est ici ${mention(target.id, target.title)}`;
  await putMmd(api, source.id, `${lines.join("\n\n")}\n`);
  await waitLinked(api, target.id, [source.id]);

  const p = await openPage(browser, target.id, account);
  // La ligne est un vrai lien vers la page source, à son bloc.
  await expect(row(p, source.id)).toHaveAttribute("href", new RegExp(`^/p/${source.id}#b=.+$`));
  await p.evaluate(() => ((window as unknown as { kept: boolean }).kept = true));
  await row(p, source.id).click();
  await expect(p).toHaveURL(new RegExp(`/p/${source.id}#b=.+$`));
  await waitSynced(p);
  expect(await p.evaluate(() => (window as unknown as { kept?: boolean }).kept)).toBe(true);
  // Le bloc qui mène à la page visée est à l'écran, et c'est celui de l'adresse.
  const blockId = decodeURIComponent(new URL(p.url()).hash.slice("#b=".length));
  const block = p.locator('.mm-editor [data-node-type="blockOuter"]').filter({ hasText: "Le lien est ici" });
  await expect(block).toBeInViewport();
  await expect(block).toHaveAttribute("data-id", blockId);
  // Le focus est dans l'éditeur, le curseur dans ce bloc ; pas sur le fil d'Ariane, ni sur `body`.
  await expect
    .poll(() =>
      p.evaluate(() => {
        const node = document.getSelection()?.anchorNode ?? null;
        const element = node instanceof Element ? node : (node?.parentElement ?? null);
        return {
          inEditor: document.activeElement?.closest(".mm-editor") != null,
          block: element?.closest('[data-node-type="blockOuter"]')?.getAttribute("data-id") ?? null,
        };
      }),
    )
    .toEqual({ inEditor: true, block: blockId });
  // « Précédent » revient à la page visée, et sa liste est relue après la synchro.
  await p.goBack();
  await expect(p).toHaveURL(new RegExp(`/p/${target.id}$`));
  await expect(row(p, source.id)).toBeVisible();
});

test("a_guest_sees_only_the_backlinks_of_pages_he_sees", async ({ browser }) => {
  const tag = unique();
  const alice = await signUpAccount(`Alice ${unique()}`);
  const aliceApi = await apiAs(alice);
  const target = await createPage(aliceApi, { title: `Cible ${tag}` });
  // Une sous-page de la page visée, que l'invité verra par héritage ; une page de l'espace, qu'il
  // ne verra pas. Les deux mentionnent la page visée.
  const seen = await createPage(aliceApi, { parentId: target.id, title: `Vue ${tag}` });
  const unseen = await createPage(aliceApi, { title: `Cachée ${tag}` });
  await putMmd(aliceApi, seen.id, `Depuis la sous-page ${mention(target.id, target.title)}\n`);
  await putMmd(aliceApi, unseen.id, `Secret ${tag} ${mention(target.id, target.title)}\n`);
  // Les droits sont posés avant d'ouvrir les pages.
  const guest = await guestOf(alice, target.id, `Invité ${unique()}`, "read");
  await waitLinked(aliceApi, target.id, [seen.id, unseen.id]);

  // Témoin : la propriétaire voit les deux.
  const owner = await openPage(browser, target.id, alice);
  await expect(row(owner, seen.id)).toBeVisible();
  await expect(row(owner, unseen.id)).toBeVisible();

  // L'invité : la réponse faite à son onglet ne porte que la sous-page, et l'écran aussi.
  const p = await newUser(browser, guest);
  const answered = p.waitForResponse((res) => BACKLINKS.test(res.url()));
  await p.goto(`/p/${target.id}`);
  const body = await (await answered).text();
  expect((JSON.parse(body) as Backlinks).pages.map((link) => link.pageId)).toEqual([seen.id]);
  expect(body).not.toContain(unseen.id);
  expect(body).not.toContain(`Secret ${tag}`);
  await expect(row(p, seen.id)).toBeVisible();
  await expect(row(p, seen.id).locator(".mm-backlink-name")).toHaveText(`Vue ${tag}`);
  await expect(section(p).getByRole("listitem")).toHaveCount(1);
  await expect(p.getByText(`Cachée ${tag}`)).toHaveCount(0);
  await expect(p.getByText(`Secret ${tag}`)).toHaveCount(0);
});

test("opening_a_page_asks_for_backlinks_after_its_first_sync", async ({ browser }) => {
  const tag = unique();
  const account = await signUpAccount(`Alice ${unique()}`);
  const api = await apiAs(account);
  const target = await createPage(api, { title: `Cible ${tag}` });
  const source = await createPage(api, { title: `Source ${tag}` });
  await putMmd(api, source.id, `Vers ${mention(target.id, target.title)}\n`);
  await waitLinked(api, target.id, [source.id]);

  const p = await newUser(browser, account);
  // La première synchro, vue sur le réseau : la connexion de synchro de la page a reçu l'étape 2
  // du serveur (message « sync » 0, sous-type 1), qui porte le document. L'attribut `data-synced`
  // de l'éditeur ne le dirait pas assez tôt : son code se charge pendant que la session se connecte.
  let syncSockets = 0;
  let synced = false;
  p.on("websocket", (socket) => {
    if (!/\/sync\//.test(socket.url())) return;
    syncSockets += 1;
    socket.on("framereceived", ({ payload }) => {
      if (typeof payload !== "string" && payload[0] === 0 && payload[1] === 1) synced = true;
    });
  });
  /** Chaque demande de rétroliens : la première synchro était-elle faite ? */
  const asked: boolean[] = [];
  p.on("request", (request) => {
    if (BACKLINKS.test(request.url())) asked.push(synced);
  });
  // La connexion de synchro ne s'ouvre qu'une fois la page relue (`usePageGate`) : cette lecture
  // retenue, la première synchro ne peut pas se faire. La première lecture de la page, celle par
  // laquelle l'application l'ouvre, passe, comme tout le reste de l'ouverture (arbre, favoris).
  let release = () => {};
  const released = new Promise<void>((resolve) => (release = resolve));
  let held = () => {};
  const gateAsked = new Promise<void>((resolve) => (held = resolve));
  let pageReads = 0;
  await p.route(`**/api/v1/pages/${target.id}`, async (route) => {
    if (route.request().method() === "GET" && ++pageReads > 1) {
      held();
      await released;
    }
    await route.continue();
  });
  const favorites = p.waitForResponse((res) => /\/favorites$/.test(res.url()));
  await p.goto(`/p/${target.id}`);
  await gateAsked;
  await favorites;
  await expect(p.locator(".mm-title-input")).toHaveValue(target.title);
  // L'ouverture est faite, la synchro non : rien n'a été demandé.
  expect(syncSockets).toBe(0);
  expect(asked).toEqual([]);

  release();
  await waitSynced(p);
  await expect(row(p, source.id)).toBeVisible();
  // Une seule demande, partie une fois la première synchro faite.
  expect(asked).toEqual([true]);
});
