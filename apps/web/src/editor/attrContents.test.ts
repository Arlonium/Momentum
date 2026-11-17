// Ce que le vrai Yjs lit de chaque type de contenu posé comme valeur d'un attribut (Ruling R702).
//
// La garde des attributs du serveur (`crates/mementum-crdt/src/attrs.rs`) ne juge jamais une
// valeur telle que yrs la lit quand Yjs la lit autrement : seule une valeur `Any` est une valeur
// d'attribut. Ce fichier tient la colonne « ce que Yjs lit » de la table du registre (lot 3,
// « Garde des attributs de bloc, tour de correction 1 ») ; la colonne « ce que yrs lit » et le
// refus sont tenus par `crates/mementum-crdt/tests/attrs.rs`. À relancer à une montée de Yjs.
import { expect, test } from "vitest";
import * as Y from "yjs";

/** Un entier variable de lib0. */
function varUint(value: number): number[] {
  const out: number[] = [];
  let rest = value;
  while (rest > 0x7f) {
    out.push(0x80 | (rest % 0x80));
    rest = Math.floor(rest / 0x80);
  }
  out.push(rest);
  return out;
}

/** Une chaîne de lib0 : sa longueur, ses octets. */
function text(value: string): number[] {
  const bytes = [...new TextEncoder().encode(value)];
  return [...varUint(bytes.length), ...bytes];
}

type Id = { client: number; clock: number };

/** Un paragraphe dont l'attribut `k` vaut « x » : son état, son élément et l'entrée de `k`. */
function base(): { state: Uint8Array; element: Id; entry: Id } {
  const doc = new Y.Doc();
  doc.clientID = 1;
  const element = new Y.XmlElement("paragraph");
  doc.getXmlFragment("f").insert(0, [element]);
  element.setAttribute("k", "x");
  const internals = element as unknown as { _item: { id: Id }; _map: Map<string, { id: Id }> };
  return { state: Y.encodeStateAsUpdate(doc), element: internals._item.id, entry: internals._map.get("k")!.id };
}

/** Le document après une mise à jour d'un élément fait à la main (client 9, horloge 0). */
function after(state: Uint8Array, info: number, origins: Id[], body: number[]): Y.Doc {
  const doc = new Y.Doc();
  Y.applyUpdate(doc, state);
  const update = [1, 1, 9, 0, info, ...origins.flatMap((id) => [...varUint(id.client), ...varUint(id.clock)]), ...body, 0];
  Y.applyUpdate(doc, Uint8Array.from(update));
  return doc;
}

const JSON_OBJECT = '{"toString":1}';

test("ce que Yjs lit de chaque contenu posé par l'origine de l'entrée d'un attribut", () => {
  const { state, entry } = base();
  // Référence du contenu, ses octets, ce que `getAttribute` rend.
  const read = (ref: number, body: number[]): unknown => {
    const doc = after(state, 0x80 | ref, [entry], body);
    const paragraph = doc.getXmlFragment("f").get(0) as Y.XmlElement;
    expect(paragraph.length, "aucun enfant").toBe(0);
    return paragraph.getAttribute("k");
  };
  // Une valeur `Any` (8) : la valeur. La seule forme qu'un client écrit, la seule que la garde juge.
  expect(read(8, [1, 119, ...text("red")])).toBe("red");
  // Du JSON (2) : `JSON.parse` de la chaîne, un objet (yrs en rend la chaîne). Refusé.
  expect(read(2, [1, ...text(JSON_OBJECT)])).toEqual({ toString: 1 });
  // Une chaîne (4) : son dernier caractère (yrs rend toute la chaîne). Refusée.
  expect(read(4, text("red"))).toBe("d");
  // Un objet incorporé (5) : l'objet. Refusé.
  expect(read(5, text(JSON_OBJECT))).toEqual({ toString: 1 });
  // Des octets (3). Refusés.
  expect(read(3, [2, 1, 2])).toBeInstanceOf(Uint8Array);
  // Une marque (6) et une suppression (1) : aucune valeur.
  expect(read(6, [...text("b"), ...text("true")])).toBeUndefined();
  expect(read(1, [1])).toBeUndefined();
  // Un type (7, ici un élément XML) et un sous-document (9) : un objet de Yjs. Refusés.
  expect(read(7, [3, ...text("p")])).toBeInstanceOf(Y.XmlElement);
  expect(read(9, [...text("guid"), 118, 0])).toBeInstanceOf(Y.Doc);
  // Un déplacement (11) : Yjs 13 ne le connaît pas, et lève. Le serveur le refuse à la lecture.
  expect(() => read(11, [0])).toThrow();
});

test("une valeur entre un enfant et une entrée de map est un enfant pour Yjs, pas un attribut", () => {
  // I1 de la relecture (Ruling R703) : origine = l'élément (un enfant du fragment), origine
  // droite = l'entrée de `k`. yrs recopie la clé du voisin de droite : un attribut ; Yjs prend
  // celle de gauche, nulle : un enfant sans type, et y-prosemirror lève en le lisant.
  const { state, element, entry } = base();
  const doc = after(state, 0xc8, [element, entry], [1, 119, ...text("red")]);
  const fragment = doc.getXmlFragment("f");
  expect((fragment.get(0) as Y.XmlElement).getAttribute("k")).toBe("x");
  expect(fragment.length).toBe(2);
  expect(fragment.toArray()[1]).toBe("red");
});

test("une largeur qui n'est pas un nombre voyage telle quelle", () => {
  // C2 de la relecture (Ruling R705) : lib0 écrit `NaN` en flottant, et le relit `NaN`.
  const doc = new Y.Doc();
  const cell = new Y.XmlElement("tableCell");
  doc.getXmlFragment("f").insert(0, [cell]);
  cell.setAttribute("colwidth", [Number.NaN, 120, undefined] as unknown as string);
  const copy = new Y.Doc();
  Y.applyUpdate(copy, Y.encodeStateAsUpdate(doc));
  expect((copy.getXmlFragment("f").get(0) as Y.XmlElement).getAttribute("colwidth")).toEqual([Number.NaN, 120, undefined]);
});
