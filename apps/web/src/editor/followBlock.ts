// Le curseur suit son bloc quand le document le recrée ailleurs.
//
// Un bloc déplacé est supprimé, puis recréé à sa nouvelle place avec le même id (la forme à
// clés, R220 : `patches/y-prosemirror@1.3.7.patch`, `keyedBlocks.test.ts`). y-prosemirror remet
// la sélection de l'onglet après chaque changement venu du document partagé, par des positions
// relatives : elles pointent l'élément d'avant, qui n'est plus là. Il la remet alors où était le
// bloc, c'est-à-dire dans celui qui a pris sa place, ou à la fin de la page si le bloc était le
// premier. Qui tapait dans le bloc qu'un autre déplace écrivait donc la suite dans un autre
// bloc ; et pour un bloc sélectionné (celui qu'on vient de déposer), Suppr retirait un voisin.
//
// Cette extension remet la sélection dans le bloc de même id qu'avant, à la même place dans son
// texte. Elle n'écrit rien dans le document : la transaction qu'elle ajoute ne porte qu'une
// sélection, et part dans le même envoi que le changement reçu, pendant que y-prosemirror tient
// son verrou. Elle ne touche pas une sélection restée dans son bloc quand ce bloc est toujours le
// même élément : c'est y-prosemirror qui sait suivre un curseur à travers ce que les autres
// tapent autour.
import { createExtension } from "@blocknote/core";
import {
  NodeSelection,
  Plugin,
  PluginKey,
  TextSelection,
  type EditorState,
  type Selection,
  type Transaction,
} from "prosemirror-state";
import { ySyncPluginKey, type ProsemirrorBinding } from "y-prosemirror";
import * as Y from "yjs";

type PmNode = EditorState["doc"];
type ResolvedPos = Selection["$anchor"];

/** L'id d'un bloc ; `null` pour tout autre nœud (une mention porte un `id` qui n'en est pas un). */
function blockId(node: PmNode): string | null {
  const { id } = node.attrs;
  return node.isBlock && typeof id === "string" && id !== "" ? id : null;
}

/** Un endroit dans un bloc : son id, et le décalage depuis le début de son nœud. */
type Place = { id: string; offset: number };

/** Le bloc le plus proche qui contient `$pos`, et où `$pos` y est. */
function placeOf($pos: ResolvedPos): Place | null {
  for (let depth = $pos.depth; depth > 0; depth -= 1) {
    const id = blockId($pos.node(depth));
    if (id !== null) return { id, offset: $pos.pos - $pos.before(depth) };
  }
  return null;
}

/**
 * Où commence, dans `doc`, le bloc d'id `id` ; `null` s'il n'y est pas. Un id n'est qu'une fois
 * dans le document de l'éditeur : BlockNote en donne un neuf, chez lui, aux blocs en double.
 */
function find(doc: PmNode, id: string): { pos: number; node: PmNode } | null {
  let found: { pos: number; node: PmNode } | undefined;
  doc.descendants((node, pos) => {
    if (found !== undefined) return false;
    if (blockId(node) === id) found = { pos, node };
    return found === undefined;
  });
  return found ?? null;
}

/**
 * La position de `place` dans `doc` : le même décalage dans le bloc de même id, ramené dans son
 * contenu propre (le bloc recréé peut être plus court : sans cela, le curseur irait dans un
 * sous-bloc ou dans le bloc suivant). `null` si le bloc n'y est plus.
 */
function locate(doc: PmNode, place: Place): number | null {
  const block = find(doc, place.id);
  if (block === null) return null;
  const content = block.node.firstChild;
  const start = block.pos + 1;
  if (content === null) return start;
  const last = start + Math.max(content.nodeSize - 1, 0);
  const first = Math.min(start + 1, last);
  return Math.min(Math.max(block.pos + place.offset, first), last);
}

/** Ce que y-prosemirror dit d'une transaction qu'il fait (`ySyncPluginKey`). */
type SyncMeta = {
  isChangeOrigin?: boolean;
  isUndoRedoOperation?: boolean;
  snapshot?: unknown;
  prevSnapshot?: unknown;
};

/**
 * Un changement venu du document partagé, par y-prosemirror : ce qu'un autre a écrit, ou une
 * annulation (elle passe aussi par l'observateur du document). Pas l'affichage d'un instantané.
 */
function fromDocument(tr: Transaction): boolean {
  const meta = tr.getMeta(ySyncPluginKey) as SyncMeta | undefined;
  return meta?.isChangeOrigin === true && meta.snapshot == null && meta.prevSnapshot == null;
}

/**
 * La position relative `relative` ne mène plus nulle part : le texte (ou le paragraphe vide)
 * qu'elle désignait a été supprimé. Lu pendant le changement, avant que Yjs ne ramasse ce qu'il
 * supprime.
 */
function isGone(relative: Y.RelativePosition, doc: Y.Doc): boolean {
  const absolute = Y.createAbsolutePositionFromRelativePosition(relative, doc);
  return absolute === null || absolute.type._item?.deleted === true;
}

/**
 * La sélection à remettre après le changement `change`, ou `null` s'il n'y a rien à faire : la
 * sélection d'avant (`before`) désignait un bloc, le changement l'en a sortie, et ce bloc existe
 * encore.
 */
function followed(change: Transaction, before: Selection, state: EditorState): Selection | null {
  const { doc, selection } = state;

  if (before instanceof NodeSelection) {
    const id = blockId(before.node);
    if (id === null) return null;
    if (selection instanceof NodeSelection && blockId(selection.node) === id) return null;
    const block = find(doc, id);
    return block === null ? null : NodeSelection.create(doc, block.pos);
  }

  if (!(before instanceof TextSelection)) return null;
  const anchor = placeOf(before.$anchor);
  const head = placeOf(before.$head);
  if (anchor === null || head === null) return null;
  // Un bout a quitté son bloc. Ou il y est resté par hasard (le bloc recréé est là où
  // y-prosemirror a remis la sélection), mais pas à sa place : sa position relative, celle que
  // y-prosemirror avait prise avant un changement reçu, ne mène plus nulle part. Une annulation
  // remet une autre sélection que celle d'avant : sa position relative n'est pas lue.
  const { binding } = ySyncPluginKey.getState(state) as { binding: ProsemirrorBinding };
  const { isUndoRedoOperation } = change.getMeta(ySyncPluginKey) as SyncMeta;
  const relative = isUndoRedoOperation ? null : binding.beforeTransactionSelection;
  const left = (place: Place, $now: ResolvedPos, was: Y.RelativePosition | undefined) =>
    placeOf($now)?.id !== place.id || (was !== undefined && isGone(was, binding.doc));
  if (
    !left(anchor, selection.$anchor, relative?.anchor) &&
    !left(head, selection.$head, relative?.head)
  ) {
    return null;
  }
  const from = locate(doc, anchor);
  const to = locate(doc, head);
  if (from === null || to === null) return null;
  const wanted = TextSelection.between(doc.resolve(from), doc.resolve(to));
  return wanted.eq(selection) ? null : wanted;
}

const followBlockKey = new PluginKey("mementum-follow-block");

/**
 * L'extension de l'éditeur de la page. La transaction ajoutée dit, comme celle de y-prosemirror,
 * qu'elle vient du document (`isChangeOrigin`) et qu'elle n'est pas un geste à annuler : elle
 * est la fin du changement reçu, pas une action de la personne.
 */
export const followBlock = createExtension({
  key: "followBlock",
  prosemirrorPlugins: [
    new Plugin({
      key: followBlockKey,
      appendTransaction(transactions, oldState, newState) {
        const change = transactions.find(fromDocument);
        if (change === undefined) return null;
        const selection = followed(change, oldState.selection, newState);
        if (selection === null) return null;
        const { isUndoRedoOperation } = change.getMeta(ySyncPluginKey) as SyncMeta;
        return newState.tr
          .setSelection(selection)
          .setMeta(ySyncPluginKey, { isChangeOrigin: true, isUndoRedoOperation: isUndoRedoOperation === true })
          .setMeta("addToHistory", false);
      },
    }),
  ],
});
