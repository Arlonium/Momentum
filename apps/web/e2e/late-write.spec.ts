// Une frappe livrée en retard par l'ancienne connexion du même onglet (R784 ; constat C1 de la
// re-relecture 4 de la garde des attributs), dans un vrai navigateur.
//
// 1. L'onglet tape « a » : la mise à jour part sur sa connexion S1, où elle est retenue en route
//    (réseau coupé : le noyau la garde à renvoyer).
// 2. S1 tombe pour l'onglet (pas pour le serveur, qui n'a rien vu). Il tape « b » hors ligne :
//    Yjs fond « ab » en un élément.
// 3. Il rouvre une connexion, S2. Le serveur l'accueille par son étape 1 (sans « a »).
// 4. La frappe de S1 arrive au serveur.
// 5. L'onglet reçoit l'étape 1 et répond par son étape 2, calculée depuis elle : « ab », à cheval
//    sur ce que le serveur connaît maintenant.
// Le serveur refusait cette étape 2 et fermait S2 (R740) ; il la coupe, comme Yjs l'aurait écrite
// pour lui, et ne ferme rien.
import { expect, test } from "@playwright/test";
import { closeContexts, createPage, getMmd, isWrite, newUser, unique, waitSynced } from "./helpers";

test.afterEach(closeContexts);

test("a_keystroke_delivered_late_by_the_old_connection_closes_nothing", async ({ browser, request }) => {
  const { id } = await createPage(request, { title: `Retard ${unique()}` });
  const p = await newUser(browser);
  let sockets = 0;
  let holdS1 = false;
  const heldS1: (string | Buffer)[] = [];
  let flushS1: () => void = () => {};
  const closedByServer: number[] = [];
  const log: string[] = [];
  await p.routeWebSocket(/\/sync\//, (ws) => {
    sockets += 1;
    const n = sockets;
    const server = ws.connectToServer();
    // La fermeture par l'onglet n'est pas portée au serveur pour S1 : le réseau est coupé.
    ws.onClose((code, reason) => {
      log.push(`S${n} fermée par l'onglet (${code})`);
      if (n !== 1) server.close({ code, reason });
    });
    server.onClose((code, reason) => {
      log.push(`S${n} fermée par le serveur (${code} ${reason})`);
      closedByServer.push(n);
      ws.close({ code, reason });
    });
    if (n === 1) {
      flushS1 = () => {
        for (const m of heldS1.splice(0)) server.send(m);
      };
      ws.onMessage((m) => {
        if (holdS1 && typeof m !== "string" && isWrite([...m])) {
          log.push("S1 : une écriture retenue en route");
          heldS1.push(m);
        } else server.send(m);
      });
      server.onMessage((m) => ws.send(m));
      return;
    }
    ws.onMessage((m) => {
      if (typeof m !== "string" && m[0] === 0 && m[1] === 1) log.push(`S${n} : étape 2 de l'onglet [${[...m].join(",")}]`);
      server.send(m);
    });
    let first = n === 2;
    server.onMessage((m) => {
      if (first && typeof m !== "string" && m[0] === 0 && m[1] === 0) {
        first = false;
        // L'étape 1 du serveur est partie : la frappe de S1 arrive, puis l'onglet reçoit l'étape 1.
        log.push("S2 : étape 1 du serveur ; la frappe de S1 est livrée");
        flushS1();
        void (async () => {
          await expect.poll(() => getMmd(request, id)).toBe("Avanta\n");
          ws.send(m);
        })();
        return;
      }
      ws.send(m);
    });
  });
  await p.goto(`/p/${id}`);
  await waitSynced(p);
  const editor = p.locator(".bn-editor");
  await editor.click();
  await p.keyboard.type("Avant");
  await expect.poll(() => getMmd(request, id)).toBe("Avant\n");

  holdS1 = true;
  await p.keyboard.type("a");
  await expect.poll(() => heldS1.length).toBeGreaterThan(0);
  // S1 tombe pour l'onglet.
  await p.evaluate(() => window.__mementum!.provider.disconnect());
  await p.keyboard.type("b");
  expect(await getMmd(request, id)).toBe("Avant\n");
  // Il revient.
  await p.evaluate(() => window.__mementum!.provider.connect());

  await expect.poll(() => getMmd(request, id), { timeout: 20_000 }).toBe("Avantab\n");
  // L'étape 2 est partie d'un tenant, depuis l'horloge d'avant « a » : c'est bien la course.
  expect(log.some((line) => line.startsWith("S2 : étape 2 de l'onglet")), log.join("\n")).toBe(true);
  // Ce que l'onglet tape ensuite arrive par S2, restée ouverte : aucune connexion de plus.
  await p.keyboard.type("c");
  await expect.poll(() => getMmd(request, id)).toBe("Avantabc\n");
  expect(closedByServer, `aucune connexion d'un onglet honnête n'est fermée par le serveur : ${log.join(" ; ")}`).toEqual([]);
  expect(sockets, log.join("\n")).toBe(2);
});
