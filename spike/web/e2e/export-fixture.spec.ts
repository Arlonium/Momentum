import { expect, test } from "@playwright/test";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { openPage, uniquePage } from "./helpers";

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "fixtures");

test("export_all_blocks_fixture", async ({ browser }) => {
  // Réécrit des fixtures commitées (avec des ids neufs à chaque fois) : seulement sur demande.
  test.skip(process.env.EXPORT_FIXTURES !== "1", "régénération des fixtures : relancer avec EXPORT_FIXTURES=1");
  const p = await openPage(browser, uniquePage("fixture"), "Fixture", "&fixture=all-blocks");
  await p.waitForFunction(() => (window as any).__mementum?.fixtureReady === true, null, { timeout: 30_000 });

  const { state, document } = await p.evaluate(() => {
    const m = (window as any).__mementum;
    return { state: m.exportState() as string, document: m.editor.document };
  });

  mkdirSync(join(FIXTURES, "yjs"), { recursive: true });
  writeFileSync(join(FIXTURES, "yjs", "all-blocks.bin"), Buffer.from(state, "base64"));
  writeFileSync(join(FIXTURES, "blocknote", "all-blocks.json"), JSON.stringify(document, null, 2) + "\n");

  expect(document).toHaveLength(28);
});
