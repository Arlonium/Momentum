// « Rétablir » sans rien à rétablir ne fait rien (Rulings R731 à R735).
//
// BlockNote 0.57 lie Ctrl+Maj+Z (`Shift-Mod-z`) à `editor.redo()` (`KeyboardShortcutsExtension`), qui
// rend faux quand la pile de rétablissement de Yjs est vide. prosemirror-keymap 1.2.3
// (`keydownHandler`) essaie alors, dans la même table, la touche sans Maj quand `event.key` est une
// lettre : `Mod-z`, l'annulation. Un navigateur donne la lettre en minuscule avec Maj quand Verr. Maj.
// est activé (sous Windows et Linux ; pas sous macOS, où w3c-keyname relit la lettre d'après son code),
// et Playwright aussi (`press("Control+Shift+z")`) : « rétablir » retirait alors la dernière frappe, et
// un second « rétablir » la rendait (R731). Ctrl+Y n'a pas ce défaut : sans Maj, pas de repli.
//
// Cette table passe avant celle de BlockNote (une extension de BlockNote a une priorité d'au moins 91,
// sa table de raccourcis 50) : elle rétablit s'il y a de quoi, et prend la touche dans tous les cas
// (R732). Ni la table de BlockNote ni le « rétablir » natif du navigateur, que ProseMirror laisse
// passer pour Ctrl+Maj+Z quand personne ne prend la touche (`captureKeyDown` ne retient que Ctrl+Z et
// Ctrl+Y), ne la reçoivent plus.
import { createExtension } from "@blocknote/core";

/** L'extension de l'éditeur de la page. */
export const redoKeys = createExtension({
  key: "redoKeys",
  keyboardShortcuts: {
    "Shift-Mod-z": ({ editor }) => {
      editor.redo();
      return true;
    },
  },
});
