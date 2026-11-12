// Un bloc déplacé pendant que quelqu'un d'autre y écrit (limite R39 du lot 1, que la Task 13
// lève, R138). Yjs n'a pas de déplacement, et ce qu'une autre personne a tapé en même temps dans
// le bloc ne le suit pas. Dans le navigateur, un glisser n'est même pas une copie suivie d'une
// suppression : y-prosemirror récrit sur place les éléments Yjs entre l'ancienne place et la
// nouvelle, chacun prenant l'id et le texte de son voisin (constat de la Task 12, au registre).
//
// Ces tests disent ce qui est voulu, pas ce qui se passe aujourd'hui : après le scénario, le
// bloc X est juste sous le bloc Z, une seule fois, et porte le texte que Tom y a tapé, chez
// Mona, chez Tom et dans le MMD que relit le serveur. Deux ordres d'arrivée au serveur, un
// test chacun : l'édition faite hors ligne arrive après le déplacement, puis le déplacement
// fait hors ligne arrive après l'édition. Chaque test vérifie d'abord, sur des blocs témoins,
// que le glisser par la poignée et le retour du réseau marchent seuls : un échec ne vient donc
// pas d'un glisser manqué ni d'une page qui n'a pas resynchronisé.
//
// Ce qu'ils montrent aujourd'hui est au registre du lot 3, sous « Task 12 » : perte connue (R39),
// d'où `test.fixme` jusqu'à la Task 13.
import {
  expect,
  test,
  type APIRequestContext,
  type Browser,
  type Locator,
  type Page,
} from "@playwright/test";
import {
  closeContexts,
  createPage,
  fragmentXml,
  getMmd,
  openPage,
  ownerAndMember,
  putMmd,
  setPageLevel,
  unique,
} from "./helpers";

test.afterEach(closeContexts);

/** Le contenu propre d'un bloc (son paragraphe), sans ses enfants. */
const OWN_CONTENT = ":scope > .bn-block > .bn-block-content";

/** La page de départ : les trois blocs du scénario, puis trois témoins. */
const START = "Bloc X\n\nBloc Y\n\nBloc Z\n\nTémoin A\n\nTémoin B\n\nTémoin C\n";

/** Ce que Tom tape dans le bloc X. */
const TYPED = " de Tom";

type Shown = { id: string; text: string };

/**
 * Les blocs de premier niveau que montre l'éditeur de `p`, de haut en bas : id et texte. Le
 * curseur d'une autre personne, posé dans un bloc, y écrit son nom : il n'est pas compté.
 */
function shownBlocks(p: Page): Promise<Shown[]> {
  return p
    .locator('.bn-editor > [data-node-type="blockGroup"] > [data-node-type="blockOuter"]')
    .evaluateAll((outers) =>
      outers.map((outer) => {
        const content = outer.querySelector(".bn-block-content")?.cloneNode(true) as
          | Element
          | undefined;
        content
          ?.querySelectorAll(".bn-collaboration-cursor__base")
          .forEach((cursor) => cursor.remove());
        return { id: outer.getAttribute("data-id") ?? "", text: content?.textContent ?? "" };
      }),
    );
}

/** Les textes des blocs de premier niveau de `p`, de haut en bas. */
async function shownTexts(p: Page): Promise<string[]> {
  return (await shownBlocks(p)).map((b) => b.text);
}

/** Le seul bloc de l'éditeur de `p` dont le texte est `text`, retrouvé par son id. */
async function blockOf(p: Page, text: string): Promise<Locator> {
  const blocks = await shownBlocks(p);
  const found = blocks.filter((b) => b.text === text);
  if (found.length !== 1) {
    const texts = JSON.stringify(blocks.map((b) => b.text));
    throw new Error(`${found.length} blocs « ${text} » dans ${texts}`);
  }
  return p.locator(`.bn-editor [data-node-type="blockOuter"][data-id="${found[0].id}"]`);
}

/** `blocks`, où le bloc de texte `moved` est retiré, puis remis juste sous le bloc `under`. */
function movedUnder(blocks: Shown[], moved: string, under: string): Shown[] {
  const block = blocks.find((b) => b.text === moved);
  if (!block) throw new Error(`pas de bloc « ${moved} » dans ${JSON.stringify(blocks)}`);
  const rest = blocks.filter((b) => b !== block);
  const at = rest.findIndex((b) => b.text === under);
  if (at < 0) throw new Error(`pas de bloc « ${under} » dans ${JSON.stringify(blocks)}`);
  return [...rest.slice(0, at + 1), block, ...rest.slice(at + 1)];
}

/**
 * Chez `p`, déplace le bloc de texte `moved` juste sous celui de texte `under`, comme une
 * personne : le bloc survolé montre sa poignée (BlockNote), saisie à la souris, puis lâchée sur
 * le bas du bloc visé. Vérifie que le bloc a changé de place chez `p`, avec son id et son texte,
 * et que rien d'autre n'a bougé.
 */
async function dragUnder(p: Page, moved: string, under: string): Promise<void> {
  const before = await shownBlocks(p);
  await (await blockOf(p, moved)).locator(OWN_CONTENT).hover();
  const handle = p.getByRole("button", { name: "Ouvrir le menu du bloc" });
  await expect(handle).toBeVisible();
  const grip = (await handle.boundingBox())!;
  const target = (await (await blockOf(p, under)).locator(OWN_CONTENT).boundingBox())!;
  await p.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await p.mouse.down();
  await p.mouse.move(target.x + target.width / 2, target.y + target.height * 0.8, { steps: 8 });
  await p.mouse.up();
  await expect
    .poll(() => shownBlocks(p), { message: `le glisser de « ${moved} » sous « ${under} »` })
    .toEqual(movedUnder(before, moved, under));
}

/** Tape `typed` à la fin du bloc de texte `text`, chez `p`, et attend que l'éditeur le montre. */
async function typeAtEnd(p: Page, text: string, typed: string): Promise<void> {
  await (await blockOf(p, text)).locator(OWN_CONTENT).click();
  await p.keyboard.press("End");
  await p.keyboard.type(typed);
  await expect.poll(() => shownTexts(p)).toContain(text + typed);
}

/** Coupe `p` du réseau, comme `offline.spec.ts`. */
async function goOffline(p: Page): Promise<void> {
  await p.context().setOffline(true);
  await p.evaluate(() => window.__mementum!.provider.disconnect());
}

/** Rend le réseau à `p` et attend que sa connexion de synchro ait reçu l'état du serveur. */
async function comeBack(p: Page): Promise<void> {
  await p.context().setOffline(false);
  await p.evaluate(() => window.__mementum!.provider.connect());
  await expect
    .poll(() => p.evaluate(() => window.__mementum!.provider.synced), { timeout: 15_000 })
    .toBe(true);
}

/** Les documents Yjs de `a` et de `b` sont les mêmes : ce que l'un a fait, l'autre l'a reçu. */
async function converged(a: Page, b: Page): Promise<void> {
  await expect
    .poll(async () => (await fragmentXml(a)) === (await fragmentXml(b)), {
      timeout: 15_000,
      message: "les documents de Mona et de Tom n'ont pas convergé",
    })
    .toBe(true);
}

/** Le MMD de la page, un bloc par entrée (les paragraphes y sont séparés par une ligne vide). */
async function mmdBlocks(api: APIRequestContext, id: string): Promise<string[]> {
  return (await getMmd(api, id)).trimEnd().split("\n\n");
}

/** Une page que Mona possède et où Tom écrit, ouverte chez les deux, avec les ids de X, Y et Z. */
async function scene(browser: Browser) {
  const {
    alice: mona,
    aliceApi: monaApi,
    space,
    bruno: tom,
    brunoId: tomId,
  } = await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Bloc déplacé ${unique()}` });
  await setPageLevel(monaApi, page.id, tomId, "edit");
  await putMmd(monaApi, page.id, START);
  const monaPage = await openPage(browser, page.id, mona);
  const tomPage = await openPage(browser, page.id, tom);
  const shown = await shownBlocks(monaPage);
  expect(shown.map((b) => b.text)).toEqual([
    "Bloc X",
    "Bloc Y",
    "Bloc Z",
    "Témoin A",
    "Témoin B",
    "Témoin C",
  ]);
  expect(await shownBlocks(tomPage)).toEqual(shown);
  const [x, y, z] = shown.map((b) => b.id);
  return { monaApi, id: page.id, mona: monaPage, tom: tomPage, ids: { x, y, z } };
}

type Scene = Awaited<ReturnType<typeof scene>>;

type Ids = { x: string; y: string; z: string };

/** Les textes de la page voulue, de haut en bas : Y, Z, X avec le texte de Tom, puis `rest`. */
function wantedTexts(rest: string[]): string[] {
  return ["Bloc Y", "Bloc Z", `Bloc X${TYPED}`, ...rest];
}

/**
 * Vérifie chez Mona, chez Tom et dans le MMD du serveur ce que la Task 13 doit rendre vrai.
 * Le serveur peut récrire la page juste après avoir relayé une mise à jour, et Mona et Tom
 * peuvent avoir convergé avant de recevoir cette récriture : la page voulue est d'abord attendue
 * partout, 15 s au plus, sans rien affirmer ; puis chaque exigence est relevée sur ce qui est là.
 */
async function expectWantedEverywhere(s: Scene, rest: string[]): Promise<void> {
  const wanted = JSON.stringify(wantedTexts(rest));
  const isWanted = (blocks: Shown[]) =>
    JSON.stringify(blocks.map((b) => b.text)) === wanted &&
    [s.ids.y, s.ids.z, s.ids.x].every((id, i) => blocks[i]?.id === id);
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const [mona, tom, mmd] = await Promise.all([
      shownBlocks(s.mona),
      shownBlocks(s.tom),
      mmdBlocks(s.monaApi, s.id),
    ]);
    if (isWanted(mona) && isWanted(tom) && JSON.stringify(mmd) === wanted) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  expectWanted("Chez Mona", await shownBlocks(s.mona), s.ids, rest);
  expectWanted("Chez Tom", await shownBlocks(s.tom), s.ids, rest);
  expectWantedMmd(await mmdBlocks(s.monaApi, s.id), rest);
}

/**
 * Ce que la Task 13 doit rendre vrai, vu chez `who` (`blocks`, ses blocs de premier niveau) :
 * une exigence par assertion, chacune nommée, toutes relevées (`expect.soft`). `rest` : les
 * témoins, tels qu'ils doivent être sous X.
 */
function expectWanted(who: string, blocks: Shown[], ids: Ids, rest: string[]): void {
  const seen = JSON.stringify(blocks.map((b) => b.text));
  const xs = blocks.filter((b) => b.id === ids.x);
  expect
    .soft(xs.length, `${who} : le bloc X est dans la page une seule fois (vu : ${seen})`)
    .toBe(1);
  const at = blocks.findIndex((b) => b.id === ids.x);
  expect
    .soft(at > 0 ? blocks[at - 1].id : null, `${who} : X est juste sous Z (vu : ${seen})`)
    .toBe(ids.z);
  expect
    .soft(xs[0]?.text, `${who} : X porte le texte que Tom y a tapé (vu : ${seen})`)
    .toBe(`Bloc X${TYPED}`);
  const elsewhere = blocks.filter((b) => b.id !== ids.x && b.text.includes(TYPED.trim()));
  expect
    .soft(elsewhere, `${who} : le texte de Tom n'est dans aucun autre bloc (vu : ${seen})`)
    .toEqual([]);
  expect
    .soft(blocks, `${who} : la page entière`)
    .toEqual([
      { id: ids.y, text: "Bloc Y" },
      { id: ids.z, text: "Bloc Z" },
      { id: ids.x, text: `Bloc X${TYPED}` },
      ...rest.map((text) => ({ id: expect.any(String), text })),
    ]);
}

/** Les mêmes exigences, sur le MMD du serveur, où les blocs n'ont que leur texte. */
function expectWantedMmd(blocks: string[], rest: string[]): void {
  const seen = JSON.stringify(blocks);
  const xs = blocks.filter((b) => b.startsWith("Bloc X"));
  expect
    .soft(xs.length, `MMD du serveur : le bloc X y est une seule fois (vu : ${seen})`)
    .toBe(1);
  const at = blocks.findIndex((b) => b.startsWith("Bloc X"));
  expect
    .soft(at > 0 ? blocks[at - 1] : null, `MMD du serveur : X est juste sous Z (vu : ${seen})`)
    .toBe("Bloc Z");
  expect
    .soft(xs[0], `MMD du serveur : X porte le texte de Tom (vu : ${seen})`)
    .toBe(`Bloc X${TYPED}`);
  const elsewhere = blocks.filter((b) => !b.startsWith("Bloc X") && b.includes(TYPED.trim()));
  expect
    .soft(elsewhere, `MMD du serveur : le texte de Tom n'est dans aucun autre bloc (vu : ${seen})`)
    .toEqual([]);
  expect
    .soft(blocks, "MMD du serveur : la page entière")
    .toEqual(wantedTexts(rest));
}

test.fixme("text_typed_while_someone_else_moves_the_block_survives", async ({ browser }) => {
  const s = await scene(browser);
  const { monaApi, id, mona, tom } = s;

  // Témoin 1 : le glisser seul, tout le monde en ligne. Tom et le serveur le reçoivent.
  await dragUnder(mona, "Témoin A", "Témoin C");
  const afterMove = await shownBlocks(mona);
  await expect.poll(() => shownBlocks(tom)).toEqual(afterMove);
  await expect
    .poll(() => mmdBlocks(monaApi, id))
    .toEqual(["Bloc X", "Bloc Y", "Bloc Z", "Témoin B", "Témoin C", "Témoin A"]);

  // Témoin 2 : une frappe faite hors ligne, seule, arrive au retour, chez Mona et au serveur.
  await goOffline(tom);
  await typeAtEnd(tom, "Témoin B", " revenu");
  await comeBack(tom);
  await converged(mona, tom);
  await expect.poll(() => shownTexts(mona)).toContain("Témoin B revenu");
  await expect
    .poll(() => mmdBlocks(monaApi, id))
    .toEqual(["Bloc X", "Bloc Y", "Bloc Z", "Témoin B revenu", "Témoin C", "Témoin A"]);

  // Le scénario : Tom, coupé du réseau, tape dans X ; Mona, en ligne, glisse X sous Z.
  await goOffline(tom);
  await typeAtEnd(tom, "Bloc X", TYPED);
  await dragUnder(mona, "Bloc X", "Bloc Z");
  // Le déplacement est au serveur ; le texte de Tom n'est encore que chez lui.
  await expect
    .poll(() => mmdBlocks(monaApi, id))
    .toEqual(["Bloc Y", "Bloc Z", "Bloc X", "Témoin B revenu", "Témoin C", "Témoin A"]);
  expect((await shownTexts(tom)).slice(0, 3)).toEqual([
    `Bloc X${TYPED}`,
    "Bloc Y",
    "Bloc Z",
  ]);

  // Tom revient : son édition arrive au serveur après le déplacement.
  await comeBack(tom);
  await converged(mona, tom);
  await expectWantedEverywhere(s, ["Témoin B revenu", "Témoin C", "Témoin A"]);
});

test.fixme("block_moved_offline_keeps_the_text_typed_meanwhile", async ({ browser }) => {
  const s = await scene(browser);
  const { monaApi, id, mona, tom } = s;

  // Témoin 1 : un glisser fait hors ligne, seul, arrive au retour, chez Tom et au serveur.
  await goOffline(mona);
  await dragUnder(mona, "Témoin A", "Témoin C");
  const afterMove = await shownBlocks(mona);
  await comeBack(mona);
  await converged(mona, tom);
  expect(await shownBlocks(tom)).toEqual(afterMove);
  await expect
    .poll(() => mmdBlocks(monaApi, id))
    .toEqual(["Bloc X", "Bloc Y", "Bloc Z", "Témoin B", "Témoin C", "Témoin A"]);

  // Témoin 2 : une frappe seule, tout le monde en ligne. Mona et le serveur la reçoivent.
  await typeAtEnd(tom, "Témoin B", " reçu");
  await expect.poll(() => shownTexts(mona)).toContain("Témoin B reçu");
  await expect
    .poll(() => mmdBlocks(monaApi, id))
    .toEqual(["Bloc X", "Bloc Y", "Bloc Z", "Témoin B reçu", "Témoin C", "Témoin A"]);

  // Le scénario : Mona, coupée du réseau, glisse X sous Z ; Tom, en ligne, tape dans X.
  await goOffline(mona);
  await dragUnder(mona, "Bloc X", "Bloc Z");
  await typeAtEnd(tom, "Bloc X", TYPED);
  // L'édition est au serveur ; le déplacement n'est encore que chez Mona.
  await expect
    .poll(() => mmdBlocks(monaApi, id))
    .toEqual([`Bloc X${TYPED}`, "Bloc Y", "Bloc Z", "Témoin B reçu", "Témoin C", "Témoin A"]);
  expect((await shownTexts(mona)).slice(0, 3)).toEqual([
    "Bloc Y",
    "Bloc Z",
    "Bloc X",
  ]);

  // Mona revient : son déplacement arrive au serveur après l'édition.
  await comeBack(mona);
  await converged(mona, tom);
  await expectWantedEverywhere(s, ["Témoin B reçu", "Témoin C", "Témoin A"]);
});
