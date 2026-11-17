// Ce que l'éditeur d'une page fait d'un envoi de fichier (Task C8, tour de correction 1) : qui reçoit
// de quoi envoyer, et ce que devient le bloc d'un envoi qui n'a pas abouti. L'envoi lui-même est dans
// `files.ts`, qui ne sait rien de l'éditeur.
import { textProp } from "./blocks/props";
import type { MementumEditor } from "./debug";
import { FILE_BLOCKS, fileLimits, uploader, type UploaderOptions } from "./files";

/** Ce que la page donne à l'éditeur pour envoyer : ce qu'elle dit d'un envoi, et de quoi la relire. */
export type PageUploads = Pick<UploaderOptions, "onRefused" | "reread">;

/**
 * De quoi envoyer un fichier, pour l'éditeur de `session` (l'option `uploadFile` de BlockNote), ou
 * `undefined` : **une session en lecture seule n'envoie jamais rien** (lecteur, corbeille, onglet
 * d'un autre schéma, R35), que la page donne de quoi ou non ; sans l'option, BlockNote n'a ni onglet
 * d'envoi dans son panneau de fichier, ni envoi au dépôt ou au collage. `files` est lu à chaque envoi
 * (la page peut en changer sans recréer l'éditeur) ; `editor` : l'éditeur de la session, une fois créé.
 */
export function sessionUploader(
  session: { readOnly: boolean; pageId: string },
  files: () => PageUploads | null,
  editor: () => MementumEditor | null,
  signal: AbortSignal,
): ReturnType<typeof uploader> | undefined {
  if (session.readOnly || files() === null) return undefined;
  return uploader({
    pageId: session.pageId,
    limits: fileLimits,
    blockType: (id) => editor()?.getBlock(id)?.type,
    onRefused: (message) => files()?.onRefused(message),
    reread: () => files()?.reread() ?? Promise.resolve(undefined),
    discard: (id, fileName) => {
      const current = editor();
      if (current !== null) discardUpload(current, id, fileName);
    },
    signal,
  });
}

/**
 * Retire le bloc `blockId` d'un envoi de `fileName` qui n'a pas abouti, **s'il est encore tel que
 * l'envoi l'a laissé** (R685) : un bloc de fichier sans adresse, sans légende, sans sous-bloc, qui
 * porte le nom du fichier. C'est le bloc que BlockNote pose au dépôt ou au collage d'un fichier, avant
 * de l'envoyer : son nom, celui du fichier sur le poste de la personne, est dans le document, lu par
 * tous, et y restait après un refus ou une annulation. Le seul bloc d'une page redevient un paragraphe
 * vide (le dépôt en avait changé le type). Tout autre bloc reste : un bloc qui a changé depuis (une
 * adresse, une légende, un autre nom, une autre sorte), ou qui était là avant l'envoi (posé par le
 * menu « / », il n'a pas de nom). Rien dans un éditeur qui n'écrit plus.
 */
export function discardUpload(editor: MementumEditor, blockId: string, fileName: string): void {
  if (fileName === "" || !editor.isEditable) return;
  const block = editor.getBlock(blockId);
  if (block === undefined || !FILE_BLOCKS.has(block.type) || block.children.length > 0) return;
  const props = block.props as Readonly<Record<string, unknown>>;
  if (textProp(props, "url") !== "" || textProp(props, "caption") !== "" || props.name !== fileName) return;
  try {
    editor.removeBlocks([blockId]);
  } catch {
    // Le seul bloc de la page : ProseMirror n'admet pas un document vide.
  }
  if (editor.getBlock(blockId) === undefined) return;
  try {
    editor.updateBlock(blockId, { type: "paragraph", props: {} });
  } catch (error) {
    console.error("Le bloc d'un envoi qui n'a pas abouti n'a pas pu être retiré :", error);
  }
}
