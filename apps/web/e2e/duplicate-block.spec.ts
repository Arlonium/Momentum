// Dupliquer un bloc (Task C5, R253) : Ctrl+D et « Dupliquer » dans le menu de la poignée insèrent,
// après le bloc, sa copie sans aucun identifiant de bloc. Avec l'identifiant de l'original, le
// dédoublonnage du serveur supprimerait la copie (preuve 5 du plan) : le serveur doit garder les
// deux blocs, vus par l'autre personne de la page, et les garder pendant qu'on écrit dans l'un et
// dans l'autre. Hors du texte de l'éditeur, Ctrl+D reste au navigateur (le favori).
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, createPage, getMmd, openPage, ownerAndMember, putMmd, titleOf, unique } from "./helpers";

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
