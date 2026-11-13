// Entretien des connexions d'un onglet où il ne se passe rien.
//
// La connexion de synchro (Ruling R107). y-websocket ferme sa WebSocket quand il n'a reçu aucun
// message depuis 30 s (`messageReconnectTimeout`, dans `y-websocket/src/y-websocket.js`), ce
// qu'il vérifie toutes les 3 s ; puis il se reconnecte. Or le serveur n'envoie rien à un onglet
// seul sur sa page, où personne ne tape : après la première synchro, plus aucun message.
// L'onglet se reconnectait donc toutes les 30 s, et chaque reconnexion relit la session, les
// droits et la page. Le serveur envoie maintenant un message de présence vide à une connexion à
// qui il n'a rien envoyé depuis 20 s : le client ne se croit plus seul.
//
// Le canal de l'espace (R215, S6). Le client ferme un canal resté muet 45 s
// (`EVENTS_SILENCE_MS`, `useSpaceEvents.ts`) : une connexion à demi ouverte ne se voit pas
// autrement. Dans un espace où rien ne s'écrit, le serveur y redit donc sa version à chaque
// ping, toutes les 20 s (`EVENTS_PING`), et le canal reste ouvert. Le serveur parle en temps
// réel : une horloge simulée dans la page n'y changerait rien.
//
// D'où la durée de ce test : 50 s sans toucher à rien. C'est le chien de garde du canal (45 s),
// qui couvre aussi la constante de y-websocket (30 s) et son pas (3 s), et une marge. Plus
// court, il passerait sans rien prouver.
import { expect, test } from "@playwright/test";
import {
  apiAs,
  closeContexts,
  createPage,
  newUser,
  signUpAccount,
  unique,
  waitSynced,
} from "./helpers";

test.afterEach(closeContexts);

/**
 * Plus que le chien de garde du canal de l'espace (45 s), donc que les 30 s au bout desquelles
 * y-websocket se reconnecte, et ses 3 s de pas.
 */
const IDLE_MS = 50_000;

test("idle_tab_keeps_its_sync_connection", async ({ browser }) => {
  // 50 s d'attente, plus l'ouverture de la page : au-delà du délai ordinaire d'un test (60 s)
  // sur un poste chargé.
  test.setTimeout(100_000);
  // Un compte neuf, seul dans son espace : rien ne s'y écrit pendant l'attente, et le canal de
  // l'espace ne reçoit que ce que le serveur redit.
  const account = await signUpAccount(`Repos ${unique()}`);
  const { id } = await createPage(await apiAs(account), { title: `Entretien ${unique()}` });
  const p = await newUser(browser, account);
  // Les WebSocket que l'onglet ouvre, celles qui se ferment, et les trames du canal de l'espace.
  const opened: string[] = [];
  const closed: string[] = [];
  const channels: string[] = [];
  const channelsClosed: string[] = [];
  const frames: string[] = [];
  p.on("websocket", (ws) => {
    if (ws.url().includes("/events/")) {
      channels.push(ws.url());
      ws.on("framereceived", (frame) => frames.push(String(frame.payload)));
      ws.on("close", () => channelsClosed.push(ws.url()));
    } else if (ws.url().includes("/sync")) {
      opened.push(ws.url());
      ws.on("close", () => closed.push(ws.url()));
    }
  });

  await p.goto(`/p/${id}`);
  await waitSynced(p);
  expect(opened, "une WebSocket de synchro, celle de la page ouverte").toHaveLength(1);
  expect(opened[0]).toContain(`/sync/${id}`);
  // Le canal de l'espace est ouvert, et le serveur y a dit sa version.
  await expect.poll(() => frames.length).toBeGreaterThan(0);
  expect(channels, "un canal de l'espace, celui de la page ouverte").toHaveLength(1);

  // Plus rien : ni frappe, ni souris, ni appel, et rien ne s'écrit dans l'espace.
  await p.waitForTimeout(IDLE_MS);

  expect(closed, "la WebSocket de synchro s'est fermée pendant l'attente").toEqual([]);
  expect(opened, "l'onglet s'est reconnecté pendant l'attente").toHaveLength(1);
  expect(await p.evaluate(() => window.__mementum!.provider.wsconnected)).toBe(true);
  expect(channelsClosed, "le canal de l'espace s'est fermé pendant l'attente").toEqual([]);
  expect(channels, "le canal de l'espace a été rouvert pendant l'attente").toHaveLength(1);
  // Témoin : le serveur a redit sa version à chaque ping (deux en 50 s, plus la première),
  // toujours le même numéro. C'est ce qui a tenu le canal ouvert au-delà des 45 s.
  expect(frames.length).toBeGreaterThanOrEqual(3);
  expect(new Set(frames).size).toBe(1);
  expect(frames[0]).toMatch(/^\{"lsn":\d+\}$/);
});
