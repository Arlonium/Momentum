// Retour du réseau : ce qu'on tape aussitôt revenu, avant que l'onglet ait renvoyé ce qu'il
// avait tapé hors ligne, n'est pas perdu et ne fait pas fermer la connexion (Ruling R131).
//
// À la reconnexion, y-websocket envoie son étape 1, mais il n'envoie ce que le serveur n'a pas
// (les frappes faites hors ligne) qu'en réponse à l'étape 1 du serveur. Une frappe faite entre
// les deux part aussitôt, avant elles : ses horloges commencent au-delà de ce que le serveur
// connaît de ce client. Le serveur l'intégrait en laissant un trou, que les navigateurs ne
// comblaient pas ; ou, comme ici, où elle prolonge le texte tapé hors ligne, il la refusait et
// fermait la connexion. Il la retient désormais jusqu'à ce que la suite arrive.
import { expect, test } from "@playwright/test";
import { closeContexts, createPage, getMmd, isWrite, newUser, unique, waitSynced } from "./helpers";

test.afterEach(closeContexts);

test("typing_right_after_coming_back_online_loses_nothing", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Retour ${unique()}` });
  const p = await newUser(browser);
  // Chaque ouverture d'une WebSocket de synchro de l'onglet.
  let openings = 0;
  p.on("websocket", (ws) => {
    if (ws.url().includes("/sync/")) openings += 1;
  });
  await p.goto(`/p/${id}`);
  await waitSynced(p);
  const editor = p.locator(".bn-editor");
  await editor.click();
  await p.keyboard.type("Avant");
  await expect.poll(() => getMmd(request, id)).toBe("Avant\n");

  // Hors ligne, on écrit.
  await p.context().setOffline(true);
  await p.evaluate(() => window.__mementum!.provider.disconnect());
  await p.keyboard.press("End");
  await p.keyboard.type(" hors ligne");
  expect(await getMmd(request, id)).toBe("Avant\n");

  // Au retour, ce que le serveur envoie à l'onglet est retenu en route jusqu'à ce que la frappe
  // suivante soit partie : l'onglet n'a alors pas reçu l'étape 1 du serveur, ni donc renvoyé ce
  // qu'il avait tapé hors ligne. Sans ce délai, la même fenêtre dure quelques millisecondes, et
  // une frappe n'y tombe qu'au hasard. Ce que l'onglet envoie passe tel quel.
  let release = () => {};
  const released = new Promise<void>((resolve) => (release = resolve));
  let writes = 0;
  await p.routeWebSocket(/\/sync\//, (ws) => {
    const server = ws.connectToServer();
    ws.onMessage((message) => {
      if (typeof message !== "string" && isWrite([...message])) writes += 1;
      server.send(message);
    });
    const held: (string | Buffer)[] = [];
    let open = false;
    server.onMessage((message) => (open ? ws.send(message) : held.push(message)));
    void released.then(() => {
      open = true;
      for (const message of held.splice(0)) ws.send(message);
    });
  });
  await p.context().setOffline(false);
  await p.evaluate(() => window.__mementum!.provider.connect());
  await expect.poll(() => p.evaluate(() => window.__mementum!.provider.wsconnected)).toBe(true);
  await p.keyboard.type(" en ligne");
  // La frappe est partie, avant ce que l'onglet avait tapé hors ligne.
  await expect.poll(() => writes).toBeGreaterThan(0);
  release();

  await expect
    .poll(() => getMmd(request, id), { timeout: 15_000 })
    .toBe("Avant hors ligne en ligne\n");
  await expect(editor).toContainText("Avant hors ligne en ligne");
  // L'ouverture de la page, puis le retour : le serveur n'a jamais fermé la connexion.
  expect(openings).toBeLessThanOrEqual(2);
});
