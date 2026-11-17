// « Rétablir » sans rien à rétablir ne fait rien (Rulings R731 à R735).
//
// BlockNote 0.57 lie Ctrl+Maj+Z (`Shift-Mod-z`) et Ctrl+Y (`Mod-y`) à `editor.redo()`, qui rend
// faux quand la pile de rétablissement est vide. prosemirror-keymap essaie alors, dans la même
// table, la touche sans Maj quand `event.key` est une lettre (`Mod-z` : l'annulation) : la touche
// « rétablir » retirait la dernière frappe dès que le navigateur la donnait en minuscule avec Maj
// (Verr. Maj. sous Windows et Linux ; `press("Control+Shift+z")` de Playwright). Ctrl+Y, sans Maj,
// n'a pas de repli : il est là en témoin. Les éditeurs sont ceux du banc (`test/editors.ts`), avec
// les extensions de l'éditeur de la page.
import { yUndoPluginKey } from "y-prosemirror";
import { expect, test } from "vitest";
import * as Y from "yjs";
import {
  exchange,
  line,
  peer,
  REMOTE,
  seedState,
  shown,
  stored,
  typeAtEnd,
  XYZ,
  type Peer,
} from "../test/editors";
import { EDITOR_OPTIONS } from "./Editor";

/** La touche, reçue par l'éditeur de `p` comme d'un clavier. Rend vrai si quelqu'un l'a prise. */
function key(p: Peer, init: KeyboardEventInit & { keyCode?: number }): boolean {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  // Le code de la touche, que jsdom ne prend pas dans `init` : prosemirror-keymap s'en sert en repli.
  if (init.keyCode !== undefined) Object.defineProperty(event, "keyCode", { value: init.keyCode });
  p.editor.prosemirrorView.dom.dispatchEvent(event);
  return event.defaultPrevented;
}

/** Ctrl+Maj+Z, la lettre en minuscule : Verr. Maj. activé, ou Playwright. */
const REDO_LOWER = { key: "z", ctrlKey: true, shiftKey: true, keyCode: 90 };
/** Ctrl+Maj+Z, la lettre en majuscule : le cas ordinaire d'un navigateur. */
const REDO_UPPER = { key: "Z", ctrlKey: true, shiftKey: true, keyCode: 90 };
/** Ctrl+Y. */
const REDO_Y = { key: "y", ctrlKey: true, keyCode: 89 };
/** Ctrl+Z. */
const UNDO = { key: "z", ctrlKey: true, keyCode: 90 };

/** Ferme le pas d'annulation en cours : la frappe suivante en ouvre un autre (`captureTimeout`). */
function closeStep(p: Peer): void {
  (yUndoPluginKey.getState(p.editor.prosemirrorState) as { undoManager: Y.UndoManager }).undoManager.stopCapturing();
}

/** Mona, avec les extensions de l'éditeur de la page (sauf `plain`, pour le témoin), a tapé deux fois. */
async function typed({ plain = false } = {}): Promise<Peer> {
  const mona = await peer("Mona", 2, await seedState(XYZ), plain ? undefined : EDITOR_OPTIONS.extensions);
  typeAtEnd(mona, "x", " deux");
  closeStep(mona);
  typeAtEnd(mona, "x", " trois");
  closeStep(mona);
  return mona;
}

const TYPED = "x « Bloc X deux trois » | y « Bloc Y » | z « Bloc Z »";

test.each([
  ["Ctrl+Maj+Z en minuscule", REDO_LOWER],
  ["Ctrl+Maj+Z en majuscule", REDO_UPPER],
  ["Ctrl+Y", REDO_Y],
])("redo_with_nothing_to_redo_leaves_the_typing (%s)", async (_, init) => {
  const mona = await typed();
  expect(line(shown(mona))).toBe(TYPED);
  // La touche est prise : ni l'annulation, ni le « rétablir » du navigateur ne la reçoivent.
  expect(key(mona, init)).toBe(true);
  expect(line(shown(mona))).toBe(TYPED);
  expect(key(mona, init)).toBe(true);
  expect(line(shown(mona))).toBe(TYPED);
});

test("undo_and_redo_by_the_keys_still_work", async () => {
  const mona = await typed();
  expect(key(mona, UNDO)).toBe(true);
  expect(line(shown(mona))).toBe("x « Bloc X deux » | y « Bloc Y » | z « Bloc Z »");
  expect(key(mona, UNDO)).toBe(true);
  expect(line(shown(mona))).toBe("x « Bloc X » | y « Bloc Y » | z « Bloc Z »");
  // Rétablir tout, par les trois formes de la touche, puis une de plus : rien ne bouge.
  expect(key(mona, REDO_LOWER)).toBe(true);
  expect(line(shown(mona))).toBe("x « Bloc X deux » | y « Bloc Y » | z « Bloc Z »");
  expect(key(mona, REDO_UPPER)).toBe(true);
  expect(line(shown(mona))).toBe(TYPED);
  expect(key(mona, REDO_LOWER)).toBe(true);
  expect(line(shown(mona))).toBe(TYPED);
  // L'annulation suivante défait encore la dernière frappe, et Ctrl+Y la rétablit.
  expect(key(mona, UNDO)).toBe(true);
  expect(line(shown(mona))).toBe("x « Bloc X deux » | y « Bloc Y » | z « Bloc Z »");
  expect(key(mona, REDO_Y)).toBe(true);
  expect(line(shown(mona))).toBe(TYPED);
});

test("with_two_people_redo_with_nothing_to_redo_takes_nothing_away", async () => {
  const base = await seedState(XYZ);
  const mona = await peer("Mona", 2, base, EDITOR_OPTIONS.extensions);
  const tom = await peer("Tom", 3, base, EDITOR_OPTIONS.extensions);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  typeAtEnd(mona, "x", " de Mona");
  typeAtEnd(tom, "y", " de Tom");
  await exchange(server, [mona, tom]);
  const both = "x « Bloc X de Mona » | y « Bloc Y de Tom » | z « Bloc Z »";
  expect(line(stored(server))).toBe(both);

  expect(key(mona, REDO_LOWER)).toBe(true);
  expect(key(tom, REDO_LOWER)).toBe(true);
  await exchange(server, [mona, tom]);
  expect(line(stored(server))).toBe(both);
  expect(line(shown(mona))).toBe(both);
  expect(line(shown(tom))).toBe(both);
});

// Témoin : BlockNote 0.57 tel que publié, sans nos extensions, retombe sur l'annulation. Le jour où
// ce test rougit, la dépendance ne le fait plus, et `redoKeys` peut partir (R732).
test("the_published_editor_undoes_on_a_redo_with_nothing_to_redo", async () => {
  const mona = await typed({ plain: true });
  expect(key(mona, REDO_LOWER)).toBe(true);
  expect(line(shown(mona))).toBe("x « Bloc X deux » | y « Bloc Y » | z « Bloc Z »");
  // Et la majuscule n'est prise par personne : le navigateur la reçoit (son « rétablir » à lui).
  expect(key(mona, REDO_UPPER)).toBe(true);
  expect(line(shown(mona))).toBe(TYPED);
  expect(key(mona, REDO_UPPER)).toBe(false);
  expect(line(shown(mona))).toBe(TYPED);
});
