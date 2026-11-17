// La garde des attributs de bloc (Rulings R689 à R695, `crates/mementum-crdt/src/attrs.rs`).
//
// Yjs garde toute valeur JSON dans un attribut d'élément, y-prosemirror la passe telle quelle au
// nœud, et BlockNote écrit `String(valeur)` de chaque prop dans un attribut `data-…`. Un compte
// qui peut modifier une page, avec un client à lui, y écrivait `backgroundColor =
// {"toString": 1}` : « Cannot convert object to primitive value », et la page ne s'ouvrait plus
// chez personne (relecture de la Task C8, sondes `V7` et `V9`).
//
// Le serveur refuse désormais une telle mise à jour, et retire la valeur d'une page dont le
// journal la porte déjà (écrite avant la garde).
import { dirname, join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test, type Page } from "@playwright/test";
import * as Y from "yjs";
// Le type de `window.__mementum`.
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
import { accountsDir } from "./servers";

test.afterEach(closeContexts);

/** Les erreurs de la page (une exception hors de tout gestionnaire), relevées dès maintenant. */
function pageErrors(p: Page): string[] {
  const errors: string[] = [];
  p.on("pageerror", (error) => errors.push(String(error)));
  return errors;
}

/** La valeur de `backgroundColor` du premier bloc, dans le document de l'onglet. */
function background(p: Page): Promise<unknown> {
  return p.evaluate(() => {
    type Element = { get(index: number): Element; getAttribute(name: string): unknown };
    const fragment = window.__mementum!.doc.getXmlFragment("document-store") as unknown as Element;
    const value = fragment.get(0).get(0).get(0).getAttribute("backgroundColor");
    return value === undefined ? "absente" : value;
  });
}

test("a_hostile_attribute_is_refused_and_the_page_still_opens", async ({ browser, baseURL }) => {
  // Mona écrit dans une page ; Ève, membre qui peut la modifier, parle au serveur par une
  // WebSocket brute.
  const { alice: mona, aliceApi: monaApi, space, bruno: eve, brunoApi: eveApi, brunoId: eveId } =
    await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Attributs ${unique()}` });
  await setPageLevel(monaApi, page.id, eveId, "edit");
  const monaPage = await openPage(browser, page.id, mona);
  const monaErrors = pageErrors(monaPage);
  const editor = monaPage.locator(".bn-editor");
  await editor.click();
  await monaPage.keyboard.type("Bonjour");
  await expect.poll(() => getMmd(monaApi, page.id)).toContain("Bonjour");

  // L'entrée courante de `backgroundColor` du premier bloc, lue dans le document de Mona : la
  // valeur hostile l'écrase, comme `setAttribute` le ferait (elle l'a pour origine, et ne nomme
  // ni son élément ni sa clé).
  const entry = await monaPage.evaluate(() => {
    type Item = { id: { client: number; clock: number } };
    type Element = { get(index: number): Element; _map: Map<string, Item> };
    const fragment = window.__mementum!.doc.getXmlFragment("document-store") as unknown as Element;
    const item = fragment.get(0).get(0).get(0)._map.get("backgroundColor");
    return item ? { client: item.id.client, clock: item.id.clock } : null;
  });
  expect(entry).not.toBeNull();

  // Un client, un élément à l'horloge 0 : une valeur `Any` (8), une origine (0x80), une entrée
  // de map (0x20) ; la valeur : l'objet `{"toString": 1}` (118 : un objet d'une entrée ; 125 :
  // un entier).
  const key = [...new TextEncoder().encode("toString")];
  const update = [
    1,
    1,
    ...varUint(777_001),
    0,
    0xa8,
    ...varUint(entry!.client),
    ...varUint(entry!.clock),
    1,
    118,
    1,
    key.length,
    ...key,
    125,
    1,
    0, // aucune suppression
  ];
  const socket = await rawSync(eveApi, baseURL!, page.id);
  try {
    socket.send(Uint8Array.from([0, 2, ...varUint(update.length), ...update]));
    // Refusée : le serveur ferme la connexion qui l'a envoyée.
    expect(await socket.settled()).toBe("fermée");
  } finally {
    socket.close();
  }

  // Mona n'a rien reçu de la valeur, et ce qu'elle tape ensuite arrive au serveur.
  // (Le curseur est resté à la fin de « Bonjour ».)
  await monaPage.keyboard.type(" encore");
  await expect.poll(() => getMmd(monaApi, page.id)).toContain("Bonjour encore");
  expect(await background(monaPage)).toBe("default");

  // La page s'ouvre toujours chez les autres : l'éditeur d'Ève se monte, et montre le texte.
  const evePage = await openPage(browser, page.id, eve);
  const eveErrors = pageErrors(evePage);
  await expect(evePage.locator(".bn-editor")).toContainText("Bonjour encore");
  expect(await background(evePage)).toBe("default");
  expect(monaErrors).toEqual([]);
  expect(eveErrors).toEqual([]);
});

test("a_page_whose_journal_already_holds_the_attribute_opens_corrected", async ({ browser }) => {
  const { alice: mona, aliceApi: monaApi, space } = await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Journal ${unique()}` });

  // Le journal de la page, dans la base du serveur des tests : son document de départ, que
  // personne n'a encore ouvert (aucun acteur ne tient la page). On y pose, comme une entrée
  // écrite avant la garde, ce qu'un client à soi aurait écrit : la valeur de la sonde `V9` sur
  // la couleur de fond, une liste pour l'alignement, un nombre pour l'identifiant du bloc.
  const db = new DatabaseSync(join(dirname(accountsDir()), "data", "mementum.db"));
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    const doc = new Y.Doc();
    let last = 0;
    const snapshot = db
      .prepare("SELECT seq, state_blob FROM doc_snapshots WHERE node_id = ?")
      .get(page.id) as { seq: number; state_blob: Uint8Array } | undefined;
    if (snapshot) {
      Y.applyUpdate(doc, new Uint8Array(snapshot.state_blob));
      last = snapshot.seq;
    }
    const updates = db
      .prepare("SELECT seq, update_blob FROM doc_updates WHERE node_id = ? ORDER BY seq")
      .all(page.id) as { seq: number; update_blob: Uint8Array }[];
    for (const row of updates) {
      Y.applyUpdate(doc, new Uint8Array(row.update_blob));
      last = Math.max(last, row.seq);
    }
    const before = Y.encodeStateVector(doc);
    const container = (doc.getXmlFragment("document-store").get(0) as Y.XmlElement).get(0) as Y.XmlElement;
    const paragraph = container.get(0) as Y.XmlElement;
    expect(paragraph.nodeName).toBe("paragraph");
    doc.transact(() => {
      paragraph.setAttribute("backgroundColor", { toString: 1 } as unknown as string);
      paragraph.setAttribute("textAlignment", ["center"] as unknown as string);
      container.setAttribute("id", 7 as unknown as string);
    });
    db.prepare(
      "INSERT INTO doc_updates (node_id, seq, update_blob, created_at, actor_id) VALUES (?, ?, ?, ?, NULL)",
    ).run(page.id, last + 1, Y.encodeStateAsUpdate(doc, before), Date.now());
  } finally {
    db.close();
  }

  // La page s'ouvre, corrigée : l'éditeur se monte, la valeur n'est plus là, et l'on y écrit.
  const monaPage = await openPage(browser, page.id, mona);
  const errors = pageErrors(monaPage);
  expect(await background(monaPage)).toBe("absente");
  const editor = monaPage.locator(".bn-editor");
  await editor.click();
  await monaPage.keyboard.type("Bonjour");
  await expect.poll(() => getMmd(monaApi, page.id)).toContain("Bonjour");
  await expect(editor).toContainText("Bonjour");
  expect(errors).toEqual([]);
});

test("a_version_kept_before_the_guard_is_previewed_in_the_history", async ({ browser }) => {
  // Ruling R701 : une version coupée avant la garde, d'une page attaquée, porte encore la valeur.
  // Son état est servi relu : l'aperçu de « Historique » se monte, et montre son texte.
  const { alice: mona, aliceApi: monaApi, space } = await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Version ${unique()}` });
  const db = new DatabaseSync(join(dirname(accountsDir()), "data", "mementum.db"));
  try {
    db.exec("PRAGMA busy_timeout = 10000");
    const snapshot = db
      .prepare("SELECT state_blob FROM doc_snapshots WHERE node_id = ?")
      .get(page.id) as { state_blob: Uint8Array };
    const doc = new Y.Doc();
    Y.applyUpdate(doc, new Uint8Array(snapshot.state_blob));
    const container = (doc.getXmlFragment("document-store").get(0) as Y.XmlElement).get(0) as Y.XmlElement;
    const paragraph = container.get(0) as Y.XmlElement;
    doc.transact(() => {
      paragraph.insert(0, [new Y.XmlText("Ancien texte")]);
      paragraph.setAttribute("backgroundColor", { toString: 1 } as unknown as string);
    });
    db.prepare(
      "INSERT INTO doc_versions (id, node_id, seq, covers_seq, reason, authors, created_at, state_blob) " +
        "VALUES (?, ?, 0, 0, 'periodic', '[]', ?, ?)",
    ).run(crypto.randomUUID(), page.id, Date.now(), Y.encodeStateAsUpdate(doc));
  } finally {
    db.close();
  }

  const monaPage = await openPage(browser, page.id, mona);
  const errors = pageErrors(monaPage);
  await monaPage.getByRole("button", { name: "Actions de la page", exact: true }).click();
  await monaPage.getByRole("menuitem", { name: "Historique" }).click();
  const dialog = monaPage.getByRole("dialog", { name: "Historique" });
  const versions = dialog.getByRole("list", { name: "Versions de la page" }).getByRole("listitem");
  await expect(versions).toHaveCount(2);
  await versions.nth(1).getByRole("button").first().click();
  await expect(dialog.locator(".mm-preview-editor .bn-editor")).toContainText("Ancien texte");
  expect(errors).toEqual([]);
});
