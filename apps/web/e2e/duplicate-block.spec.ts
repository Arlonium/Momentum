// Dupliquer un bloc (Task C5, R253) : Ctrl+D et « Dupliquer » dans le menu de la poignée insèrent,
// après le bloc, sa copie sans aucun identifiant de bloc. Avec l'identifiant de l'original, le
// dédoublonnage du serveur supprimerait la copie (preuve 5 du plan) : le serveur doit garder les
// deux blocs, vus par l'autre personne de la page, et les garder pendant qu'on écrit dans l'un et
// dans l'autre. Hors du texte de l'éditeur, Ctrl+D reste au navigateur (le favori).
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, createPage, getMmd, openPage, ownerAndMember, putMmd, titleOf, treeRow, unique, waitSynced } from "./helpers";

test.afterEach(closeContexts);

/** Les blocs de premier niveau que montre l'éditeur de `p` : leur id et leur texte. */
async function blocksOf(p: Page): Promise<{ id: string; text: string }[]> {
  return p.evaluate(() =>
    window.__mementum!.editor.document.map((b) => ({
      id: b.id,
      text: Array.isArray(b.content) ? b.content.map((c) => ("text" in c ? String(c.text) : "")).join("") : "",
    })),
  );
}

/** Les textes des blocs de premier niveau de `p`. */
async function textsOf(p: Page): Promise<string[]> {
  return (await blocksOf(p)).map((b) => b.text);
}

/** Le contenu propre d'un bloc (pas celui de ses enfants), par son texte. */
function blockContent(p: Page, text: string) {
  return p.locator(".mm-editor .bn-block-content").filter({ hasText: text }).first();
}

/**
 * L'élément qui a le focus dans le menu de la poignée (aucun tant que le focus n'y est pas) : Mantine
 * le donne à un repère invisible du menu, pas à sa première entrée.
 */
function handleMenuFocus(p: Page) {
  return p.locator(".bn-drag-handle-menu[role=menu]").locator(":focus");
}

/** Mona (propriétaire de l'espace) et Tom (membre), sur une page de « Espace » qui porte `mmd`. */
async function duo(browser: Parameters<typeof openPage>[0], mmd: string) {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Dupliquer ${unique()}` });
  await putMmd(team.aliceApi, page.id, mmd);
  const mona = await openPage(browser, page.id, team.alice);
  const tom = await openPage(browser, page.id, team.bruno);
  return { ...team, page, mona, tom };
}

test("ctrl_d_duplicates_and_the_server_keeps_both", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Original\n\nAprès\n");
  await expect.poll(() => textsOf(tom)).toEqual(["Original", "Après"]);

  // Mona, au bout de « Original », fait Ctrl+D : la copie suit, et son curseur y passe.
  await blockContent(mona, "Original").click();
  await mona.keyboard.press("End");
  await mona.keyboard.press("Control+d");
  await expect.poll(() => textsOf(mona)).toEqual(["Original", "Original", "Après"]);
  // Tom voit la copie ; le serveur garde les deux blocs, sous trois ids distincts, les mêmes partout.
  await expect.poll(() => textsOf(tom)).toEqual(["Original", "Original", "Après"]);
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("Original\n\nOriginal\n\nAprès\n");
  const ids = (await blocksOf(mona)).map((b) => b.id);
  expect(new Set(ids).size).toBe(3);
  expect((await blocksOf(tom)).map((b) => b.id)).toEqual(ids);

  // Vingt frappes, l'une après l'autre : Mona dans la copie, où est son curseur, Tom dans l'original ;
  // chacune vue par l'autre avant la suivante. Le dédoublonnage du serveur ne retire aucun bloc.
  await blockContent(tom, "Original").click();
  await tom.keyboard.press("Home");
  await tom.keyboard.press("End");
  let original = "Original";
  let copy = "Original";
  for (let i = 0; i < 20; i += 1) {
    if (i % 2 === 0) {
      await mona.keyboard.type("m");
      copy += "m";
    } else {
      await tom.keyboard.type("t");
      original += "t";
    }
    const expected = [original, copy, "Après"];
    await expect.poll(() => textsOf(mona)).toEqual(expected);
    await expect.poll(() => textsOf(tom)).toEqual(expected);
  }
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe(`${original}\n\n${copy}\n\nAprès\n`);
  expect((await blocksOf(mona)).map((b) => b.id)).toEqual(ids);
  expect((await blocksOf(tom)).map((b) => b.id)).toEqual(ids);
});

test("duplicate_from_the_handle_menu", async ({ browser }) => {
  const { mona, tom, aliceApi, page } = await duo(browser, "Premier bloc\n\nDeuxième bloc\n");
  await blockContent(mona, "Premier bloc").hover();
  await mona.getByRole("button", { name: "Ouvrir le menu du bloc" }).click();
  // « Commenter » (Mona peut commenter), « Dupliquer », puis les entrées de BlockNote.
  const items = mona.getByRole("menuitem");
  await expect(items).toHaveText(["Commenter", "Dupliquer", "Supprimer", "Couleurs"]);
  await mona.getByRole("menuitem", { name: "Dupliquer" }).click();
  await expect(mona.getByRole("menu")).toBeHidden();
  await expect.poll(() => textsOf(mona)).toEqual(["Premier bloc", "Premier bloc", "Deuxième bloc"]);
  // Le menu fermé, le focus est au texte, au bout de la copie : la frappe y va.
  await expect(mona.locator(".mm-editor .bn-editor")).toBeFocused();
  await mona.keyboard.type(", la copie");
  await expect.poll(() => textsOf(tom)).toEqual(["Premier bloc", "Premier bloc, la copie", "Deuxième bloc"]);
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("Premier bloc\n\nPremier bloc, la copie\n\nDeuxième bloc\n");
});

test("ctrl_d_outside_the_editor_is_left_to_the_browser", async ({ browser }) => {
  const { mona, aliceApi, page } = await duo(browser, "Un bloc\n");
  // Ce que devient chaque Ctrl+D une fois passé par l'application (écouté sur `window`, après React).
  await mona.evaluate(() => {
    const seen: boolean[] = [];
    (window as unknown as { ctrlD: boolean[] }).ctrlD = seen;
    window.addEventListener("keydown", (event) => {
      if (event.key === "d" && event.ctrlKey) seen.push(event.defaultPrevented);
    });
  });
  const prevented = () => mona.evaluate(() => (window as unknown as { ctrlD: boolean[] }).ctrlD);

  // Dans le titre de la page : la touche n'est pas annulée, rien n'est copié.
  const title = titleOf(mona);
  const before = await title.inputValue();
  await title.click();
  await mona.keyboard.press("Control+d");
  await expect.poll(prevented).toEqual([false]);
  await expect(title).toHaveValue(before);
  expect(await textsOf(mona)).toEqual(["Un bloc"]);

  // Témoin : dans le texte de l'éditeur, elle est prise, et le bloc est copié.
  await blockContent(mona, "Un bloc").click();
  await mona.keyboard.press("Control+d");
  await expect.poll(prevented).toEqual([false, true]);
  await expect.poll(() => textsOf(mona)).toEqual(["Un bloc", "Un bloc"]);
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe("Un bloc\n\nUn bloc\n");
});

test("a_selection_leaving_a_column_is_duplicated_by_ctrl_d_and_by_the_menu", async ({ browser }) => {
  const columns = "<columns>\n<column>\nGauche\n</column>\n<column>\nDroite\n</column>\n</columns>";
  const { mona, tom, aliceApi, page } = await duo(browser, `Avant\n\n${columns}\n\nAprès\n`);
  const errors: string[] = [];
  mona.on("pageerror", (error) => errors.push(String(error)));
  await mona.evaluate(() => {
    const seen: boolean[] = [];
    (window as unknown as { ctrlD: boolean[] }).ctrlD = seen;
    window.addEventListener("keydown", (event) => {
      if (event.key === "d" && event.ctrlKey) seen.push(event.defaultPrevented);
    });
  });
  /** Sélectionne le texte, de la colonne de gauche à celle de droite de la liste de rang `at`. */
  const selectAcrossColumns = (at: number) =>
    mona.evaluate((index) => {
      const editor = window.__mementum!.editor;
      const list = editor.document[index];
      editor.setSelection(list.children[0].children[0].id, list.children[1].children[0].id);
      editor.focus();
    }, at);
  const types = async (p: Page) => p.evaluate(() => window.__mementum!.editor.document.map((b) => b.type));

  // Ctrl+D : la sélection sort d'une colonne ; la copie remonte à la liste de colonnes entière.
  await selectAcrossColumns(1);
  await mona.keyboard.press("Control+d");
  await expect.poll(() => mona.evaluate(() => (window as unknown as { ctrlD: boolean[] }).ctrlD)).toEqual([true]);
  await expect.poll(() => types(tom)).toEqual(["paragraph", "columnList", "columnList", "paragraph"]);
  await expect.poll(() => getMmd(aliceApi, page.id)).toBe(`Avant\n\n${columns}\n\n${columns}\n\nAprès\n`);

  // « Dupliquer » du menu de la poignée de « Gauche », la même sélection gardée : une troisième liste,
  // le menu se ferme, le focus est au texte.
  await selectAcrossColumns(1);
  await blockContent(mona, "Gauche").hover();
  await mona.getByRole("button", { name: "Ouvrir le menu du bloc" }).click();
  await mona.getByRole("menuitem", { name: "Dupliquer" }).click();
  await expect(mona.getByRole("menu")).toBeHidden();
  await expect(mona.locator(".mm-editor .bn-editor")).toBeFocused();
  await expect.poll(() => types(tom)).toEqual(["paragraph", "columnList", "columnList", "columnList", "paragraph"]);
  expect(errors).toEqual([]);
});

test("escape_on_the_handle_menu_gives_the_focus_back_to_the_text", async ({ browser }) => {
  const { mona } = await duo(browser, "Premier bloc\n\nDeuxième bloc\n");
  await blockContent(mona, "Premier bloc").hover();
  await mona.getByRole("button", { name: "Ouvrir le menu du bloc" }).click();
  await expect(mona.getByRole("menuitem", { name: "Dupliquer" })).toBeVisible();
  // Échap sur le menu, donc le focus dedans. Mantine pose le menu, encore transparent (son fondu
  // commence à l'image suivante), puis y met le focus par un minuteur ; il n'écoute Échap que sur
  // le menu. Envoyé avant, Échap tombe sur la poignée, et le menu reste ouvert (R796).
  await expect(handleMenuFocus(mona)).toHaveCount(1);
  await mona.keyboard.press("Escape");
  await expect(mona.getByRole("menu")).toBeHidden();
  // Jamais sur `body` : au texte de l'éditeur, où la frappe reprend.
  await expect(mona.locator(".mm-editor .bn-editor")).toBeFocused();
  await expect.poll(() => textsOf(mona)).toEqual(["Premier bloc", "Deuxième bloc"]);
});

test("a_refused_copy_message_leaves_with_the_page", async ({ browser }) => {
  // Une page de 1 001 blocs : Ctrl+A deux fois puis Ctrl+D en copierait plus de 1 000.
  const team = await ownerAndMember();
  const big = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Grande ${unique()}` });
  const other = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Autre ${unique()}` });
  const mmd = `${Array.from({ length: 1_001 }, (_, i) => `Bloc ${i + 1}`).join("\n\n")}\n`;
  await putMmd(team.aliceApi, big.id, mmd);
  const mona = await openPage(browser, big.id, team.alice);
  await blockContent(mona, "Bloc 1").click();
  await mona.keyboard.press("Control+a");
  await mona.keyboard.press("Control+a");
  await mona.keyboard.press("Control+d");
  const refused = mona.locator(".mm-copy-refused[role=alert]");
  await expect(refused).toHaveText(
    "Trop de blocs à dupliquer : 1 000 au plus d'un coup, sous-blocs compris. Rien n'a été copié.",
  );
  // Le message dit un geste de cette page : il part avec elle, sans attendre ses six secondes.
  await treeRow(mona, other.id).getByRole("link").click();
  await expect(titleOf(mona)).toHaveValue(other.title);
  await waitSynced(mona);
  // Lu une fois, sans attendre : une attente de cinq secondes laisserait au message le temps de partir
  // de lui-même (six secondes après la touche), et le test passerait à tort.
  expect(await refused.count()).toBe(0);
  expect(await getMmd(team.aliceApi, big.id)).toBe(mmd);
});
