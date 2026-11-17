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

test("une chaîne à cheval à origine droite, puis une valeur qui la suit, sont deux enfants pour Yjs", () => {
  // N1 de la re-relecture (Ruling R723). Le client 5 a écrit un texte vide dans le paragraphe
  // (son horloge 0). Il envoie ensuite, à cette même horloge, la chaîne « ab » d'origine droite
  // l'entrée de `k`, puis la valeur « ok » d'origine la fin de la chaîne. yrs n'intègre que « b »,
  // en fait l'entrée courante de `k` (la clé de son voisin de droite), puis « ok » l'écrase : le
  // serveur lisait `k = "ok"` et un paragraphe sans enfant. Yjs prend la clé du voisin de
  // gauche, le texte, qui n'en a pas : deux enfants sans type, et `k` garde sa valeur.
  const { state, entry } = base();
  const writer = new Y.Doc();
  writer.clientID = 5;
  Y.applyUpdate(writer, state);
  (writer.getXmlFragment("f").get(0) as Y.XmlElement).push([new Y.XmlText()]);
  const withText = Y.encodeStateAsUpdate(writer);
  const id = (at: Id): number[] => [...varUint(at.client), ...varUint(at.clock)];
  const children = (update: number[]): { k: unknown; rest: unknown[] } => {
    const doc = new Y.Doc();
    Y.applyUpdate(doc, withText);
    Y.applyUpdate(doc, Uint8Array.from(update));
    const paragraph = doc.getXmlFragment("f").get(0) as Y.XmlElement;
    return { k: paragraph.getAttribute("k"), rest: paragraph.toArray().filter((child) => !(child instanceof Y.XmlText)) };
  };
  // Tel que le client l'envoie : deux morceaux, la chaîne à cheval sur l'horloge 0.
  const sent = [1, 2, 5, 0, 0xc4, 5, 0, ...id(entry), ...text("ab"), 0x88, 5, 1, 1, 119, ...text("ok"), 0];
  expect(children(sent)).toEqual({ k: "x", rest: ["b", "ok"] });
  // Tel que le serveur le relayait : ce que yrs a intégré (« b » avec la marque d'une entrée de
  // map, que Yjs ne lit pas quand l'élément a une origine), et ce que yrs a supprimé : le texte,
  // son voisin de gauche, et « b », écrasé par « ok ».
  const relayed = [1, 2, 5, 1, 0xe4, 5, 0, ...id(entry), ...text("b"), 0xa8, 5, 1, 1, 119, ...text("ok"), 1, 5, 1, 0, 2];
  expect(children(relayed)).toEqual({ k: "x", rest: ["ok"] });
});

test("Yjs n'écrit pas d'élément à cheval sur l'état que le destinataire lui a annoncé", () => {
  // Ce que Yjs encode pour un destinataire part du vecteur d'état reçu de lui, même au milieu
  // d'un élément fusionné (`Item.write` avec un décalage récrit l'origine), et la mise à jour
  // d'une transaction part de l'état d'avant elle. Un élément n'est donc à cheval sur ce que le
  // serveur connaît que si le serveur en a appris davantage entre son annonce et la réponse :
  // une frappe arrivée par l'ancienne connexion du même onglet (`astride.test.ts`, Ruling R784,
  // qui corrige R741 : le serveur coupe alors l'élément comme Yjs l'aurait écrit).
  const doc = new Y.Doc();
  doc.clientID = 5;
  const text = doc.getText("t");
  const updates: Uint8Array[] = [];
  doc.on("update", (update: Uint8Array) => updates.push(update));
  // Dix lettres tapées une à une : Yjs les fond en un seul élément (horloges 0 à 9).
  for (let n = 0; n < 10; n += 1) text.insert(n, String.fromCharCode(97 + n));
  const structs = (doc as unknown as { store: { clients: Map<number, { length: number }[]> } }).store.clients.get(5);
  expect(structs?.length, "un seul élément fusionné").toBe(1);
  /** Le client, l'horloge de départ et le nombre d'éléments de la première entrée d'une mise à jour. */
  const first = (update: Uint8Array): { structs: number; client: number; clock: number } => {
    expect(update[0]).toBe(1);
    return { structs: update[1] ?? 0, client: update[2] ?? 0, clock: update[3] ?? 0 };
  };
  // Une étape 2 vers un serveur qui connaît les horloges 0 à `known - 1` : elle part de `known`.
  for (let known = 0; known <= 9; known += 1) {
    const serverState = Uint8Array.from([1, 5, known]);
    const step2 = Y.encodeStateAsUpdate(doc, known === 0 ? Uint8Array.from([0]) : serverState);
    expect(first(step2), `étape 2 depuis ${known}`).toEqual({ structs: 1, client: 5, clock: known });
  }
  // La mise à jour de chaque frappe part de l'horloge d'avant elle, et ne compte qu'une lettre.
  updates.forEach((update, n) => {
    expect(first(update), `frappe ${n}`).toEqual({ structs: 1, client: 5, clock: n });
    const applied = new Y.Doc();
    for (const earlier of updates.slice(0, n + 1)) Y.applyUpdate(applied, earlier);
    expect(applied.getText("t").toString()).toHaveLength(n + 1);
  });
});
