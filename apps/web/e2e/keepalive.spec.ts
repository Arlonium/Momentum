// Entretien de la connexion de synchro d'un onglet où il ne se passe rien (Ruling R107).
//
// y-websocket ferme sa WebSocket quand il n'a reçu aucun message depuis 30 s
// (`messageReconnectTimeout`, dans `y-websocket/src/y-websocket.js`), ce qu'il vérifie toutes
// les 3 s ; puis il se reconnecte. Or le serveur n'envoie rien à un onglet seul sur sa page,
// où personne ne tape : après la première synchro, plus aucun message. L'onglet se
// reconnectait donc toutes les 30 s, et chaque reconnexion relit la session, les droits et
// la page. Le serveur envoie maintenant un message de présence vide à une connexion à qui il
// n'a rien envoyé depuis 20 s : le client ne se croit plus seul.
//
// D'où la durée de ce test : 40 s sans toucher à rien. C'est la constante du client (30 s),
// son pas de vérification (3 s), et une marge. Plus court, il passerait sans rien prouver.
import { expect, test } from "@playwright/test";
import { closeContexts, createPage, newUser, unique, waitSynced } from "./helpers";

test.afterEach(closeContexts);

/** Plus que les 30 s au bout desquelles y-websocket se reconnecte, et ses 3 s de pas. */
const IDLE_MS = 40_000;

test("idle_tab_keeps_its_sync_connection", async ({ browser, request }) => {
  // 40 s d'attente, plus l'ouverture de la page : au-delà du délai ordinaire d'un test (60 s)
  // sur un poste chargé.
  test.setTimeout(90_000);
  const { id } = await createPage(request, { title: `Entretien ${unique()}` });
  const p = await newUser(browser);
  // Les WebSocket de synchro que l'onglet ouvre, et celles qui se ferment.
  const opened: string[] = [];
  const closed: string[] = [];
  p.on("websocket", (ws) => {
    if (!ws.url().includes("/sync")) return;
    opened.push(ws.url());
    ws.on("close", () => closed.push(ws.url()));
  });

  await p.goto(`/p/${id}`);
  await waitSynced(p);
  expect(opened, "une WebSocket de synchro, celle de la page ouverte").toHaveLength(1);
  expect(opened[0]).toContain(`/sync/${id}`);

  // Plus rien : ni frappe, ni souris, ni appel.
  await p.waitForTimeout(IDLE_MS);

  expect(closed, "la WebSocket de synchro s'est fermée pendant l'attente").toEqual([]);
  expect(opened, "l'onglet s'est reconnecté pendant l'attente").toHaveLength(1);
  expect(await p.evaluate(() => window.__mementum!.provider.wsconnected)).toBe(true);
});
