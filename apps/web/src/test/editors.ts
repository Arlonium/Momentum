// De vrais éditeurs BlockNote liés à des `Y.Doc`, pour les tests unitaires du document partagé
// (`editor/blockMoves.test.ts`, `editor/keyedBlocks.test.ts`, `editor/followBlock.test.ts`).
//
// Chaque éditeur est lié par `withCollaboration` (le chemin Yjs 13 de l'application) à un
// document dont l'identifiant de client est fixé : l'issue d'un conflit d'insertion se tranche
// par ces identifiants, et elle est ainsi la même à chaque exécution. Rien ne relie les
// documents : leurs mises à jour s'échangent à la main (`exchange`), et passent par un troisième
// document, celui du « serveur », lu sans éditeur.
//
// Un glisser est joué comme le dépôt de ProseMirror (`drag`) : jsdom n'a pas de mise en page,
// donc pas de vrai dépôt ; celui d'un navigateur est dans `e2e/block-move.spec.ts`. Tab, Maj+Tab
// et le déplacement au clavier passent par la touche, que l'éditeur reçoit comme d'un clavier.
import { BlockNoteEditor } from "@blocknote/core";
import { withCollaboration } from "@blocknote/core/yjs";
import { NodeSelection, TextSelection } from "prosemirror-state";
import { afterEach } from "vitest";
import * as Y from "yjs";
import { FRAGMENT } from "../editor/collab";
import type { MementumEditor } from "../editor/debug";
import { schema } from "../editor/schema";

/** Origine des mises à jour reçues : elles ne repartent pas. */
export const REMOTE = "remote";

/** Identifiant de client de qui écrit la page de départ. */
export const SEED = 1;

type Transaction = Parameters<Parameters<MementumEditor["transact"]>[0]>[0];
export type PmNode = Transaction["doc"];
export type Seed = Parameters<MementumEditor["replaceBlocks"]>[1];
type Extensions = NonNullable<Parameters<typeof BlockNoteEditor.create>[0]>["extensions"];

/** Un bloc : id, type, texte, enfants. */
export type Shown = { id: string; type: string; text: string; children: Shown[] };

/** Un bloc attendu (paragraphe sans enfants par défaut). */
export function block(
  id: string,
  text: string,
  { type = "paragraph", children = [] }: { type?: string; children?: Shown[] } = {},
): Shown {
  return { id, type, text, children };
}

// ---------------------------------------------------------------------------
// Des pages de départ, et leurs blocs tels qu'on les attend
// ---------------------------------------------------------------------------

export const XYZ: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "paragraph", content: "Bloc Y" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];
export const X = block("x", "Bloc X");
export const Y_ = block("y", "Bloc Y");
export const Z = block("z", "Bloc Z");

export const XYZW: Seed = [...XYZ, { id: "w", type: "paragraph", content: "Bloc W" }];
export const W = block("w", "Bloc W");

export const XYZA: Seed = [...XYZ, { id: "a", type: "paragraph", content: "Bloc A" }];
export const A = block("a", "Bloc A");

/** Y est un titre : un élément de contenu d'un autre type que ses voisins. */
export const TITLED: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "heading", props: { level: 2 }, content: "Titre Y" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];
export const TITLE_Y = block("y", "Titre Y", { type: "heading" });

/** X a un enfant, X1. */
export const PARENTED: Seed = [
  {
    id: "x",
    type: "paragraph",
    content: "Bloc X",
    children: [{ id: "x1", type: "paragraph", content: "Enfant X1" }],
  },
  { id: "y", type: "paragraph", content: "Bloc Y" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];
/** X et son enfant, dont le texte est `text`. */
export const xWithChild = (text: string) => block("x", "Bloc X", { children: [block("x1", text)] });

/** W, X, Y au même niveau : Tab dans X en fait l'enfant de W. */
export const W_FIRST: Seed = [
  { id: "w", type: "paragraph", content: "Bloc W" },
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "paragraph", content: "Bloc Y" },
];
/** X enfant de W : Maj+Tab dans X le remonte au niveau de W. */
export const W_PARENT: Seed = [
  {
    id: "w",
    type: "paragraph",
    content: "Bloc W",
    children: [{ id: "x", type: "paragraph", content: "Bloc X" }],
  },
  { id: "y", type: "paragraph", content: "Bloc Y" },
];
/** W et son enfant X, dont le texte est `text`. */
export const wWithChild = (text: string) => block("w", "Bloc W", { children: [block("x", text)] });

// ---------------------------------------------------------------------------
// Les éditeurs, les gestes, l'échange
// ---------------------------------------------------------------------------

/** Une personne : son document, son éditeur, et ce qu'il a écrit, pas encore envoyé. */
export type Peer = { name: string; doc: Y.Doc; editor: MementumEditor; writes: Uint8Array[] };

/** Laisse passer les minuteurs de y-prosemirror (son premier rendu est différé). */
export const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Les éditeurs montés par le test en cours : démontés à sa fin, avec leur document. */
const mounted: { peer: Peer; host: HTMLElement }[] = [];
afterEach(() => {
  for (const { peer, host } of mounted.splice(0)) {
    peer.editor.unmount();
    host.remove();
    peer.doc.destroy();
  }
});

/**
 * Un éditeur monté, lié à un document neuf d'identifiant de client `clientID`, parti de `base`.
 * `extensions` : celles de l'éditeur de la page que le test exerce.
 */
export async function peer(
  name: string,
  clientID: number,
  base?: Uint8Array,
  extensions?: Extensions,
): Promise<Peer> {
  const doc = new Y.Doc();
  doc.clientID = clientID;
  if (base) Y.applyUpdate(doc, base, REMOTE);
  const writes: Uint8Array[] = [];
  doc.on("update", (update: Uint8Array, origin: unknown) => {
    if (origin !== REMOTE) writes.push(update);
  });
  const editor = BlockNoteEditor.create(
    withCollaboration({
      schema,
      extensions,
      collaboration: {
        fragment: doc.getXmlFragment(FRAGMENT),
        user: { name, color: "#66655f" },
        provider: undefined,
      },
    }),
  ) as unknown as MementumEditor;
  const host = document.createElement("div");
  document.body.append(host);
  editor.mount(host);
  await settle();
  const made = { name, doc, editor, writes };
  mounted.push({ peer: made, host });
  return made;
}

/**
 * L'état d'une page de départ, écrit par un éditeur d'identifiant de client `SEED` dans un
 * document vide : les attributs que pose BlockNote, ceux qu'écrit aussi le serveur.
 */
export async function seedState(blocks: Seed): Promise<Uint8Array> {
  const writer = await peer("Départ", SEED);
  writer.editor.replaceBlocks(writer.editor.document, blocks);
  await settle();
  return Y.encodeStateAsUpdate(writer.doc);
}

/**
 * La page `blocks` ouverte par deux personnes, Mona et Tom, aux identifiants de client `monaId`
 * et `tomId`, et le document du serveur. `quiet` : ni l'une ni l'autre n'a rien écrit en ouvrant
 * la page.
 */
export async function duo(blocks: Seed, monaId: number, tomId: number) {
  const base = await seedState(blocks);
  const mona = await peer("Mona", monaId, base);
  const tom = await peer("Tom", tomId, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  const quiet = mona.writes.length === 0 && tom.writes.length === 0;
  return { base, mona, tom, server, quiet };
}

/** Les blocs que montre l'éditeur de `p`, avec leur type et leurs enfants. */
export function shown(p: Peer): Shown[] {
  const read = (blocks: MementumEditor["document"]): Shown[] =>
    blocks.map((b) => ({
      id: b.id,
      type: b.type,
      text: Array.isArray(b.content)
        ? b.content.map((c) => ("text" in c ? String(c.text) : "")).join("")
        : "",
      children: read(b.children),
    }));
  return read(p.editor.document);
}

/**
 * Les blocs du document Yjs `doc`, avec leur type et leurs enfants, lus sans éditeur : ce que le
 * serveur garde et relit. Un éditeur, lui, donne un id neuf à un bloc dont l'id revient
 * (extension `UniqueID` de BlockNote), sans l'écrire au document : seule cette lecture montre un
 * id en double.
 */
export function stored(doc: Y.Doc): Shown[] {
  const read = (group: Y.XmlElement | undefined): Shown[] =>
    (group?.toArray() ?? []).map((node) => {
      const container = node as Y.XmlElement;
      const [content, children] = container.toArray() as Y.XmlElement[];
      const text = content
        .toArray()
        .map((piece) =>
          piece instanceof Y.XmlText
            ? piece
                .toDelta()
                .map((d: { insert: unknown }) => String(d.insert))
                .join("")
            : "",
        )
        .join("");
      return {
        id: String(container.getAttribute("id")),
        type: content.nodeName,
        text,
        children: read(children),
      };
    });
  return read(doc.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement | undefined);
}

/** Une page en une ligne, pour les messages : `y « Bloc Y » | x « Bloc X » [x1 « … »]`. */
export function line(page: Shown[]): string {
  return page
    .map((b) => {
      const type = b.type === "paragraph" ? "" : ` (${b.type})`;
      const children = b.children.length ? ` [${line(b.children)}]` : "";
      return `${b.id}${type} « ${b.text} »${children}`;
    })
    .join(" | ");
}

/** Les ids qui reviennent plus d'une fois dans `blocks`, enfants compris. */
export function repeatedIds(blocks: Shown[]): string[] {
  const seen = new Map<string, number>();
  const walk = (list: Shown[]) =>
    list.forEach((b) => {
      seen.set(b.id, (seen.get(b.id) ?? 0) + 1);
      walk(b.children);
    });
  walk(blocks);
  return [...seen].filter(([, n]) => n > 1).map(([id]) => id);
}

/** Le `blockContainer` d'id `id` dans `doc`, et sa position (juste avant lui). */
export function containerOf(doc: PmNode, id: string): { pos: number; node: PmNode } {
  let found: { pos: number; node: PmNode } | undefined;
  doc.descendants((node, pos) => {
    if (found) return false;
    if (node.type.name === "blockContainer" && node.attrs.id === id) {
      found = { pos, node };
      return false;
    }
    return true;
  });
  if (!found) throw new Error(`aucun bloc ${id}`);
  return found;
}

/**
 * Chez `p`, glisse le bloc `moved` sous le bloc `under`, ou au-dessus du bloc `above`, comme le
 * dépôt de ProseMirror dans un navigateur (`editHandlers.drop` de prosemirror-view), relevé le
 * 2026-11-13 :
 *  - une transaction retire le bloc, puis le remet, relu du presse-papiers (des nœuds neufs), et
 *    sélectionne le bloc déposé comme un nœud. C'est elle que y-prosemirror écrit : il ne regarde
 *    que l'état final du document et la sélection (`_prosemirrorChanged` ;
 *    `a_drop_is_written_with_the_selection_on_what_it_drops`, `e2e/block-move.spec.ts`) ;
 *  - BlockNote remplace aussitôt cette sélection par un curseur de texte à la fin du bloc déposé
 *    (`the_cursor_left_in_a_dropped_block…`, `e2e/block-move.spec.ts`). Pour un bloc sans texte
 *    (une image), ce second temps n'a pas été relevé : la sélection de nœud est laissée.
 */
export function drag(p: Peer, moved: string, place: { under: string } | { above: string }): void {
  p.editor.transact((tr) => {
    const { pos, node } = containerOf(tr.doc, moved);
    const copy = p.editor.pmSchema.nodeFromJSON(node.toJSON());
    tr.delete(pos, pos + node.nodeSize);
    const target = containerOf(tr.doc, "under" in place ? place.under : place.above);
    const at = "under" in place ? target.pos + target.node.nodeSize : target.pos;
    tr.insert(at, copy);
    tr.setSelection(NodeSelection.create(tr.doc, at));
  });
  p.editor.transact((tr) => {
    const { pos, node } = containerOf(tr.doc, moved);
    const content = node.firstChild!;
    if (content.inlineContent) tr.setSelection(TextSelection.create(tr.doc, pos + 2 + content.content.size));
  });
}

/**
 * `p` sélectionne le bloc `id` entier, comme un nœud : son conteneur. Ce n'est pas ce que donne un
 * clic sur une image (`selectContent`).
 */
export function selectBlock(p: Peer, id: string): void {
  p.editor.transact((tr) => {
    tr.setSelection(NodeSelection.create(tr.doc, containerOf(tr.doc, id).pos));
  });
}

/**
 * `p` sélectionne, comme un nœud, le contenu du bloc `id` : ce que donne un clic sur une image
 * dans un navigateur (`e2e/block-move.spec.ts`). Le nœud sélectionné est l'image, qui n'a pas
 * d'id ; c'est son conteneur qui le porte.
 */
export function selectContent(p: Peer, id: string): void {
  p.editor.transact((tr) => {
    tr.setSelection(NodeSelection.create(tr.doc, containerOf(tr.doc, id).pos + 1));
  });
}

/** Chez `p`, tape `text` dans le bloc `id`, à `offset` caractères du début de son texte. */
export function typeAt(p: Peer, id: string, offset: number, text: string): void {
  p.editor.transact((tr) => {
    const { pos } = containerOf(tr.doc, id);
    tr.insertText(text, pos + 2 + offset);
  });
}

/** Chez `p`, tape `text` à la fin du texte du bloc `id`. */
export function typeAtEnd(p: Peer, id: string, text: string): void {
  const { node } = containerOf(p.editor.prosemirrorState.doc, id);
  typeAt(p, id, node.firstChild!.content.size, text);
}

/** `p` pose son curseur dans le bloc `id`, à `offset` caractères du début de son texte. */
export function cursor(p: Peer, id: string, offset: number): void {
  p.editor.transact((tr) => {
    const { pos } = containerOf(tr.doc, id);
    tr.setSelection(TextSelection.create(tr.doc, pos + 2 + offset));
  });
}

/**
 * Où est le curseur de `p` : l'id du bloc qui le porte, et sa place dans le texte de ce bloc
 * (`null` hors du texte d'un bloc).
 */
export function cursorOf(p: Peer): { id: string; offset: number } | null {
  const { $anchor } = p.editor.prosemirrorState.selection;
  for (let depth = $anchor.depth; depth > 0; depth -= 1) {
    const node = $anchor.node(depth);
    if (node.type.name === "blockContainer") {
      return { id: String(node.attrs.id), offset: $anchor.pos - $anchor.before(depth) - 2 };
    }
  }
  return null;
}

/**
 * Chez `p`, la touche `key` (avec ses modificateurs), reçue par l'éditeur comme d'un clavier :
 * les raccourcis de BlockNote la traitent. Rend vrai si l'un d'eux l'a prise.
 */
export function press(
  p: Peer,
  key: string,
  modifiers: { shiftKey?: boolean; ctrlKey?: boolean } = {},
): boolean {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...modifiers });
  p.editor.prosemirrorView.dom.dispatchEvent(event);
  return event.defaultPrevented;
}

/** `p` pose son curseur dans le bloc `id` (à la fin de son texte), puis presse `key`. */
function pressIn(
  p: Peer,
  id: string,
  key: string,
  modifiers: { shiftKey?: boolean; ctrlKey?: boolean } = {},
): void {
  p.editor.setTextCursorPosition(id, "end");
  if (!press(p, key, modifiers)) throw new Error(`la touche ${key} n'a rien fait dans le bloc ${id}`);
}

/** `p` pose son curseur dans le bloc `id`, à `offset` caractères du début, puis presse `key`. */
export function pressAt(p: Peer, id: string, offset: number, key: string): void {
  cursor(p, id, offset);
  if (!press(p, key)) throw new Error(`la touche ${key} n'a rien fait dans le bloc ${id}`);
}

/** Chez `p`, Tab dans le bloc `id` : il devient l'enfant du bloc d'au-dessus. */
export function tab(p: Peer, id: string): void {
  pressIn(p, id, "Tab");
}

/** Chez `p`, Maj+Tab dans le bloc `id` : il remonte d'un niveau. */
export function shiftTab(p: Peer, id: string): void {
  pressIn(p, id, "Tab", { shiftKey: true });
}

/** Chez `p`, le bloc `id` descend d'une place au clavier (Ctrl+Maj+↓). */
export function keyboardDown(p: Peer, id: string): void {
  pressIn(p, id, "ArrowDown", { ctrlKey: true, shiftKey: true });
}

/** Chez `p`, le bloc `id` monte d'une place au clavier (Ctrl+Maj+↑). */
export function keyboardUp(p: Peer, id: string): void {
  pressIn(p, id, "ArrowUp", { ctrlKey: true, shiftKey: true });
}

/** Ce que `p` a écrit depuis le dernier envoi, en une mise à jour (rien : `null`). */
export function outgoing(p: Peer): Uint8Array | null {
  const writes = p.writes.splice(0);
  return writes.length === 0 ? null : Y.mergeUpdates(writes);
}

/**
 * Ce que chacun a écrit part chez l'autre et au serveur, dans l'ordre de `order`, jusqu'à ce que
 * plus personne n'écrive : un éditeur peut récrire en recevant.
 */
export async function exchange(server: Y.Doc, order: Peer[]): Promise<void> {
  for (let round = 0; round < 5; round += 1) {
    const sent = order.map((from) => ({ from, update: outgoing(from) }));
    if (sent.every(({ update }) => update === null)) return;
    for (const { from, update } of sent) {
      if (update === null) continue;
      Y.applyUpdate(server, update, REMOTE);
      for (const to of order) if (to !== from) Y.applyUpdate(to.doc, update, REMOTE);
    }
    await settle();
  }
  throw new Error("les éditeurs écrivent encore après cinq échanges");
}

/**
 * Ce qu'une mise à jour porte, compté par sorte : les éléments créés (`<paragraph>`), les textes
 * (`texte`) et leurs caractères, les attributs posés sur un élément neuf (`@id`), les attributs
 * récrits sur un élément gardé (`attribut récrit` : un élément qui change d'id en est un), et le
 * nombre de structures supprimées ; puis son poids en octets.
 */
export function counts(update: Uint8Array) {
  const { structs, ds } = Y.decodeUpdate(update);
  const made: Record<string, number> = {};
  const add = (kind: string, n = 1) => {
    made[kind] = (made[kind] ?? 0) + n;
  };
  for (const struct of structs) {
    if (!(struct instanceof Y.Item)) {
      add(struct.constructor.name, struct.length);
      continue;
    }
    const { content } = struct;
    if (content instanceof Y.ContentType) {
      const { type } = content;
      add(type instanceof Y.XmlElement ? `<${type.nodeName}>` : "texte");
    } else if (content instanceof Y.ContentString) add("caractères", struct.length);
    // Une entrée de table écrite par-dessus une autre a pour origine celle qu'elle remplace : son
    // nom n'est pas dans la mise à jour.
    else if (content instanceof Y.ContentAny) add(struct.parentSub === null ? "attribut récrit" : `@${struct.parentSub}`);
    else add(content.constructor.name, struct.length);
  }
  let deleted = 0;
  ds.clients.forEach((ranges) => ranges.forEach((range) => (deleted += range.len)));
  return { made, deleted, bytes: update.length };
}

/** Le fragment de la page, en XML : deux documents égaux y rendent la même chaîne. */
export function xml(doc: Y.Doc): string {
  return doc.getXmlFragment(FRAGMENT).toString();
}

/** Les éléments Yjs de la page, dans l'ordre du document. */
export function elements(doc: Y.Doc): Y.XmlElement[] {
  const found: Y.XmlElement[] = [];
  const walk = (parent: Y.XmlElement | Y.XmlFragment) =>
    parent.toArray().forEach((child) => {
      if (child instanceof Y.XmlElement) {
        found.push(child);
        walk(child);
      }
    });
  walk(doc.getXmlFragment(FRAGMENT));
  return found;
}

/**
 * Un élément nommé pour les messages : `blockContainer x` par son id, les autres par le bloc qui
 * les porte (`paragraph de x`, `blockGroup de w`, `blockGroup racine`).
 */
export function nameOf(element: Y.XmlElement): string {
  if (element.nodeName === "blockContainer") return `blockContainer ${element.getAttribute("id")}`;
  const parent = element.parent;
  const owner = parent instanceof Y.XmlElement ? `de ${parent.getAttribute("id")}` : "racine";
  return `${element.nodeName} ${owner}`;
}

/** Ce qu'un geste fait des éléments Yjs du document de `p` (`writtenIn`). */
export function written(p: Peer, gesture: () => void) {
  return writtenIn(p.doc, gesture);
}

/**
 * Ce qu'un geste fait des éléments Yjs de la page de `doc` : ceux qu'il supprime et ceux qu'il
 * crée (nommés avant et après lui), et l'id que porte désormais chaque `blockContainer` gardé
 * dont l'id a changé (par son id d'avant).
 */
export function writtenIn(doc: Y.Doc, gesture: () => void) {
  const before = new Map(elements(doc).map((element) => [element, nameOf(element)]));
  gesture();
  const after = elements(doc);
  const deleted = [...before].filter(([element]) => element._item?.deleted).map(([, n]) => n);
  const created = after.filter((element) => !before.has(element)).map(nameOf);
  const renamed: Record<string, string> = {};
  for (const element of after) {
    const was = before.get(element);
    if (was?.startsWith("blockContainer ") && was !== nameOf(element)) {
      renamed[was.slice("blockContainer ".length)] = String(element.getAttribute("id"));
    }
  }
  return { deleted, created, renamed };
}
