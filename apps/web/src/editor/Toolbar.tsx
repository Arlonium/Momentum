// La barre de mise en forme de l'éditeur de la page : celle de BlockNote, ses boutons dans son
// ordre, à deux différences près.
//
// - Les boutons de fichier (légende, remplacer, renommer, supprimer, télécharger, aperçu) ne vont
//   qu'aux blocs fichier (image, vidéo, son, fichier). BlockNote les donne à tout bloc qui a une
//   prop `url` en chaîne : sur un signet, « Remplacer le fichier » ouvrait son panneau, qui écrit
//   l'adresse telle qu'on la tape, sans la vérifier (vu à l'écran le 2026-11-15). Sur un bloc sans
//   contenu qui n'est pas un fichier (un signet), il ne reste rien : la barre n'est pas montrée.
// - Le choix du type de bloc (« Transformer en ») propose aussi le callout.
import {
  blockTypeSelectItems,
  FormattingToolbar,
  getFormattingToolbarItems,
  useBlockNoteEditor,
  useEditorState,
  type BlockTypeSelectItem,
} from "@blocknote/react";
import { useMemo } from "react";
import { CalloutIcon } from "../icons";
import type { MementumEditor } from "./debug";

/** Les blocs fichier de BlockNote, seuls à garder les boutons de fichier. */
const FILE_BLOCKS: ReadonlySet<string> = new Set(["image", "video", "audio", "file"]);

/** Les clés des boutons de fichier dans `getFormattingToolbarItems` (BlockNote 0.57). */
const FILE_BUTTONS: ReadonlySet<string> = new Set([
  "fileCaptionButton",
  "replaceFileButton",
  "fileRenameButton",
  "fileDeleteButton",
  "fileDownloadButton",
  "filePreviewButton",
]);

/** L'icône du callout dans le choix du type de bloc (BlockNote lui donne une taille). */
const CalloutTypeIcon: BlockTypeSelectItem["icon"] = ({ size }) => (
  <CalloutIcon size={typeof size === "number" ? size : 16} />
);

/** Ce que la barre peut offrir pour la sélection : tout, tout sauf les fichiers, ou rien. */
type Offer = "all" | "no-file" | "nothing";

function offerFor(editor: MementumEditor): Offer {
  const blocks = editor.getSelection()?.blocks ?? [editor.getTextCursorPosition().block];
  if (blocks.length === 1 && FILE_BLOCKS.has(blocks[0].type)) return "all";
  // Un seul bloc sans contenu (un signet, une équation) : aucun bouton de BlockNote ne le règle.
  if (blocks.length === 1 && blocks[0].content === undefined) return "nothing";
  return "no-file";
}

export function MementumFormattingToolbar() {
  const editor = useBlockNoteEditor() as unknown as MementumEditor;
  const offer = useEditorState({ editor, selector: ({ editor }) => offerFor(editor as unknown as MementumEditor) });
  const typeItems = useMemo<BlockTypeSelectItem[]>(
    () => [
      ...blockTypeSelectItems(editor as never),
      { name: "Callout", type: "callout", icon: CalloutTypeIcon },
    ],
    [editor],
  );
  if (offer === "nothing") return null;
  const items = getFormattingToolbarItems(typeItems).filter(
    (item) => offer === "all" || !FILE_BUTTONS.has(String(item.key)),
  );
  return <FormattingToolbar>{items}</FormattingToolbar>;
}
