// Le curseur suit son bloc quand quelqu'un d'autre le déplace (`followBlock.ts`).
//
// Depuis la forme à clés (R220, `keyedBlocks.test.ts`), un bloc déplacé est supprimé et recréé
// ailleurs avec le même id. Le curseur de qui tapait dedans pointait l'ancien texte : y-prosemirror
// le remet là où était le bloc, donc dans celui qui a pris sa place, ou à la fin de la page si le
// bloc était le premier. La frappe suivante partait dans un autre bloc. Mêmes éditeurs que
// `blockMoves.test.ts` (`test/editors.ts`), avec l'extension : de vrais BlockNote 0.57, dans jsdom.
import { NodeSelection, TextSelection } from "prosemirror-state";
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
  peer,
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
