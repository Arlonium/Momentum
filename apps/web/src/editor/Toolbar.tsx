// La barre de mise en forme de l'éditeur de la page : celle de BlockNote, ses boutons dans son
// ordre, à deux différences près.
//
// - Les boutons de fichier (légende, remplacer, renommer, supprimer, télécharger, aperçu) ne vont
//   qu'aux blocs fichier (image, vidéo, son, fichier). BlockNote les donne à tout bloc qui a une
//   prop `url` en chaîne : sur un signet, « Remplacer le fichier » ouvrait son panneau, qui écrit
//   l'adresse telle qu'on la tape, sans la vérifier (vu à l'écran le 2026-11-15). Sur un bloc sans
//   contenu qui n'est pas un fichier (un signet), il ne reste rien : la barre n'est pas montrée.
// - Le choix du type de bloc (« Transformer en ») propose aussi le callout.
// - « Télécharger » est le nôtre (`DownloadButton`, R681) : celui de BlockNote ouvrait l'adresse par
//   `window.open(adresse)`, avec son ouvreur, et le site ouvert pouvait emmener l'onglet de Mementum
//   ailleurs. Le nôtre fait ce que fait le lien d'un bloc fichier (`followFile`,
//   `blocks/FileBlock.tsx`), et n'est pas montré pour une adresse qui n'est pas permise.
import {
  blockTypeSelectItems,
  FormattingToolbar,
  getFormattingToolbarItems,
  useBlockNoteEditor,
  useComponentsContext,
  useDictionary,
  useEditorState,
  type BlockTypeSelectItem,
} from "@blocknote/react";
import { useMemo } from "react";
import { CalloutIcon, DownloadIcon } from "../icons";
import { followFile } from "./blocks/FileBlock";
import { textProp } from "./blocks/props";
import type { MementumEditor } from "./debug";
import { BLANK, FILE_BLOCKS, shownUrl } from "./files";

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

/** La clé du bouton « Télécharger » de BlockNote, que le nôtre remplace. */
const DOWNLOAD = "fileDownloadButton";

/** Le bloc de fichier sélectionné, seul : sa sorte, son adresse, son nom ; sinon `null`. */
function selectedFile(editor: MementumEditor): { type: string; url: string; name: string } | null {
  const blocks = editor.getSelection()?.blocks ?? [editor.getTextCursorPosition().block];
  if (blocks.length !== 1 || !FILE_BLOCKS.has(blocks[0].type)) return null;
  const props = blocks[0].props as Readonly<Record<string, unknown>>;
  return { type: blocks[0].type, url: textProp(props, "url"), name: textProp(props, "name") };
}

/**
 * « Télécharger » : un fichier de l'instance est téléchargé sans quitter la page, une adresse web
 * s'ouvre dans un autre onglet sans son ouvreur (`followFile`). Pas de bouton pour un bloc sans
 * adresse, ni pour une adresse qui n'est pas permise.
 */
function DownloadButton() {
  const Components = useComponentsContext()!;
  const dictionary = useDictionary();
  const editor = useBlockNoteEditor() as unknown as MementumEditor;
  // Une chaîne : l'état ne change que si le bloc, son adresse ou son nom changent.
  const selected = useEditorState({
    editor,
    selector: ({ editor }) => JSON.stringify(selectedFile(editor as unknown as MementumEditor)),
  });
  const file = JSON.parse(selected) as ReturnType<typeof selectedFile>;
  if (file === null || file.url === "" || shownUrl(file.url) === BLANK) return null;
  const tooltips = dictionary.formatting_toolbar.file_download.tooltip as Record<string, string>;
  const label = tooltips[file.type] ?? tooltips.file;
  return (
    <Components.FormattingToolbar.Button
      className="bn-button"
      label={label}
      mainTooltip={label}
      icon={<DownloadIcon size={18} />}
      onClick={() => {
        editor.focus();
        followFile(file.url, file.name);
      }}
    />
  );
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
  const items = getFormattingToolbarItems(typeItems)
    .filter((item) => offer === "all" || !FILE_BUTTONS.has(String(item.key)))
    .map((item) => (item.key === DOWNLOAD ? <DownloadButton key={DOWNLOAD} /> : item));
  return <FormattingToolbar>{items}</FormattingToolbar>;
}
