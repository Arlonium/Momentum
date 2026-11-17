// Ce que l'éditeur d'une page fait d'un envoi de fichier (Task C8, tour de correction 1) : qui reçoit
// de quoi envoyer, et ce que devient le bloc d'un envoi qui n'a pas abouti (il reste, sans le nom du
// fichier). L'envoi lui-même est dans
// `files.ts`, qui ne sait rien de l'éditeur.
import { copyFile, expectedAccount } from "../api";
import { adoptingPaste, stampGestures, type AdoptOptions } from "./adopt";
import { dropAdoption } from "./adoptDrop";
import { textProp } from "./blocks/props";
import type { MementumEditor } from "./debug";
import { fileLimits, uploader, type UploaderOptions } from "./files";
import { copier } from "./provenance";

/** Ce que la page donne à l'éditeur pour envoyer : ce qu'elle dit d'un envoi, et de quoi la relire. */
export type PageUploads = Pick<UploaderOptions, "onRefused" | "reread"> & {
  /** Un fichier d'une autre page a été rattaché à celle-ci : ce que la page en dit (Task C9). */
  onAdopted?: (message: string) => void;
};

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
      if (current !== null) forgetUploadName(current, id, fileName);
    },
    signal,
  });
}

/**
 * Le collage de l'éditeur de `session` (l'option `pasteHandler` de BlockNote), ou `undefined` (le
 * collage de BlockNote, tel quel) : après un collage, les fichiers d'autres pages que portent les blocs
 * collés sont rattachés à cette page (`adopt.ts`, Task C9). Comme l'envoi : **jamais pour une session
 * en lecture seule** (lecteur, corbeille, onglet d'un autre schéma), ni sans ce que la page donne.
 */
export function sessionPaste(
  session: { readOnly: boolean; pageId: string },
  files: () => PageUploads | null,
  signal: AbortSignal,
): ReturnType<typeof adoptingPaste> | undefined {
  if (session.readOnly || files() === null) return undefined;
  return adoptingPaste(adoption(session.pageId, files, signal));
}

/**
 * Tant que l'éditeur de la page `pageId` est là, **dans les deux modes** (qui ne fait que lire une page
 * en copie aussi les blocs) : ce qu'on y copie, coupe ou commence à glisser porte la marque de sa
 * provenance, au nom du compte de l'application (`provenance.ts`, R746). Rend de quoi arrêter.
 */
export function sessionStamps(pageId: string): () => void {
  return stampGestures(pageId, () => copier(expectedAccount()));
}

/** De quoi rattacher à la page `pageId` les fichiers d'autres pages : la recopie, et ce que la page en dit. */
function adoption(pageId: string, files: () => PageUploads | null, signal: AbortSignal): AdoptOptions {
  return {
    pageId,
    copier: () => copier(expectedAccount()),
    copy: copyFile,
    signal,
    onAdopted: (message) => files()?.onAdopted?.(message),
    onRefused: (message) => files()?.onRefused(message),
    reread: () => files()?.reread() ?? Promise.resolve(undefined),
  };
}

/**
 * L'extension qui rattache à la page les fichiers des blocs **déposés** depuis un autre onglet
 * (`adoptDrop.ts`, R714), ou `undefined` : comme le collage, jamais pour une session en lecture seule.
 */
export function sessionDrop(
  session: { readOnly: boolean; pageId: string },
  files: () => PageUploads | null,
  editor: () => MementumEditor | null,
  signal: AbortSignal,
): ReturnType<typeof dropAdoption> | undefined {
  if (session.readOnly || files() === null) return undefined;
  return dropAdoption(editor, adoption(session.pageId, files, signal));
}

/**
 * Après un envoi de `fileName` vers le bloc `blockId` qui n'a pas abouti : **retire du bloc le nom du
 * fichier que l'envoi y avait mis**, et rien d'autre (R685, récrit au tour 2). BlockNote pose le bloc
 * d'un dépôt ou d'un collage avant d'envoyer, avec le nom du fichier sur le poste de la personne : ce
 * nom est dans le document, lu par tous, et y restait après un refus ou une annulation. Le bloc, lui,
 * reste, vide : **le supprimer emportait ce qu'un autre y avait mis sans que cet onglet l'ait encore
 * reçu** (un bloc glissé dessous avec son texte, une légende, une adresse), alors qu'une écriture
 * d'attribut ne perd rien de ce qu'un autre fait au même instant.
 *
 * Le nom n'est retiré que s'il est encore celui du fichier et que le bloc n'a pas d'adresse (avec une
 * adresse, c'est le nom du fichier montré : un autre envoi a abouti). Un bloc qui était là avant
 * l'envoi et portait déjà ce nom le perd aussi : rien ne dit qui l'y a mis, et sans adresse il ne
 * nomme rien. Rien dans un éditeur qui n'écrit plus.
 */
export function forgetUploadName(editor: MementumEditor, blockId: string, fileName: string): void {
  if (!editor.isEditable) return;
  const block = editor.getBlock(blockId);
  if (block === undefined) return;
  const props = block.props as Readonly<Record<string, unknown>>;
  if (textProp(props, "url") !== "" || props.name !== fileName) return;
  try {
    editor.updateBlock(blockId, { props: { name: "" } } as never);
  } catch (error) {
    console.error("Le nom du fichier d'un envoi qui n'a pas abouti n'a pas pu être retiré de son bloc :", error);
  }
}
