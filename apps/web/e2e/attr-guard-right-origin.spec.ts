// Garde des attributs, I1 de la relecture (Ruling R703) : un élément dont l'origine est un bloc
// et l'origine droite une entrée de map. Sonde du relecteur, gardée.
import { expect, test, type Page } from "@playwright/test";
import type {} from "../src/editor/debug";
import {
  closeContexts,
  createPage,
  getMmd,
  openPage,
  ownerAndMember,
  setPageLevel,
  unique,
} from "./helpers";
import { rawSync, varUint } from "./raw-sync";

test.afterEach(closeContexts);

function pageErrors(p: Page): string[] {
  const errors: string[] = [];
  p.on("pageerror", (error) => errors.push(String(error).slice(0, 200)));
  p.on("console", (m) => {
    if (m.type() === "error") errors.push(`console: ${m.text().slice(0, 200)}`);
  });
  return errors;
}

test("a_value_between_a_block_and_a_map_entry_is_refused", async ({ browser, baseURL }) => {
  const { alice: mona, aliceApi: monaApi, space, bruno: eve, brunoApi: eveApi, brunoId: eveId } =
    await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Droite ${unique()}` });
  await setPageLevel(monaApi, page.id, eveId, "edit");
  const monaPage = await openPage(browser, page.id, mona);
  const monaErrors = pageErrors(monaPage);
  const editor = monaPage.locator(".bn-editor");
  await editor.click();
  await monaPage.keyboard.type("Bonjour");
  await monaPage.keyboard.press("Enter");
  await monaPage.keyboard.type("fin");
  await expect.poll(() => getMmd(monaApi, page.id)).toContain("fin");

  const ids = await monaPage.evaluate(() => {
    type Id = { client: number; clock: number };
    type Element = { get(index: number): Element; length: number; _map: Map<string, { id: Id }>; _item: { id: Id } };
    const fragment = window.__mementum!.doc.getXmlFragment("document-store") as unknown as Element;
    const group = fragment.get(0);
    const last = group.get(group.length - 1);
    const entry = group.get(0).get(0)._map.get("backgroundColor")!;
    return { left: last._item.id, right: entry.id };
  });

  // Un élément : une valeur `Any` (8), une origine (0x80), une origine droite (0x40) ; la valeur :
  // la chaîne « red ».
  const update = [
    1,
    1,
    ...varUint(777_003),
    0,
    0xc8,
    ...varUint(ids.left.client),
    ...varUint(ids.left.clock),
    ...varUint(ids.right.client),
    ...varUint(ids.right.clock),
    1,
    119,
    3,
    114,
    101,
    100,
    0,
  ];
  const socket = await rawSync(eveApi, baseURL!, page.id);
  let outcome = "";
  try {
    socket.send(Uint8Array.from([0, 2, ...varUint(update.length), ...update]));
    outcome = await socket.settled();
  } finally {
    socket.close();
  }
  console.log("SONDE connexion hostile :", outcome);
  await monaPage.waitForTimeout(1500);
  const seen = await monaPage.evaluate(() => {
    type Element = { get(index: number): Element; toArray(): unknown[]; getAttributes(): Record<string, unknown> };
    const fragment = window.__mementum!.doc.getXmlFragment("document-store") as unknown as Element;
    const group = fragment.get(0);
    return {
      children: group.toArray().map((c) => (typeof c === "string" ? `chaîne ${c}` : (c as { nodeName: string }).nodeName)),
      attrs: group.getAttributes(),
    };
  });
  console.log("SONDE chez Mona, le groupe racine :", JSON.stringify(seen));
  console.log("SONDE erreurs chez Mona :", JSON.stringify(monaErrors.slice(0, 3)));
  console.log("SONDE MMD du serveur :", JSON.stringify(await getMmd(monaApi, page.id)));

  const context = await browser.newContext({ storageState: eve.storageState });
  const evePage = await context.newPage();
  const eveErrors = pageErrors(evePage);
  await evePage.goto(`/p/${page.id}`);
  let mounted = true;
  try {
    await evePage.waitForSelector('.mm-editor[data-synced="true"]', { timeout: 20_000 });
    await expect(evePage.locator(".bn-editor")).toContainText("Bonjour", { timeout: 5_000 });
  } catch {
    mounted = false;
  }
  console.log("SONDE éditeur d'Ève monté :", mounted, "; erreurs :", JSON.stringify(eveErrors.slice(0, 3)));
  let shared = false;
  if (mounted) {
    await evePage.locator(".bn-editor").click();
    await evePage.keyboard.type("ZZ");
    try {
      await expect(monaPage.locator(".bn-editor")).toContainText("ZZ", { timeout: 5_000 });
      shared = true;
    } catch {
      shared = false;
    }
  }
  console.log("SONDE Mona voit la frappe d'Ève :", shared, "; erreurs chez Mona :", JSON.stringify(monaErrors.slice(0, 4)));
  await context.close();
  expect(outcome).toBe("fermée");
});
