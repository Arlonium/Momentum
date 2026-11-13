// Le curseur suit son bloc quand quelqu'un d'autre le déplace (`followBlock.ts`).
//
// Depuis la forme à clés (R220, `keyedBlocks.test.ts`), un bloc déplacé est supprimé et recréé
// ailleurs avec le même id. Le curseur de qui tapait dedans pointait l'ancien texte : y-prosemirror
// le remet là où était le bloc, donc dans celui qui a pris sa place, ou à la fin de la page si le
// bloc était le premier. La frappe suivante partait dans un autre bloc. Mêmes éditeurs que
// `blockMoves.test.ts` (`test/editors.ts`), avec l'extension : de vrais BlockNote 0.57, dans jsdom.
import { NodeSelection, Selection, TextSelection } from "prosemirror-state";
import { describe, expect, test } from "vitest";
import * as Y from "yjs";
import {
  block,
  containerOf,
  cursor,
  cursorOf,
  drag,
  exchange,
  keyboardDown,
  line,
  PARENTED,
  peer,
  pressAt,
  REMOTE,
  seedState,
  selectBlock,
  shown,
  stored,
  tab,
  typeAt,
  typeAtEnd,
  W,
  X,
  XYZW,
  Y_,
  Z,
  type Peer,
} from "../test/editors";
import { followBlock } from "./followBlock";

/**
 * La page « Bloc X, Bloc Y, Bloc Z, Bloc W » ouverte par Mona et par Tom, l'éditeur de chacun
 * avec `followBlock` (sauf `plain`, pour le témoin), et le document du serveur.
 */
async function pair({ plain = false } = {}) {
  const base = await seedState(XYZW);
  const extensions = plain ? undefined : [followBlock];
  const mona = await peer("Mona", 2, base, extensions);
  const tom = await peer("Tom", 3, base, extensions);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  expect(mona.writes.length + tom.writes.length, "rien d'écrit à l'ouverture").toBe(0);
  return { mona, tom, server };
}

/** `p` tape `text` là où est son curseur, comme au clavier. */
function type(p: Peer, text: string): void {
  p.editor.transact((tr) => tr.insertText(text));
}

describe("cursor_follows_its_block_when_someone_else_moves_it", () => {
  test("témoin : sans l'extension, le curseur part dans un autre bloc, et la frappe suivante avec", async () => {
    // X est le premier bloc : le curseur tombe à la fin de la page.
    const first = await pair({ plain: true });
    cursor(first.tom, "x", 3);
    drag(first.mona, "x", { under: "z" });
    await exchange(first.server, [first.mona, first.tom]);
    expect(cursorOf(first.tom)).toEqual({ id: "w", offset: 6 });
    type(first.tom, "!");
    await exchange(first.server, [first.tom, first.mona]);
    expect(stored(first.server)).toEqual([Y_, Z, X, block("w", "Bloc W!")]);

    // Y est au milieu : le curseur passe dans le bloc qui a pris sa place.
    const middle = await pair({ plain: true });
    cursor(middle.tom, "y", 3);
    drag(middle.mona, "y", { under: "w" });
    await exchange(middle.server, [middle.mona, middle.tom]);
    expect(cursorOf(middle.tom)).toEqual({ id: "z", offset: 0 });
  });

  test("le premier bloc glissé tout en bas : le curseur y garde sa place, pas la fin du texte", async () => {
    // Sans l'extension, le curseur tombe à la fin de la page, qui est alors la fin de ce bloc :
    // dans le bon bloc par hasard, pas à sa place.
    const plain = await pair({ plain: true });
    cursor(plain.tom, "x", 3);
    drag(plain.mona, "x", { under: "w" });
    await exchange(plain.server, [plain.mona, plain.tom]);
    expect(cursorOf(plain.tom)).toEqual({ id: "x", offset: 6 });

    const { mona, tom, server } = await pair();
    cursor(tom, "x", 3);
    drag(mona, "x", { under: "w" });
    await exchange(server, [mona, tom]);
    expect(cursorOf(tom)).toEqual({ id: "x", offset: 3 });
  });

  test("glissé par un autre : le curseur reste dans son bloc, à la même place, et la frappe suivante aussi", async () => {
    const { mona, tom, server } = await pair();
    cursor(tom, "x", 3);
    drag(mona, "x", { under: "z" });
    await exchange(server, [mona, tom]);
    expect(shown(tom)).toEqual([Y_, Z, X, W]);
    expect(cursorOf(tom)).toEqual({ id: "x", offset: 3 });
    type(tom, "!");
    await exchange(server, [tom, mona]);
    const wanted = [Y_, Z, block("x", "Blo!c X"), W];
    expect(stored(server)).toEqual(wanted);
    expect(shown(mona)).toEqual(wanted);
  });

  test("déplacé au clavier, ou rentré sous son voisin (Tab) : le curseur suit", async () => {
    const down = await pair();
    cursor(down.tom, "x", 3);
    keyboardDown(down.mona, "x");
    await exchange(down.server, [down.mona, down.tom]);
    expect(cursorOf(down.tom)).toEqual({ id: "x", offset: 3 });

    const nested = await pair();
    cursor(nested.tom, "y", 3);
    tab(nested.mona, "y");
    await exchange(nested.server, [nested.mona, nested.tom]);
    expect(shown(nested.tom)).toEqual([block("x", "Bloc X", { children: [Y_] }), Z, W]);
    expect(cursorOf(nested.tom)).toEqual({ id: "y", offset: 3 });
  });

  test("une sélection de texte suit, ses deux bouts à leur place", async () => {
    const { mona, tom, server } = await pair();
    tom.editor.transact((tr) => {
      const { pos } = containerOf(tr.doc, "x");
      tr.setSelection(TextSelection.create(tr.doc, pos + 2 + 1, pos + 2 + 4));
    });
    drag(mona, "x", { under: "z" });
    await exchange(server, [mona, tom]);
    const { selection, doc } = tom.editor.prosemirrorState;
    const { pos } = containerOf(doc, "x");
    expect([selection.anchor, selection.head]).toEqual([pos + 2 + 1, pos + 2 + 4]);
    // Ce qui est tapé remplace la sélection, « loc ».
    type(tom, "…");
    await exchange(server, [tom, mona]);
    expect(stored(server)).toEqual([Y_, Z, block("x", "B… X"), W]);
  });

  test("un autre change le type du bloc (son contenu est recréé, pas le bloc) : le curseur garde sa place", async () => {
    // Sans l'extension, le curseur passe au début du bloc suivant (c'était déjà vrai avant la
    // forme à clés : un paragraphe devenu titre est un élément supprimé et un élément créé).
    const plain = await pair({ plain: true });
    cursor(plain.tom, "y", 3);
    plain.mona.editor.updateBlock("y", { type: "heading", props: { level: 2 } });
    await exchange(plain.server, [plain.mona, plain.tom]);
    expect(cursorOf(plain.tom)).toEqual({ id: "z", offset: 0 });

    const { mona, tom, server } = await pair();
    cursor(tom, "y", 3);
    mona.editor.updateBlock("y", { type: "heading", props: { level: 2 } });
    await exchange(server, [mona, tom]);
    expect(shown(tom)).toEqual([X, block("y", "Bloc Y", { type: "heading" }), Z, W]);
    expect(cursorOf(tom)).toEqual({ id: "y", offset: 3 });
  });

  test("le bloc recréé est plus court que la place du curseur : le curseur va à sa fin", async () => {
    const { mona, tom, server } = await pair();
    // Tom vient de taper à la fin de X, et Mona déplace X sans l'avoir vu : sa copie ne l'a pas.
    typeAtEnd(tom, "x", " de Tom");
    cursor(tom, "x", 13);
    drag(mona, "x", { under: "z" });
    await exchange(server, [mona, tom]);
    expect(shown(tom)).toEqual([Y_, Z, X, W]);
    expect(cursorOf(tom)).toEqual({ id: "x", offset: 6 });
  });

  test("un bloc sélectionné comme un nœud (un clic sur une image) reste sélectionné quand un autre le déplace", async () => {
    const { mona, tom, server } = await pair();
    drag(mona, "x", { under: "y" });
    await exchange(server, [mona, tom]);
    selectBlock(mona, "x");
    expect(mona.editor.prosemirrorState.selection).toBeInstanceOf(NodeSelection);
    // Tom l'emmène tout en bas, puis au milieu : la sélection de Mona le suit, et n'est jamais
    // sur un voisin (Suppr retirerait alors un autre bloc que celui qu'elle voit sélectionné).
    for (const place of [{ under: "w" }, { above: "z" }] as const) {
      drag(tom, "x", place);
      await exchange(server, [tom, mona]);
      const { selection } = mona.editor.prosemirrorState;
      expect(selection).toBeInstanceOf(NodeSelection);
      expect((selection as NodeSelection).node.attrs.id).toBe("x");
    }
    expect(shown(mona)).toEqual([Y_, X, Z, W]);
  });
});

// Entrée au milieu ou à la fin d'un bloc qui a des sous-blocs : y-prosemirror garde l'élément du
// bloc pour le bloc neuf, qui prend les sous-blocs et la fin du texte, et donne l'id d'avant à un
// élément neuf, qui prend le début (`ordinary_gestures_write_what_they_wrote_before`,
// `keyedBlocks.test.ts`). « Même id » ne dit donc pas « même texte » : le curseur de qui tapait
// après la coupure est resté dans son texte, sous un autre id, et ne doit pas en sortir ; celui
// de qui tapait avant la coupure a perdu son texte, et le retrouve dans le bloc de même id.
describe("cursor_stays_with_its_text_when_someone_else_splits_its_block", () => {
  /**
   * Tom a son curseur dans X (qui a un sous-bloc), à `tomAt` caractères du début ; Mona presse
   * Entrée dans X, à `enterAt` ; Tom tape « ! » sans recliquer. Rend la page du serveur, l'id du
   * bloc neuf écrit `neuf`.
   */
  async function split(tomAt: number, enterAt: number, { plain = false } = {}): Promise<string> {
    const base = await seedState(PARENTED);
    const extensions = plain ? undefined : [followBlock];
    const mona = await peer("Mona", 2, base, extensions);
    const tom = await peer("Tom", 3, base, extensions);
    const server = new Y.Doc();
    Y.applyUpdate(server, base, REMOTE);
    cursor(tom, "x", tomAt);
    pressAt(mona, "x", enterAt, "Enter");
    await exchange(server, [mona, tom]);
    type(tom, "!");
    await exchange(server, [tom, mona]);
    expect(shown(mona)).toEqual(stored(server));
    return line(stored(server)).replace(/[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}/g, "neuf");
  }

  const rest = "[x1 « Enfant X1 »] | y « Bloc Y » | z « Bloc Z »";

  test("témoin : sans l'extension, le curseur d'avant la coupure part dans le bloc neuf", async () => {
    expect(await split(1, 3, { plain: true })).toBe(`x « Blo » | neuf « !c X » ${rest}`);
    expect(await split(3, 6, { plain: true })).toBe(`x « Bloc X » | neuf « ! » ${rest}`);
    // Celui d'après la coupure est juste : il est resté dans son texte.
    expect(await split(6, 3, { plain: true })).toBe(`x « Blo » | neuf « c X! » ${rest}`);
    expect(await split(5, 3, { plain: true })).toBe(`x « Blo » | neuf « c !X » ${rest}`);
  });

  test.each([
    { tomAt: 6, enterAt: 3, page: `x « Blo » | neuf « c X! » ${rest}` },
    { tomAt: 5, enterAt: 3, page: `x « Blo » | neuf « c !X » ${rest}` },
    { tomAt: 1, enterAt: 3, page: `x « B!lo » | neuf « c X » ${rest}` },
    { tomAt: 3, enterAt: 6, page: `x « Blo!c X » | neuf «  » ${rest}` },
    { tomAt: 6, enterAt: 6, page: `x « Bloc X! » | neuf «  » ${rest}` },
  ])("Tom en $tomAt, Entrée de Mona en $enterAt : $page", async ({ tomAt, enterAt, page }) => {
    expect(await split(tomAt, enterAt)).toBe(page);
  });

  test("Tom juste à la coupure : sa frappe va d'un côté ou de l'autre, pas ailleurs", async () => {
    // Les deux sont justes ; aujourd'hui, c'est la fin de la première moitié.
    expect([`x « Blo! » | neuf « c X » ${rest}`, `x « Blo » | neuf « !c X » ${rest}`]).toContain(
      await split(3, 3),
    );
  });
});

// Pendant un glisser, BlockNote sélectionne en bloc ce qu'on tient au bout de la souris (sa
// `MultipleNodeSelection`), et le dépôt de ProseMirror supprime la sélection avant d'insérer ce
// qu'on tenait. y-prosemirror ne connaît pas cette sorte de sélection : au premier changement
// reçu, il en faisait une sélection de texte, et le dépôt supprimait alors le texte du bloc au
// lieu de le déplacer (vu dans un navigateur, `e2e/block-move.spec.ts`, « en ligne »).
describe("held_blocks_stay_selected_while_someone_else_writes", () => {
  /** Comme le début d'un glisser : le bloc `id` sélectionné en bloc, et tenu. */
  function hold(p: Peer, id: string): void {
    p.editor.transact((tr) => {
      const { pos, node } = containerOf(tr.doc, id);
      tr.setSelection(
        Selection.fromJSON(tr.doc, { type: "multiple-node", anchor: pos, head: pos + node.nodeSize }),
      );
    });
    const view = p.editor.prosemirrorView;
    view.dragging = { slice: view.state.selection.content(), move: true };
  }

  /** Comme le dépôt de ProseMirror : la sélection supprimée, ce qu'on tenait inséré sous `under`. */
  function dropUnder(p: Peer, under: string): void {
    const view = p.editor.prosemirrorView;
    const { slice } = view.dragging!;
    p.editor.transact((tr) => {
      tr.deleteSelection();
      const target = containerOf(tr.doc, under);
      tr.insert(target.pos + target.node.nodeSize, slice.content);
    });
    view.dragging = null;
  }

  const held = (p: Peer) => {
    const { selection } = p.editor.prosemirrorState;
    const blocks = (selection as unknown as { nodes?: { attrs: { id: string } }[] }).nodes ?? [];
    return { type: (selection.toJSON() as { type: string }).type, ids: blocks.map((b) => b.attrs.id) };
  };

  test("témoin : sans l'extension, le dépôt vide le bloc au lieu de le déplacer", async () => {
    const { mona, tom, server } = await pair({ plain: true });
    hold(mona, "x");
    typeAtEnd(tom, "y", " de Tom");
    await exchange(server, [tom, mona]);
    expect(held(mona).type).toBe("text");
    dropUnder(mona, "z");
    await exchange(server, [mona, tom]);
    // Le bloc reste en tête, vidé de son texte ; ce qu'on tenait est inséré en plus.
    expect(stored(server).map((b) => b.text)).toEqual(["", "Bloc Y de Tom", "Bloc Z", "Bloc X", "Bloc W"]);
  });

  test("un autre écrit dans un voisin pendant qu'on tient le bloc : il est déposé entier", async () => {
    const { mona, tom, server } = await pair();
    hold(mona, "x");
    expect(held(mona)).toEqual({ type: "multiple-node", ids: ["x"] });
    typeAtEnd(tom, "y", " de Tom");
    await exchange(server, [tom, mona]);
    expect(held(mona)).toEqual({ type: "multiple-node", ids: ["x"] });
    dropUnder(mona, "z");
    await exchange(server, [mona, tom]);
    const wanted = [block("y", "Bloc Y de Tom"), Z, X, W];
    expect(stored(server)).toEqual(wanted);
    expect(shown(tom)).toEqual(wanted);
  });

  test("un autre écrit dans le bloc qu'on tient : on dépose ce qu'il est devenu", async () => {
    const { mona, tom, server } = await pair();
    hold(mona, "x");
    typeAtEnd(tom, "x", " de Tom");
    await exchange(server, [tom, mona]);
    expect(held(mona)).toEqual({ type: "multiple-node", ids: ["x"] });
    dropUnder(mona, "z");
    await exchange(server, [mona, tom]);
    const wanted = [Y_, Z, block("x", "Bloc X de Tom"), W];
    expect(stored(server)).toEqual(wanted);
    expect(shown(tom)).toEqual(wanted);
  });

  test("un autre déplace le bloc qu'on tient : il reste tenu, à sa nouvelle place", async () => {
    const { mona, tom, server } = await pair();
    hold(mona, "x");
    drag(tom, "x", { under: "w" });
    await exchange(server, [tom, mona]);
    expect(held(mona)).toEqual({ type: "multiple-node", ids: ["x"] });
    dropUnder(mona, "y");
    await exchange(server, [mona, tom]);
    expect(stored(server)).toEqual([Y_, X, Z, W]);
  });
});

describe("cursor_is_left_alone_when_nothing_moved", () => {
  test("une frappe ailleurs, une frappe avant le curseur, un autre bloc déplacé : le curseur est où y-prosemirror l'a mis", async () => {
    const { mona, tom, server } = await pair();
    cursor(tom, "y", 3);
    typeAtEnd(mona, "x", " de Mona");
    await exchange(server, [mona, tom]);
    expect(cursorOf(tom)).toEqual({ id: "y", offset: 3 });
    // Mona écrit avant le curseur de Tom, dans son bloc : le curseur avance avec le texte.
    typeAt(mona, "y", 0, "Ah, ");
    await exchange(server, [mona, tom]);
    expect(cursorOf(tom)).toEqual({ id: "y", offset: 7 });
    // Mona déplace un autre bloc par-dessus celui de Tom.
    drag(mona, "x", { under: "z" });
    await exchange(server, [mona, tom]);
    expect(cursorOf(tom)).toEqual({ id: "y", offset: 7 });
    type(tom, "!");
    await exchange(server, [tom, mona]);
    expect(stored(server)).toEqual([block("y", "Ah, Blo!c Y"), Z, block("x", "Bloc X de Mona"), W]);
  });

  test("deux personnes tapent dans le même bloc vide : chaque curseur reste où y-prosemirror le met", async () => {
    // Chacun crée son texte dans le paragraphe vide ; à la fusion, y-prosemirror verse l'un dans
    // l'autre et supprime le second. Le bloc, lui, est toujours le même élément : la place d'un
    // curseur dans son texte a changé, et c'est y-prosemirror qui la connaît (vu dans un
    // navigateur : `offline_edits_merge_on_reconnect`, le curseur revenait au milieu du mot).
    const cursors = async (plain: boolean) => {
      const base = await seedState([{ id: "e", type: "paragraph", content: "" }]);
      const extensions = plain ? undefined : [followBlock];
      const mona = await peer("Mona", 2, base, extensions);
      const tom = await peer("Tom", 3, base, extensions);
      const server = new Y.Doc();
      Y.applyUpdate(server, base, REMOTE);
      cursor(mona, "e", 0);
      type(mona, "A en ligne");
      cursor(tom, "e", 0);
      type(tom, "B hors ligne");
      await exchange(server, [mona, tom]);
      expect(stored(server).map((b) => b.text.length)).toEqual(["A en ligneB hors ligne".length]);
      return { text: stored(server)[0].text, mona: cursorOf(mona), tom: cursorOf(tom) };
    };
    expect(await cursors(false)).toEqual(await cursors(true));
  });

  test("le bloc du curseur est supprimé par un autre : rien n'est suivi, rien ne casse", async () => {
    const { mona, tom, server } = await pair();
    cursor(tom, "x", 3);
    mona.editor.removeBlocks(["x"]);
    await exchange(server, [mona, tom]);
    expect(shown(tom)).toEqual([Y_, Z, W]);
    expect(cursorOf(tom)?.id).not.toBe("x");
  });

  test("annuler une frappe faite ailleurs laisse le curseur où on l'a mis", async () => {
    const { tom } = await pair();
    typeAtEnd(tom, "x", " abc");
    cursor(tom, "w", 1);
    expect(tom.editor.undo()).toBe(true);
    expect(shown(tom)).toEqual([X, Y_, Z, W]);
    expect(cursorOf(tom)).toEqual({ id: "w", offset: 1 });
  });
});

// L'annulation passe par l'observateur du document, comme un changement reçu : annuler un glisser
// supprime le bloc déposé et le recrée à sa place d'avant.
describe("annuler son propre déplacement", () => {
  test("le curseur que le dépôt a laissé dans le bloc glissé le suit à sa place d'avant", async () => {
    const { mona } = await pair();
    drag(mona, "y", { under: "w" });
    expect(cursorOf(mona)).toEqual({ id: "y", offset: 6 });
    expect(mona.editor.undo()).toBe(true);
    expect(shown(mona)).toEqual([X, Y_, Z, W]);
    expect(cursorOf(mona)).toEqual({ id: "y", offset: 6 });
  });

  test("le bloc glissé, sélectionné ensuite comme un nœud, le reste : pas son voisin", async () => {
    const { mona } = await pair();
    drag(mona, "x", { under: "z" });
    selectBlock(mona, "x");
    expect(mona.editor.undo()).toBe(true);
    expect(shown(mona)).toEqual([X, Y_, Z, W]);
    const { selection } = mona.editor.prosemirrorState;
    expect(selection).toBeInstanceOf(NodeSelection);
    expect((selection as NodeSelection).node.attrs.id).toBe("x");
  });

  test("le curseur de qui tapait dans le bloc le suit aussi au retour", async () => {
    const { mona, tom, server } = await pair();
    drag(mona, "x", { under: "z" });
    await exchange(server, [mona, tom]);
    cursor(tom, "x", 3);
    expect(mona.editor.undo()).toBe(true);
    await exchange(server, [mona, tom]);
    expect(shown(tom)).toEqual([X, Y_, Z, W]);
    expect(cursorOf(tom)).toEqual({ id: "x", offset: 3 });
  });
});

describe("nothing_is_written_to_the_document_by_following", () => {
  test("suivre son bloc n'écrit rien : ni mise à jour, ni présence faussée", async () => {
    const { mona, tom, server } = await pair();
    cursor(tom, "x", 3);
    drag(mona, "x", { under: "z" });
    await exchange(server, [mona, tom]);
    expect(cursorOf(tom)).toEqual({ id: "x", offset: 3 });
    expect(tom.writes).toHaveLength(0);
    expect(Y.encodeStateAsUpdate(tom.doc)).toEqual(Y.encodeStateAsUpdate(server));
  });
});
