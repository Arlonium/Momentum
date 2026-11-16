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
import type { MementumEditor } from "./debug";

type Block = MementumEditor["document"][number];
type PartialBlock = Parameters<MementumEditor["insertBlocks"]>[0][number];

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

/**
 * Insère, juste après le dernier des blocs `ids` (donnés dans l'ordre du document, comme les rend
 * `getSelection`), leurs copies dans le même ordre, sans aucun id de bloc ; pose le curseur à la fin
 * de la première copie (`setTextCursorPosition` : la fin de son texte ; pour une liste de colonnes, la
 * fin du dernier texte qu'elle porte ; une image est sélectionnée) ; rend les ids neufs des copies. Un id qui n'est plus dans le document (un autre
 * vient de supprimer le bloc) est passé, un id répété copié une fois. Une seule transaction : une
 * écriture, et Ctrl+Z la défait d'un coup.
 */
export function duplicateBlocks(editor: MementumEditor, ids: string[]): string[] {
  const blocks = [...new Set(ids)]
    .map((id) => editor.getBlock(id))
    .filter((found): found is Block => found !== undefined);
  if (blocks.length === 0) return [];
  return editor.transact(() => {
    const made = editor.insertBlocks(blocks.map(withoutIds), blocks[blocks.length - 1], "after");
    editor.setTextCursorPosition(made[0], "end");
    return made.map((copy) => copy.id);
  });
}

/**
 * Les blocs que vise Ctrl+D : ceux de la sélection, sinon celui du curseur (le choix de BlockNote
 * pour ses propres gestes, comme le déplacement au clavier). Un bloc sélectionné par un clic (une
 * image : une sélection de nœud) est celui du curseur.
 */
export function selectedBlockIds(editor: MementumEditor): string[] {
  const blocks = editor.getSelection()?.blocks ?? [editor.getTextCursorPosition().block];
  return blocks.map((selected) => selected.id);
}

/**
 * Les blocs que vise « Dupliquer » du menu de la poignée du bloc `id` : la sélection si elle le
 * contient, sinon lui seul (la règle de « Supprimer » de BlockNote, `RemoveBlockItem`).
 */
export function handleBlockIds(editor: MementumEditor, id: string): string[] {
  const selected = editor.getSelection()?.blocks;
  return selected?.some((block) => block.id === id) ? selected.map((block) => block.id) : [id];
}
