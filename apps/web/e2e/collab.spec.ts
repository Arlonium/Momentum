// Édition à plusieurs : latence, verrou avant la synchro, lecture seule d'un client d'un
// autre schéma (Review Focus 2) et fusion après une coupure réseau (Review Focus 4).
import { expect, test } from "@playwright/test";
import {
  closeContexts,
  createPage,
  fragmentXml,
  getMmd,
  newUser,
  openPage,
  percentile,
  putMmd,
  REGISTRY_VERSION,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

type Arrival = { t: number; len: number };
type Probe = { __arrivals: Arrival[] };

test("two_browsers_converge", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Convergence ${unique()}` });
  const a = await openPage(browser, id);
  const b = await openPage(browser, id);

  // Chez B, chaque mise à jour venue du serveur est horodatée à l'événement `update` du
  // Y.Doc, avec la longueur du texte (unités UTF-16) du document à cet instant : on sait
  // ainsi quelle mise à jour a apporté quelle frappe.
  await b.evaluate(() => {
    const m = window.__mementum!;
    const probe = window as unknown as Probe;
    probe.__arrivals = [];
    type Node = { length: number; toArray?: () => Node[] };
    const textLength = (node: Node): number =>
      node.toArray ? node.toArray().reduce((n, child) => n + textLength(child), 0) : node.length;
    const fragment = m.doc.getXmlFragment("document-store") as unknown as Node;
    m.doc.on("update", (_update: Uint8Array, origin: unknown) => {
      const t = Date.now();
      if (origin === m.provider) probe.__arrivals.push({ t, len: textLength(fragment) });
    });
  });

  const text = "Frappe en direct, avec accents é à ç et 🚀 aussi !";
  const keystrokes = Array.from(text);
  expect(keystrokes).toHaveLength(49);
  await a.locator(".bn-editor").click();

  const latencies: number[] = [];
  let expected = 0;
  for (const ch of keystrokes) {
    expected += ch.length;
    // Chronomètre lancé juste avant la frappe chez A, arrêté par la mise à jour de B qui
    // contient cette frappe.
    const t0 = Date.now();
    await a.keyboard.type(ch);
    const arrived = await b.waitForFunction(
      (n) => (window as unknown as Probe).__arrivals.find((x) => x.len >= n)?.t,
      expected,
      { timeout: 5_000 },
    );
    latencies.push(Number(await arrived.jsonValue()) - t0);
  }

  const p50 = percentile(latencies, 0.5);
  const p95 = percentile(latencies, 0.95);
  console.log(
    `MESURE frappe → visible chez l'autre : p50 ${p50} ms, p95 ${p95} ms, ` +
      `max ${Math.max(...latencies)} ms (${latencies.length} frappes)`,
  );
  expect(p95).toBeLessThan(150);
  await expect.poll(() => fragmentXml(b)).toBe(await fragmentXml(a));
  await expect(b.locator(".bn-editor")).toContainText(text);
});

test("editing_locked_until_synced", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Verrou ${unique()}` });
  const p = await newUser(browser);

  // Les messages du serveur sont retenus jusqu'au relâchement ; ceux du client passent.
  let release = () => {};
  const released = new Promise<void>((resolve) => (release = resolve));
  await p.routeWebSocket(/\/sync\//, (ws) => {
    const server = ws.connectToServer();
    const held: (string | Buffer)[] = [];
    let open = false;
    server.onMessage((message) => (open ? ws.send(message) : held.push(message)));
    void released.then(() => {
      open = true;
      for (const message of held.splice(0)) ws.send(message);
    });
  });

  await p.goto(`/p/${id}`);
  const editor = p.locator(".bn-editor");
  await expect(editor).toHaveAttribute("contenteditable", "false");
  // Rien ne déverrouille l'éditeur tant que le serveur n'a pas répondu, et une frappe
  // ne passe pas.
  await p.waitForTimeout(1_000);
  await expect(editor).toHaveAttribute("contenteditable", "false");
  expect(await p.evaluate(() => window.__mementum?.synced)).toBe(false);
  await editor.click({ force: true });
  await p.keyboard.type("trop tôt");

  release();
  await expect(editor).toHaveAttribute("contenteditable", "true");
  await waitSynced(p);
  expect(await fragmentXml(p)).not.toContain("trop tôt");
  expect(await getMmd(request, id)).not.toContain("trop tôt");
});

/** Nœud Yjs d'un type que BlockNote ne connaît pas (bloc d'une version plus récente, R4). */
const FOREIGN_BLOCK =
  '<block type="futureWidget" id="b_future01" ' +
  'data="eyJhdHRycyI6eyJhIjoiMSJ9LCJjb250ZW50IjpbXSwiY2hpbGRyZW4iOltdfQ==" foreign=true/>';

test("stale_schema_is_read_only", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Schéma ${unique()}` });
  await putMmd(request, id, `Écrit par le serveur\n\n${FOREIGN_BLOCK}\n`);
  const before = await getMmd(request, id);
  expect(before).toContain('type="futureWidget"');
  const p = await newUser(browser);

  // Le serveur annonce un schéma plus récent que celui du client.
  await p.route("**/api/v1/info", async (route) => {
    const response = await route.fetch();
    const info = (await response.json()) as { schemaVersion: number };
    await route.fulfill({ response, json: { ...info, schemaVersion: info.schemaVersion + 1 } });
  });
  // Toute mise à jour envoyée par le client (message y-websocket « sync », sous-type
  // « update ») serait une écriture.
  const writes: number[][] = [];
  const sockets: URL[] = [];
  p.on("websocket", (ws) => {
    if (!ws.url().includes("/sync/")) return;
    sockets.push(new URL(ws.url()));
    ws.on("framesent", ({ payload }) => {
      const bytes = [...(typeof payload === "string" ? Buffer.from(payload) : payload)];
      if (bytes[0] === 0 && bytes[1] === 2) writes.push(bytes);
    });
  });

  await p.goto(`/p/${id}`);
  await expect(
    p.getByText(
      "Une nouvelle version de Mementum est disponible. Recharge la page pour continuer à écrire.",
    ),
  ).toBeVisible();
  await waitSynced(p);
  // Le client annonce toujours sa propre version de schéma (sans elle, schéma 0).
  expect(sockets.map((url) => url.searchParams.get("schema"))).toEqual([String(REGISTRY_VERSION)]);
  const editor = p.locator(".bn-editor");
  await expect(editor).toContainText("Écrit par le serveur");
  await expect(editor).toHaveAttribute("contenteditable", "false");
  expect(await p.evaluate(() => window.__mementum!.readOnly)).toBe(true);
  // Précondition (sinon ce test ne prouverait rien) : y-prosemirror a bien supprimé le nœud
  // qu'il ne sait pas construire, dans la copie locale de l'onglet.
  await expect.poll(() => fragmentXml(p)).not.toContain("futureWidget");

  await editor.click({ force: true });
  await p.keyboard.type("frappe interdite");
  await p.waitForTimeout(1_000);
  // Ni la suppression ni la frappe n'ont quitté l'onglet.
  expect(await getMmd(request, id)).toBe(before);
  expect(writes).toEqual([]);
});

test("offline_edits_merge_on_reconnect", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Hors ligne ${unique()}` });
  const a = await openPage(browser, id);
  const b = await openPage(browser, id);

  await b.context().setOffline(true);
  await b.evaluate(() => window.__mementum!.provider.disconnect());

  await a.locator(".bn-editor").click();
  await a.keyboard.press("Control+End");
  await a.keyboard.type("A en ligne");
  await b.locator(".bn-editor").click();
  await b.keyboard.press("Control+End");
  await b.keyboard.type("B hors ligne");

  // Le serveur a le texte de A, B ne l'a pas : B est bien coupé.
  await expect.poll(() => getMmd(request, id)).toContain("A en ligne");
  expect(await getMmd(request, id)).not.toContain("B hors ligne");
  expect(await fragmentXml(b)).not.toContain("A en ligne");

  await b.context().setOffline(false);
  await b.evaluate(() => window.__mementum!.provider.connect());

  await expect
    .poll(async () => (await fragmentXml(a)) === (await fragmentXml(b)), { timeout: 10_000 })
    .toBe(true);
  const xml = await fragmentXml(a);
  expect(xml).toContain("A en ligne");
  expect(xml).toContain("B hors ligne");
  await expect(a.locator(".bn-editor")).toContainText("B hors ligne");
  await expect(b.locator(".bn-editor")).toContainText("A en ligne");
  await expect.poll(() => getMmd(request, id)).toContain("B hors ligne");
  expect(await getMmd(request, id)).toContain("A en ligne");
});
