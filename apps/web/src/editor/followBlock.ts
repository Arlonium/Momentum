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
// De même pour les blocs qu'on tient au bout de la souris pendant un glisser : BlockNote les
// sélectionne en bloc, y-prosemirror en fait une sélection de texte au premier changement reçu,
// et le dépôt supprimait alors le texte du bloc au lieu de le déplacer (c'était déjà vrai avant
// la forme à clés).
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
  Selection,
  TextSelection,
  type EditorState,
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

/**
 * La sorte de sélection que BlockNote pose sur les blocs qu'on tient au bout de la souris, le
 * temps d'un glisser (sa `MultipleNodeSelection`, des blocs entiers et voisins). Le dépôt de
 * ProseMirror supprime la sélection, puis insère ce qu'on tenait : devenue une sélection de
 * texte, comme y-prosemirror la remet après un changement reçu, c'est le texte du bloc qu'il
 * supprimait.
 */
const HELD_BLOCKS = "multiple-node";

/** La sorte d'une sélection, telle qu'elle s'écrit en JSON (`text`, `node`, `multiple-node`…). */
function kindOf(selection: Selection): string {
  return String((selection.toJSON() as { type?: unknown }).type);
}

/** Les ids du premier et du dernier des blocs tenus ; `null` si l'un n'en a pas. */
function heldEnds(selection: Selection): { first: string; last: string } | null {
  const { nodeAfter } = selection.$from;
  const { nodeBefore } = selection.$to;
  const first = nodeAfter === null ? null : blockId(nodeAfter);
  const last = nodeBefore === null ? null : blockId(nodeBefore);
  return first === null || last === null ? null : { first, last };
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
 * La position relative `relative` ne mène plus nulle part : l'élément qui portait le texte
 * qu'elle désignait (le paragraphe d'un bloc) a été supprimé, avec son bloc ou seul. Lu pendant
 * le changement, avant que Yjs ne ramasse ce qu'il supprime.
 *
 * Un texte supprimé dans un élément resté là n'en est pas : quand deux personnes écrivent en même
 * temps dans un paragraphe vide, y-prosemirror verse l'un des deux textes dans l'autre et le
 * supprime. Le curseur a alors changé de place dans son texte, et c'est y-prosemirror qui sait
 * où il est.
 */
function isGone(relative: Y.RelativePosition, doc: Y.Doc): boolean {
  const absolute = Y.createAbsolutePositionFromRelativePosition(relative, doc);
  if (absolute === null) return true;
  const holder = absolute.type instanceof Y.XmlText ? absolute.type.parent : absolute.type;
  return holder instanceof Y.XmlElement && holder._item?.deleted === true;
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

  if (kindOf(before) === HELD_BLOCKS) {
    const ends = heldEnds(before);
    if (ends === null) return null;
    const now = kindOf(selection) === HELD_BLOCKS ? heldEnds(selection) : null;
    if (now !== null && now.first === ends.first && now.last === ends.last) return null;
    const first = find(doc, ends.first);
    const last = find(doc, ends.last);
    if (first === null || last === null || first.pos > last.pos) return null;
    const end = last.pos + last.node.nodeSize;
    if (!doc.resolve(first.pos).sameParent(doc.resolve(end))) return null;
    return Selection.fromJSON(doc, { type: HELD_BLOCKS, anchor: first.pos, head: end });
  }

  if (!(before instanceof TextSelection)) return null;
  const anchor = placeOf(before.$anchor);
  const head = placeOf(before.$head);
  if (anchor === null || head === null) return null;
  // Un bout est à suivre quand son texte n'est plus où y-prosemirror a remis la sélection :
  //  - l'élément qui portait ce texte a été supprimé, avec son bloc ou seul : sa position
  //    relative, celle que y-prosemirror avait prise avant le changement reçu, ne mène plus nulle
  //    part. Le bout peut être resté dans son bloc par hasard (le bloc recréé est là où
  //    y-prosemirror a remis la sélection), mais pas à sa place ;
  //  - une annulation l'a sorti de son bloc. Elle remet une autre sélection que celle d'avant :
  //    sa position relative n'est pas lue ;
  //  - l'élément est toujours là, sous un autre id : y-prosemirror a récrit des blocs sur place
  //    (Entrée dans un bloc qui a des sous-blocs : l'élément passe au bloc neuf, avec la fin du
  //    texte ; un onglet sans la forme à clés qui déplace un bloc). « Même id » ne dit alors plus
  //    « même texte » : le bout n'est suivi que si sa place tient dans le bloc qui porte
  //    désormais son id, sans y être rabattue. Sinon son texte est resté dans l'élément (il est
  //    après la coupure), et le bout avec lui.
  const { binding } = ySyncPluginKey.getState(state) as { binding: ProsemirrorBinding };
  const { isUndoRedoOperation } = change.getMeta(ySyncPluginKey) as SyncMeta;
  const relative = isUndoRedoOperation ? null : binding.beforeTransactionSelection;
  const fits = (place: Place) => {
    const content = find(doc, place.id)?.node.firstChild;
    return content != null && place.offset <= content.nodeSize;
  };
  const left = (place: Place, $now: ResolvedPos, was: Y.RelativePosition | undefined) => {
    if (was !== undefined && isGone(was, binding.doc)) return true;
    if (placeOf($now)?.id === place.id) return false;
    return was === undefined || fits(place);
  };
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
      view: () => ({
        // Ce qu'on tient au bout de la souris a été copié au début du glisser. Si le document
        // change pendant qu'on le tient (un autre écrit dans le bloc), c'est ce que les blocs
        // sont devenus qu'il faut déposer : sinon le dépôt remettrait le bloc d'avant.
        update(view, previous) {
          const { dragging, state } = view;
          if (dragging === null || previous.doc === state.doc) return;
          if (kindOf(state.selection) !== HELD_BLOCKS) return;
          view.dragging = { slice: state.selection.content(), move: dragging.move };
        },
      }),
    }),
  ],
});
