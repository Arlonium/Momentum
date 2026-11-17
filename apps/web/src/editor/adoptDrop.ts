// Un bloc glissé depuis un autre onglet (Task C9, tour de correction 1, R714) : déposé dans la page, il
// porte l'adresse du fichier de sa première page, comme un bloc collé. Ce greffon voit le dépôt avant
// que l'éditeur pose les blocs, relève ceux qui nomment déjà les fichiers que porte le glisser, puis,
// une fois le dépôt traité, fait rattacher à la page les blocs neufs (`watchPosed`, `adopt.ts`).
//
// Seulement pour un glisser venu d'ailleurs (`view.dragging` est nul : pas celui de cet éditeur, où
// rien n'est à recopier et où un bloc déplacé est recréé au même id, R223), qui porte un bloc de
// Mementum (`blocknote/html`) et la marque de provenance de ce compte dans ce navigateur, pour des
// fichiers de la page d'où il est glissé (`carriedFiles`, R746 : un bloc glissé depuis un autre site
// n'a pas cette marque, et ne fait rien recopier), dans un éditeur qui écrit. Il ne retient jamais
// l'événement : le dépôt reste celui de BlockNote et de ProseMirror.
import { createExtension } from "@blocknote/core";
import { Plugin, PluginKey } from "prosemirror-state";
import { carriedFiles, unvouchedFiles, watchPosed, type AdoptOptions } from "./adopt";
import type { MementumEditor } from "./debug";

const adoptDropKey = new PluginKey("mementum-adopt-drop");

/**
 * L'extension de l'éditeur d'une page qu'on modifie. `editor` : l'éditeur, une fois créé.
 */
export function dropAdoption(editor: () => MementumEditor | null, options: AdoptOptions) {
  return createExtension({
    key: "adoptDrop",
    prosemirrorPlugins: [
      new Plugin({
        key: adoptDropKey,
        props: {
          handleDOMEvents: {
            drop(view, event) {
              const current = editor();
              if (current === null || !current.isEditable || view.dragging != null) return false;
              const data = (event as DragEvent).dataTransfer;
              const foreign = carriedFiles(data, options.pageId, options.copier("drag"));
              const unvouched = unvouchedFiles(data, options.pageId, foreign);
              if (foreign.size === 0 && unvouched.size === 0) return false;
              const posed = watchPosed(current, foreign, options, unvouched);
              // Une fois le dépôt traité par tous ses écouteurs (une tâche plus tard).
              setTimeout(posed, 0);
              return false;
            },
          },
        },
      }),
    ],
  });
}
