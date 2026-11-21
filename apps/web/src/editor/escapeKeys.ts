// Échap dans le texte d'une page (question 37 de la recette du lot 4, choix (a), retenu le
// 2026-11-21 ; R1143). BlockNote 0.57 y retirait le focus de l'éditeur (son extension tiptap
// `OverrideEscape`, `editor.blur()` quand aucun menu de suggestion n'est ouvert) : il tombait sur
// `body`, et la frappe suivante n'allait nulle part. Cette extension est retirée (`disableExtensions`
// d'`EDITOR_OPTIONS`) : sans menu ouvert, Échap ne fait rien, le curseur reste où il est, comme dans
// Notion.
//
// Ce qu'Échap fait ailleurs ne change pas : les menus « / », `@`, `[[` et le sélecteur d'emoji le
// prennent eux-mêmes (ils sont ouverts, et `OverrideEscape` leur laissait déjà la touche) ; le menu
// de la poignée, la fenêtre d'un bloc, le panneau des commentaires, la fenêtre de recherche et la
// fenêtre de l'historique ont le focus quand on l'y tape. Une fenêtre (`dialog`) qui contient un
// éditeur, l'aperçu d'une version, se ferme désormais par Échap tapé dans cet éditeur : la touche
// n'y est plus retenue.
//
// Le seul geste que `OverrideEscape` rendait et qu'il faut garder : la barre de mise en forme, qui
// paraît sur du texte sélectionné, se fermait (le focus parti, la barre partait). Ici, Échap réduit la
// sélection à sa fin : la barre part, le focus reste, et la frappe suivante s'écrit là, sans rien
// remplacer. À relire à chaque montée de BlockNote (le nom `OverrideEscape`, la clé de
// `FormattingToolbarExtension`).
import { createExtension } from "@blocknote/core";
import { FormattingToolbarExtension } from "@blocknote/core/extensions";
import { TextSelection } from "prosemirror-state";

/** Le nom de l'extension tiptap de BlockNote qui retirait le focus sur Échap. */
export const BLUR_ON_ESCAPE = "OverrideEscape";

export const escapeKeys = createExtension({
  key: "escapeKeys",
  keyboardShortcuts: {
    Escape: ({ editor }) => {
      const shown = editor.getExtension(FormattingToolbarExtension)?.store.state;
      const view = editor.prosemirrorView;
      const { selection } = view.state;
      // Une sélection de texte seulement : un bloc sélectionné en entier (une image) reste tel quel.
      if (shown !== true || selection.empty || !(selection instanceof TextSelection)) return false;
      view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, selection.head)));
      return true;
    },
  },
});
