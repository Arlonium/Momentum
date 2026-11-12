// Ce que le serveur relaie d'une mise à jour du document est ce qu'il en a lu, réencodé,
// jamais les octets reçus (Ruling R130).
//
// yrs, côté serveur, et Yjs, dans le navigateur, ne lisent pas les mêmes octets pareil : yrs
// laisse tomber les bits d'un entier au-delà de 32, Yjs lit l'entier en entier. Une suppression
// dont l'horloge est écrite avec 2^32 de plus effaçait donc un texte sur le serveur, et rien
// dans les navigateurs : on continuait d'y écrire, dans un texte que le serveur n'avait plus,
// et tout était perdu au rechargement, sans que rien le dise.
import { expect, test } from "@playwright/test";
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

test.afterEach(closeContexts);

/** Une plage d'éléments de Yjs : le client qui les a écrits, leur horloge, leur longueur. */
type Range = { client: number; clock: number; len: number };

/**
 * Le message de synchro d'une mise à jour sans bloc, qui supprime `ranges` (un seul client),
 * chaque horloge écrite avec `shift` de plus.
 */
function deletion(ranges: Range[], shift: number): Uint8Array<ArrayBuffer> {
  const client = ranges[0].client;
  expect(ranges.every((range) => range.client === client)).toBe(true);
  const update = [
    0, // aucun bloc
    1, // un client dans les suppressions
    ...varUint(client),
    ...varUint(ranges.length),
    ...ranges.flatMap((range) => [...varUint(range.clock + shift), ...varUint(range.len)]),
  ];
  // Message de synchro (0), mise à jour (2), puis ses octets.
  return Uint8Array.from([0, 2, ...varUint(update.length), ...update]);
}

test("update_read_otherwise_by_a_browser_is_relayed_as_the_server_read_it", async ({
  browser,
  baseURL,
}) => {
  // Mona écrit dans une page ; Ève, membre qui peut la modifier, parle au serveur par une
  // WebSocket brute.
  const { alice: mona, aliceApi: monaApi, space, brunoApi: eveApi, brunoId: eveId } =
    await ownerAndMember();
  const page = await createPage(monaApi, { spaceId: space.id, title: `Relais ${unique()}` });
  await setPageLevel(monaApi, page.id, eveId, "edit");
  const monaPage = await openPage(browser, page.id, mona);
  const editor = monaPage.locator(".bn-editor");
  await editor.click();
  await monaPage.keyboard.type("Bonjour");
  await expect.poll(() => getMmd(monaApi, page.id)).toContain("Bonjour");

  // Les éléments du texte, lus dans le document de Mona.
  const ranges = await monaPage.evaluate(() => {
    type Item = {
      id: { client: number; clock: number };
      length: number;
      deleted: boolean;
      content: { str?: string };
      right: Item | null;
    };
    type Node = { toArray?: () => Node[]; toDelta?: () => unknown; _start?: Item | null };
    const texts: Node[] = [];
    const walk = (node: Node) => {
      // Un `Y.XmlText` : il a `toDelta`, un élément non.
      if (typeof node.toDelta === "function") texts.push(node);
      else for (const child of node.toArray?.() ?? []) walk(child);
    };
    walk(window.__mementum!.doc.getXmlFragment("document-store") as unknown as Node);
    const found: { client: number; clock: number; len: number }[] = [];
    for (const text of texts) {
      for (let item = text._start ?? null; item !== null; item = item.right) {
        if (!item.deleted && item.content.str !== undefined) {
          found.push({ client: item.id.client, clock: item.id.clock, len: item.length });
        }
      }
    }
    return found;
  });
  expect(ranges.reduce((sum, range) => sum + range.len, 0)).toBe("Bonjour".length);

  const socket = await rawSync(eveApi, baseURL!, page.id);
  try {
    // Les horloges écrites avec 2^32 de plus : le texte pour le serveur, une plage inconnue
    // pour un navigateur qui recevrait ces octets.
    socket.send(deletion(ranges, 2 ** 32));
    await socket.processed();

    // Le serveur a supprimé le texte, et le navigateur de Mona aussi : il a reçu ce que le
    // serveur a lu.
    await expect.poll(() => getMmd(monaApi, page.id)).not.toContain("Bonjour");
    await expect(editor).not.toContainText("Bonjour");
    // Ce que Mona tape ensuite arrive bien au serveur.
    await editor.click();
    await monaPage.keyboard.type("Encore là");
    await expect.poll(() => getMmd(monaApi, page.id)).toContain("Encore là");
  } finally {
    socket.close();
  }
});
