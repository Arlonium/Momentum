import { expect, test } from "@playwright/test";
import { fragmentXml, openPage, percentile, uniquePage } from "./helpers";

test("typing_shows_up_remotely", async ({ browser }) => {
  const id = uniquePage("t2-type");
  const a = await openPage(browser, id, "Alice");
  const b = await openPage(browser, id, "Bob");

  await b.evaluate(() => {
    const m = (window as any).__mementum;
    m.arrivals = [];
    m.doc.on("update", (_u: Uint8Array, origin: unknown) => {
      if (origin === m.provider) m.arrivals.push(Date.now());
    });
  });

  await a.locator(".bn-editor").click();
  const text = "Mementum écrit en direct, accents et emojis 🚀 compris !";
  const latencies: number[] = [];
  for (const ch of text.slice(0, 50)) {
    const before = await b.evaluate(() => (window as any).__mementum.arrivals.length);
    const t0 = Date.now();
    await a.keyboard.type(ch);
    await b.waitForFunction((n) => (window as any).__mementum.arrivals.length > n, before, { timeout: 5_000 });
    const arrived = await b.evaluate(() => {
      const arr = (window as any).__mementum.arrivals;
      return arr[arr.length - 1];
    });
    latencies.push(arrived - t0);
  }
  const p95 = percentile(latencies, 0.95);
  console.log(`MESURE frappe→distant : p50 ${percentile(latencies, 0.5)} ms, p95 ${p95} ms (${latencies.length} frappes)`);
  expect(p95).toBeLessThan(150);
  await expect.poll(() => fragmentXml(b)).toBe(await fragmentXml(a));
});

test("remote_cursor_visible", async ({ browser }) => {
  const id = uniquePage("t2-cursor");
  const a = await openPage(browser, id, "Alice");
  const b = await openPage(browser, id, "Bob");
  await a.locator(".bn-editor").click();
  await a.keyboard.type("Bonjour");
  await expect(b.locator(".bn-collaboration-cursor__base", { hasText: "Alice" })).toHaveCount(1, { timeout: 5_000 });
});

test("offline_edits_merge_on_reconnect", async ({ browser }) => {
  const id = uniquePage("t2-offline");
  const a = await openPage(browser, id, "Alice");
  const b = await openPage(browser, id, "Bob");

  await a.context().setOffline(true);
  await a.evaluate(() => (window as any).__mementum.provider.disconnect());
  await a.locator(".bn-editor").click();
  await a.keyboard.type("hors ligne");
  await b.locator(".bn-editor").click();
  await b.keyboard.press("Control+End");
  await b.keyboard.type("en ligne");

  await a.context().setOffline(false);
  await a.evaluate(() => (window as any).__mementum.provider.connect());
  await expect.poll(async () => (await fragmentXml(a)) === (await fragmentXml(b)), { timeout: 10_000 }).toBe(true);
  const xml = await fragmentXml(a);
  expect(xml).toContain("hors ligne");
  expect(xml).toContain("en ligne");
});

test("open_10k_block_page", async ({ browser, request }) => {
  // Sans la page `bench-10k`, le délai dépassé serait compté comme l'échec attendu : faux vert.
  const pre = await (await request.get("http://localhost:8080/pages/bench-10k")).json();
  const blocks = String(pre.markdown ?? "").split("\n\n").length;
  test.skip(blocks < 10_000, "page bench-10k absente : lancer d'abord `cargo run --release -p spike-server --bin measure`");
  // Budget non tenu par BlockNote sans affichage paresseux (mesure consignée dans RESULTS.md).
  test.fail(true, "BlockNote rend les 10 000 blocs d'un coup : ~1,3 s sur PC");
  const p = await openPage(browser, uniquePage("t2-warmup"), "Alice"); // application déjà chargée
  await p.goto("/?page=bench-10k&user=Alice");
  await p.waitForSelector('[data-id="bench-0"]', { timeout: 60_000 });
  const { total, synced } = await p.evaluate(() => {
    const m = (window as any).__mementum;
    return { total: performance.now() - m.t0, synced: m.tSynced - m.t0 };
  });
  console.log(`MESURE ouverture page 10 000 blocs : synchro ${Math.round(synced)} ms, premier bloc affiché ${Math.round(total)} ms`);
  expect(total).toBeLessThan(300);
});
