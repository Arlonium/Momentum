// Ruling R495 (constat N3 de la re-relecture du tour 5 de la piste S) : l'éditeur ne pose pas de
// lien de plus de 16 Kio, que la garde du serveur refuserait (la connexion se fermerait et
// rouvrirait sans fin), et le dit ; ce qui arrive du document partagé n'est jamais refusé.
import { afterEach, describe, expect, test } from "vitest";
import * as Y from "yjs";
import { REMOTE, peer, seedState, settle, type Peer, XYZ } from "../test/editors";
import { LINK_TOO_LONG, MAX_HREF_BYTES, linkBound } from "./linkBound";

const extensions = [linkBound];

afterEach(() => {
  for (const element of document.querySelectorAll(".mm-link-refused")) element.remove();
});

/** Les adresses des liens du bloc `id`, telles que l'éditeur les montre. */
function links(p: Peer, id: string): string[] {
  const block = p.editor.getBlock(id);
  const content = Array.isArray(block?.content) ? block.content : [];
  return content.flatMap((c) => ("href" in c && typeof c.href === "string" ? [c.href] : []));
}

/** Les adresses des liens écrits dans le document partagé. */
function storedLinks(doc: Y.Doc): string[] {
  const found: string[] = [];
  const walk = (node: Y.XmlElement | Y.XmlFragment | Y.XmlText) => {
    if (node instanceof Y.XmlText) {
      for (const op of node.toDelta() as { attributes?: { link?: { href?: string } } }[]) {
        const href = op.attributes?.link?.href;
        if (href !== undefined) found.push(href);
      }
    } else {
      for (const child of node.toArray()) walk(child as Y.XmlElement | Y.XmlText);
    }
  };
  walk(doc.getXmlFragment("document-store"));
  return found;
}

function alert(): string | null {
  return document.querySelector(".mm-link-refused[role=alert]")?.textContent ?? null;
}

describe("link_bound", () => {
  test("un lien à la borne est posé ; un octet de plus est refusé, et le message le dit", async () => {
    const base = await seedState(XYZ);
    const mona = await peer("Mona", 2, base, extensions);
    const atBound = `https://e.fr/${"u".repeat(MAX_HREF_BYTES - 13)}`;
    expect(new TextEncoder().encode(atBound).length).toBe(MAX_HREF_BYTES);
    mona.editor.setTextCursorPosition("x", "end");
    mona.editor.createLink(atBound, "lien");
    await settle();
    expect(links(mona, "x")).toEqual([atBound]);
    expect(alert()).toBeNull();

    const over = `${atBound}u`;
    mona.editor.setTextCursorPosition("y", "end");
    mona.editor.createLink(over, "trop");
    await settle();
    expect(links(mona, "y")).toEqual([]);
    expect(storedLinks(mona.doc)).toEqual([atBound]);
    expect(alert()).toBe(LINK_TOO_LONG);
  });

  // Ruling R537 (constat E1 de la re-relecture du tour 7) : la borne se juge sur l'adresse
  // elle-même, comme la garde du serveur la juge désormais, pas sur le JSON que Yjs en écrit, que
  // l'échappement allonge (`\"`, `\\`, `\u0001`) : à la borne, l'adresse passe, même quand son JSON
  // dépasse de loin la borne ; un octet de plus, elle est refusée.
  test("un lien à la borne, échappements compris, est posé ; un octet de plus est refusé", async () => {
    const base = await seedState(XYZ);
    const mona = await peer("Mona", 2, base, extensions);
    const tail = Array.from({ length: MAX_HREF_BYTES - 13 }, (_, i) => '"\\\u0001u'[i % 4]).join("");
    const atBound = `https://e.fr/${tail}`;
    expect(new TextEncoder().encode(atBound).length).toBe(MAX_HREF_BYTES);
    expect(JSON.stringify({ href: atBound }).length).toBeGreaterThan(2 * MAX_HREF_BYTES);
    mona.editor.setTextCursorPosition("x", "end");
    mona.editor.createLink(atBound, "lien");
    await settle();
    expect(links(mona, "x")).toEqual([atBound]);
    expect(storedLinks(mona.doc)).toEqual([atBound]);
    expect(alert()).toBeNull();

    mona.editor.setTextCursorPosition("y", "end");
    mona.editor.createLink(`${atBound}"`, "trop");
    await settle();
    expect(links(mona, "y")).toEqual([]);
    expect(alert()).toBe(LINK_TOO_LONG);
  });

  test("un contenu inséré qui porte un lien trop long est refusé (collage)", async () => {
    const base = await seedState(XYZ);
    const mona = await peer("Mona", 2, base, extensions);
    const over = `https://e.fr/${"é".repeat(MAX_HREF_BYTES / 2)}`;
    mona.editor.setTextCursorPosition("z", "end");
    mona.editor.insertInlineContent([{ type: "link", href: over, content: "collé" }]);
    await settle();
    expect(links(mona, "z")).toEqual([]);
    expect(alert()).toBe(LINK_TOO_LONG);
  });

  test("un lien trop long reçu du document partagé n'est pas refusé", async () => {
    const base = await seedState(XYZ);
    const mona = await peer("Mona", 2, base, extensions);
    const tom = await peer("Tom", 3, base);
    const over = `https://e.fr/${"u".repeat(MAX_HREF_BYTES)}`;
    tom.editor.setTextCursorPosition("x", "end");
    tom.editor.createLink(over, "reçu");
    await settle();
    Y.applyUpdate(mona.doc, Y.encodeStateAsUpdate(tom.doc), REMOTE);
    await settle();
    expect(links(mona, "x")).toEqual([over]);
    expect(alert()).toBeNull();
  });
});
