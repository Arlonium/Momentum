// Un bloc déplacé pendant que quelqu'un d'autre écrit, dans un vrai navigateur : le glisser se
// fait par la poignée de BlockNote, à la souris, et passe par le dépôt de ProseMirror.
//
// Yjs n'a pas de déplacement. Depuis la forme à clés (R220, `patches/y-prosemirror@1.3.7.patch`,
// `src/editor/keyedBlocks.test.ts`), le bloc glissé est supprimé et recréé à sa nouvelle place
// avec le même id, et les blocs qu'il croise ne sont pas touchés. Avant, tous les blocs entre
// l'ancienne place et la nouvelle étaient récrits sur place, chacun prenant l'id et le texte de
// son voisin : ce qu'un autre tapait dans l'un d'eux finissait chez un voisin, et deux glissers
// abîmaient la page.
//
// Les tests verts : ce qui est tapé dans un bloc voisin pendant qu'un autre glisse reste à sa
// place ; deux personnes qui glissent deux blocs gardent la page ; le curseur de qui tape dans le
// bloc qu'un autre glisse le suit (`followBlock`), comme celui que le dépôt laisse dans le bloc
// déposé, et comme la sélection d'une image sur laquelle on a cliqué.
//
// Les deux `test.fixme` : ce qui est tapé dans le bloc glissé lui-même. Ils disent ce qui est
// voulu, pas ce qui se passe : après le scénario, le bloc X est juste sous le bloc Z, une seule
// fois, et porte le texte que Tom y a tapé, chez Mona, chez Tom et dans le MMD que relit le
// serveur. Les clients seuls perdent ce texte, qui est dans l'élément que le glisser supprime
// (plus personne ne le retrouve chez un voisin). Un test par ordre d'arrivée au serveur :
//  - le déplacement fait hors ligne arrive après l'édition : le serveur a le texte quand le
//    déplacement arrive, et le reportera sur le bloc recréé (R221, Task 13b) ;
//  - l'édition faite hors ligne arrive après le déplacement : elle a été faite sans l'avoir vu,
//    et reste perdue (limite R222, dite aux utilisateurs).
// Chacun vérifie d'abord, sur des blocs témoins, que le glisser par la poignée et le retour du
// réseau marchent seuls : un échec ne vient donc pas d'un glisser manqué ni d'une page qui n'a
// pas resynchronisé.
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
 * L'éditeur de la page. Pendant un glisser, BlockNote pose dans le document, hors de la page,
 * une copie du bloc glissé (l'image que suit la souris), avec les classes de l'éditeur : elle
 * n'est pas un bloc de la page.
 */
const EDITOR = ".mm-editor-frame .bn-editor";

/**
 * Les blocs de premier niveau que montre l'éditeur de `p`, de haut en bas : id et texte. Le
 * curseur d'une autre personne, posé dans un bloc, y écrit son nom : il n'est pas compté.
 */
function shownBlocks(p: Page): Promise<Shown[]> {
  return p
    .locator(`${EDITOR} > [data-node-type="blockGroup"] > [data-node-type="blockOuter"]`)
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
  return p.locator(`${EDITOR} [data-node-type="blockOuter"][data-id="${found[0].id}"]`);
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
  await grab(p, moved);
  await dropUnder(p, moved, under);
}

/**
 * Chez `p`, saisit par sa poignée le bloc de texte `moved`, et commence à le glisser : le bouton
 * de la souris reste enfoncé, jusqu'à `dropUnder`.
 */
async function grab(p: Page, moved: string): Promise<void> {
  await (await blockOf(p, moved)).locator(OWN_CONTENT).hover();
  const handle = p.getByRole("button", { name: "Ouvrir le menu du bloc" });
  await expect(handle).toBeVisible();
  const grip = (await handle.boundingBox())!;
  await p.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await p.mouse.down();
  // Les premiers mouvements, bouton enfoncé, font partir le glisser du navigateur.
  await p.mouse.move(grip.x + grip.width / 2 + 12, grip.y + grip.height / 2 + 12, { steps: 3 });
}

/**
 * Chez `p`, lâche le bloc saisi (`grab`), de texte `moved`, sur le bas du bloc de texte `under`,
 * tels qu'ils sont à cet instant. Vérifie que le bloc a changé de place chez `p`, avec son id et
 * son texte, et que rien d'autre n'a bougé.
 */
async function dropUnder(p: Page, moved: string, under: string): Promise<void> {
  const before = await shownBlocks(p);
  const target = (await (await blockOf(p, under)).locator(OWN_CONTENT).boundingBox())!;
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

/** Une page de contenu `mmd` que Mona possède et où Tom écrit, ouverte chez les deux. */
async function opened(browser: Browser, mmd: string) {
  const {
    alice: mona,
    aliceApi: monaApi,
    space,
    bruno: tom,
    brunoId: tomId,
  } = await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Bloc déplacé ${unique()}` });
  await setPageLevel(monaApi, page.id, tomId, "edit");
  await putMmd(monaApi, page.id, mmd);
  const monaPage = await openPage(browser, page.id, mona);
  const tomPage = await openPage(browser, page.id, tom);
  // Une exception dans un éditeur (un changement reçu qu'il ne sait pas appliquer) le fige : les
  // tests verts affirment qu'il n'y en a eu aucune.
  const errors: string[] = [];
  for (const [who, p] of [["Mona", monaPage], ["Tom", tomPage]] as const) {
    p.on("pageerror", (error) => errors.push(`${who} : ${error.message}`));
  }
  return { monaApi, id: page.id, mona: monaPage, tom: tomPage, errors };
}

type Opened = Awaited<ReturnType<typeof opened>>;

/** La page de départ (`START`) ouverte chez Mona et chez Tom, avec les ids de ses six blocs. */
async function scene(browser: Browser) {
  const s = await opened(browser, START);
  const shown = await shownBlocks(s.mona);
  expect(shown.map((b) => b.text)).toEqual([
    "Bloc X",
    "Bloc Y",
    "Bloc Z",
    "Témoin A",
    "Témoin B",
    "Témoin C",
  ]);
  expect(await shownBlocks(s.tom)).toEqual(shown);
  const [x, y, z, a, b, c] = shown.map((block) => block.id);
  return { ...s, ids: { x, y, z, a, b, c } };
}

type Scene = Awaited<ReturnType<typeof scene>>;

type Ids = { x: string; y: string; z: string };

/**
 * La page `wanted` (ses blocs de premier niveau, ids et textes, de haut en bas) chez Mona, chez
 * Tom et, pour les textes, dans le MMD du serveur : attendue 15 s au plus, puis affirmée. Chaque
 * id n'y est qu'une fois, et aucun éditeur n'a levé d'exception.
 */
async function expectPageEverywhere(s: Opened, wanted: Shown[]): Promise<void> {
  const texts = wanted.map((block) => block.text);
  const same = (blocks: Shown[]) => JSON.stringify(blocks) === JSON.stringify(wanted);
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const [mona, tom, mmd] = await Promise.all([
      shownBlocks(s.mona),
      shownBlocks(s.tom),
      mmdBlocks(s.monaApi, s.id),
    ]);
    if (same(mona) && same(tom) && JSON.stringify(mmd) === JSON.stringify(texts)) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  expect.soft(await shownBlocks(s.mona), "chez Mona").toEqual(wanted);
  expect.soft(await shownBlocks(s.tom), "chez Tom").toEqual(wanted);
  expect.soft(await mmdBlocks(s.monaApi, s.id), "dans le MMD du serveur").toEqual(texts);
  expect.soft(new Set(wanted.map((block) => block.id)).size, "chaque id une seule fois").toBe(wanted.length);
  expect.soft(await fragmentXml(s.tom), "Mona et Tom ont le même document").toBe(await fragmentXml(s.mona));
  expect.soft(s.errors, "aucune exception dans un éditeur").toEqual([]);
}

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

/** Les six blocs de la page de départ, tels que `changed` les donne (id et texte). */
function page(s: Scene, changed: Partial<Record<keyof Scene["ids"], string>> = {}): Record<keyof Scene["ids"], Shown> {
  const base = {
    x: "Bloc X",
    y: "Bloc Y",
    z: "Bloc Z",
    a: "Témoin A",
    b: "Témoin B",
    c: "Témoin C",
  };
  const block = (key: keyof Scene["ids"]) => ({ id: s.ids[key], text: changed[key] ?? base[key] });
  return { x: block("x"), y: block("y"), z: block("z"), a: block("a"), b: block("b"), c: block("c") };
}

// Y et Z sont entre l'ancienne place de X et la nouvelle. Trois façons d'y écrire pendant que
// Mona glisse X sous Z : pendant qu'elle le tient au bout de sa souris ; hors ligne, de sorte
// que les deux écritures se croisent à coup sûr ; ou en ligne pendant qu'elle glisse hors ligne.
for (const mode of ["en ligne", "hors ligne", "glisser hors ligne"] as const) {
  test(`typing_in_a_neighbour_while_someone_drags_stays_in_place (${mode})`, async ({ browser }) => {
    const s = await scene(browser);
    const { mona, tom, monaApi, id } = s;
    const moved = ["Bloc Y", "Bloc Z", "Bloc X", "Témoin A", "Témoin B", "Témoin C"];
    if (mode === "en ligne") {
      await grab(mona, "Bloc X");
      await typeAtEnd(tom, "Bloc Y", TYPED);
      await typeAtEnd(tom, "Bloc Z", " aussi");
      // Mona reçoit la frappe pendant qu'elle tient le bloc, puis le lâche.
      await expect.poll(() => shownTexts(mona)).toContain("Bloc Z aussi");
      await dropUnder(mona, "Bloc X", "Bloc Z aussi");
    } else if (mode === "hors ligne") {
      await goOffline(tom);
      await typeAtEnd(tom, "Bloc Y", TYPED);
      await typeAtEnd(tom, "Bloc Z", " aussi");
      await dragUnder(mona, "Bloc X", "Bloc Z");
      // Le déplacement est au serveur ; ce que Tom a tapé n'est encore que chez lui.
      await expect.poll(() => mmdBlocks(monaApi, id)).toEqual(moved);
      await comeBack(tom);
      await converged(mona, tom);
    } else {
      await goOffline(mona);
      await dragUnder(mona, "Bloc X", "Bloc Z");
      await typeAtEnd(tom, "Bloc Y", TYPED);
      await typeAtEnd(tom, "Bloc Z", " aussi");
      // Ce que Tom a tapé est au serveur ; le déplacement n'est encore que chez Mona.
      await expect
        .poll(() => mmdBlocks(monaApi, id))
        .toEqual(["Bloc X", `Bloc Y${TYPED}`, "Bloc Z aussi", "Témoin A", "Témoin B", "Témoin C"]);
      await comeBack(mona);
      await converged(mona, tom);
    }
    const { x, y, z, a, b, c } = page(s, { y: `Bloc Y${TYPED}`, z: "Bloc Z aussi" });
    await expectPageEverywhere(s, [y, z, x, a, b, c]);
  });
}

test("two_people_moving_two_blocks_keep_the_page", async ({ browser }) => {
  const s = await scene(browser);
  const { mona, tom, monaApi, id } = s;
  // Mona, coupée du réseau, glisse X sous Z ; Tom, en ligne, glisse Y sous le premier témoin.
  await goOffline(mona);
  await dragUnder(mona, "Bloc X", "Bloc Z");
  await dragUnder(tom, "Bloc Y", "Témoin A");
  await expect
    .poll(() => mmdBlocks(monaApi, id))
    .toEqual(["Bloc X", "Bloc Z", "Témoin A", "Bloc Y", "Témoin B", "Témoin C"]);
  await comeBack(mona);
  await converged(mona, tom);
  // Les deux déplacements sont faits : chaque bloc une fois, avec son id et son texte.
  const { x, y, z, a, b, c } = page(s);
  await expectPageEverywhere(s, [z, x, a, y, b, c]);
});

test("typing_goes_on_in_the_block_someone_else_just_moved", async ({ browser }) => {
  const s = await scene(browser);
  const { mona, tom } = s;
  // Tom pose son curseur dans X, après « Blo ».
  await (await blockOf(tom, "Bloc X")).locator(OWN_CONTENT).click();
  await tom.keyboard.press("Home");
  for (let i = 0; i < 3; i += 1) await tom.keyboard.press("ArrowRight");
  // Mona glisse X sous Z, et Tom le voit partir.
  await dragUnder(mona, "Bloc X", "Bloc Z");
  await expect.poll(() => shownTexts(tom)).toEqual(["Bloc Y", "Bloc Z", "Bloc X", "Témoin A", "Témoin B", "Témoin C"]);
  // Il continue de taper, sans recliquer : son curseur a suivi le bloc, à la même place.
  await tom.keyboard.type("!");
  const { x, y, z, a, b, c } = page(s, { x: "Blo!c X" });
  await expectPageEverywhere(s, [y, z, x, a, b, c]);
});

test("the_cursor_left_in_a_dropped_block_follows_it_when_someone_else_moves_it", async ({ browser }) => {
  const s = await scene(browser);
  const { mona, tom } = s;
  // Mona dépose X tout en bas : le dépôt laisse son curseur dans X, à la fin de son texte (c'est
  // ce que suppose l'aide `drag` des tests unitaires, `src/test/editors.ts`).
  await dragUnder(mona, "Bloc X", "Témoin C");
  const selection = await mona.evaluate(() => {
    const { selection, doc } = window.__mementum!.editor.prosemirrorState;
    const { $anchor } = selection;
    return {
      type: (selection.toJSON() as { type: string }).type,
      collapsed: selection.empty,
      block: $anchor.node($anchor.depth - 1).attrs.id as string,
      atEnd: $anchor.parentOffset === $anchor.parent.content.size,
      lastBlock: selection.anchor > doc.content.size - 6,
    };
  });
  expect(selection).toEqual({ type: "text", collapsed: true, block: s.ids.x, atEnd: true, lastBlock: true });
  await expect
    .poll(() => shownTexts(tom))
    .toEqual(["Bloc Y", "Bloc Z", "Témoin A", "Témoin B", "Témoin C", "Bloc X"]);
  // Tom le remonte sous Y. Mona tape sans recliquer : c'est dans X, pas dans le dernier bloc.
  await dragUnder(tom, "Bloc X", "Bloc Y");
  await expect
    .poll(() => shownTexts(mona))
    .toEqual(["Bloc Y", "Bloc X", "Bloc Z", "Témoin A", "Témoin B", "Témoin C"]);
  await mona.keyboard.type(" de Mona");
  const { x, y, z, a, b, c } = page(s, { x: "Bloc X de Mona" });
  await expectPageEverywhere(s, [y, x, z, a, b, c]);
});

/** Un PNG d'un pixel, que `width` étire : de quoi cliquer. */
const PIXEL =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/**
 * Ce que désigne la sélection de l'éditeur de `p` : pour une sélection de nœud, le type du nœud
 * et l'id du bloc qui le porte (le sien, si c'est un bloc entier).
 */
function selectionOf(p: Page): Promise<{ kind: string; node: string | null; block: string | null }> {
  return p.evaluate(() => {
    const { selection } = window.__mementum!.editor.prosemirrorState;
    const { node } = selection as unknown as {
      node?: { type: { name: string }; attrs: Record<string, unknown> };
    };
    const { $from } = selection;
    let block = node?.type.name === "blockContainer" ? String(node.attrs.id) : null;
    for (let depth = $from.depth; block === null && depth > 0; depth -= 1) {
      const holder = $from.node(depth);
      if (holder.type.name === "blockContainer") block = String(holder.attrs.id);
    }
    return {
      kind: (selection.toJSON() as { type: string }).type,
      node: node?.type.name ?? null,
      block,
    };
  });
}

// Un clic sur une image sélectionne le nœud image, qui n'a pas d'id. Quand un autre déplace
// l'image, y-prosemirror remettait la sélection à l'ancienne place, sur le bloc qui l'avait prise,
// entier : Suppr retirait ce bloc, et l'image restait. `followBlock` la garde sur l'image.
test("an_image_clicked_then_moved_by_someone_else_is_the_one_delete_removes", async ({ browser }) => {
  const s = await opened(browser, `Bloc X\n\n![](${PIXEL}) {width=200}\n\nBloc Y\n\nBloc Z\n`);
  const { mona, tom } = s;
  const start = await shownBlocks(mona);
  expect(start.map((b) => b.text)).toEqual(["Bloc X", "", "Bloc Y", "Bloc Z"]);
  await expect.poll(() => shownBlocks(tom)).toEqual(start);
  const [x, image, y, z] = start;

  // Mona clique sur l'image : c'est elle qui est sélectionnée, pas son bloc.
  const picture = mona.locator(`${EDITOR} [data-node-type="blockOuter"][data-id="${image.id}"] img`);
  await expect(picture).toBeVisible();
  await picture.click();
  const onImage = { kind: "node", node: "image", block: image.id };
  expect(await selectionOf(mona)).toEqual(onImage);

  // Tom la glisse tout en bas, par sa poignée. Chez Mona, elle reste sélectionnée.
  await dragUnder(tom, "", "Bloc Z");
  await expect.poll(() => shownBlocks(mona)).toEqual([x, y, z, image]);
  await converged(mona, tom);
  expect(await selectionOf(mona)).toEqual(onImage);

  // Mona presse Suppr sans recliquer : c'est l'image qui part, pas le bloc qui a pris sa place.
  await mona.keyboard.press("Delete");
  await expectPageEverywhere(s, [x, y, z]);
});

// Le déplacement arrive au serveur avant la frappe, faite sans l'avoir vu : limite R222. Les
// clients seuls, et le serveur après la Task 13b, perdent ce texte ; la limite est dite aux
// utilisateurs (Task 13c), et a son plan à part après le lot 3.
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

// La frappe est au serveur quand le déplacement arrive : il la reportera sur le bloc recréé
// (R221). Attend la Task 13b.
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
