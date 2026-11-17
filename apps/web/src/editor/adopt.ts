// Un fichier venu d'une autre page (Task C9, R249, R706 à R712).
//
// Un fichier appartient à la page où il a été envoyé (R245) : un bloc collé dans une autre page garde
// l'adresse de sa première page. Qui colle voit son image ; ceux qui ne lisent pas la première page ne
// la voient pas, et elle disparaît avec cette page. Après un collage, l'éditeur fait donc **recopier
// l'attache** dans la page où l'on colle (`POST …/files/copies` : le serveur exige de pouvoir modifier
// ici et lire là-bas, et ne renvoie aucun octet), puis récrit l'adresse du bloc collé.
//
// - **Quand** : au collage seulement (`adoptingPaste`, le `pasteHandler` de BlockNote), dans un
//   éditeur qui écrit, et seulement si le presse-papiers porte le chemin d'un fichier d'une autre page
//   (`foreignFiles`). Rien à l'ouverture d'une page, rien à la frappe : la page n'est parcourue
//   (`blocksNaming`) que pour un tel collage, une fois avant lui et une fois après.
// - **Quoi** : les blocs que ce collage a posés, et eux seuls. Un bloc qui nommait déjà ce fichier
//   avant le collage (écrit par MMD, par un agent, par un autre) garde son adresse : personne n'a
//   demandé de le changer. Seule la prop `url` est récrite (une écriture d'attribut, qui ne perd rien
//   de ce qu'un autre fait au même instant), et seulement si le bloc porte encore l'adresse collée.
// - **Combien** : une recopie à la fois, une par fichier (des blocs qui nomment le même fichier
//   partagent la même attache), `MAX_ADOPTED_FILES` au plus par collage ; au-delà, les blocs gardent
//   leur adresse et la page le dit.
// - **Refus** : `422 invalid_source` (la première page ne se lit pas, ou le fichier n'en est plus) ne
//   dit rien de plus : le bloc garde son adresse, que la route de lecture juge pour chacun. Tout autre
//   échec (droit perdu ici, corbeille, coupure) arrête les recopies du collage, et la page le dit :
//   les fichiers restent ceux de leur première page. Une session perdue demande la connexion ; la
//   recopie repart une fois au retour du compte, si la page est encore modifiable.
// - Ce que rend le serveur n'est posé que si c'est un fichier **de cette page** (`filePath`).
//
// Rien de BlockNote n'est importé ici : l'éditeur est reçu.
import type { CopyFile, FileInfo, PageNode } from "@mementum/protocol";
import { ApiError, isSessionLost } from "../api";
import { canEdit } from "../share/levels";
import type { MementumEditor } from "./debug";
import { blocksNaming } from "./fileBlocks";
import { FILE_BLOCKS, filePath } from "./files";

/** Le plus de fichiers d'autres pages qu'un collage fait recopier. */
export const MAX_ADOPTED_FILES = 100;

export type AdoptOptions = {
  /** La page où l'on colle. */
  pageId: string;
  /** La recopie d'une attache (`copyFile` d'`api.ts`). */
  copy: (pageId: string, source: CopyFile, signal?: AbortSignal) => Promise<FileInfo>;
  /** Annulé quand l'éditeur de la page part : plus rien n'est recopié ni écrit. */
  signal: AbortSignal;
  /** Des fichiers collés n'ont pas pu être rattachés à la page : ce qu'elle en dit. */
  onRefused: (message: string) => void;
  /** Relit la page (`usePageGate`), le compte revenu s'il le fallait. */
  reread: () => Promise<PageNode | null | undefined>;
};

const HEX = "[0-9a-fA-F]";
const UUID = `${HEX}{8}-${HEX}{4}-${HEX}{4}-${HEX}{4}-${HEX}{12}`;
/** Un chemin de fichier dans un texte : borné des deux côtés par ce qui ne peut pas en faire partie. */
const PATH_IN_TEXT = new RegExp(`(?<![\\w/.:%-])/api/v1/pages/${UUID}/files/${UUID}(?![\\w/?#.%-])`, "g");

/**
 * Les fichiers d'une **autre** page que `pageId` que nomment `texts` (ce que porte un presse-papiers) :
 * pour chaque chemin entier, sa page et son fichier. Un chemin suivi d'une requête ou d'un segment,
 * précédé d'un hôte, ou d'une autre casse, n'en est pas un (`filePath`).
 */
export function foreignFiles(texts: Iterable<string>, pageId: string): Map<string, CopyFile> {
  const found = new Map<string, CopyFile>();
  const here = pageId.toLowerCase();
  for (const text of texts) {
    for (const [path] of text.matchAll(PATH_IN_TEXT)) {
      const named = filePath(path);
      if (named !== null && named.pageId.toLowerCase() !== here) found.set(path, named);
    }
  }
  return found;
}

/** Les types du presse-papiers où BlockNote lit un collage, et où une adresse peut se trouver. */
const TEXT_TYPES = ["blocknote/html", "text/html", "text/markdown", "text/plain"];

type PasteContext = {
  event: ClipboardEvent;
  editor: unknown;
  defaultPasteHandler: () => boolean | undefined;
};

/**
 * Le `pasteHandler` de l'éditeur d'une page qu'on modifie : le collage de BlockNote, puis, si le
 * presse-papiers portait le chemin d'un fichier d'une autre page, la recopie des fichiers des blocs
 * que ce collage a posés (`adoptForeignFiles`), sans l'attendre.
 */
export function adoptingPaste(options: AdoptOptions): (context: PasteContext) => boolean | undefined {
  return ({ event, editor: given, defaultPasteHandler }) => {
    const editor = given as MementumEditor;
    const data = event.clipboardData;
    const texts = data === null ? [] : TEXT_TYPES.filter((type) => data.types.includes(type)).map((type) => data.getData(type));
    const foreign = editor.isEditable ? foreignFiles(texts, options.pageId) : new Map<string, CopyFile>();
    if (foreign.size === 0) return defaultPasteHandler();
    const urls = new Set(foreign.keys());
    const before = blocksNaming(editor, urls);
    const handled = defaultPasteHandler();
    const pasted = new Map([...blocksNaming(editor, urls)].filter(([id]) => !before.has(id)));
    if (pasted.size > 0) {
      void adoptForeignFiles(editor, pasted, options).catch((error: unknown) => {
        console.error("Les fichiers collés n'ont pas pu être rattachés à la page :", error);
      });
    }
    return handled;
  };
}

const kept = (n: number) =>
  n === 1
    ? "Un fichier collé n'a pas pu être rattaché à cette page : il reste celui de sa page d'origine, et ne sera vu que de qui peut la lire."
    : `${n} fichiers collés n'ont pas pu être rattachés à cette page : ils restent ceux de leur page d'origine, et ne seront vus que de qui peut la lire.`;

/**
 * Fait recopier dans la page les fichiers d'autres pages que nomment les blocs `blocks` (pour chaque
 * id de bloc, l'adresse collée), une recopie à la fois, puis récrit l'adresse de chaque bloc qui la
 * porte encore. Rend le nombre de blocs récrits.
 */
export async function adoptForeignFiles(
  editor: MementumEditor,
  blocks: ReadonlyMap<string, string>,
  options: AdoptOptions,
): Promise<number> {
  const { pageId, signal } = options;
  // Les fichiers, dans l'ordre des blocs ; chacun avec les blocs qui le nomment.
  const files = new Map<string, string[]>();
  for (const [id, url] of blocks) files.set(url, [...(files.get(url) ?? []), id]);
  const sources = [...files.keys()];
  const taken = sources.slice(0, MAX_ADOPTED_FILES);
  let rewritten = 0;
  /** Les fichiers qui restent ceux de leur page d'origine sans que le serveur ait dit pourquoi. */
  let left = 0;
  let said = "";

  /** Recopie un fichier ; une session perdue attend le retour du compte, et repart une fois. */
  const copyOnce = async (source: CopyFile): Promise<FileInfo> => {
    try {
      return await options.copy(pageId, source, signal);
    } catch (error) {
      if (!isSessionLost(error) || signal.aborted) throw error;
      const node = await options.reread();
      if (signal.aborted || node == null || node.trashedAt !== null || !canEdit(node.level)) throw error;
      return options.copy(pageId, source, signal);
    }
  };

  for (const [at, url] of taken.entries()) {
    if (signal.aborted) return rewritten;
    const source = filePath(url);
    if (source === null) continue;
    let info: FileInfo;
    try {
      info = await copyOnce(source);
    } catch (error) {
      if (signal.aborted) return rewritten;
      // La première page ne se lit pas, ou le fichier n'en est plus : rien de plus à en dire.
      if (error instanceof ApiError && error.code === "invalid_source") continue;
      // Tout autre échec vaut pour les suivants : ils ne partent pas.
      left += taken.length - at;
      const refusedToAccount = error instanceof ApiError && error.status >= 400 && error.status < 500 && !isSessionLost(error);
      if (refusedToAccount) {
        said = ` Le serveur dit : ${(error as ApiError).message}`;
        // Un droit, ou la page, ont changé : la page relue le montre.
        void options.reread().catch(() => undefined);
      }
      break;
    }
    if (signal.aborted) return rewritten;
    // Ce que dit le serveur n'est posé que si c'est un fichier de cette page.
    if (filePath(info?.url)?.pageId.toLowerCase() !== pageId.toLowerCase()) {
      left += 1;
      continue;
    }
    for (const id of files.get(url) ?? []) {
      if (!editor.isEditable) return rewritten;
      const block = editor.getBlock(id);
      // Le bloc est parti, ou ne porte plus l'adresse collée (on l'a changée entre-temps) : il reste.
      if (block === undefined || !FILE_BLOCKS.has(block.type) || (block.props as { url?: unknown }).url !== url) continue;
      editor.updateBlock(id, { props: { url: info.url } } as never);
      rewritten += 1;
    }
  }
  if (left > 0) options.onRefused(kept(left) + said);
  const beyond = sources.length - taken.length;
  if (beyond > 0 && !signal.aborted) {
    options.onRefused(
      `Plus de ${MAX_ADOPTED_FILES} fichiers d'autres pages collés d'un coup : les ${MAX_ADOPTED_FILES} premiers sont rattachés à cette page, les ${beyond} autres restent ceux de leur page d'origine, et ne seront vus que de qui peut la lire.`,
    );
  }
  return rewritten;
}
