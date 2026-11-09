import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { openPage, uniquePage } from "./helpers";

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "fixtures");
const MMD = readFileSync(join(FIXTURES, "mmd", "all-blocks.mmd"), "utf8");
const EXPECTED = JSON.parse(readFileSync(join(FIXTURES, "blocknote", "all-blocks.json"), "utf8"));
const API = "http://localhost:8080";

/** Retire les ids générés (sauf celui du bloc inconnu, qui doit survivre). */
function stripIds(blocks: any[]): any[] {
  return blocks.map((b) => {
    const { id, children, ...rest } = b;
    return { ...rest, ...(id === "b_unknown01" ? { id } : {}), children: stripIds(children ?? []) };
  });
}

test("server_written_doc_opens_in_blocknote", async ({ browser, request }) => {
  const id = uniquePage("t4");
  const put = await request.put(`${API}/pages/${id}/mmd`, { data: MMD, headers: { "content-type": "text/markdown" } });
  expect(put.status()).toBe(204);
  const p = await openPage(browser, id, "Lecteur");
  await expect.poll(() => p.evaluate(() => (window as any).__mementum.editor.document.length)).toBe(28);
  // Même sérialisation que la fixture (JSON.stringify retire les clés `undefined`).
  const doc = await p.evaluate(() => JSON.parse(JSON.stringify((window as any).__mementum.editor.document)));
  expect(stripIds(doc)).toEqual(stripIds(EXPECTED));
});

test("browser_edit_reads_back_as_mmd", async ({ browser, request }) => {
  const id = uniquePage("t4-edit");
  await request.put(`${API}/pages/${id}/mmd`, { data: MMD, headers: { "content-type": "text/markdown" } });
  const p = await openPage(browser, id, "Éditeur");
  await expect.poll(() => p.evaluate(() => (window as any).__mementum.editor.document.length)).toBe(28);
  // Taper « ok » à la fin du premier paragraphe (« Livrer … »).
  await p.locator(".bn-block-content", { hasText: "Livrer un espace" }).first().click();
  await p.keyboard.press("End");
  await p.keyboard.type(" ok");
  const expected = MMD.replace("et $E = mc^2$.", "et $E = mc^2$. ok");
  await expect
    .poll(async () => (await (await request.get(`${API}/pages/${id}`)).json()).markdown, { timeout: 5_000 })
    .toBe(expected);
});
