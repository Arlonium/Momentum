// La forme à clés (R220) : ce qu'un bloc déplacé écrit dans le document partagé depuis le
// correctif de y-prosemirror (`patches/y-prosemirror@1.3.7.patch`, fonction `updateYFragment`).
//
// y-prosemirror ne regarde que l'état final du document, et appariait par position les enfants
// qui diffèrent : un bloc qui change de place faisait récrire sur place tous les blocs entre
// l'ancienne place et la nouvelle, chacun prenant l'id et le texte de son voisin. Une frappe
// faite au même moment dans l'un d'eux finissait chez un voisin, et deux déplacements abîmaient
// la page (`blockMoves.test.ts`). Avec le correctif, quand des blocs présents avant et après ont
// changé d'ordre, ils sont appariés par leur `id` : ceux qui gardent leur ordre restent les
// mêmes éléments, sans rien y écrire ; les autres sont supprimés, et recréés à leur place avec le
// même `id`.
//
// Le correctif ne joue que là : sans bloc qui change d'ordre, ou dès qu'un `id` manque ou revient
// deux fois, c'est le code d'origine, tel quel. `ordinary_gestures_write_what_they_wrote_before`
// le tient : ce que chaque geste ordinaire écrit a été relevé sur le code d'origine, avant le
// correctif, et n'a pas le droit de changer.
//
// Un bloc déplacé étant désormais un bloc retiré, le correctif garde aussi l'endroit où
// y-prosemirror remet une sélection de nœud (`restoreRelativeSelection`) : il levait quand le bloc
// sélectionné n'était plus là, et l'éditeur ne montrait plus les changements des autres.
//
// Pour porter le correctif à une autre version de y-prosemirror : `pnpm patch y-prosemirror@<v>`,
// reporter dans `src/plugins/sync-plugin.js` et dans `dist/y-prosemirror.cjs` les trois ajouts,
// chacun entre ses deux lignes `mementum:moved-blocks` (dans `dist`, `Y.` s'écrit
// `Y__namespace.`), `pnpm patch-commit`, puis mettre à jour ici les empreintes des fichiers
// publiés (`PUBLISHED`). Cette suite dit si le portage est juste.
//
// Mêmes éditeurs que `blockMoves.test.ts` (`test/editors.ts`) : de vrais BlockNote 0.57, dans
// jsdom. Le dépôt d'un vrai navigateur est dans `e2e/block-move.spec.ts`.
import * as pmState from "prosemirror-state";
import { describe, expect, test, vi } from "vitest";
import {
  getRelativeSelection,
  ProsemirrorBinding,
  prosemirrorToYXmlFragment,
  updateYFragment,
  ySyncPluginKey,
  yXmlFragmentToProseMirrorRootNode,
} from "y-prosemirror";
import * as Y from "yjs";
import {
  A,
  block,
  containerOf,
  counts,
  cursor,
  drag,
  duo,
  exchange,
  keyboardDown,
  keyboardUp,
  line,
  outgoing,
  PARENTED,
  peer,
  press,
  pressAt,
  REMOTE,
  repeatedIds,
  seedState,
  selectBlock,
  settle,
  shiftTab,
  shown,
  stored,
  tab,
  typeAt,
  typeAtEnd,
  W_FIRST,
  W_PARENT,
  written,
  writtenIn,
  X,
  xml,
  XYZ,
  XYZA,
  Y_,
  Z,
  type Peer,
  type PmNode,
  type Seed,
  type Shown,
} from "../test/editors";
import {
  nodeModule,
  type Crypto,
  type Fs,
  type Module,
  type Path,
  type Url,
} from "../test/node";
import { FRAGMENT } from "./collab";

/** Les deux ordres des identifiants de client de Mona et de Tom. */
const ORDERS = [
  { label: "Mona avant Tom", mona: 2, tom: 3 },
  { label: "Tom avant Mona", mona: 3, tom: 2 },
] as const;

/** Une personne seule sur la page `blocks`, qui n'a rien écrit en l'ouvrant. */
async function alone(blocks: Seed): Promise<Peer> {
  const p = await peer("Mona", 2, await seedState(blocks));
  expect(p.writes, "rien d'écrit à l'ouverture").toHaveLength(0);
  return p;
}

/** Un bloc recréé : son conteneur et son contenu, supprimés puis créés, et rien d'autre. */
const recreated = (id: string, content = "paragraph") => ({
  deleted: [`blockContainer ${id}`, `${content} de ${id}`],
  created: [`blockContainer ${id}`, `${content} de ${id}`],
  renamed: {},
});

// ---------------------------------------------------------------------------
// La forme
// ---------------------------------------------------------------------------

describe("un bloc déplacé est recréé, les autres ne sont pas touchés", () => {
  test("a_move_rewrites_only_the_moved_block", async () => {
    const mona = await alone(XYZ);
    expect(written(mona, () => drag(mona, "x", { under: "z" }))).toEqual(recreated("x"));
    expect(shown(mona)).toEqual([Y_, Z, X]);
    expect(stored(mona.doc)).toEqual([Y_, Z, X]);
    // Un conteneur, son paragraphe et son texte, avec leurs attributs : rien n'est récrit sur
    // un élément gardé, et aucun élément ne change d'id.
    expect(counts(outgoing(mona)!).made).toEqual({
      "<blockContainer>": 1,
      "<paragraph>": 1,
      texte: 1,
      caractères: 6,
      "@id": 1,
      "@backgroundColor": 1,
      "@textColor": 1,
      "@textAlignment": 1,
    });
  });

  // Ce que chaque autre geste écrit (plusieurs places, un titre croisé, un bloc et ses enfants,
  // Tab, Maj+Tab) est dans `blockMoves.test.ts`, « ce que chaque geste écrit dans le document ».

  test("swapping_two_neighbours_recreates_the_block_that_was_moved", async () => {
    // Deux voisins échangés : l'un ou l'autre peut rester. C'est celui qu'on a déplacé, celui où
    // le geste laisse le curseur, qui est recréé ; la page finale est la même dans les quatre cas.
    const down = await alone(XYZ);
    expect(written(down, () => keyboardDown(down, "x"))).toEqual(recreated("x"));
    expect(stored(down.doc)).toEqual([Y_, X, Z]);

    const up = await alone(XYZ);
    expect(written(up, () => keyboardUp(up, "y"))).toEqual(recreated("y"));
    expect(stored(up.doc)).toEqual([Y_, X, Z]);

    const dropped = await alone(XYZ);
    expect(written(dropped, () => drag(dropped, "x", { under: "y" }))).toEqual(recreated("x"));
    expect(stored(dropped.doc)).toEqual([Y_, X, Z]);

    const lifted = await alone(XYZ);
    expect(written(lifted, () => drag(lifted, "y", { above: "x" }))).toEqual(recreated("y"));
    expect(stored(lifted.doc)).toEqual([Y_, X, Z]);
  });

  test("a_big_move_writes_a_small_update", async () => {
    // Le premier bloc de 2 000 glissé tout en bas : avant le correctif, les 2 000 blocs étaient
    // récrits (plus de 100 Ko). Un bloc supprimé et un bloc créé tiennent en moins de 2 Ko.
    const count = 2_000;
    const blocks: Seed = Array.from({ length: count }, (_, i) => ({
      id: `b${i}`,
      type: "paragraph" as const,
      content: `Bloc ${i}`,
    }));
    const mona = await alone(blocks);
    expect(written(mona, () => drag(mona, "b0", { under: `b${count - 1}` }))).toEqual(recreated("b0"));
    const page = stored(mona.doc);
    expect(page).toHaveLength(count);
    expect([page[0].id, page[count - 2].id, page[count - 1].id]).toEqual(["b1", `b${count - 1}`, "b0"]);
    expect(outgoing(mona)!.length).toBeLessThan(2_048);
  }, 60_000);
});

// ---------------------------------------------------------------------------
// Ce que les autres gardent
// ---------------------------------------------------------------------------

describe.each(ORDERS)("pendant qu'un autre déplace un bloc ($label)", (order) => {
  test("typing_in_another_block_of_the_range_stays_in_its_block", async () => {
    const s = await duo(XYZ, order.mona, order.tom);
    expect(s.quiet).toBe(true);
    drag(s.mona, "x", { under: "z" });
    // Y et Z sont entre l'ancienne place de X et la nouvelle : Tom y tape, au début et à la fin.
    typeAtEnd(s.tom, "y", " de Tom");
    typeAt(s.tom, "z", 0, "Aussi : ");
    await exchange(s.server, [s.mona, s.tom]);
    const wanted = [block("y", "Bloc Y de Tom"), block("z", "Aussi : Bloc Z"), X];
    expect(shown(s.mona)).toEqual(wanted);
    expect(shown(s.tom)).toEqual(wanted);
    expect(stored(s.server)).toEqual(wanted);
  });

  test("two_moves_of_different_blocks_keep_every_block_once", async () => {
    const s = await duo(XYZA, order.mona, order.tom);
    expect(s.quiet).toBe(true);
    drag(s.mona, "x", { under: "z" });
    drag(s.tom, "y", { under: "a" });
    await exchange(s.server, [s.mona, s.tom]);
    const wanted = [Z, X, A, Y_];
    expect(stored(s.server)).toEqual(wanted);
    expect(shown(s.mona)).toEqual(wanted);
    expect(shown(s.tom)).toEqual(wanted);
    expect(xml(s.mona.doc)).toBe(xml(s.server));
    expect(xml(s.tom.doc)).toBe(xml(s.server));
  });

  test("undo_of_a_move_leaves_other_blocks_alone", async () => {
    const s = await duo(XYZ, order.mona, order.tom);
    expect(s.quiet).toBe(true);
    drag(s.mona, "x", { under: "z" });
    await exchange(s.server, [s.mona, s.tom]);
    // Tom a reçu le déplacement, et écrit dans les deux blocs que Mona n'a pas déplacés.
    typeAtEnd(s.tom, "y", " de Tom");
    typeAtEnd(s.tom, "z", " aussi");
    await exchange(s.server, [s.tom, s.mona]);
    const typed = [block("y", "Bloc Y de Tom"), block("z", "Bloc Z aussi")];
    expect(stored(s.server)).toEqual([...typed, X]);

    // Mona annule son déplacement : X revient en tête, recréé ; Y et Z gardent ce que Tom y a
    // écrit, dans les mêmes éléments.
    expect(written(s.mona, () => expect(s.mona.editor.undo()).toBe(true))).toEqual(recreated("x"));
    await exchange(s.server, [s.mona, s.tom]);
    expect(stored(s.server)).toEqual([X, ...typed]);
    expect(shown(s.mona)).toEqual([X, ...typed]);
    expect(shown(s.tom)).toEqual([X, ...typed]);
  });
});

// Un nœud peut être sélectionné : un bloc entier (ici), ou l'image d'un bloc image, par un clic
// (`followBlock.test.ts`). Si quelqu'un d'autre retire alors ce bloc, ou le déplace (il est recréé
// ailleurs), y-prosemirror 1.3.7 cherchait le nœud à son ancienne place : quand plus rien ne la
// suivait (le bloc était le dernier), il levait dans l'observateur du document, et l'éditeur ne
// montrait pas le changement. Avec la forme à clés, un bloc déplacé est un bloc retiré : le
// correctif garde donc aussi cet endroit (`restoreRelativeSelection`).
describe("a_selected_block_taken_away_by_someone_else_does_not_break_the_editor", () => {
  /** Mona a sélectionné Z, le dernier bloc de la page. */
  async function selected() {
    const s = await duo(XYZ, 2, 3);
    selectBlock(s.mona, "z");
    expect(s.mona.editor.prosemirrorState.selection).toBeInstanceOf(pmState.NodeSelection);
    return s;
  }

  test("supprimé par un autre", async () => {
    const s = await selected();
    s.tom.editor.removeBlocks(["z"]);
    await exchange(s.server, [s.tom, s.mona]);
    expect(shown(s.mona)).toEqual([X, Y_]);
    // L'éditeur de Mona montre toujours ce que Tom écrit.
    typeAtEnd(s.tom, "y", " de Tom");
    await exchange(s.server, [s.tom, s.mona]);
    expect(shown(s.mona)).toEqual([X, block("y", "Bloc Y de Tom")]);
    expect(stored(s.server)).toEqual(shown(s.mona));
  });

  test("déplacé par un autre", async () => {
    const s = await selected();
    drag(s.tom, "z", { above: "x" });
    await exchange(s.server, [s.tom, s.mona]);
    expect(shown(s.mona)).toEqual([Z, X, Y_]);
    typeAtEnd(s.tom, "z", " de Tom");
    await exchange(s.server, [s.tom, s.mona]);
    expect(shown(s.mona)).toEqual([block("z", "Bloc Z de Tom"), X, Y_]);
    expect(stored(s.server)).toEqual(shown(s.mona));
  });
});

// ---------------------------------------------------------------------------
// Ce qui ne change pas
// ---------------------------------------------------------------------------

/** Ce qu'un geste écrit : `counts`, le nombre d'éléments gardés dont l'id change, et la page. */
type Trace = {
  made: Record<string, number>;
  deleted: number;
  bytes: number;
  renamed: number;
  page: string;
};

const paragraph = (id: string, content: string) => ({ id, type: "paragraph" as const, content });

/** Les attributs que BlockNote pose sur un conteneur neuf et sur son paragraphe. */
const NEW_BLOCK = {
  "<blockContainer>": 1,
  "<paragraph>": 1,
  "@id": 1,
  "@backgroundColor": 1,
  "@textColor": 1,
  "@textAlignment": 1,
};

/**
 * Les gestes ordinaires, et ce que chacun écrivait avec y-prosemirror 1.3.7 d'origine : relevé
 * le 2026-11-13 sur le code d'origine, avant la première ligne du correctif. Un bloc neuf créé
 * par Entrée a un id tiré au hasard, de longueur fixe (`neuf` dans la page).
 */
const ORDINARY: { name: string; seed: Seed; act: (p: Peer) => void; wrote: Trace }[] = [
  {
    name: "frappe à la fin d'un bloc",
    seed: XYZ,
    act: (p) => typeAtEnd(p, "y", "a"),
    wrote: {
      made: { caractères: 1 },
      deleted: 0,
      bytes: 10,
      renamed: 0,
      page: "x « Bloc X » | y « Bloc Ya » | z « Bloc Z »",
    },
  },
  {
    name: "frappe au milieu d'un bloc",
    seed: XYZ,
    act: (p) => typeAt(p, "y", 3, "a"),
    wrote: {
      made: { caractères: 1 },
      deleted: 0,
      bytes: 12,
      renamed: 0,
      page: "x « Bloc X » | y « Bloac Y » | z « Bloc Z »",
    },
  },
  {
    name: "Entrée au début d'un bloc",
    seed: XYZ,
    act: (p) => pressAt(p, "y", 0, "Enter"),
    wrote: {
      made: NEW_BLOCK,
      deleted: 0,
      bytes: 166,
      renamed: 0,
      page: "x « Bloc X » | neuf «  » | y « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "Entrée au milieu d'un bloc",
    seed: XYZ,
    act: (p) => pressAt(p, "y", 3, "Enter"),
    wrote: {
      made: { ...NEW_BLOCK, texte: 1, caractères: 3 },
      deleted: 3,
      bytes: 183,
      renamed: 0,
      page: "x « Bloc X » | y « Blo » | neuf « c Y » | z « Bloc Z »",
    },
  },
  {
    name: "Entrée à la fin d'un bloc",
    seed: XYZ,
    act: (p) => pressAt(p, "y", 6, "Enter"),
    wrote: {
      made: NEW_BLOCK,
      deleted: 0,
      bytes: 166,
      renamed: 0,
      page: "x « Bloc X » | y « Bloc Y » | neuf «  » | z « Bloc Z »",
    },
  },
  {
    name: "Entrée au début d'un bloc qui a des enfants",
    seed: PARENTED,
    act: (p) => pressAt(p, "x", 0, "Enter"),
    wrote: {
      made: NEW_BLOCK,
      deleted: 0,
      bytes: 164,
      renamed: 0,
      page: "neuf «  » | x « Bloc X » [x1 « Enfant X1 »] | y « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    // Le code d'origine garde l'élément pour le bloc neuf, qui prend les enfants : il change
    // d'id, et un élément neuf porte l'id d'avant. Une forme à clés sans la règle « un bloc a
    // changé d'ordre » viderait l'élément et recréerait tous les enfants.
    name: "Entrée au milieu d'un bloc qui a des enfants",
    seed: PARENTED,
    act: (p) => pressAt(p, "x", 3, "Enter"),
    wrote: {
      made: { ...NEW_BLOCK, texte: 1, caractères: 3, "attribut récrit": 1 },
      deleted: 4,
      bytes: 190,
      renamed: 1,
      page: "x « Blo » | neuf « c X » [x1 « Enfant X1 »] | y « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "Entrée à la fin d'un bloc qui a des enfants",
    seed: PARENTED,
    act: (p) => pressAt(p, "x", 6, "Enter"),
    wrote: {
      made: { ...NEW_BLOCK, texte: 1, caractères: 6, "attribut récrit": 1 },
      deleted: 7,
      bytes: 193,
      renamed: 1,
      page: "x « Bloc X » | neuf «  » [x1 « Enfant X1 »] | y « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "retour arrière dans le texte",
    seed: XYZ,
    act: (p) =>
      p.editor.transact((tr) => {
        const { pos } = containerOf(tr.doc, "y");
        tr.delete(pos + 2 + 5, pos + 2 + 6);
      }),
    wrote: {
      made: {},
      deleted: 1,
      bytes: 6,
      renamed: 0,
      page: "x « Bloc X » | y « Bloc  » | z « Bloc Z »",
    },
  },
  {
    name: "retour arrière au début d'un bloc (il rejoint celui d'au-dessus)",
    seed: XYZ,
    act: (p) => pressAt(p, "y", 0, "Backspace"),
    wrote: {
      made: { caractères: 6 },
      deleted: 13,
      bytes: 19,
      renamed: 0,
      page: "x « Bloc XBloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "retour arrière au début d'un sous-bloc",
    seed: PARENTED,
    act: (p) => pressAt(p, "x1", 0, "Backspace"),
    wrote: {
      made: { caractères: 9 },
      deleted: 17,
      bytes: 22,
      renamed: 0,
      page: "x « Bloc XEnfant X1 » | y « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "retour arrière au début du bloc qui suit un bloc à enfants",
    seed: PARENTED,
    act: (p) => pressAt(p, "y", 0, "Backspace"),
    wrote: {
      made: { caractères: 6 },
      deleted: 13,
      bytes: 19,
      renamed: 0,
      page: "x « Bloc X » [x1 « Enfant X1Bloc Y »] | z « Bloc Z »",
    },
  },
  {
    name: "insertion d'un bloc",
    seed: XYZ,
    act: (p) => void p.editor.insertBlocks([paragraph("n", "Bloc N")], "x", "after"),
    wrote: {
      made: { ...NEW_BLOCK, texte: 1, caractères: 6 },
      deleted: 0,
      bytes: 147,
      renamed: 0,
      page: "x « Bloc X » | n « Bloc N » | y « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "insertion d'un bloc en tête",
    seed: XYZ,
    act: (p) => void p.editor.insertBlocks([paragraph("n", "Bloc N")], "x", "before"),
    wrote: {
      made: { ...NEW_BLOCK, texte: 1, caractères: 6 },
      deleted: 0,
      bytes: 145,
      renamed: 0,
      page: "n « Bloc N » | x « Bloc X » | y « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "suppression d'un bloc",
    seed: XYZ,
    act: (p) => void p.editor.removeBlocks(["y"]),
    wrote: { made: {}, deleted: 13, bytes: 6, renamed: 0, page: "x « Bloc X » | z « Bloc Z »" },
  },
  {
    name: "suppression d'un bloc qui a des enfants",
    seed: PARENTED,
    act: (p) => void p.editor.removeBlocks(["x"]),
    wrote: { made: {}, deleted: 30, bytes: 6, renamed: 0, page: "y « Bloc Y » | z « Bloc Z »" },
  },
  {
    name: "changement de type",
    seed: XYZ,
    act: (p) => void p.editor.updateBlock("y", { type: "heading", props: { level: 2 } }),
    wrote: {
      made: {
        "<heading>": 1,
        texte: 1,
        caractères: 6,
        "@backgroundColor": 1,
        "@textColor": 1,
        "@textAlignment": 1,
        "@level": 1,
        "@isToggleable": 1,
      },
      deleted: 11,
      bytes: 148,
      renamed: 0,
      page: "x « Bloc X » | y (heading) « Bloc Y » | z « Bloc Z »",
    },
  },
  {
    name: "Tab",
    seed: W_FIRST,
    act: (p) => tab(p, "x"),
    wrote: {
      made: { ...NEW_BLOCK, "<blockGroup>": 1, texte: 1, caractères: 6 },
      deleted: 13,
      bytes: 165,
      renamed: 0,
      page: "w « Bloc W » [x « Bloc X »] | y « Bloc Y »",
    },
  },
  {
    name: "Maj+Tab",
    seed: W_PARENT,
    act: (p) => shiftTab(p, "x"),
    wrote: {
      made: { ...NEW_BLOCK, texte: 1, caractères: 6 },
      deleted: 14,
      bytes: 151,
      renamed: 0,
      page: "w « Bloc W » | x « Bloc X » | y « Bloc Y »",
    },
  },
  {
    // Aucun bloc ne change d'ordre : le code d'origine récrit sur place, comme avant.
    name: "deux blocs remplacés par deux neufs",
    seed: XYZ,
    act: (p) =>
      void p.editor.replaceBlocks(["x", "y"], [paragraph("n1", "Neuf 1"), paragraph("n2", "Neuf 2")]),
    wrote: {
      made: { "attribut récrit": 2, caractères: 12 },
      deleted: 14,
      bytes: 51,
      renamed: 2,
      page: "n1 « Neuf 1 » | n2 « Neuf 2 » | z « Bloc Z »",
    },
  },
  {
    name: "un bloc supprimé et son voisin modifié en une transaction",
    seed: XYZ,
    act: (p) =>
      p.editor.transact(() => {
        p.editor.removeBlocks(["x"]);
        p.editor.updateBlock("y", { content: "Bloc Y changé" });
      }),
    wrote: {
      made: { "attribut récrit": 1, caractères: 8 },
      deleted: 15,
      bytes: 31,
      renamed: 1,
      page: "y « Bloc Y changé » | z « Bloc Z »",
    },
  },
];

/** Ce que `act` écrit chez une personne seule sur la page `seed`. */
async function trace(seed: Seed, act: (p: Peer) => void): Promise<Trace> {
  const p = await alone(seed);
  const { renamed } = written(p, () => act(p));
  const page = line(shown(p)).replace(/[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}/g, "neuf");
  return { ...counts(outgoing(p)!), renamed: Object.keys(renamed).length, page };
}

describe("ordinary_gestures_write_what_they_wrote_before", () => {
  test.each(ORDINARY)("$name", async ({ seed, act, wrote }) => {
    expect(await trace(seed, act)).toEqual(wrote);
  });
});

// ---------------------------------------------------------------------------
// Sans clés sûres : le code d'origine
// ---------------------------------------------------------------------------

type Keyed = { id: string | null; text: string };

/** Une page de paragraphes, en document ProseMirror du schéma de l'application. */
function pageOf(schema: Peer["editor"]["pmSchema"], blocks: Keyed[]): PmNode {
  return schema.nodeFromJSON({
    type: "doc",
    content: [
      {
        type: "blockGroup",
        content: blocks.map(({ id, text }) => ({
          type: "blockContainer",
          attrs: { id },
          content: [{ type: "paragraph", content: [{ type: "text", text }] }],
        })),
      },
    ],
  });
}

/** Un document Yjs qui porte la page `page`, écrit par y-prosemirror sans éditeur. */
function yDocOf(page: PmNode): Y.Doc {
  const doc = new Y.Doc();
  prosemirrorToYXmlFragment(page, doc.getXmlFragment(FRAGMENT));
  return doc;
}

/** Met `doc` à l'état de `page`, comme le fait le binding après une transaction de l'éditeur. */
function sync(doc: Y.Doc, page: PmNode): void {
  const meta = { mapping: new Map(), isOMark: new Map() };
  doc.transact(() => updateYFragment(doc, doc.getXmlFragment(FRAGMENT), page, meta));
}

describe("missing_or_duplicate_ids_fall_back_to_positions", () => {
  const A1 = { id: "a", text: "Un" };
  const B = { id: "b", text: "Deux" };
  const C = { id: "c", text: "Trois" };

  /** Passe la page de `from` à `to`, et rend ce que cela écrit et si le document est bien `to`. */
  async function rewrite(from: Keyed[], to: Keyed[]) {
    const { pmSchema } = (await peer("Schéma", 9)).editor;
    const target = pageOf(pmSchema, to);
    const doc = yDocOf(pageOf(pmSchema, from));
    const wrote = writtenIn(doc, () => sync(doc, target));
    const reached = yXmlFragmentToProseMirrorRootNode(doc.getXmlFragment(FRAGMENT), pmSchema).eq(target);
    return { ...wrote, reached };
  }

  test("des ids distincts : le bloc déplacé est recréé", async () => {
    expect(await rewrite([A1, B, C], [B, C, A1])).toEqual({ ...recreated("a"), reached: true });
  });

  test("un id absent : par position, aucun élément créé ni supprimé", async () => {
    const anonymous = { id: null, text: "Deux" };
    const wrote = await rewrite([A1, anonymous, C], [anonymous, C, A1]);
    expect(wrote).toMatchObject({ deleted: [], created: [], reached: true });
    expect(Object.keys(wrote.renamed)).not.toHaveLength(0);
  });

  test("un id vide : par position", async () => {
    const empty = { id: "", text: "Deux" };
    const wrote = await rewrite([A1, empty, C], [empty, C, A1]);
    expect(wrote).toMatchObject({ deleted: [], created: [], reached: true });
  });

  test("un id en double dans la page voulue : par position", async () => {
    const twin = { id: "a", text: "Trois" };
    const wrote = await rewrite([A1, B, C], [B, twin, A1]);
    expect(wrote).toMatchObject({ deleted: [], created: [], reached: true });
  });

  test("un id en double dans le document : par position", async () => {
    const twin = { id: "a", text: "Trois" };
    const wrote = await rewrite([A1, B, twin], [B, C, A1]);
    expect(wrote).toMatchObject({ deleted: [], created: [], reached: true });
  });

  test("des mentions échangées dans un paragraphe : leur id n'est pas celui d'un bloc, par position", async () => {
    // Une mention porte un `id` (celui de la page ou du compte nommé) : ce n'est pas une clé.
    const { pmSchema } = (await peer("Schéma", 9)).editor;
    const mention = (id: string) => ({ type: "mention", attrs: { kind: "page", id, label: id } });
    const page = (ids: string[]) =>
      pmSchema.nodeFromJSON({
        type: "doc",
        content: [
          {
            type: "blockGroup",
            content: [
              {
                type: "blockContainer",
                attrs: { id: "a" },
                content: [{ type: "paragraph", content: ids.map(mention) }],
              },
            ],
          },
        ],
      });
    const target = page(["p2", "p1"]);
    const doc = yDocOf(page(["p1", "p2"]));
    expect(writtenIn(doc, () => sync(doc, target))).toMatchObject({ deleted: [], created: [] });
    expect(yXmlFragmentToProseMirrorRootNode(doc.getXmlFragment(FRAGMENT), pmSchema).eq(target)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Les deux fichiers de la dépendance
// ---------------------------------------------------------------------------

describe("le correctif est dans les deux fichiers de y-prosemirror", () => {
  test("l'éditeur charge le fichier que ces tests importent", async () => {
    const mona = await alone(XYZ);
    const { binding } = ySyncPluginKey.getState(mona.editor.prosemirrorState);
    expect(binding).toBeInstanceOf(ProsemirrorBinding);
  });

  test("both_builds_of_the_dependency_carry_the_fix", async () => {
    // `import` charge `src/` (ce que font Vite et Vitest), `require` charge `dist/…cjs`, avec sa
    // propre copie de Yjs : le même déplacement, écrit par l'un puis par l'autre.
    const { pmSchema } = (await alone(XYZ)).editor;
    const from = pageOf(pmSchema, [
      { id: "a", text: "Un" },
      { id: "b", text: "Deux" },
      { id: "c", text: "Trois" },
    ]);
    const to = pageOf(pmSchema, [
      { id: "b", text: "Deux" },
      { id: "c", text: "Trois" },
      { id: "a", text: "Un" },
    ]);

    const { createRequire } = await nodeModule<Module>("node:module");
    const require = createRequire(import.meta.url);
    // Yjs prévient, par `console.error`, qu'il est chargé deux fois : c'est voulu ici.
    const warned = vi.spyOn(console, "error").mockImplementation(() => {});
    const cjs = {
      Y: require("yjs") as typeof Y,
      yProsemirror: require("y-prosemirror") as {
        prosemirrorToYXmlFragment: typeof prosemirrorToYXmlFragment;
        updateYFragment: typeof updateYFragment;
      },
    };
    warned.mockRestore();
    expect(cjs.yProsemirror.updateYFragment).not.toBe(updateYFragment);

    for (const [build, lib, yjs] of [
      ["src (import)", { prosemirrorToYXmlFragment, updateYFragment }, Y],
      ["dist (require)", cjs.yProsemirror, cjs.Y],
    ] as const) {
      const doc = new yjs.Doc();
      const fragment = doc.getXmlFragment(FRAGMENT);
      lib.prosemirrorToYXmlFragment(from, fragment);
      const containers = () => (fragment.get(0) as Y.XmlElement).toArray() as Y.XmlElement[];
      const before = containers();
      const meta = { mapping: new Map(), isOMark: new Map() };
      doc.transact(() => lib.updateYFragment(doc, fragment, to, meta));
      const after = containers();
      expect(after.map((c) => c.getAttribute("id")), build).toEqual(["b", "c", "a"]);
      // B et C sont les mêmes éléments, A est un élément neuf.
      expect([after[0] === before[1], after[1] === before[2]], build).toEqual([true, true]);
      expect(before.includes(after[2]), build).toBe(false);
      expect(before[0]._item?.deleted, build).toBe(true);
    }
  });
});

/**
 * Empreintes SHA-256 des deux fichiers de y-prosemirror 1.3.7 que le correctif touche, tels que
 * publiés : `src/plugins/sync-plugin.js` et `dist/y-prosemirror.cjs`.
 */
const PUBLISHED = {
  src: "2525b13d7b9cae04e8e81fc59674493dddb5017b68f3097a76994ea65ff264f8",
  dist: "d5193952412d8880ae19e1e13dca889d2455a1ac402b5ed5b886574372061f12",
};

/** Ce que le correctif ajoute à un fichier, entre deux lignes repères : rien d'autre n'y change. */
const ADDED = /^[ \t]*\/\/ mementum:moved-blocks:begin\n[\s\S]*?^[ \t]*\/\/ mementum:moved-blocks:end\n/gm;

/**
 * Les deux fichiers installés, et ce qu'il en reste sans les ajouts du correctif (`published`),
 * avec son empreinte. `file` : le chemin de `src/plugins/sync-plugin.js`, liens suivis.
 */
async function installedFiles() {
  const fs = await nodeModule<Fs>("node:fs/promises");
  const path = await nodeModule<Path>("node:path");
  const crypto = await nodeModule<Crypto>("node:crypto");
  const { createRequire } = await nodeModule<Module>("node:module");
  // `…/y-prosemirror/dist/y-prosemirror.cjs` : le dossier du paquet installé est deux crans au-dessus.
  const dist = createRequire(import.meta.url).resolve("y-prosemirror");
  const file = path.join(path.dirname(path.dirname(dist)), "src/plugins/sync-plugin.js");
  const read = async (from: string) => {
    const installed = await fs.readFile(from, "utf8");
    const published = installed.replace(ADDED, "");
    return {
      installed,
      published,
      added: installed.match(ADDED) ?? [],
      sha256: crypto.createHash("sha256").update(published).digest("hex"),
    };
  };
  return { file, src: await read(file), dist: await read(dist) };
}

test("the_patch_only_adds_and_adds_the_same_to_both_files", async () => {
  const { src, dist } = await installedFiles();
  // Hors des ajouts, chaque fichier est celui que y-prosemirror 1.3.7 publie.
  expect({ src: src.sha256, dist: dist.sha256 }).toEqual(PUBLISHED);
  // Trois ajouts par fichier : la garde de la sélection de nœud, les fonctions, leur appel.
  expect(src.added).toHaveLength(3);
  // Le fichier de `dist` nomme Yjs autrement : à cela près, ce sont les mêmes lignes.
  expect(dist.added.map((lines) => lines.replaceAll("instanceof Y__namespace.", "instanceof Y."))).toEqual(
    src.added,
  );
});

// ---------------------------------------------------------------------------
// Un onglet corrigé, un onglet d'origine
// ---------------------------------------------------------------------------

type UpdateYFragment = typeof updateYFragment;
let published: Promise<UpdateYFragment> | undefined;

/**
 * `updateYFragment` tel que y-prosemirror 1.3.7 le publie, pour un onglet « d'origine » : le
 * fichier installé, moins ce que le correctif y ajoute. L'empreinte du résultat est celle du
 * fichier publié : le correctif ne fait donc qu'ajouter, et ce code est bien celui d'origine.
 * Lève si le correctif n'est pas installé. Le code est évalué avec les modules mêmes que charge
 * l'éditeur (Yjs, les clés des plugins) : l'origine des transactions et les `instanceof` tiennent.
 */
function publishedUpdateYFragment(): Promise<UpdateYFragment> {
  published ??= (async () => {
    const path = await nodeModule<Path>("node:path");
    const url = await nodeModule<Url>("node:url");
    const { createRequire } = await nodeModule<Module>("node:module");
    const { file, src } = await installedFiles();
    if (src.added.length === 0) {
      throw new Error(`le correctif de y-prosemirror n'est pas installé (${file})`);
    }
    expect(src.sha256).toBe(PUBLISHED.src);
    const source = src.published;

    // Les imports du fichier, donnés à la main : les fichiers voisins et Yjs sont les modules
    // que l'éditeur a chargés ; le reste (lib0, sans état) vient par `require`.
    const fromPackage = createRequire(file);
    const dependencies: Record<string, unknown> = { yjs: Y, "prosemirror-state": pmState };
    const body = source
      .replace(/^import\s+(?:\*\s+as\s+(\w+)|(\{[^}]*\}))\s+from\s+['"]([^'"]+)['"].*$/gm, (_, all, names, from) => {
        dependencies[from] ??= from.startsWith(".")
          ? url.pathToFileURL(path.resolve(path.dirname(file), from)).href
          : fromPackage(from);
        return `const ${all ?? names} = __dependencies[${JSON.stringify(from)}];`;
      })
      .replace(/^export (const|class) /gm, "$1 ");
    if (/^(import|export)\b/m.test(body)) throw new Error("un import ou un export n'a pas été repris");
    for (const [from, value] of Object.entries(dependencies)) {
      if (typeof value === "string") dependencies[from] = await import(/* @vite-ignore */ value);
    }
    expect((dependencies["./keys.js"] as { ySyncPluginKey: unknown }).ySyncPluginKey).toBe(ySyncPluginKey);
    const module = new Function("__dependencies", `"use strict";\n${body}\nreturn { updateYFragment };`);
    return (module(dependencies) as { updateYFragment: UpdateYFragment }).updateYFragment;
  })();
  return published;
}

/** Fait de `p` un onglet d'origine : son binding écrit avec le `updateYFragment` publié. */
async function unpatched(p: Peer): Promise<Peer> {
  const original = await publishedUpdateYFragment();
  const { binding } = ySyncPluginKey.getState(p.editor.prosemirrorState) as {
    binding: ProsemirrorBinding;
  };
  // Le corps de `ProsemirrorBinding._prosemirrorChanged`, avec la fonction d'origine.
  binding._prosemirrorChanged = (doc) => {
    binding.doc.transact(() => {
      original(binding.doc, binding.type, doc, binding);
      binding.beforeTransactionSelection = getRelativeSelection(binding, binding.prosemirrorView.state);
    }, ySyncPluginKey);
  };
  return p;
}

/** Des nombres au hasard, les mêmes à chaque exécution pour une même graine (mulberry32). */
function random(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4_294_967_296;
  };
}

/** Tous les blocs de `page`, enfants compris, dans l'ordre du document. */
function flat(page: Shown[]): Shown[] {
  return page.flatMap((b) => [b, ...flat(b.children)]);
}

/** `a` et `b` sont la même page, aux ids près des blocs dont l'id est en double dans `b`. */
function sameButTwins(a: Shown[], b: Shown[]): boolean {
  const twins = new Set(repeatedIds(b));
  const same = (left: Shown[], right: Shown[]): boolean =>
    left.length === right.length &&
    left.every((l, i) => {
      const r = right[i];
      return (
        (l.id === r.id || twins.has(r.id)) &&
        l.type === r.type &&
        l.text === r.text &&
        same(l.children, r.children)
      );
    });
  return same(a, b);
}

/** Ce qu'un onglet peut faire, au hasard. `only` : seulement taper, ou seulement déplacer. */
function act(p: Peer, next: () => number, step: number, only?: "type" | "move"): string {
  const page = shown(p);
  const blocks = flat(page);
  const pick = <T>(list: T[]): T => list[Math.floor(next() * list.length)];
  const target = pick(blocks);
  const roll = only === "type" ? 0 : only === "move" ? 0.4 + next() * 0.4 : next();
  if (roll < 0.4) {
    // Un jeton qui nomme son bloc, jamais au milieu d'un autre jeton.
    const within = new Set<number>();
    for (const token of target.text.matchAll(/‹[^›]*›/g)) {
      for (let at = token.index + 1; at < token.index + token[0].length; at += 1) within.add(at);
    }
    const offsets = Array.from({ length: target.text.length + 1 }, (_, at) => at).filter(
      (at) => !within.has(at),
    );
    typeAt(p, target.id, pick(offsets), `‹${target.id}·${step}›`);
    return `${p.name} tape dans ${target.id}`;
  }
  if (roll < 0.7) {
    const inside = new Set(flat([target]).map((b) => b.id));
    const places = blocks.filter((b) => !inside.has(b.id));
    if (places.length === 0) return `${p.name} ne fait rien`;
    const place = pick(places);
    const under = next() < 0.5;
    drag(p, target.id, under ? { under: place.id } : { above: place.id });
    return `${p.name} glisse ${target.id} ${under ? "sous" : "au-dessus de"} ${place.id}`;
  }
  if (roll < 0.8) {
    const down = next() < 0.5;
    p.editor.setTextCursorPosition(target.id, "end");
    press(p, down ? "ArrowDown" : "ArrowUp", { ctrlKey: true, shiftKey: true });
    return `${p.name} ${down ? "descend" : "monte"} ${target.id} au clavier`;
  }
  if (roll < 0.87) {
    cursor(p, target.id, Math.floor(next() * (target.text.length + 1)));
    press(p, "Enter");
    return `${p.name} presse Entrée dans ${target.id}`;
  }
  if (roll < 0.95) {
    const out = next() < 0.5;
    p.editor.setTextCursorPosition(target.id, "end");
    press(p, "Tab", { shiftKey: out });
    return `${p.name} presse ${out ? "Maj+Tab" : "Tab"} dans ${target.id}`;
  }
  if (blocks.length > 3) {
    p.editor.removeBlocks([target.id]);
    return `${p.name} supprime ${target.id}`;
  }
  return `${p.name} ne fait rien`;
}

/** Ce que `from` a écrit part au serveur et chez `to`. */
async function deliver(server: Y.Doc, from: Peer, to: Peer): Promise<void> {
  const update = outgoing(from);
  if (update === null) return;
  Y.applyUpdate(server, update, REMOTE);
  Y.applyUpdate(to.doc, update, REMOTE);
  await settle();
}

const SIX: Seed = Array.from({ length: 6 }, (_, i) => paragraph(`b${i}`, `Bloc ${i}`));

/** Quarante pas au hasard : un onglet agit, ou reçoit ce que l'autre a écrit. */
const STEPS = 40;

describe("keyed_and_unkeyed_tabs_converge", () => {
  test("l'onglet d'origine écrit bien la forme d'origine", async () => {
    const tom = await unpatched(await alone(XYZ));
    expect(written(tom, () => drag(tom, "x", { under: "z" }))).toEqual({
      deleted: [],
      created: [],
      renamed: { x: "y", y: "z", z: "x" },
    });
    expect(stored(tom.doc)).toEqual([Y_, Z, X]);
  });

  // Un onglet corrigé et un onglet d'origine sur la même page, puis deux onglets corrigés : après
  // chaque geste, le document de son auteur est ce que montre son éditeur ; à la fin, les trois
  // documents sont le même, et chaque éditeur montre le sien (aux ids près des blocs en double,
  // que BlockNote renomme chez lui sans l'écrire).
  describe.each([
    { pair: "un onglet corrigé, un onglet d'origine", mixed: true },
    { pair: "deux onglets corrigés", mixed: false },
  ])("$pair", ({ mixed }) => {
    test.each(Array.from({ length: 20 }, (_, i) => i + 1))("graine %i", async (seed) => {
      const next = random(mixed ? seed : seed + 1_000);
      // L'ordre des identifiants change avec la graine.
      const s = await duo(SIX, seed % 2 === 0 ? 2 : 3, seed % 2 === 0 ? 3 : 2);
      if (mixed) await unpatched(s.tom);
      const journal: string[] = [];
      for (let step = 0; step < STEPS; step += 1) {
        const roll = next();
        if (roll < 0.15) await deliver(s.server, s.mona, s.tom);
        else if (roll < 0.3) await deliver(s.server, s.tom, s.mona);
        else {
          const p = roll < 0.65 ? s.mona : s.tom;
          journal.push(act(p, next, step));
          expect(line(stored(p.doc)), `après « ${journal.join(" ; ")} »`).toBe(line(shown(p)));
        }
      }
      await exchange(s.server, [s.mona, s.tom]);
      const told = `après « ${journal.join(" ; ")} »`;
      expect(xml(s.mona.doc), told).toBe(xml(s.server));
      expect(xml(s.tom.doc), told).toBe(xml(s.server));
      const page = stored(s.server);
      for (const p of [s.mona, s.tom]) {
        expect(sameButTwins(shown(p), page), `${p.name} montre ${line(shown(p))}, pas ${line(page)}, ${told}`).toBe(true);
      }
    });
  });
});

// ---------------------------------------------------------------------------
// La promesse, au hasard
// ---------------------------------------------------------------------------

// Mona déplace des blocs, Tom tape, chacun sans attendre l'autre. Chaque frappe de Tom est un
// jeton qui nomme son bloc. À la fin : aucun jeton n'est dans un autre bloc que le sien, ni en
// double, et aucun bloc n'a disparu. Un jeton tapé dans un bloc pendant que Mona le déplaçait
// peut manquer : c'est la part que le serveur reprend (Task 13b), et la limite R222.
describe("tokens_typed_while_someone_else_moves_blocks_never_change_block", () => {
  test.each(Array.from({ length: 20 }, (_, i) => i + 1))("graine %i", async (seed) => {
    const next = random(seed + 2_000);
    const s = await duo(SIX, seed % 2 === 0 ? 2 : 3, seed % 2 === 0 ? 3 : 2);
    const typed: string[] = [];
    for (let step = 0; step < STEPS; step += 1) {
      const roll = next();
      if (roll < 0.15) await deliver(s.server, s.mona, s.tom);
      else if (roll < 0.3) await deliver(s.server, s.tom, s.mona);
      else if (roll < 0.6) act(s.mona, next, step, "move");
      else typed.push(act(s.tom, next, step, "type"));
    }
    await exchange(s.server, [s.mona, s.tom]);
    const page = stored(s.server);
    const told = `page : ${line(page)}`;
    expect(page.map((b) => b.id).sort(), told).toEqual(SIX.map((b) => b.id));
    expect(shown(s.mona), told).toEqual(page);
    expect(shown(s.tom), told).toEqual(page);
    const tokens = page.flatMap((b) =>
      [...b.text.matchAll(/‹(\w+)·(\d+)›/g)].map(([token, id]) => ({ token, id, in: b.id })),
    );
    expect(tokens.filter((t) => t.id !== t.in), told).toEqual([]);
    expect(new Set(tokens.map((t) => t.token)).size, told).toBe(tokens.length);
    // Ce qui reste de chaque texte, une fois les jetons retirés, est le texte du bloc.
    for (const b of page) expect(b.text.replace(/‹\w+·\d+›/g, ""), told).toBe(`Bloc ${b.id.slice(1)}`);
    expect(typed.length).toBeGreaterThan(0);
  });
});
