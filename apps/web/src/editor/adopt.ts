// Un fichier venu d'une autre page (Task C9, R249, R706 à R715).
//
// Un fichier appartient à la page où il a été envoyé (R245) : un bloc collé ou déposé dans une autre
// page garde l'adresse de sa première page. Qui le pose voit son image ; ceux qui ne lisent pas la
// première page ne la voient pas, et elle disparaît avec cette page. Après un collage ou un dépôt,
// l'éditeur fait donc **recopier l'attache** dans la page d'ici (`POST …/files/copies` : le serveur
// exige de pouvoir modifier ici et lire là-bas, et ne renvoie aucun octet), puis récrit l'adresse du
// bloc posé.
//
// - **Quand** : au collage (`adoptingPaste`, le `pasteHandler` de BlockNote) et au dépôt d'un bloc
//   glissé d'un autre onglet (`adoptDrop.ts`), dans un éditeur qui écrit, et **seulement si le
//   presse-papiers porte `blocknote/html`**, ce que Mementum écrit en copiant ou en glissant un bloc,
//   **et la marque de provenance de ce compte dans ce navigateur** (`provenance.ts`, `carriedFiles`,
//   R746 à R752) : seuls les fichiers **de la page d'où le bloc a été copié** sont recopiés. Un
//   texte, un Markdown ou un HTML venu d'ailleurs (un message, une page web) qui nomme le fichier
//   d'une page ne recopie rien, ni un presse-papiers forgé par un autre site, ni un bloc qui nommait
//   déjà, dans la page où on le copie, le fichier d'une autre page (planté là par qui ne lit pas cette
//   autre page) : la personne partagerait sans le savoir un fichier d'une page privée. Rien à
//   l'ouverture d'une page, rien à la frappe : la page n'est parcourue (`blocksNaming`) que pour un
//   tel geste, une fois avant lui et une fois après.
// - **La marque** est écrite à la copie, à la coupe et au début d'un glisser (`stampGestures`), dans
//   les deux modes de l'éditeur ; ce que BlockNote écrit n'est pas touché.
// - **Quoi** : les blocs que ce geste a posés, et eux seuls. Un bloc qui nommait déjà ce fichier avant
//   (écrit par MMD, par un agent, par un autre) garde son adresse : personne n'a demandé de le changer.
//   Seule la prop `url` est récrite (une écriture d'attribut, qui ne perd rien de ce qu'un autre fait
//   au même instant), seulement si le bloc porte encore l'adresse posée, et **hors de l'historique de
//   l'annulation** : Ctrl+Z retire le collage d'un coup, il ne rend pas au bloc l'adresse de sa
//   première page.
// - **Combien** : une recopie à la fois, une par fichier (des blocs qui nomment le même fichier
//   partagent la même attache), `MAX_ADOPTED_FILES` au plus par geste ; au-delà, les blocs gardent
//   leur adresse et la page le dit.
// - **Ce qui est dit** : toute recopie réussie (`onAdopted` : un fichier d'une autre page est
//   désormais vu de ceux qui lisent celle-ci), une fois par geste. `422 invalid_source` (la première
//   page ne se lit pas, ou le fichier n'en est plus) ne dit rien de plus que le serveur : le bloc garde
//   son adresse, que la route de lecture juge pour chacun. Tout autre échec (droit perdu ici,
//   corbeille, coupure, éditeur parti) arrête les recopies du geste, et la page le dit : les fichiers
//   restent ceux de leur première page. Une session perdue demande la connexion ; la recopie repart
//   une fois au retour du compte, si la page est encore modifiable.
// - Ce que rend le serveur n'est posé que si c'est un fichier **de cette page** (`filePath`).
//
// Rien de BlockNote n'est importé ici : l'éditeur est reçu.
import type { CopyFile, FileInfo, PageNode } from "@mementum/protocol";
import { ApiError, isSessionLost } from "../api";
import { canEdit } from "../share/levels";
import type { MementumEditor } from "./debug";
import { blocksNaming } from "./fileBlocks";
import { filePath } from "./files";
import { markedPage, ORIGIN, originMark, vouches, type Copier } from "./provenance";

/** Le plus de fichiers d'autres pages qu'un collage ou un dépôt fait recopier. */
export const MAX_ADOPTED_FILES = 100;

export type AdoptOptions = {
  /** La page où l'on colle. */
  pageId: string;
  /** Qui colle : le compte de l'onglet et le secret de son navigateur (`provenance.ts`) ; `null` : personne. */
  copier: () => Copier | null;
  /** La recopie d'une attache (`copyFile` d'`api.ts`). */
  copy: (pageId: string, source: CopyFile, signal?: AbortSignal) => Promise<FileInfo>;
  /** Annulé quand l'éditeur de la page part : plus rien n'est recopié ni écrit. */
  signal: AbortSignal;
  /** Des fichiers d'autres pages ont été rattachés à celle-ci : ce que la page en dit (un état). */
  onAdopted: (message: string) => void;
  /** Des fichiers posés n'ont pas pu être rattachés à la page : ce qu'elle en dit (une alerte). */
  onRefused: (message: string) => void;
  /** Relit la page (`usePageGate`), le compte revenu s'il le fallait. */
  reread: () => Promise<PageNode | null | undefined>;
};

const HEX = "[0-9a-fA-F]";
const UUID = `${HEX}{8}-${HEX}{4}-${HEX}{4}-${HEX}{4}-${HEX}{12}`;
/** Un chemin de fichier dans un texte : borné des deux côtés par ce qui ne peut pas en faire partie. */
const PATH_IN_TEXT = new RegExp(`(?<![\\w/.:%-])/api/v1/pages/${UUID}/files/${UUID}(?![\\w/?#.%-])`, "g");

/**
 * Les fichiers que nomment `texts` dont la page est (`of`) ou n'est pas (`!of`) `pageId` : pour chaque
 * chemin entier, sa page et son fichier. Un chemin suivi d'une requête ou d'un segment, précédé d'un
 * hôte, ou d'une autre casse, n'en est pas un (`filePath`).
 */
function namedFiles(texts: Iterable<string>, pageId: string, of: boolean): Map<string, CopyFile> {
  const found = new Map<string, CopyFile>();
  const page = pageId.toLowerCase();
  for (const text of texts) {
    for (const [path] of text.matchAll(PATH_IN_TEXT)) {
      const named = filePath(path);
      if (named !== null && (named.pageId.toLowerCase() === page) === of) found.set(path, named);
    }
  }
  return found;
}

/** Les fichiers d'une **autre** page que `pageId` que nomment `texts`. */
export function foreignFiles(texts: Iterable<string>, pageId: string): Map<string, CopyFile> {
  return namedFiles(texts, pageId, false);
}

/** Les fichiers **de** la page `pageId` que nomment `texts`. */
export function filesOf(texts: Iterable<string>, pageId: string): Map<string, CopyFile> {
  return namedFiles(texts, pageId, true);
}

/** Le type que Mementum (BlockNote) écrit dans le presse-papiers en copiant ou en glissant un bloc. */
const BLOCKS = "blocknote/html";

/**
 * Joint à ce que BlockNote vient d'écrire dans un presse-papiers ou un glisser (`blocknote/html`) la
 * marque de provenance (`provenance.ts`), si ce bloc nomme des fichiers de la page `pageId` d'où `by`
 * le copie. Rien d'autre n'est touché ; sans bloc de Mementum, sans fichier de cette page (un bloc qui
 * nomme le fichier d'une autre page n'emporte pas ce fichier, R748) ou sans copieur, rien n'est écrit.
 */
export function stampOrigin(
  data: Pick<DataTransfer, "types" | "getData" | "setData"> | null,
  pageId: string,
  by: Copier | null,
): void {
  if (data === null || by === null || !Array.from(data.types).includes(BLOCKS)) return;
  const files = filesOf([data.getData(BLOCKS)], pageId);
  if (files.size > 0) data.setData(ORIGIN, originMark(by, pageId, files.keys()));
}

/** Les gestes qui écrivent un presse-papiers ou un glisser. */
const STAMPED = ["copy", "cut", "dragstart"] as const;

/**
 * Tant que l'éditeur de la page `pageId` est là : marque la provenance de ce que BlockNote écrit à une
 * copie, une coupe ou au début d'un glisser (`stampOrigin`). Écouté sur le document, à la remontée :
 * après ProseMirror et BlockNote (sur l'éditeur) et après la poignée d'un bloc (React, sur sa racine),
 * dont le glisser part d'un bouton hors de la vue de l'éditeur. Un aperçu de version de la page copie
 * des blocs de la même page. Rend de quoi ne plus écouter.
 */
export function stampGestures(pageId: string, by: () => Copier | null): () => void {
  const stamp = (event: Event) => {
    const data = (event as ClipboardEvent).clipboardData ?? (event as DragEvent).dataTransfer ?? null;
    stampOrigin(data, pageId, by());
  };
  for (const type of STAMPED) document.addEventListener(type, stamp);
  return () => {
    for (const type of STAMPED) document.removeEventListener(type, stamp);
  };
}

/**
 * Les fichiers à rattacher à la page `pageId` que porte un presse-papiers ou un glisser : ceux **de la
 * page d'où le bloc a été copié**, une autre que celle-ci, si la marque de provenance est celle de `by`
 * (ce compte, dans ce navigateur) pour exactement ces fichiers-là (R746, R748). Sans bloc de Mementum
 * (`blocknote/html`, R713), sans marque, sous une marque d'ailleurs, forgée ou rejouée avec d'autres
 * fichiers : rien. Un fichier d'une autre page que celle de la marque n'est jamais rendu.
 */
export function carriedFiles(
  data: Pick<DataTransfer, "types" | "getData"> | null,
  pageId: string,
  by: Copier | null,
): Map<string, CopyFile> {
  const none = new Map<string, CopyFile>();
  if (data === null || by === null) return none;
  const types = Array.from(data.types);
  if (!types.includes(BLOCKS) || !types.includes(ORIGIN)) return none;
  const mark = data.getData(ORIGIN);
  const from = markedPage(mark);
  if (from === null || from === pageId.toLowerCase()) return none;
  const files = filesOf([data.getData(BLOCKS)], from);
  return files.size > 0 && vouches(mark, by, files.keys()) ? files : none;
}

/**
 * Juste avant un geste qui pose des blocs (un collage, un dépôt) : relève les blocs qui nomment déjà
 * les fichiers `foreign`, et rend ce qu'il faut appeler une fois les blocs posés : les blocs neufs
 * qui les nomment sont rattachés à la page (`adoptForeignFiles`), sans l'attendre.
 */
export function watchPosed(editor: MementumEditor, foreign: ReadonlyMap<string, CopyFile>, options: AdoptOptions): () => void {
  const urls = new Set(foreign.keys());
  const before = blocksNaming(editor, urls);
  return () => {
    if (options.signal.aborted || !editor.isEditable) return;
    const posed = new Map([...blocksNaming(editor, urls)].filter(([id]) => !before.has(id)));
    if (posed.size === 0) return;
    void adoptForeignFiles(editor, posed, options).catch((error: unknown) => {
      console.error("Les fichiers posés n'ont pas pu être rattachés à la page :", error);
    });
  };
}

type PasteContext = {
  event: ClipboardEvent;
  editor: unknown;
  defaultPasteHandler: () => boolean | undefined;
};

/**
 * Le `pasteHandler` de l'éditeur d'une page qu'on modifie : le collage de BlockNote, puis, si le
 * presse-papiers était celui d'un bloc copié par ce compte, dans ce navigateur, depuis une autre page
 * dont il nomme des fichiers (`carriedFiles`), la recopie des fichiers des blocs que ce collage a posés.
 */
export function adoptingPaste(options: AdoptOptions): (context: PasteContext) => boolean | undefined {
  return ({ event, editor: given, defaultPasteHandler }) => {
    const editor = given as MementumEditor;
    const foreign = editor.isEditable
      ? carriedFiles(event.clipboardData, options.pageId, options.copier())
      : new Map<string, CopyFile>();
    if (foreign.size === 0) return defaultPasteHandler();
    const posed = watchPosed(editor, foreign, options);
    const handled = defaultPasteHandler();
    posed();
    return handled;
  };
}

const kept = (n: number) =>
  n === 1
    ? "Un fichier collé n'a pas pu être rattaché à cette page : il reste celui de sa page d'origine, et ne sera vu que de qui peut la lire."
    : `${n} fichiers collés n'ont pas pu être rattachés à cette page : ils restent ceux de leur page d'origine, et ne seront vus que de qui peut la lire.`;

const adopted = (n: number) =>
  n === 1
    ? "Un fichier d'une autre page a été rattaché à celle-ci : ceux qui lisent cette page le voient."
    : `${n} fichiers d'autres pages ont été rattachés à celle-ci : ceux qui lisent cette page les voient.`;

/**
 * Fait recopier dans la page les fichiers d'autres pages que nomment les blocs `blocks` (pour chaque
 * id de bloc, l'adresse posée), une recopie à la fois, puis récrit l'adresse de chaque bloc qui la
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
  /** Les fichiers rattachés : au moins un bloc en porte désormais la recopie. */
  let attached = 0;
  /** Les fichiers qui restent ceux de leur page d'origine sans que le serveur ait dit pourquoi. */
  let left = 0;
  let said = "";
  /** L'éditeur n'écrit plus (parti : session recréée, page quittée ; ou passé en lecture seule). */
  const stopped = () => signal.aborted || !editor.isEditable;

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
    // L'éditeur est parti pendant la file : ce qui reste n'est pas rattaché, et la page le dit si elle
    // est encore là (une session recréée, R714 ; quittée, elle ne montre plus rien).
    if (stopped()) {
      left += taken.length - at;
      break;
    }
    const source = filePath(url);
    if (source === null) continue;
    let info: FileInfo;
    try {
      info = await copyOnce(source);
    } catch (error) {
      if (stopped()) {
        left += taken.length - at;
        break;
      }
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
    if (stopped()) {
      left += taken.length - at;
      break;
    }
    // Ce que dit le serveur n'est posé que si c'est un fichier de cette page.
    if (filePath(info?.url)?.pageId.toLowerCase() !== pageId.toLowerCase()) {
      left += 1;
      continue;
    }
    let set = 0;
    for (const id of files.get(url) ?? []) {
      // Le bloc est parti, ou ne porte plus l'adresse posée (on l'a changée entre-temps) : il reste.
      if ((editor.getBlock(id)?.props as { url?: unknown } | undefined)?.url !== url) continue;
      // Hors de l'historique de l'annulation : Ctrl+Z retire le collage, pas cette récriture (R715).
      editor.transact((tr) => {
        tr.setMeta("addToHistory", false);
        editor.updateBlock(id, { props: { url: info.url } } as never);
      });
      set += 1;
    }
    rewritten += set;
    if (set > 0) attached += 1;
  }
  if (attached > 0) options.onAdopted(adopted(attached));
  if (left > 0) options.onRefused(kept(left) + said);
  const beyond = sources.length - taken.length;
  if (beyond > 0 && !signal.aborted) {
    options.onRefused(
      `Plus de ${MAX_ADOPTED_FILES} fichiers d'autres pages collés d'un coup : les ${MAX_ADOPTED_FILES} premiers sont rattachés à cette page, les ${beyond} autres restent ceux de leur page d'origine, et ne seront vus que de qui peut la lire.`,
    );
  }
  return rewritten;
}
