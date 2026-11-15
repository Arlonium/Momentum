// Le menu « / » de l'éditeur de la page : celui de BlockNote (titres, listes, code, tableau,
// médias…), les colonnes, puis le groupe « Blocs riches », dans cet ordre au fil des tâches :
// Callout, Signet (C1b) ; Équation, Équation en ligne (C2, entre les deux) ; Table des matières,
// Lien vers une page (C3). Chaque élément a ses noms en français et en anglais.
//
// Choisir un bloc riche fait le geste de BlockNote (R308, récrit) : dans un bloc vide, ou qui ne
// porte que « / », ce bloc devient celui qu'on a choisi (il garde son id ; une frappe faite au même
// instant dans ce bloc par quelqu'un d'autre est perdue, R303) ; ailleurs, il est inséré après.
// Un signet s'insère vide et ouvre sa fenêtre, sur son champ d'adresse.
import { combineByGroup, insertOrUpdateBlockForSlashMenu } from "@blocknote/core";
import { getDefaultReactSlashMenuItems, type DefaultReactSuggestionItem } from "@blocknote/react";
import { getMultiColumnSlashMenuItems } from "@blocknote/xl-multi-column";
import { BookmarkIcon, CalloutIcon } from "../icons";
import { openBlockWindow } from "./blocks/blockWindows";
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
      title: "Signet",
      subtext: "Une page web, montrée par son adresse",
      aliases: ["lien", "adresse", "url", "site", "bookmark", "link", "web"],
      group: RICH_GROUP,
      icon: <BookmarkIcon />,
      onItemClick: () => {
        const block = insertOrUpdateBlockForSlashMenu(editor, { type: "bookmark" });
        openBlockWindow(editor, block.id);
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
