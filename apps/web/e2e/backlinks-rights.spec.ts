// Les droits des rétroliens, de bout en bout (lot 4, Task X1) : « Pages qui mènent ici » ne nomme
// que des pages du même espace que le compte lit, et tient devant un droit qui change pendant que
// la page visée est ouverte.
//
// Rien ne pousse un changement de rétroliens : la liste se relit au retour sur l'onglet (cinq
// secondes au moins après la dernière demande) ; une source qui quitte l'arbre de l'onglet quitte la
// liste sans relecture. L'index suit une écriture après un temps de calme : `waitLinked` avant
// d'ouvrir les onglets, et les droits posés avant aussi.
import { expect, test, type Page } from "@playwright/test";
import type { Backlinks } from "@mementum/protocol";
import type * as Y from "yjs";
import {
  accessLost,
  apiAs,
  closeContexts,
  closeEventsChannel,
  createPage,
  editRawDoc,
  firstSpace,
  hideSyncClosures,
  holdTreeReads,
  joinSpace,
  mention,
  newUser,
  openPage,
  ownerAndMember,
  putMmd,
  readerBanner,
  setPageLevel,
  signUpAccount,
  titleOf,
  treeRow,
  unique,
  waitFindable,
  waitLinked,
  waitSynced,
  word,
} from "./helpers";

test.afterEach(closeContexts);

const section = (p: Page) => p.getByRole("navigation", { name: "Pages qui mènent ici" });
const row = (p: Page, id: string) => section(p).locator(`[data-backlink-page="${id}"]`);
const BACKLINKS = /\/api\/v1\/pages\/[^/]+\/backlinks$/;
/** Les demandes de rétroliens de l'onglet, comptées dès cet appel. */
function backlinkReads(p: Page): () => number {
  let reads = 0;
  p.on("request", (request) => {
    if (BACKLINKS.test(new URL(request.url()).pathname)) reads += 1;
  });
  return () => reads;
}
const focusOnBody = (p: Page) => p.evaluate(() => document.activeElement === document.body);

test("a_source_that_leaves_while_the_target_is_open_leaves_the_list", async ({ browser }) => {
  const tag = unique();
  const { aliceApi, space, bruno, brunoApi, brunoId } = await ownerAndMember();
  const target = await createPage(aliceApi, { spaceId: space.id, title: `Cible ${tag}` });
  // Deux sources privées d'Alice, que Bruno lit par un accès à son nom, et une de l'espace.
  const withdrawn = await createPage(aliceApi, { title: `Retirée ${tag}`, section: "private" });
  const thrown = await createPage(aliceApi, { title: `Jetée ${tag}`, section: "private" });
  const stays = await createPage(aliceApi, { spaceId: space.id, title: `Reste ${tag}` });
  for (const source of [withdrawn, thrown, stays]) {
    await putMmd(aliceApi, source.id, `Depuis ${source.title} ${mention(target.id, target.title)}\n`);
  }
  await setPageLevel(aliceApi, withdrawn.id, brunoId, "read");
  await setPageLevel(aliceApi, thrown.id, brunoId, "read");
  await waitLinked(brunoApi, target.id, [withdrawn.id, thrown.id, stays.id]);

  const p = await newUser(browser, bruno);
  const reads = backlinkReads(p);
  await p.goto(`/p/${target.id}`);
  await waitSynced(p);
  for (const source of [withdrawn, thrown, stays]) await expect(row(p, source.id)).toBeVisible();
  expect(reads()).toBe(1);

  // L'accès à une source est retiré, le pointeur hors de la section : sa ligne part quand l'arbre
  // de l'onglet ne la porte plus, sans que les rétroliens soient relus.
  await p.mouse.move(5, 5);
  expect((await aliceApi.delete(`/api/v1/pages/${withdrawn.id}/access/users/${brunoId}`)).status()).toBe(200);
  await expect(treeRow(p, withdrawn.id)).toHaveCount(0);
  await expect(row(p, withdrawn.id)).toHaveCount(0);
  await expect(section(p)).not.toContainText(withdrawn.title);
  await expect(row(p, thrown.id)).toBeVisible();
  expect(reads()).toBe(1);

  // Une source part à la corbeille, le pointeur sur sa ligne : rien ne bouge dessous. La ligne
  // reste, sans titre ni extrait (« Page indisponible »), jusqu'à ce que le pointeur sorte.
  await row(p, thrown.id).hover();
  expect((await aliceApi.post(`/api/v1/pages/${thrown.id}/trash`)).status()).toBe(200);
  await expect(treeRow(p, thrown.id)).toHaveCount(0);
  await expect(row(p, thrown.id).locator(".mm-backlink-name")).toHaveText("Page indisponible");
  await expect(row(p, thrown.id).locator(".mm-backlink-excerpt")).toHaveCount(0);
  await expect(section(p)).not.toContainText(thrown.title);
  await p.mouse.move(5, 5);
  await expect(row(p, thrown.id)).toHaveCount(0);
  // Celle qui reste est là, et rien n'a été redemandé au serveur.
  await expect(row(p, stays.id)).toBeVisible();
  await expect(section(p).getByRole("listitem")).toHaveCount(1);
  expect(reads()).toBe(1);

  // Au retour suivant, la route ne les rend plus non plus.
  const body = await (await brunoApi.get(`/api/v1/pages/${target.id}/backlinks`)).text();
  expect((JSON.parse(body) as Backlinks).pages.map((link) => link.pageId)).toEqual([stays.id]);
  for (const leak of [withdrawn.id, thrown.id, "Retirée", "Jetée"]) expect(body, leak).not.toContain(leak);
});

test("the_404_of_backlinks_is_said_by_the_page_read_again", async ({ browser }) => {
  const tag = unique();
  const { aliceApi, bruno, brunoApi, brunoId } = await ownerAndMember();
  const target = await createPage(aliceApi, { title: `Cible ${tag}`, section: "private" });
  const source = await createPage(aliceApi, { title: `Source ${tag}`, section: "private" });
  await putMmd(aliceApi, source.id, `Vers ${mention(target.id, target.title)}\n`);
  await setPageLevel(aliceApi, target.id, brunoId, "read");
  await setPageLevel(aliceApi, source.id, brunoId, "read");
  await waitLinked(brunoApi, target.id, [source.id]);

  // L'onglet de Bruno n'apprendra rien d'un changement de droit par ses deux voies ordinaires : le
  // canal de l'espace est fermé, les fermetures de synchro lui sont cachées.
  const p = await newUser(browser, bruno);
  await p.clock.install();
  await closeEventsChannel(p);
  await hideSyncClosures(p);
  await p.goto(`/p/${target.id}`);
  await waitSynced(p);
  await expect(row(p, source.id)).toBeVisible();
  // Ni par l'arbre, relu au retour sur l'onglet : ses lectures sont retenues à partir d'ici.
  const tree = await holdTreeReads(p);

  expect((await aliceApi.delete(`/api/v1/pages/${target.id}/access/users/${brunoId}`)).status()).toBe(200);
  expect((await brunoApi.get(`/api/v1/pages/${target.id}`)).status()).toBe(404);
  // L'onglet ne le sait pas encore : la page et sa liste sont là.
  await expect(row(p, source.id)).toBeVisible();

  // Le retour sur l'onglet relit les rétroliens : `404`, la réponse d'une page inconnue.
  const reread = p.waitForResponse((res) => BACKLINKS.test(new URL(res.url()).pathname));
  await p.clock.fastForward(5_500);
  await p.evaluate(() => window.dispatchEvent(new Event("focus")));
  const refused = await reread;
  expect(refused.status()).toBe(404);
  const unknown = await brunoApi.get("/api/v1/pages/00000000-0000-7000-8000-000000000000/backlinks");
  expect(await refused.text()).toBe(await unknown.text());
  // C'est la page relue qui le dit, l'arbre toujours retenu : plus de page, plus de liste, pas de
  // message d'erreur des rétroliens resté à l'écran, rien de la source.
  await expect(accessLost(p)).toBeVisible();
  await expect(section(p)).toHaveCount(0);
  await expect(p.locator(".mm-backlinks-error")).toHaveCount(0);
  await expect(p.locator("main")).not.toContainText(source.title);
  await expect(p.locator(".mm-editor")).toHaveCount(0);
  tree.release();
});

test("a_reader_follows_a_backlink_to_its_block", async ({ browser }) => {
  const tag = unique();
  const { aliceApi, bruno, brunoApi, brunoId } = await ownerAndMember();
  const target = await createPage(aliceApi, { title: `Cible ${tag}`, section: "private" });
  const source = await createPage(aliceApi, { title: `Source ${tag}`, section: "private" });
  const lines = Array.from({ length: 120 }, (_, i) => `Paragraphe ${i + 1} de la source.`);
  lines[99] = `Le lien est ici ${mention(target.id, target.title)}`;
  await putMmd(aliceApi, source.id, `${lines.join("\n\n")}\n`);
  // Bruno ne fait que lire les deux pages, qui vivent.
  await setPageLevel(aliceApi, target.id, brunoId, "read");
  await setPageLevel(aliceApi, source.id, brunoId, "read");
  await waitLinked(brunoApi, target.id, [source.id]);

  const p = await openPage(browser, target.id, bruno);
  await expect(readerBanner(p)).toBeVisible();
  await expect(row(p, source.id)).toHaveAttribute("href", new RegExp(`^/p/${source.id}#b=.+$`));
  await row(p, source.id).click();
  await expect(p).toHaveURL(new RegExp(`/p/${source.id}#b=.+$`));
  await waitSynced(p);
  await expect(p.locator(".mm-editor")).toHaveAttribute("data-editable", "false");
  const blockId = decodeURIComponent(new URL(p.url()).hash.slice("#b=".length));
  const block = p.locator(".mm-editor [data-node-type='blockOuter']").filter({ hasText: "Le lien est ici" });
  await expect(block).toHaveAttribute("data-id", blockId);
  await expect(block).toBeInViewport();
  // Le focus est dans l'éditeur en lecture seule, la sélection dans le bloc ; pas sur `body`.
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
  await expect(p.locator('[role="alert"]')).toHaveCount(0);
});

test("a_backlink_without_block_opens_the_page_with_the_focus_on_the_breadcrumb", async ({ browser }) => {
  const tag = unique();
  const account = await signUpAccount(`Alice ${tag}`);
  const api = await apiAs(account);
  const target = await createPage(api, { title: `Cible ${tag}` });
  const source = await createPage(api, { title: `Source ${tag}` });
  await putMmd(api, source.id, `Vers ${mention(target.id, target.title)}\n`);
  await waitLinked(api, target.id, [source.id]);
  /** Les blocs que la route nomme pour les rétroliens de la page visée. */
  const linkingBlocks = async () => {
    const res = await api.get(`/api/v1/pages/${target.id}/backlinks`);
    return ((await res.json()) as Backlinks).pages.map((link) => link.blockId);
  };
  expect(await linkingBlocks()).toEqual([expect.any(String)]);
  // Un bloc sans `id` : ni l'éditeur ni `PUT /mmd` n'en écrivent, un client Yjs nu si. L'index lui
  // donne une clé de remplacement, qui ne nomme aucun bloc qu'on rejoint : `blockId` est nul.
  await editRawDoc(
    api,
    source.id,
    (root) => {
      for (const container of root.toArray() as Y.XmlElement[]) container.removeAttribute("id");
    },
    async () => (await linkingBlocks())[0] === null,
  );

  const p = await openPage(browser, target.id, account);
  // Le lien mène à la page, sans bloc.
  await expect(row(p, source.id)).toHaveAttribute("href", `/p/${source.id}`);
  await row(p, source.id).click();
  await expect(p).toHaveURL(new RegExp(`/p/${source.id}$`));
  await expect(titleOf(p)).toHaveValue(source.title);
  await waitSynced(p);
  // Un seul preneur de focus : le fil d'Ariane (R1128). Ni `body`, ni l'éditeur.
  await expect
    .poll(() => p.evaluate(() => document.activeElement?.closest(".mm-breadcrumb") != null))
    .toBe(true);
  expect(await focusOnBody(p)).toBe(false);
  await expect(p.locator(".mm-editor .mm-mention")).toHaveText([`@${target.title}`]);
});

test("a_link_from_another_space_is_never_listed", async ({ browser }) => {
  const tag = unique();
  const alice = await signUpAccount(`Alice ${tag}`);
  const aliceApi = await apiAs(alice);
  const here = await firstSpace(aliceApi);
  const bruno = await signUpAccount(`Bruno ${tag}`);
  const brunoApi = await apiAs(bruno);
  const there = await firstSpace(brunoApi);
  await joinSpace(brunoApi, there.id, alice, "member");
  // Alice lit les deux espaces. La page visée est du sien ; une source du sien, une de l'autre.
  const target = await createPage(aliceApi, { spaceId: here.id, title: `Cible ${tag}` });
  const near = await createPage(aliceApi, { spaceId: here.id, title: `Voisine ${tag}` });
  const far = await createPage(aliceApi, { spaceId: there.id, title: `Lointaine ${tag}` });
  const witness = word();
  await putMmd(aliceApi, near.id, `De près ${mention(target.id, target.title)}\n`);
  await putMmd(aliceApi, far.id, `De loin ${witness} ${mention(target.id, target.title)}\n`);
  // La source de l'autre espace est indexée (son texte se trouve), et la route ne la rend pas.
  await waitFindable(aliceApi, there.id, witness, far.id);
  await waitLinked(aliceApi, target.id, [near.id]);

  const p = await newUser(browser, alice);
  const answered = p.waitForResponse((res) => BACKLINKS.test(new URL(res.url()).pathname));
  await p.goto(`/p/${target.id}`);
  const body = await (await answered).text();
  expect((JSON.parse(body) as Backlinks).pages.map((link) => link.pageId)).toEqual([near.id]);
  for (const leak of [far.id, witness, "De loin", "Lointaine"]) expect(body, leak).not.toContain(leak);
  await waitSynced(p);
  await expect(row(p, near.id)).toBeVisible();
  await expect(section(p).getByRole("listitem")).toHaveCount(1);
  await expect(section(p)).not.toContainText("Lointaine");
  // Et dans l'autre sens : la page de l'autre espace, ouverte, ne liste rien de celui-ci.
  await putMmd(aliceApi, near.id, `De près ${mention(target.id, target.title)} et ${mention(far.id, far.title)}\n`);
  await waitFindable(aliceApi, here.id, "Lointaine", near.id);
  const other = p.waitForResponse((res) => BACKLINKS.test(new URL(res.url()).pathname));
  await p.goto(`/p/${far.id}`);
  expect(((await (await other).json()) as Backlinks).pages).toEqual([]);
  await waitSynced(p);
  await expect(section(p)).toHaveCount(0);
});
