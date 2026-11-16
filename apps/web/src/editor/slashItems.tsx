// Le menu « / » de l'éditeur de la page : celui de BlockNote (titres, listes, code, tableau,
// médias…), les colonnes, puis le groupe « Blocs riches », dans cet ordre au fil des tâches :
// Callout, Équation, Équation en ligne, Signet (C1b, C2) ; Table des matières, Lien vers une page
// (C3). Chaque élément a ses noms en français et en anglais.
//
// Choisir un bloc riche fait le geste de BlockNote (R308, récrit) : dans un bloc vide, ou qui ne
// porte que « / », ce bloc devient celui qu'on a choisi (il garde son id ; une frappe faite au même
// instant dans ce bloc par quelqu'un d'autre est perdue, R303) ; ailleurs, il est inséré après.
// Un signet s'insère vide et ouvre sa fenêtre, sur son champ d'adresse ; fermée, elle rend le focus
// au texte, dans le paragraphe qui suit le signet, où la frappe reprend.
import { combineByGroup, insertOrUpdateBlockForSlashMenu } from "@blocknote/core";
import { getDefaultReactSlashMenuItems, type DefaultReactSuggestionItem } from "@blocknote/react";
import { getMultiColumnSlashMenuItems } from "@blocknote/xl-multi-column";
import { BookmarkIcon, CalloutIcon, EquationIcon, InlineEquationIcon } from "../icons";
import { openBlockWindow } from "./blocks/blockWindows";
import { openInlineMath } from "./blocks/InlineMath";
import type { MementumEditor } from "./debug";

/** Le groupe des blocs riches, le dernier du menu. */
export const RICH_GROUP = "Blocs riches";

/** Les blocs riches du menu « / », dans leur ordre. */
export function richSlashItems(editor: MementumEditor): DefaultReactSuggestionItem[] {
  return [
    {
      title: "Callout",
      subtext: "Un encadré, avec une icône et une couleur",
      aliases: ["encadré", "encadre", "note", "astuce", "info", "avertissement", "callout", "aside", "tip", "notice"],
      group: RICH_GROUP,
      icon: <CalloutIcon size={18} />,
      onItemClick: () => {
        insertOrUpdateBlockForSlashMenu(editor, { type: "callout" });
      },
    },
    {
      title: "Équation",
      subtext: "Une formule mathématique, centrée sur sa ligne (LaTeX)",
      aliases: ["equation", "formule", "maths", "math", "latex", "katex", "tex"],
      group: RICH_GROUP,
      icon: <EquationIcon />,
      onItemClick: () => {
        const block = insertOrUpdateBlockForSlashMenu(editor, { type: "equation" });
        // Posée depuis le texte : le focus y revient, à la fermeture de sa fenêtre (R474).
        openBlockWindow(editor, block, "editor");
      },
    },
    {
      title: "Équation en ligne",
      subtext: "Une formule dans le texte (LaTeX)",
      aliases: ["equation en ligne", "formule en ligne", "inline", "math", "latex", "katex", "tex"],
      group: RICH_GROUP,
      icon: <InlineEquationIcon />,
      onItemClick: () => {
        editor.insertInlineContent([{ type: "math", props: { latex: "" } }]);
        // Le curseur est juste après la formule posée ; sa vue arrive au rendu suivant.
        openInlineMath(editor, { pos: editor.prosemirrorState.selection.from - 1 }, "editor");
      },
    },
    {
      title: "Signet",
      subtext: "Une page web, montrée par son adresse",
      aliases: ["lien", "adresse", "url", "site", "bookmark", "link", "web"],
      group: RICH_GROUP,
      icon: <BookmarkIcon />,
      onItemClick: () => {
        const block = insertOrUpdateBlockForSlashMenu(editor, { type: "bookmark" });
        // Posé depuis le texte : le focus y revient, dans le paragraphe qui suit le signet (R474).
        openBlockWindow(editor, block, "editor");
      },
    },
  ];
}

/** Tout le menu « / » : BlockNote, les colonnes, puis les blocs riches. */
export function slashMenuItems(editor: MementumEditor): DefaultReactSuggestionItem[] {
  return combineByGroup(
    getDefaultReactSlashMenuItems(editor),
    getMultiColumnSlashMenuItems(editor),
    richSlashItems(editor),
  );
}
