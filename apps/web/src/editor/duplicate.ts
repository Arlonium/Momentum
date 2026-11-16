// Dupliquer des blocs : Ctrl+D (Cmd+D) et « Dupliquer » dans le menu de la poignée (Task C5,
// R253). Branché dans `Editor.tsx`.
//
// Une copie ne reprend **aucun identifiant de bloc**, à aucune profondeur (enfants, colonnes) :
// BlockNote en tire de neufs à l'insertion. Avec l'identifiant de l'original, le document porterait
// deux blocs du même id, le dédoublonnage du serveur supprimerait le second, la copie, chez tout le
// monde (`crates/mementum-crdt/src/dedupe.rs`), et l'éditeur qui la reçoit récrirait l'original
// sous un id neuf, ce qui détache ses fils de commentaires (mesuré : `gestures.test.ts`,
// `a_copy_that_kept_its_id_is_not_a_copy`). L'`id` d'une mention (une page, une personne, une
// date) est une prop d'un contenu en ligne, pas celui d'un bloc : il reste.
//
// La copie est une insertion, qui ne touche aucun élément existant : une frappe faite au même
// instant dans l'original ou dans le bloc suivant est gardée (`duplicating_touches_no_existing_element`).
// Ce n'est pas un réordonnancement : le correctif de y-prosemirror n'y joue pas (R223).
//
// Les blocs visés ne sont pas toujours frères (une sélection qui sort d'une colonne, ou qui part
// d'un sous-bloc) : la copie remonte à leur parent commun (R640). Elle lit la page en un parcours de
// ses seuls nœuds de structure, jamais un bloc par son id (`getBlock` reparcourt la page à chaque
// appel), et copie au plus `MAX_DUPLICATED_BLOCKS` blocs par geste (R641).
import { getNearestBlockPos, nodeToBlock } from "@blocknote/core";
import type { MementumEditor } from "./debug";
import { announce } from "./editorNotice";
import type { MementumSchema } from "./schema";

type Block = MementumEditor["document"][number];
type PartialBlock = Parameters<MementumEditor["insertBlocks"]>[0][number];
type PmNode = MementumEditor["prosemirrorState"]["doc"];

/** Le plus de blocs qu'un geste copie, sous-blocs compris (R641). */
export const MAX_DUPLICATED_BLOCKS = 1_000;

/** Ce que l'éditeur dit d'une copie de trop de blocs. */
export const TOO_MANY_BLOCKS =
  "Trop de blocs à dupliquer : 1 000 au plus d'un coup, sous-blocs compris. Rien n'a été copié.";

/** La zone d'alerte de ce message (`editorNotice.ts`). */
const COPY_REFUSED = "mm-copy-refused";

/** Ce que la reconnaissance lit d'un événement clavier (du DOM, ou de React). */
export type DuplicateKey = {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  getModifierState(modifier: string): boolean;
};

/**
 * Ctrl+D ou Cmd+D : la lettre tapée est « d » (`key`, jamais la position de la touche, `code` :
 * sur un autre clavier, la touche marquée D d'un QWERTY tape une autre lettre, et une autre touche
 * tape « d »), avec Ctrl ou Cmd, l'un des deux seulement (Ctrl+Cmd+D est « Rechercher » sur Mac),
 * sans Alt, sans Maj, et jamais sous AltGr (Ctrl+Alt sous Windows : un caractère tapé).
 */
export function isDuplicateShortcut(event: DuplicateKey): boolean {
  if (event.ctrlKey === event.metaKey) return false;
  if (event.altKey || event.shiftKey || event.getModifierState("AltGraph")) return false;
  return event.key.toLowerCase() === "d";
}

/** `block` sans son identifiant, ni celui d'aucun bloc qu'il contient. */
function withoutIds({ id: _id, children, ...rest }: Block): PartialBlock {
  return { ...rest, children: children.map(withoutIds) } as PartialBlock;
}

/** Les nœuds qui portent l'id d'un bloc : le conteneur, et la liste de colonnes et la colonne, que
 * BlockNote n'enveloppe pas dans un conteneur. */
const BLOCK_NODES = new Set(["blockContainer", "columnList", "column"]);

/**
 * Les blocs `ids` de `doc`, chacun par la chaîne de ses blocs, du premier niveau jusqu'à lui, dans
 * l'ordre du document. Un parcours des seuls nœuds de structure (ni textes, ni contenus en ligne),
 * qui s'arrête quand tous sont trouvés.
 */
function chainsOf(doc: PmNode, ids: Set<string>): PmNode[][] {
  const chains: PmNode[][] = [];
  const stack: PmNode[] = [];
  const walk = (node: PmNode): void => {
    for (let i = 0; i < node.childCount && chains.length < ids.size; i += 1) {
      const child = node.child(i);
      if (BLOCK_NODES.has(child.type.name)) {
        stack.push(child);
        if (ids.has(String(child.attrs.id))) chains.push([...stack]);
        walk(child);
        stack.pop();
      } else if (child.type.name === "blockGroup") {
        walk(child);
      }
    }
  };
  walk(doc);
  return chains;
}

/**
 * Les blocs visés, ramenés à des frères (R640) : chacun remonte à l'enfant direct de leur plus proche
 * parent commun (un bloc parent d'un autre visé reste lui-même) ; si ces frères sont des colonnes,
 * c'est leur liste de colonnes (une colonne ne s'insère que dans une liste, un bloc jamais dans une
 * liste) ; chaque frère une fois, dans l'ordre du document.
 */
function siblingsOf(chains: PmNode[][]): PmNode[] {
  const shortest = chains.reduce((least, chain) => Math.min(least, chain.length), Infinity);
  let level = 0;
  while (level < shortest - 1 && chains.every((chain) => chain[level] === chains[0][level])) level += 1;
  if (chains[0][level].type.name === "column") level -= 1;
  return [...new Set(chains.map((chain) => chain[level]))];
}

/** Le nombre de blocs de `nodes`, sous-blocs compris, compté jusqu'à `limit + 1` au plus. */
function countBlocks(nodes: PmNode[], limit: number): number {
  let count = 0;
  for (const node of nodes) {
    count += 1;
    node.descendants((child) => {
      if (count > limit) return false;
      if (BLOCK_NODES.has(child.type.name)) count += 1;
      return BLOCK_NODES.has(child.type.name) || child.type.name === "blockGroup";
    });
    if (count > limit) break;
  }
  return count;
}

/**
 * Insère, juste après le dernier des blocs `ids` dans le document, leurs copies dans l'ordre du
 * document, sans aucun id de bloc, en une insertion (une écriture, que Ctrl+Z défait d'un coup) ; les
 * blocs qui ne sont pas frères remontent d'abord à leur parent commun (`siblingsOf`, R640) ; pose le
 * curseur à la fin de la première copie (`setTextCursorPosition` : la fin de son texte ; pour une
 * liste de colonnes, la fin du dernier texte qu'elle porte ; une image est sélectionnée) ; rend les
 * ids neufs des copies. Un id qui n'est plus dans le document (un autre vient de supprimer le bloc)
 * est passé, un id répété copié une fois. Plus de `MAX_DUPLICATED_BLOCKS` blocs, sous-blocs compris :
 * rien n'est copié, et un message le dit (R641). BlockNote peut encore refuser l'insertion : elle
 * lève alors, sans rien écrire (`duplicateFromGesture` l'attrape).
 */
export function duplicateBlocks(editor: MementumEditor, ids: string[]): string[] {
  const doc = editor.prosemirrorState.doc;
  const chains = chainsOf(doc, new Set(ids));
  if (chains.length === 0) return [];
  const siblings = siblingsOf(chains);
  if (countBlocks(siblings, MAX_DUPLICATED_BLOCKS) > MAX_DUPLICATED_BLOCKS) {
    announce(COPY_REFUSED, TOO_MANY_BLOCKS);
    return [];
  }
  const blocks = siblings.map((node) =>
    nodeToBlock<MementumSchema["blockSchema"], MementumSchema["inlineContentSchema"], MementumSchema["styleSchema"]>(
      node,
      doc,
    ),
  );
  const last = String(siblings[siblings.length - 1].attrs.id);
  const made = editor.insertBlocks(blocks.map(withoutIds), last, "after");
  editor.setTextCursorPosition(made[0], "end");
  return made.map((copy) => copy.id);
}

/**
 * La copie d'un geste (Ctrl+D, « Dupliquer » du menu de la poignée) : les blocs de `targets`, lus au
 * moment du geste. Une exception (BlockNote qui refuse une insertion ou une sélection) ne sort jamais
 * du gestionnaire : elle est écrite dans la console, rien n'est copié, et la fonction rend faux, pour
 * que la touche suive son cours et que le menu se ferme ; vrai sinon, même quand il n'y avait rien à
 * copier ou trop (le message le dit alors).
 */
export function duplicateFromGesture(editor: MementumEditor, targets: () => string[]): boolean {
  try {
    duplicateBlocks(editor, targets());
    return true;
  } catch (error) {
    console.error("Copie de blocs refusée :", error);
    return false;
  }
}

/**
 * Les ids des blocs de la sélection, ceux que rend `getSelection()` de BlockNote, par sa règle (relue
 * dans son code, BlockNote 0.57 : `getSelection`, appelée par `SelectionManager`) mais sans convertir
 * les blocs : `getSelection` convertit chaque bloc choisi et ses sous-blocs (`nodeToBlock`), près
 * d'une demi-seconde pour toute une page de 10 000 blocs, avant même de compter (R641). `null` : pas
 * de sélection de blocs (un curseur, une sélection de nœud). À relire à chaque montée de version de
 * BlockNote ; `selected_ids_are_those_of_the_blocknote_selection_read_without_converting_blocks` les
 * compare.
 */
function selectionIds(editor: MementumEditor): string[] | null {
  const { doc, selection } = editor.prosemirrorState;
  if (selection.empty || "node" in selection) return null;
  const start = doc.resolve(getNearestBlockPos(doc, selection.from).posBeforeNode);
  const end = doc.resolve(getNearestBlockPos(doc, selection.to).posBeforeNode);
  // Le bloc de rang `index` sous le nœud de profondeur `depth` : lu par `child`, en temps constant ;
  // `getSelection` passe par `posAtIndex`, qui additionne les tailles des frères d'avant, à chaque bloc.
  const idAt = (index: number, depth: number) => String(start.node(depth).child(index).attrs.id);
  const shared = start.sharedDepth(end.pos);
  const first = start.index(shared);
  const last = end.index(shared);
  const ids: string[] = [];
  if (start.depth > shared) {
    ids.push(String(start.nodeAfter?.attrs.id));
    for (let depth = start.depth; depth > shared; depth -= 1) {
      if (!start.node(depth).type.isInGroup("childContainer")) continue;
      for (let i = start.index(depth) + 1; i < start.node(depth).childCount; i += 1) ids.push(idAt(i, depth));
    }
  } else {
    ids.push(idAt(first, shared));
  }
  for (let i = first + 1; i <= last; i += 1) ids.push(idAt(i, shared));
  return ids;
}

/**
 * Les blocs que vise Ctrl+D : ceux de la sélection, sinon celui du curseur (le choix de BlockNote
 * pour ses propres gestes, comme le déplacement au clavier). Un bloc sélectionné par un clic (une
 * image : une sélection de nœud) est celui du curseur.
 */
export function selectedBlockIds(editor: MementumEditor): string[] {
  return selectionIds(editor) ?? [editor.getTextCursorPosition().block.id];
}

/**
 * Les blocs que vise « Dupliquer » du menu de la poignée du bloc `id` : la sélection si elle le
 * contient, sinon lui seul (la règle de « Supprimer » de BlockNote, `RemoveBlockItem`).
 */
export function handleBlockIds(editor: MementumEditor, id: string): string[] {
  const selected = selectionIds(editor);
  return selected?.includes(id) ? selected : [id];
}
