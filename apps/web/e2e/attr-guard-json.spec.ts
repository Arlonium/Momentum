// Garde des attributs, C1 de la relecture (Ruling R702) : un contenu JSON posé par l'origine de
// l'entrée courante d'un attribut. yrs en rend la chaîne, Yjs un objet. Sonde du relecteur, gardée.
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
  p.on("pageerror", (error) => errors.push(String(error)));
  return errors;
}

function background(p: Page): Promise<unknown> {
  return p.evaluate(() => {
    type Element = { get(index: number): Element; getAttribute(name: string): unknown };
    const fragment = window.__mementum!.doc.getXmlFragment("document-store") as unknown as Element;
    const value = fragment.get(0).get(0).get(0).getAttribute("backgroundColor");
    return value === undefined ? "absente" : JSON.stringify(value);
  });
}

test("a_json_content_over_an_attribute_is_refused_and_the_page_still_opens", async ({
  browser,
  baseURL,
}) => {
  const { alice: mona, aliceApi: monaApi, space, bruno: eve, brunoApi: eveApi, brunoId: eveId } =
    await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `JSON ${unique()}` });
  await setPageLevel(monaApi, page.id, eveId, "edit");
  const monaPage = await openPage(browser, page.id, mona);
  const monaErrors = pageErrors(monaPage);
  const editor = monaPage.locator(".bn-editor");
  await editor.click();
  await monaPage.keyboard.type("Bonjour");
  await expect.poll(() => getMmd(monaApi, page.id)).toContain("Bonjour");

  const entry = await monaPage.evaluate(() => {
    type Item = { id: { client: number; clock: number } };
    type Element = { get(index: number): Element; _map: Map<string, Item> };
    const fragment = window.__mementum!.doc.getXmlFragment("document-store") as unknown as Element;
    const item = fragment.get(0).get(0).get(0)._map.get("backgroundColor");
    return item ? { client: item.id.client, clock: item.id.clock } : null;
  });
  expect(entry).not.toBeNull();

  // Un client, un élément à l'horloge 0 : un contenu JSON (2), une origine (0x80) ; yrs lit
  // « compte + 1 » chaînes : compte 0, puis la chaîne `{"toString":1}`.
  const json = [...new TextEncoder().encode('{"toString":1}')];
  const update = [
    1,
    1,
    ...varUint(777_002),
    0,
    0x82,
    ...varUint(entry!.client),
    ...varUint(entry!.clock),
    0,
    json.length,
    ...json,
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
  console.log("SONDE Mona lit backgroundColor =", await background(monaPage));
  console.log("SONDE erreurs chez Mona :", JSON.stringify(monaErrors));

  // Une autre personne ouvre la page.
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
  console.log("SONDE éditeur d'Ève monté :", mounted, "; erreurs :", JSON.stringify(eveErrors));
  console.log("SONDE MMD du serveur :", JSON.stringify(await getMmd(monaApi, page.id)));
  await context.close();

  expect(outcome).toBe("fermée");
  expect(mounted).toBe(true);
  expect(monaErrors).toEqual([]);
});
