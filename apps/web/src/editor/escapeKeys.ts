// Échap dans le texte d'une page (question 37 de la recette du lot 4, choix (a), retenu le
// 2026-11-21 ; R1143). BlockNote 0.57 y retirait le focus de l'éditeur (son extension tiptap
// `OverrideEscape`, `editor.blur()` quand aucun menu de suggestion n'est ouvert) : il tombait sur
// `body`, et la frappe suivante n'allait nulle part. Cette extension est retirée (`disableExtensions`
// d'`EDITOR_OPTIONS`) : sans menu ouvert, Échap ne fait rien, le curseur reste où il est, comme dans
// Notion.
//
// Ce qu'Échap fait ailleurs ne change pas. La barre de mise en forme, celle des liens et les menus
// « / », `@`, `[[` et d'emoji se ferment par `useDismiss` de floating-ui, pas par `OverrideEscape`
// (qui laissait déjà la touche aux menus de suggestion ouverts) ; le menu de la poignée, la fenêtre
// d'un bloc, le panneau des commentaires et la fenêtre de recherche ont le focus quand on l'y tape.
//
// `escapeKeys` n'ajoute qu'une chose (R1584, récrit par R1593) : sur une sélection de texte, la barre
// de mise en forme montrée, Échap réduit la sélection à sa tête (là où elle s'arrête, le bout que la
// personne déplaçait), et prend la touche ; le focus reste au texte, et la frappe suivante s'écrit
// là, sans rien remplacer. Un bloc sélectionné en entier (une image) reste tel quel.
//
// Sortir du texte au clavier sans rien écrire, depuis que Échap ne le fait plus : Ctrl+K (la
// recherche) et Ctrl+Alt+M (commenter le bloc), qui rendent le focus en se fermant, et Maj+Tab depuis
// un bloc de premier niveau. À relire à chaque montée de BlockNote (le nom `OverrideEscape`, la clé
// de `FormattingToolbarExtension`).
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
