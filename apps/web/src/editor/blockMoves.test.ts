// Ce que chaque déplacement de bloc écrit dans le document partagé, et ce que devient une frappe
// faite en même temps par quelqu'un d'autre (limite R39, Task 12 ; la Task 13 doit la lever).
//
// Deux vrais éditeurs BlockNote 0.57, liés par `withCollaboration` (le chemin Yjs 13 de
// l'application) à deux `Y.Doc`, ceux de Mona et de Tom, aux identifiants de client fixes :
// l'issue d'un conflit d'insertion se tranche par ces identifiants, et elle est ainsi la même à
// chaque exécution. Chacun fait son geste depuis le même état ; leurs mises à jour s'échangent
// ensuite à la main, et passent par un troisième document, celui du « serveur », dans un ordre
// donné. Un glisser est joué comme le dépôt de ProseMirror : le bloc retiré, puis remis, relu du
// presse-papiers (des nœuds neufs), en une transaction. Cela suffit : y-prosemirror ne regarde
// que l'état final (`_prosemirrorChanged` passe tout le document à `updateYFragment`). Tab,
// Maj+Tab et le déplacement au clavier passent par l'API de BlockNote qu'appellent leurs touches
// (`nestBlock`, `unnestBlock`, `moveBlocksDown`).
//
// Chaque scénario attend le comportement voulu : les blocs dans l'ordre, chaque id une seule
// fois, chaque texte dans son bloc, rien en double, Mona, Tom et le serveur d'accord. Ceux qui
// échouent aujourd'hui sont marqués `test.fails` : la suite reste verte, et le jour où l'un
// passe, elle le dit. `MEMENTUM_SHOW_R39=1` les lance comme des tests ordinaires, pour voir leur
// échec. Chaque geste a son témoin, vert : seul, il fait ce qu'on attend. Ce que montre chaque
// scénario aujourd'hui est au registre du lot 3, sous « Task 12 ».
//
// Une correction faite par le serveur (R138) ne rendrait pas ces tests verts : il n'y a pas de
// serveur ici. Ils disent ce que les clients écrivent et ce que donne leur fusion ; ce que le
// serveur en fait se vérifie dans `crates/mementum-sync/tests/moves.rs`, avec des octets que cette
// suite produit (fin du fichier), et dans un navigateur, dans `e2e/block-move.spec.ts`.
import { BlockNoteEditor } from "@blocknote/core";
import { withCollaboration } from "@blocknote/core/yjs";
import { describe, expect, test } from "vitest";
import * as Y from "yjs";
import { FRAGMENT } from "./collab";
import type { MementumEditor } from "./debug";
import { schema } from "./schema";

/** Origine des mises à jour reçues : elles ne repartent pas. */
const REMOTE = "remote";

/** Identifiants de client fixes : la page de départ, Mona, Tom. */
const SEED = 1;
const MONA = 2;
const TOM = 3;

/** Ce que Tom tape. */
const TYPED = " de Tom";

/**
 * Un test du comportement voulu qui échoue aujourd'hui : `test.fails`, sauf avec
 * `MEMENTUM_SHOW_R39=1`, qui le lance comme un test ordinaire et montre son échec.
 */
const knownLoss = import.meta.env.MEMENTUM_SHOW_R39 ? test : test.fails;

type Transaction = Parameters<Parameters<MementumEditor["transact"]>[0]>[0];
type PmNode = Transaction["doc"];
type Seed = Parameters<MementumEditor["replaceBlocks"]>[1];

/** Un bloc : id, type, texte, enfants. */
type Shown = { id: string; type: string; text: string; children: Shown[] };

/** Un bloc attendu (paragraphe sans enfants par défaut). */
function block(
  id: string,
  text: string,
  { type = "paragraph", children = [] }: { type?: string; children?: Shown[] } = {},
): Shown {
  return { id, type, text, children };
}

// ---------------------------------------------------------------------------
// Les pages de départ
// ---------------------------------------------------------------------------

const XYZ: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "paragraph", content: "Bloc Y" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];
const X = block("x", "Bloc X");
const Y_ = block("y", "Bloc Y");
const Z = block("z", "Bloc Z");

/** Des textes qui n'ont rien en commun : le diff de y-prosemirror récrit tout le texte. */
const CHORES: Seed = [
  { id: "x", type: "paragraph", content: "Acheter du pain" },
  { id: "y", type: "paragraph", content: "Rappeler Marc" },
  { id: "z", type: "paragraph", content: "Arroser les plantes" },
];
const CHORE_X = block("x", "Acheter du pain");
const CHORE_Y = block("y", "Rappeler Marc");
const CHORE_Z = block("z", "Arroser les plantes");

const XYZW: Seed = [...XYZ, { id: "w", type: "paragraph", content: "Bloc W" }];
const W = block("w", "Bloc W");

/** Y est un titre : un élément de contenu d'un autre type que ses voisins. */
const TITLED: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "heading", props: { level: 2 }, content: "Titre Y" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];
const TITLE_Y = block("y", "Titre Y", { type: "heading" });

/** X a un enfant, X1. */
const PARENTED: Seed = [
  {
    id: "x",
    type: "paragraph",
    content: "Bloc X",
    children: [{ id: "x1", type: "paragraph", content: "Enfant X1" }],
  },
  { id: "y", type: "paragraph", content: "Bloc Y" },
  { id: "z", type: "paragraph", content: "Bloc Z" },
];
const xWithChild = (text: string) => block("x", "Bloc X", { children: [block("x1", text)] });

/** W, X, Y au même niveau : Tab dans X en fait l'enfant de W. */
const W_FIRST: Seed = [
  { id: "w", type: "paragraph", content: "Bloc W" },
  { id: "x", type: "paragraph", content: "Bloc X" },
  { id: "y", type: "paragraph", content: "Bloc Y" },
];
/** X enfant de W : Maj+Tab dans X le remonte au niveau de W. */
const W_PARENT: Seed = [
  {
    id: "w",
    type: "paragraph",
    content: "Bloc W",
    children: [{ id: "x", type: "paragraph", content: "Bloc X" }],
  },
  { id: "y", type: "paragraph", content: "Bloc Y" },
];
const wWithChild = (text: string) => block("w", "Bloc W", { children: [block("x", text)] });

const XYZA: Seed = [...XYZ, { id: "a", type: "paragraph", content: "Bloc A" }];
const A = block("a", "Bloc A");

// ---------------------------------------------------------------------------
// Les éditeurs, les gestes, l'échange
// ---------------------------------------------------------------------------

/** Une personne : son document, son éditeur, et ce qu'il a écrit, pas encore envoyé. */
type Peer = { name: string; doc: Y.Doc; editor: MementumEditor; writes: Uint8Array[] };

/** Laisse passer les minuteurs de y-prosemirror (son premier rendu est différé). */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Un éditeur monté, lié à un document neuf d'identifiant de client `clientID`, parti de `base`. */
async function peer(name: string, clientID: number, base?: Uint8Array): Promise<Peer> {
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
  return { name, doc, editor, writes };
}

/**
 * L'état d'une page de départ, écrit par un éditeur d'identifiant de client `SEED` dans un
 * document vide : les attributs que pose BlockNote, ceux qu'écrit aussi le serveur.
 */
async function seed(blocks: Seed): Promise<Uint8Array> {
  const writer = await peer("Départ", SEED);
  writer.editor.replaceBlocks(writer.editor.document, blocks);
  await settle();
  return Y.encodeStateAsUpdate(writer.doc);
}

/** Les blocs que montre l'éditeur de `p`. */
function shown(p: Peer): Shown[] {
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
 * Les blocs du document Yjs `doc`, lus sans éditeur : ce que le serveur garde et relit. Un
 * éditeur, lui, donne un id neuf à un bloc dont l'id revient (extension `UniqueID` de
 * BlockNote), sans l'écrire au document : seule cette lecture montre un id en double.
 */
function stored(doc: Y.Doc): Shown[] {
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
function line(page: Shown[]): string {
  return page
    .map((b) => {
      const type = b.type === "paragraph" ? "" : ` (${b.type})`;
      const children = b.children.length ? ` [${line(b.children)}]` : "";
      return `${b.id}${type} « ${b.text} »${children}`;
    })
    .join(" | ");
}

/** Les ids qui reviennent plus d'une fois dans `blocks`, enfants compris. */
function repeatedIds(blocks: Shown[]): string[] {
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
function containerOf(doc: PmNode, id: string): { pos: number; node: PmNode } {
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
 * dépôt de ProseMirror : retiré, puis remis, relu du presse-papiers, en une transaction.
 */
function drag(p: Peer, moved: string, place: { under: string } | { above: string }): void {
  p.editor.transact((tr) => {
    const { pos, node } = containerOf(tr.doc, moved);
    const copy = p.editor.pmSchema.nodeFromJSON(node.toJSON());
    tr.delete(pos, pos + node.nodeSize);
    const target = containerOf(tr.doc, "under" in place ? place.under : place.above);
    tr.insert("under" in place ? target.pos + target.node.nodeSize : target.pos, copy);
  });
}

/** Chez `p`, tape `text` à la fin du texte du bloc `id`. */
function typeAtEnd(p: Peer, id: string, text: string): void {
  p.editor.transact((tr) => {
    const { pos, node } = containerOf(tr.doc, id);
    const content = node.firstChild!;
    tr.insertText(text, pos + 2 + content.content.size);
  });
}

/** Chez `p`, Tab dans le bloc `id` : il devient l'enfant du bloc d'au-dessus. */
function tab(p: Peer, id: string): void {
  p.editor.setTextCursorPosition(id, "end");
  p.editor.nestBlock();
}

/** Chez `p`, Maj+Tab dans le bloc `id` : il remonte d'un niveau. */
function shiftTab(p: Peer, id: string): void {
  p.editor.setTextCursorPosition(id, "end");
  p.editor.unnestBlock();
}

/** Chez `p`, le bloc `id` descend d'une place au clavier. */
function keyboardDown(p: Peer, id: string): void {
  p.editor.setTextCursorPosition(id, "end");
  p.editor.moveBlocksDown();
}

/** Ce que `p` a écrit depuis le dernier envoi, en une mise à jour (rien : `null`). */
function outgoing(p: Peer): Uint8Array | null {
  const writes = p.writes.splice(0);
  return writes.length === 0 ? null : Y.mergeUpdates(writes);
}

/**
 * Ce que chacun a écrit part chez l'autre et au serveur, dans l'ordre de `order`, jusqu'à ce que
 * plus personne n'écrive : un éditeur peut récrire en recevant.
 */
async function exchange(server: Y.Doc, order: Peer[]): Promise<void> {
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
 * Une scène : la page de départ, Mona et Tom qui l'ont ouverte, et le document du serveur.
 * `quiet` : ni Mona ni Tom n'a rien écrit en ouvrant la page.
 */
async function scene(blocks: Seed) {
  const base = await seed(blocks);
  const mona = await peer("Mona", MONA, base);
  const tom = await peer("Tom", TOM, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  const quiet = mona.writes.length === 0 && tom.writes.length === 0;
  return { base, mona, tom, server, quiet };
}

type Scene = Awaited<ReturnType<typeof scene>>;

/**
 * Les gestes faits, chacun vérifié chez son auteur avant l'échange (`local`) : faux, et le test
 * prouverait autre chose. Dans un `test.fails`, une précondition qui manque ne doit pas passer
 * pour l'échec attendu : elle est écrite au journal, et le test rend la main sans échouer, ce que
 * `test.fails` signale.
 */
function locallyDone(s: Scene, local: { mona: Shown[]; tom: Shown[] }): boolean {
  const same = (p: Peer, page: Shown[]) => JSON.stringify(shown(p)) === JSON.stringify(page);
  if (s.quiet && same(s.mona, local.mona) && same(s.tom, local.tom)) return true;
  console.error(
    `précondition manquée : ouverture sans écriture ${s.quiet} ; ` +
      `Mona ${line(shown(s.mona))} ; Tom ${line(shown(s.tom))}`,
  );
  return false;
}

/**
 * Après l'échange, la page voulue partout : chez Mona et chez Tom (ce que montrent leurs
 * éditeurs) et au serveur (son document, relu sans éditeur) ; chaque id une seule fois ; les
 * trois documents identiques. `wanted` : une page, ou les pages admises quand deux issues sont
 * justes.
 */
function expectEverywhere(s: Scene, wanted: Shown[] | Shown[][]): void {
  const pages = Array.isArray(wanted[0]) ? (wanted as Shown[][]) : [wanted as Shown[]];
  const views: [string, Shown[]][] = [
    ["chez Mona", shown(s.mona)],
    ["chez Tom", shown(s.tom)],
    ["au serveur", stored(s.server)],
  ];
  for (const [where, page] of views) {
    expect
      .soft(repeatedIds(page), `${where} : chaque id une seule fois (vu : ${line(page)})`)
      .toEqual([]);
    expect.soft(pages, `${where} : la page voulue (vu : ${line(page)})`).toContainEqual(page);
  }
  const xml = (doc: Y.Doc) => doc.getXmlFragment(FRAGMENT).toString();
  expect.soft(xml(s.tom.doc), "Mona et Tom ont le même document").toBe(xml(s.mona.doc));
  expect.soft(xml(s.server), "le serveur a le même document").toBe(xml(s.mona.doc));
}

/** Un témoin : les deux ouvertures n'ont rien écrit. */
function expectQuiet(s: Scene): void {
  expect(s.quiet, "Mona et Tom n'écrivent rien en ouvrant la page").toBe(true);
}

// ---------------------------------------------------------------------------
// Les scénarios
// ---------------------------------------------------------------------------

describe("glisser un bloc sous un autre, au même niveau", () => {
  test("témoin : le glisser seul met X sous Z, partout", async () => {
    const s = await scene(XYZ);
    expectQuiet(s);
    drag(s.mona, "x", { under: "z" });
    expect(shown(s.mona)).toEqual([Y_, Z, X]);
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [Y_, Z, X]);
  });

  test("témoin : la frappe seule arrive partout", async () => {
    const s = await scene(XYZ);
    expectQuiet(s);
    typeAtEnd(s.tom, "x", TYPED);
    await exchange(s.server, [s.tom, s.mona]);
    expectEverywhere(s, [block("x", "Bloc X de Tom"), Y_, Z]);
  });

  for (const [first, label] of [
    ["mona", "le déplacement arrive d'abord au serveur"],
    ["tom", "la frappe arrive d'abord au serveur"],
  ] as const) {
    knownLoss(`S1 : Tom tape dans X pendant que Mona glisse X sous Z ; ${label}`, async () => {
      const s = await scene(XYZ);
      drag(s.mona, "x", { under: "z" });
      typeAtEnd(s.tom, "x", TYPED);
      const typed = block("x", "Bloc X de Tom");
      if (!locallyDone(s, { mona: [Y_, Z, X], tom: [typed, Y_, Z] })) return;
      await exchange(s.server, first === "mona" ? [s.mona, s.tom] : [s.tom, s.mona]);
      expectEverywhere(s, [Y_, Z, typed]);
    });
  }

  knownLoss("S4 : Tom tape dans Y, que personne ne déplace, pendant que Mona glisse X sous Z", async () => {
    const s = await scene(XYZ);
    drag(s.mona, "x", { under: "z" });
    typeAtEnd(s.tom, "y", TYPED);
    const typed = block("y", "Bloc Y de Tom");
    if (!locallyDone(s, { mona: [Y_, Z, X], tom: [X, typed, Z] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [typed, Z, X]);
  });

  test("témoin : textes sans rien en commun, le glisser seul", async () => {
    const s = await scene(CHORES);
    expectQuiet(s);
    drag(s.mona, "x", { under: "z" });
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [CHORE_Y, CHORE_Z, CHORE_X]);
  });

  knownLoss("S8 : textes sans rien en commun ; Tom tape dans X pendant que Mona glisse X sous Z", async () => {
    const s = await scene(CHORES);
    drag(s.mona, "x", { under: "z" });
    typeAtEnd(s.tom, "x", TYPED);
    const typed = block("x", `Acheter du pain${TYPED}`);
    if (!locallyDone(s, { mona: [CHORE_Y, CHORE_Z, CHORE_X], tom: [typed, CHORE_Y, CHORE_Z] })) {
      return;
    }
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [CHORE_Y, CHORE_Z, typed]);
  });
});

describe("glisser un bloc de plusieurs places, vers le haut", () => {
  test("témoin : le glisser seul met W en tête, partout", async () => {
    const s = await scene(XYZW);
    expectQuiet(s);
    drag(s.mona, "w", { above: "x" });
    expect(shown(s.mona)).toEqual([W, X, Y_, Z]);
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [W, X, Y_, Z]);
  });

  knownLoss("S5 : Tom tape dans Z pendant que Mona glisse W tout en haut", async () => {
    const s = await scene(XYZW);
    drag(s.mona, "w", { above: "x" });
    typeAtEnd(s.tom, "z", TYPED);
    const typed = block("z", "Bloc Z de Tom");
    if (!locallyDone(s, { mona: [W, X, Y_, Z], tom: [X, Y_, typed, W] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [W, X, Y_, typed]);
  });
});

describe("glisser un bloc par-dessus un bloc d'un autre type", () => {
  test("témoin : le glisser seul met X sous Z, partout, le titre gardé", async () => {
    const s = await scene(TITLED);
    expectQuiet(s);
    drag(s.mona, "x", { under: "z" });
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [TITLE_Y, Z, X]);
  });

  knownLoss("S2 : Y est un titre ; Tom tape dans X pendant que Mona glisse X sous Z", async () => {
    const s = await scene(TITLED);
    drag(s.mona, "x", { under: "z" });
    typeAtEnd(s.tom, "x", TYPED);
    const typed = block("x", "Bloc X de Tom");
    if (!locallyDone(s, { mona: [TITLE_Y, Z, X], tom: [typed, TITLE_Y, Z] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [TITLE_Y, Z, typed]);
  });
});

describe("glisser un bloc qui a des enfants", () => {
  test("témoin : le glisser seul met X et son enfant sous Z, partout", async () => {
    const s = await scene(PARENTED);
    expectQuiet(s);
    drag(s.mona, "x", { under: "z" });
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [Y_, Z, xWithChild("Enfant X1")]);
  });

  test("témoin : la frappe seule dans l'enfant arrive partout", async () => {
    const s = await scene(PARENTED);
    expectQuiet(s);
    typeAtEnd(s.tom, "x1", TYPED);
    await exchange(s.server, [s.tom, s.mona]);
    expectEverywhere(s, [xWithChild("Enfant X1 de Tom"), Y_, Z]);
  });

  knownLoss("S3 : Tom tape dans l'enfant de X pendant que Mona glisse X sous Z", async () => {
    const s = await scene(PARENTED);
    drag(s.mona, "x", { under: "z" });
    typeAtEnd(s.tom, "x1", TYPED);
    const typed = xWithChild("Enfant X1 de Tom");
    if (!locallyDone(s, { mona: [Y_, Z, xWithChild("Enfant X1")], tom: [typed, Y_, Z] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [Y_, Z, typed]);
  });
});

describe("déplacer un bloc au clavier", () => {
  test("témoin : X descend d'une place, partout", async () => {
    const s = await scene(XYZ);
    expectQuiet(s);
    keyboardDown(s.mona, "x");
    expect(shown(s.mona)).toEqual([Y_, X, Z]);
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [Y_, X, Z]);
  });

  knownLoss("S7 : Tom tape dans X pendant que Mona le descend d'une place au clavier", async () => {
    const s = await scene(XYZ);
    keyboardDown(s.mona, "x");
    typeAtEnd(s.tom, "x", TYPED);
    const typed = block("x", "Bloc X de Tom");
    if (!locallyDone(s, { mona: [Y_, X, Z], tom: [typed, Y_, Z] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [Y_, typed, Z]);
  });
});

describe("changer un bloc de niveau (Tab, Maj+Tab)", () => {
  test("témoin : Tab seul fait de X l'enfant de W, partout", async () => {
    const s = await scene(W_FIRST);
    expectQuiet(s);
    tab(s.mona, "x");
    expect(shown(s.mona)).toEqual([wWithChild("Bloc X"), Y_]);
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [wWithChild("Bloc X"), Y_]);
  });

  knownLoss("S9 : Tom tape dans X pendant que Mona en fait l'enfant de W (Tab)", async () => {
    const s = await scene(W_FIRST);
    tab(s.mona, "x");
    typeAtEnd(s.tom, "x", TYPED);
    if (!locallyDone(s, { mona: [wWithChild("Bloc X"), Y_], tom: [W, block("x", "Bloc X de Tom"), Y_] })) {
      return;
    }
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [wWithChild("Bloc X de Tom"), Y_]);
  });

  test("témoin : Maj+Tab seul remonte X au niveau de W, partout", async () => {
    const s = await scene(W_PARENT);
    expectQuiet(s);
    shiftTab(s.mona, "x");
    expect(shown(s.mona)).toEqual([W, X, Y_]);
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [W, X, Y_]);
  });

  knownLoss("S10 : Tom tape dans X pendant que Mona le remonte d'un niveau (Maj+Tab)", async () => {
    const s = await scene(W_PARENT);
    shiftTab(s.mona, "x");
    typeAtEnd(s.tom, "x", TYPED);
    if (!locallyDone(s, { mona: [W, X, Y_], tom: [wWithChild("Bloc X de Tom"), Y_] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [W, block("x", "Bloc X de Tom"), Y_]);
  });
});

describe("deux glissers en même temps, sans aucune frappe", () => {
  knownLoss("S6 : Mona glisse X sous Z, Tom glisse X sous Y", async () => {
    const s = await scene(XYZ);
    drag(s.mona, "x", { under: "z" });
    drag(s.tom, "x", { under: "y" });
    if (!locallyDone(s, { mona: [Y_, Z, X], tom: [Y_, X, Z] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    // L'une ou l'autre place est juste ; pas les deux, et aucun texte ne change.
    expectEverywhere(s, [
      [Y_, Z, X],
      [Y_, X, Z],
    ]);
  });

  test("témoin : le glisser de Tom seul met Y sous A, partout", async () => {
    const s = await scene(XYZA);
    expectQuiet(s);
    drag(s.tom, "y", { under: "a" });
    await exchange(s.server, [s.tom, s.mona]);
    expectEverywhere(s, [X, Z, A, Y_]);
  });

  knownLoss("S11 : Mona glisse X sous Z, Tom glisse Y sous A", async () => {
    const s = await scene(XYZA);
    drag(s.mona, "x", { under: "z" });
    drag(s.tom, "y", { under: "a" });
    if (!locallyDone(s, { mona: [Y_, Z, X, A], tom: [X, Z, A, Y_] })) return;
    await exchange(s.server, [s.mona, s.tom]);
    expectEverywhere(s, [Z, X, A, Y_]);
  });
});

// ---------------------------------------------------------------------------
// Les octets du test de l'acteur
// ---------------------------------------------------------------------------

/** Où `crates/mementum-sync/tests/moves.rs` lit ces octets, depuis la racine du dépôt. */
const FIXTURES = "crates/mementum-sync/tests/fixtures/moves";

/**
 * Ce que rejoue le test de l'acteur, écrit par y-prosemirror depuis la même page de départ
 * (« Bloc X, Bloc Y, Bloc Z », identifiant de client `SEED`) : le glisser de X sous Z fait par
 * Mona (identifiant `MONA`), et la frappe de Tom (identifiant `TOM`) à la fin de X, ou de Y.
 */
async function fixtureBytes(): Promise<Record<string, Uint8Array>> {
  const s = await scene(XYZ);
  const tomInY = await peer("Tom", TOM, s.base);
  expect(s.quiet && tomInY.writes.length === 0, "rien d'écrit à l'ouverture").toBe(true);
  drag(s.mona, "x", { under: "z" });
  typeAtEnd(s.tom, "x", TYPED);
  typeAtEnd(tomInY, "y", TYPED);
  return {
    "base.bin": s.base,
    "mona-drags-x-under-z.bin": outgoing(s.mona)!,
    "tom-types-in-x.bin": outgoing(s.tom)!,
    "tom-types-in-y.bin": outgoing(tomInY)!,
  };
}

type Fs = {
  readFile(path: string): Promise<Uint8Array>;
  writeFile(path: string, data: Uint8Array): Promise<void>;
  mkdir(path: string, options: { recursive: true }): Promise<unknown>;
};
type Path = { dirname(path: string): string; join(...parts: string[]): string };
type Url = { fileURLToPath(url: string): string };

/**
 * Un module de Node, chargé à l'exécution : Vitest tourne sous Node, mais le code de `src` est
 * typé sans les types de Node (`tsconfig.json`).
 */
async function nodeModule<T>(name: string): Promise<T> {
  return (await import(/* @vite-ignore */ name)) as T;
}

// `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/blockMoves.test.ts`
// les réécrit ; sans la variable, le test les compare, octet pour octet.
test("les octets du test de l'acteur sont ceux qu'écrit y-prosemirror", async () => {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const url = await nodeModule<Url>("node:url");
  const dir = path.join(path.dirname(url.fileURLToPath(import.meta.url)), "../../../..", FIXTURES);
  const bytes = await fixtureBytes();
  if (import.meta.env.EXPORT_FIXTURES) {
    await fs.mkdir(dir, { recursive: true });
    for (const [name, data] of Object.entries(bytes)) await fs.writeFile(path.join(dir, name), data);
    return;
  }
  for (const [name, data] of Object.entries(bytes)) {
    const committed = await fs.readFile(path.join(dir, name));
    expect([...committed], `${name} (EXPORT_FIXTURES=1 le réécrit)`).toEqual([...data]);
  }
});
