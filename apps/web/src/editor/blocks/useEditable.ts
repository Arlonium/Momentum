// L'éditeur est-il modifiable ? Lu par les blocs riches pour montrer, ou non, le bouton qui règle le
// bloc. L'éditeur de la page est verrouillé jusqu'à sa première synchro, puis déverrouillé sans que
// sa vue soit remontée (`Editor.tsx`) : la vue d'un bloc, rendue avant, ne le saurait pas sans
// suivre cet état. Il est suivi dans la seule réserve de l'extension « lecture seule » de BlockNote,
// qui ne change qu'avec lui : rien n'est relu à chaque frappe, dans aucun bloc.
import { ReadOnlyExtension } from "@blocknote/core/extensions";
import { useExtensionState } from "@blocknote/react";
import type { MementumEditor } from "../debug";

export function useEditable(editor: MementumEditor): boolean {
  return useExtensionState(ReadOnlyExtension, {
    editor,
    selector: (state) => state.isEditable && state.enabledSet.size === 0,
  });
}
