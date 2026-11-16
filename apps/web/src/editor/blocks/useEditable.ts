// L'éditeur est-il modifiable ? Lu par les blocs riches pour montrer, ou non, le bouton qui règle le
// bloc. L'éditeur de la page est verrouillé jusqu'à sa première synchro, puis déverrouillé sans que
// sa vue soit remontée (`Editor.tsx`) : la vue d'un bloc, rendue avant, ne le saurait pas sans
// suivre cet état. Il est suivi dans la seule réserve de l'extension « lecture seule » de BlockNote,
// qui ne change qu'avec lui : rien n'est relu à chaque frappe, dans aucun bloc.
//
// `useEditable` pour une vue rendue par React ; `watchEditable` pour une vue sans React (le callout,
// le signet, le lien de page : `richView.ts`, R626), même règle.
import { ReadOnlyExtension } from "@blocknote/core/extensions";
import { useExtensionState } from "@blocknote/react";
import type { MementumEditor } from "../debug";

type ReadOnlyState = { isEditable: boolean; enabledSet: ReadonlySet<string> };

const editableIn = (state: ReadOnlyState) => state.isEditable && state.enabledSet.size === 0;

export function useEditable(editor: MementumEditor): boolean {
  return useExtensionState(ReadOnlyExtension, { editor, selector: editableIn });
}

/**
 * L'éditeur est-il modifiable, et `listener` à chaque fois que la réponse change, jusqu'à `stop`.
 * Pour une vue de bloc sans React (`richView.ts`), qui s'abonne quand ProseMirror la crée et se
 * désabonne quand il la détruit.
 */
export function watchEditable(
  editor: MementumEditor,
  listener: (editable: boolean) => void,
): { editable: boolean; stop: () => void } {
  const store = editor.getExtension(ReadOnlyExtension)!.store;
  let editable = editableIn(store.state);
  const stop = store.subscribe(({ currentVal }) => {
    const now = editableIn(currentVal);
    if (now === editable) return;
    editable = now;
    listener(now);
  });
  return { editable, stop };
}

/** L'éditeur est-il modifiable, maintenant (le rendu d'une vue sans React hors de l'éditeur). */
export function editableNow(editor: MementumEditor): boolean {
  return editableIn(editor.getExtension(ReadOnlyExtension)!.store.state);
}
