import { expect, test } from "@playwright/test";
import { openPage, percentile, uniquePage } from "./helpers";

const API = "http://localhost:8080";
// Espace finale écrite `&#32;` : en Markdown, une espace en fin de ligne serait supprimée.
const PAGE = "Bloc humain :&#32;\n\nDeuxième bloc.\n\nTroisième bloc.\n\nQuatrième bloc.\n\nCompteur agent : 0\n";
const HUMAN = "L’humain tape sans s’arrêter, accents é è à et un emoji 🚀 au milieu. ".repeat(3).slice(0, 200);

/** Texte de tous les blocs (enfants compris), dans l'ordre. */
function flatten(blocks: any[]): any[] {
  return blocks.flatMap((b) => [b, ...flatten(b.children ?? [])]);
}

test("agent_edits_while_human_types", async ({ browser, request }) => {
  test.setTimeout(120_000);
  const id = uniquePage("t5");
  await request.put(`${API}/pages/${id}/mmd`, { data: PAGE, headers: { "content-type": "text/markdown" } });
  const p = await openPage(browser, id, "Humain");
  await expect.poll(() => p.evaluate(() => (window as any).__mementum.editor.document.length)).toBe(5);

  await p.locator(".bn-block-content", { hasText: "Bloc humain" }).first().click();
  await p.keyboard.press("End");

  const latencies: number[] = [];
  let conflicts = 0;
  const human = p.keyboard.type(HUMAN, { delay: 30 });
  const agent = (async () => {
    for (let i = 0; i < 20; i++) {
      const page = await (await request.get(`${API}/pages/${id}?anchors=block`)).json();
      // Le serveur diffuse avant de répondre : chronométrer depuis l'envoi de l'appel, pas depuis la réponse.
      const t0 = Date.now();
      const res = await request.post(`${API}/pages/${id}/update`, {
        data: {
          base_revision: page.revision,
          agent: { id: "scribe", run: "e2e" },
          operations: [
            { op: "str_replace", old: `Compteur agent : ${i}`, new: `Compteur agent : ${i + 1}` },
            { op: "insert", after: "b3", markdown: `- Note de l’agent n° ${i}` },
          ],
        },
      });
      if (res.status() === 409) conflicts++;
      expect(res.status(), await res.text()).toBe(200);
      await p.waitForFunction((s) => document.body.innerText.includes(s), `Compteur agent : ${i + 1}`, { polling: 5 });
      latencies.push(Date.now() - t0);
    }
  })();
  await Promise.all([human, agent]);

  console.log(`MESURE agent → navigateur : p50 ${percentile(latencies, 0.5)} ms, p95 ${percentile(latencies, 0.95)} ms (20 appels)`);
  expect(conflicts).toBe(0);
  expect(percentile(latencies, 0.95)).toBeLessThan(150);

  // Le navigateur a tout : le texte humain complet et dans l'ordre, et les 20 éditions de l'agent.
  await expect
    .poll(async () => {
      const doc = await p.evaluate(() => JSON.parse(JSON.stringify((window as any).__mementum.editor.document)));
      return flatten(doc).map((b: any) => (b.content ?? []).map((c: any) => c.text ?? "").join("")).join("\n");
    })
    .toContain(`Bloc humain : ${HUMAN}`);
  const browserBlocks = flatten(await p.evaluate(() => JSON.parse(JSON.stringify((window as any).__mementum.editor.document))));
  const browserText = browserBlocks.map((b: any) => (b.content ?? []).map((c: any) => c.text ?? "").join("")).join("\n");
  expect(browserText).toContain("Compteur agent : 20");
  for (let i = 0; i < 20; i++) expect(browserText).toContain(`Note de l’agent n° ${i}`);

  // Le serveur a la même chose : même texte humain, mêmes éditions, même nombre de blocs.
  const server = await (await request.get(`${API}/pages/${id}?anchors=block`)).json();
  expect(server.markdown).toContain(`Bloc humain : ${HUMAN.trimEnd()}`);
  expect(server.markdown).toContain("Compteur agent : 20");
  expect(Object.keys(server.anchors).length).toBe(browserBlocks.length);
});
