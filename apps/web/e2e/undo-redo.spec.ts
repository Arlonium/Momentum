// « Rétablir » sans rien à rétablir ne fait rien (Rulings R731 à R735 ; `editor/redoKeys.ts`).
//
// Ctrl+Maj+Z quand il n'y a plus rien à rétablir retirait la dernière frappe, et un « rétablir » de
// plus la rendait : la table de raccourcis de BlockNote retombait sur l'annulation quand le
// navigateur donnait la lettre en minuscule avec Maj (Verr. Maj. activé ; `press("Control+Shift+z")`
// de Playwright fait de même). Chaque test pose une barrière après la touche : une frappe de plus,
// qu'on attend chez le serveur, à la suite du texte que la touche devait laisser.
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, createPage, getMmd, openPage, ownerAndMember, putMmd, unique } from "./helpers";

test.afterEach(closeContexts);

/** Ctrl+Maj+Z, la lettre en minuscule : ce que donne un clavier en Verr. Maj. */
const REDO_LOWER = "Control+Shift+z";
/** Ctrl+Maj+Z, la lettre en majuscule : le cas ordinaire. */
const REDO_UPPER = "Control+Shift+Z";
const REDO_Y = "Control+y";
const UNDO = "Control+z";

/** Plus que le délai de regroupement de l'annulation de Yjs (`captureTimeout`, 500 ms). */
const NEW_STEP_MS = 800;

/** Une page de l'équipe, de contenu `mmd`, ouverte par Mona. */
async function pageWith(browser: import("@playwright/test").Browser, mmd: string) {
  const team = await ownerAndMember();
  const page = await createPage(team.aliceApi, { spaceId: team.space.id, title: `Rétablir ${unique()}` });
  await putMmd(team.aliceApi, page.id, mmd);
  const mona = await openPage(browser, page.id, team.alice);
  return { team, page, mona };
}

/** Les textes des blocs de premier niveau que montre l'éditeur de `p`. */
function textsOf(p: Page): Promise<string[]> {
  return p.evaluate(() =>
    window.__mementum!.editor.document.map((b) =>
      Array.isArray(b.content) ? b.content.map((c) => ("text" in c ? String(c.text) : "")).join("") : "",
    ),
  );
}

/** Le curseur de `p` à la fin du premier bloc. */
async function atEndOfFirstBlock(p: Page): Promise<void> {
  await p.locator(".mm-editor .bn-block-content").first().click();
  await p.keyboard.press("End");
}

test("redo_with_nothing_to_redo_leaves_the_typing", async ({ browser }) => {
  const { team, page, mona } = await pageWith(browser, "Avant\n");
  const mmd = () => getMmd(team.aliceApi, page.id);
  await atEndOfFirstBlock(mona);
  await mona.keyboard.type(" deux");
  await expect.poll(mmd).toBe("Avant deux\n");

  // Chaque forme de la touche, deux fois, une barrière après chacune : la frappe reste. (Deux appuis
  // de suite sans barrière ne prouveraient rien : le second rétablissait ce que le premier retirait.)
  let typed = "Avant deux";
  for (const key of [REDO_LOWER, REDO_LOWER, REDO_UPPER, REDO_UPPER, REDO_Y, REDO_Y]) {
    await mona.keyboard.press(key);
    await mona.keyboard.type("!");
    typed += "!";
    await expect.poll(mmd, `après ${key}`).toBe(`${typed}\n`);
  }
  await expect(mona.locator(".mm-editor .bn-block-content").first()).toHaveText(typed);
});

test("undo_and_redo_everything_then_redo_once_more", async ({ browser }) => {
  const { team, page, mona } = await pageWith(browser, "Avant\n");
  const mmd = () => getMmd(team.aliceApi, page.id);
  await atEndOfFirstBlock(mona);
  await mona.keyboard.type(" deux");
  await mona.waitForTimeout(NEW_STEP_MS);
  await mona.keyboard.type(" trois");
  await expect.poll(mmd).toBe("Avant deux trois\n");
  await mona.waitForTimeout(NEW_STEP_MS);

  // L'annulation ordinaire : un pas, puis l'autre.
  await mona.keyboard.press(UNDO);
  await expect.poll(mmd).toBe("Avant deux\n");
  await mona.keyboard.press(UNDO);
  await expect.poll(mmd).toBe("Avant\n");
  // Rétablir, par les deux formes de Ctrl+Maj+Z : un pas, puis l'autre.
  await mona.keyboard.press(REDO_LOWER);
  await expect.poll(mmd).toBe("Avant deux\n");
  await mona.keyboard.press(REDO_UPPER);
  await expect.poll(mmd).toBe("Avant deux trois\n");
  // Plus rien à rétablir : la touche ne retire rien.
  await mona.keyboard.press(REDO_LOWER);
  await mona.keyboard.type("!");
  await expect.poll(mmd).toBe("Avant deux trois!\n");
  await mona.waitForTimeout(NEW_STEP_MS);
  // L'annulation défait encore le dernier pas (la barrière), et Ctrl+Y le rend.
  await mona.keyboard.press(UNDO);
  await expect.poll(mmd).toBe("Avant deux trois\n");
  await mona.keyboard.press(REDO_Y);
  await expect.poll(mmd).toBe("Avant deux trois!\n");
});

test("redo_with_nothing_to_redo_in_a_heading_leaves_the_typing", async ({ browser }) => {
  const { team, page, mona } = await pageWith(browser, "# Titre\n");
  const mmd = () => getMmd(team.aliceApi, page.id);
  await atEndOfFirstBlock(mona);
  await mona.keyboard.type(" deux");
  await expect.poll(mmd).toBe("# Titre deux\n");
  await mona.keyboard.press(REDO_LOWER);
  await mona.keyboard.type("!");
  await expect.poll(mmd).toBe("# Titre deux!\n");
});

test("with_two_tabs_redo_with_nothing_to_redo_takes_nothing_away", async ({ browser }) => {
  const { team, page, mona } = await pageWith(browser, "Avant\n\nAprès\n");
  const tom = await openPage(browser, page.id, team.bruno);
  const mmd = () => getMmd(team.aliceApi, page.id);
  await atEndOfFirstBlock(mona);
  await mona.keyboard.type(" de Mona");
  await tom.locator(".mm-editor .bn-block-content").last().click();
  await tom.keyboard.press("End");
  await tom.keyboard.type(" de Tom");
  await expect.poll(mmd).toBe("Avant de Mona\n\nAprès de Tom\n");

  await mona.keyboard.press(REDO_LOWER);
  await tom.keyboard.press(REDO_LOWER);
  await mona.keyboard.type("!");
  await tom.keyboard.type("?");
  await expect.poll(mmd).toBe("Avant de Mona!\n\nAprès de Tom?\n");
  // Chaque onglet le montre (lu dans l'éditeur : l'écran porte aussi le nom du curseur de l'autre).
  for (const p of [mona, tom]) await expect.poll(() => textsOf(p)).toEqual(["Avant de Mona!", "Après de Tom?"]);
});
