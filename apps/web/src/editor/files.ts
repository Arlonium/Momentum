// Les fichiers d'une page, côté client (Task C8, R245 à R248, R666 à R688).
//
// **Quelles adresses l'éditeur charge.** Un bloc `image`, `video`, `audio` ou `file` porte l'adresse
// de son fichier dans sa prop `url`, qui vient du document : d'un envoi fait ici, mais aussi de MMD,
// d'un agent, d'un autre client, du panneau « Intégrer » de BlockNote, qui écrit ce qu'on y tape.
// Elle n'est montrée, chargée en `src` ou proposée en téléchargement que par `shownUrl` : le chemin
// exact d'un fichier de l'instance (`/api/v1/pages/<uuid>/files/<uuid>`, sans hôte, sans requête,
// sans rien d'autre), ou une adresse web entière (`http:`, `https:`, permise depuis le lot 1, R248) ;
// tout le reste (`javascript:`, `data:`, `blob:`, un chemin relatif, une autre route de l'API)
// devient `about:blank`. Le document ne donne aucun droit : c'est la route qui les juge, à chaque
// lecture (R245), et elle sert tout fichier en téléchargement (R246).
//
// **Un envoi** (`uploader`, donné à BlockNote comme `uploadFile`) : la limite de l'instance est lue
// au premier envoi, jamais à l'ouverture d'une page, et un fichier trop gros est refusé avant toute
// requête (le `413` du serveur part sans qu'il lise le corps, et un navigateur ne le reçoit pas,
// R505). Le fichier part par `api.ts`, tel quel. Un refus du serveur est dit tel qu'il le dit, et la
// page est relue. Une coupure (réseau, ou réponse perdue pendant que le navigateur envoie encore)
// fait relire la page, qui dit ce qu'elle est devenue ; une session perdue demande la connexion, et
// le message vient au retour du compte. Rien n'est gardé dans le navigateur : ni octet, ni brouillon.
//
// **Ce que la page en montre** (`filesOf`, rendu par `page/FileNotices.tsx`) : les envois en cours,
// chacun avec de quoi l'annuler, et les refus, jusqu'à ce qu'on les ferme. Ils sont rangés par page,
// hors de l'éditeur (une session recréée en apporte un neuf), et partent avec elle.
//
// Rien de BlockNote n'est chargé ici : la page rend ses messages sans l'éditeur.
import type { FileInfo, FileLimits, PageNode } from "@mementum/protocol";
import { ApiError, getFileLimits, isSessionLost, uploadFile } from "../api";
import { canEdit } from "../share/levels";
import { webUrl } from "./blocks/urls";

/** Ce que rend `shownUrl` d'une adresse qui n'est pas permise : une page vide, qui ne charge rien. */
export const BLANK = "about:blank";

// Les chiffres hexadécimaux sans casse, comme `isPageId` ; le chemin, lui, à la lettre.
const HEX = "[0-9a-fA-F]";
const UUID = `${HEX}{8}-${HEX}{4}-${HEX}{4}-${HEX}{4}-${HEX}{12}`;
const FILE_PATH = new RegExp(`^/api/v1/pages/(${UUID})/files/(${UUID})$`);

/**
 * La page et le fichier que nomme `url`, si c'est exactement le chemin d'un fichier de l'instance :
 * sans hôte, sans requête ni fragment, sans espace de bord, sans `..` ; sinon `null`. Tout ce qui
 * n'est pas une chaîne (une prop écrite par un autre client) est refusé.
 */
export function filePath(url: unknown): { pageId: string; fileId: string } | null {
  if (typeof url !== "string") return null;
  // `$` d'une expression JavaScript ne passe pas un saut de ligne final (pas de mode multiligne).
  const found = FILE_PATH.exec(url);
  return found === null ? null : { pageId: found[1], fileId: found[2] };
}

/** `url` est-elle exactement le chemin d'un fichier de l'instance ? */
export function isFilePath(url: unknown): boolean {
  return filePath(url) !== null;
}

/**
 * L'adresse à charger pour `url` : elle-même si c'est le chemin d'un fichier de l'instance, l'adresse
 * web telle que le navigateur l'écrit si c'en est une entière (`http:`, `https:`), `about:blank`
 * sinon. **Toute adresse de fichier venue du document passe par ici** avant d'aller dans un `src`,
 * un `href` ou `window.open`.
 */
export function shownUrl(url: unknown): string {
  if (isFilePath(url)) return url as string;
  return webUrl(url) ?? BLANK;
}

// Ce que la page montre de ses fichiers : envois en cours et refus.

/** Un envoi en cours : le nom du fichier, tel qu'on peut le montrer, et de quoi l'annuler. */
export type Upload = { readonly id: number; readonly name: string; readonly cancel: () => void };
/** Un envoi refusé, ou interrompu : ce que la page en dit. */
export type Refusal = { readonly id: number; readonly text: string };
export type PageFiles = { readonly uploads: readonly Upload[]; readonly refusals: readonly Refusal[] };

const NOTHING: PageFiles = { uploads: [], refusals: [] };
/** Les pages ouvertes (`enterFiles`), et ce que chacune montre. */
const pages = new Map<string, PageFiles>();
const listeners = new Set<() => void>();
let counter = 0;

function set(pageId: string, change: (files: PageFiles) => PageFiles): void {
  const files = pages.get(pageId);
  // Une page qu'on ne regarde pas (quittée, ou jamais ouverte) ne garde rien.
  if (files === undefined) return;
  pages.set(pageId, change(files));
  for (const listener of [...listeners]) listener();
}

/** Ce que la page `pageId` montre de ses fichiers ; le même objet tant que rien n'y change. */
export function filesOf(pageId: string): PageFiles {
  return pages.get(pageId) ?? NOTHING;
}

/** Appelle `listener` à chaque changement ; rend de quoi l'arrêter. */
export function watchFiles(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** La page `pageId` est ouverte : ses envois et ses refus se montrent. */
export function enterFiles(pageId: string): void {
  if (!pages.has(pageId)) pages.set(pageId, NOTHING);
}

/**
 * La page `pageId` est quittée : ses envois en cours sont annulés, ses messages partent, et plus
 * rien ne s'y dit.
 */
export function leaveFiles(pageId: string): void {
  const files = pages.get(pageId);
  if (files === undefined) return;
  pages.delete(pageId);
  for (const upload of files.uploads) upload.cancel();
  for (const listener of [...listeners]) listener();
}

/** Dit `text` sur la page `pageId`, à la suite de ses refus, si elle est ouverte. */
export function sayRefusal(pageId: string, text: string): void {
  counter += 1;
  const id = counter;
  set(pageId, (files) => ({ ...files, refusals: [...files.refusals, { id, text }] }));
}

/** Ferme les refus de la page `pageId`. */
export function closeRefusals(pageId: string): void {
  if (filesOf(pageId).refusals.length > 0) set(pageId, (files) => ({ ...files, refusals: [] }));
}

function begin(pageId: string, name: string, cancel: () => void): number {
  counter += 1;
  const id = counter;
  set(pageId, (files) => ({ ...files, uploads: [...files.uploads, { id, name, cancel }] }));
  return id;
}

function end(pageId: string, id: number): void {
  if (!filesOf(pageId).uploads.some((upload) => upload.id === id)) return;
  set(pageId, (files) => ({ ...files, uploads: files.uploads.filter((upload) => upload.id !== id) }));
}

// La limite de l'instance : lue au premier envoi, gardée pour les suivants.

let limits: Promise<FileLimits> | null = null;

/**
 * La taille maximale d'un fichier sur l'instance : lue une fois, à la première demande (jamais à
 * l'ouverture d'une page), puis gardée ; une lecture qui échoue est refaite à la demande suivante.
 */
export function fileLimits(read: () => Promise<FileLimits> = getFileLimits): Promise<FileLimits> {
  if (limits === null) {
    const reading = read();
    limits = reading;
    reading.catch(() => {
      if (limits === reading) limits = null;
    });
  }
  return limits;
}

/** La limite gardée ne vaut plus (le serveur vient d'en dire une autre) : la prochaine demande la relit. */
export function forgetFileLimits(): void {
  limits = null;
}

// Un envoi.

/** Un nom de fichier dans un message : au plus tant de caractères. */
const MAX_NAME_CHARS = 80;
/** Caractères de contrôle et de direction : jamais dans un nom montré (la règle du serveur, R247). */
const UNSHOWN = /[\u0000-\u001F\u007F-\u009F\u061C\u200E\u200F\u202A-\u202E\u2066-\u2069]/g;

/** Le nom d'un fichier tel qu'un envoi en cours le montre : sans caractère de contrôle ni de direction, coupé. */
export function plainName(name: string): string {
  const chars = Array.from(name.replace(UNSHOWN, "").trim());
  const kept = chars.length > MAX_NAME_CHARS ? `${chars.slice(0, MAX_NAME_CHARS).join("")}…` : chars.join("");
  return kept === "" ? "fichier" : kept;
}

/** Ce nom dans une phrase : isolé (U+2068, U+2069), pour que son sens d'écriture ne déplace pas le reste. */
const quoted = (name: string) => `\u2068${plainName(name)}\u2069`;

const MIB = 1024 * 1024;

/** La limite, telle que le serveur l'écrit dans son refus (`file_too_large`). */
function bound(max: number): string {
  return max % MIB === 0 ? `${max / MIB} Mio` : `${max} octets`;
}

/** L'erreur d'un envoi qui n'a rien posé ; son message est déjà dit par `onRefused`, s'il y en a un. */
export class UploadStopped extends Error {
  constructor(why: string) {
    super(why);
    this.name = "UploadStopped";
  }
}

export type UploaderOptions = {
  /** La page où l'on envoie. */
  pageId: string;
  /** La limite de l'instance (`fileLimits`) : demandée à chaque envoi, lue une fois. */
  limits: () => Promise<FileLimits>;
  /** Le type du bloc `blockId` dans l'éditeur, à l'instant ; `undefined` : il n'y est plus. */
  blockType: (blockId: string) => string | undefined;
  /** Un envoi n'a pas abouti : ce que la page en dit. */
  onRefused: (message: string) => void;
  /**
   * Relit la page (`usePageGate`), le compte revenu s'il le fallait : ce qu'elle est devenue ; `null` :
   * elle n'est plus là pour ce compte ; `undefined` : le serveur ne répond pas.
   */
  reread: () => Promise<PageNode | null | undefined>;
  /**
   * L'envoi du fichier `fileName` vers le bloc `blockId` n'a pas abouti, et l'éditeur est encore là :
   * à lui de retirer du bloc le nom du fichier que l'envoi y avait mis (`uploads.ts`, R685). Jamais
   * appelé pour un envoi qui aboutit, ni quand l'éditeur est parti (il n'écrit plus pour personne).
   */
  discard?: (blockId: string, fileName: string) => void;
  /** Annulé quand l'éditeur de la page part (page quittée, session recréée) : tous ses envois s'arrêtent. */
  signal: AbortSignal;
  /** L'envoi lui-même (`uploadFile` d'`api.ts`) ; les tests en donnent un autre. */
  upload?: typeof uploadFile;
};

/** Les blocs qui portent un fichier : un envoi ne pose rien dans un autre. */
export const FILE_BLOCKS: ReadonlySet<string> = new Set(["image", "video", "audio", "file"]);

/**
 * Les types que le serveur lit d'un conteneur commun au son et à la vidéo (R246 : `video/webm` pour
 * tout WebM, `audio/ogg` pour tout Ogg) : ils ne disent pas lequel des deux le fichier porte.
 */
const SHARED_CONTAINERS: ReadonlySet<string> = new Set(["video/webm", "audio/ogg"]);

/**
 * Le bloc qui montre un fichier de type `mime` (celui que le serveur a lu dans les octets), posé dans
 * un bloc `block` : `undefined` s'il reste ce bloc. Une image n'est montrée que par un bloc image ;
 * un son et une vidéo partagent leurs conteneurs : dans un conteneur commun, le bloc choisi d'après le
 * fichier reste, sinon le type du serveur choisit entre le bloc son et le bloc vidéo ; tout le reste
 * va dans un bloc fichier, qui prend tout (R673, R686).
 */
function blockFor(block: string, mime: string): "file" | "audio" | "video" | undefined {
  if (block === "file") return undefined;
  if (block === "image") return mime.startsWith("image/") ? undefined : "file";
  const kind = mime.startsWith("audio/") ? "audio" : mime.startsWith("video/") ? "video" : null;
  if (kind === null) return "file";
  return kind === block || SHARED_CONTAINERS.has(mime) ? undefined : kind;
}

/** Ce qu'un envoi pose dans son bloc (`updateBlock`). */
export type Uploaded = { type?: "file" | "audio" | "video"; props: { url: string; name: string } };

/**
 * De quoi envoyer un fichier dans la page, pour BlockNote (`uploadFile`). Rend ce qu'il pose dans le
 * bloc : `{ props: { url, name } }`, l'adresse et le nom que dit le serveur, avec le `type` du bloc
 * qui montre ce fichier quand ce n'est pas le sien (`blockFor` : un HTML nommé `photo.png` dans un
 * bloc image devient un bloc fichier, à télécharger). Rejette quand rien n'est à poser ; ce qu'il y a
 * à en dire est alors passé par `onRefused`, sauf pour un envoi annulé par la personne, et le bloc
 * est rendu à l'éditeur (`discard`).
 */
export function uploader(options: UploaderOptions): (file: File, blockId?: string) => Promise<Uploaded> {
  const { pageId, signal, onRefused } = options;
  const send = options.upload ?? uploadFile;
  const interrupted = (file: File) => `L'envoi de ${quoted(file.name)} a été interrompu. Réessaie.`;
  const notSent = (file: File, why: string) => `${quoted(file.name)} n'a pas été envoyé : ${why}`;

  /**
   * Après une coupure, ou quand l'éditeur est parti pendant l'envoi : la page relue dit pourquoi, une
   * fois le compte revenu s'il le fallait. Une page quittée entre-temps ne montre plus rien
   * (`sayRefusal` y est sans effet).
   */
  const afterCut = async (file: File) => {
    let node: PageNode | null | undefined;
    try {
      node = await options.reread();
    } catch {
      node = undefined;
    }
    if (node === undefined) {
      onRefused(notSent(file, "le serveur ne répond pas. Vérifie ta connexion, puis réessaie."));
    } else if (node === null) onRefused(notSent(file, "tu n'as plus accès à cette page."));
    else if (node.trashedAt !== null) onRefused(notSent(file, "la page est à la corbeille."));
    else if (!canEdit(node.level)) onRefused(notSent(file, "tu ne peux plus modifier cette page."));
    else onRefused(interrupted(file));
  };

  return async (file, blockId) => {
    if (signal.aborted) throw new UploadStopped("éditeur parti");
    /** L'envoi n'aboutit pas : son bloc est rendu à l'éditeur, s'il est encore là. */
    const giveBack = () => {
      if (blockId !== undefined && !signal.aborted) options.discard?.(blockId, file.name);
    };
    let max: number;
    try {
      max = (await options.limits()).maxFileBytes;
    } catch (error) {
      if (signal.aborted) throw new UploadStopped("éditeur parti");
      // Une session perdue demande la connexion par-dessus la page (`api.ts`) ; le message reste.
      onRefused(notSent(file, error instanceof Error ? error.message : "le serveur ne répond pas."));
      giveBack();
      throw error;
    }
    if (signal.aborted) throw new UploadStopped("éditeur parti");
    if (typeof max !== "number" || file.size > max) {
      onRefused(
        `${quoted(file.name)} est trop gros : ${bound(max)} au plus par fichier sur cette instance. Rien n'a été envoyé.`,
      );
      giveBack();
      throw new UploadStopped("fichier trop gros");
    }

    // L'envoi a son annulation à lui (« Annuler »), et s'arrête aussi avec l'éditeur.
    const own = new AbortController();
    let cancelled = false;
    const stop = () => own.abort();
    signal.addEventListener("abort", stop);
    const id = begin(pageId, plainName(file.name), () => {
      cancelled = true;
      own.abort();
    });
    let info: FileInfo;
    try {
      info = await send(pageId, file, own.signal);
    } catch (error) {
      // Annulé par la personne, ou la page quittée (elle a déjà retiré l'envoi) : rien à dire.
      if (cancelled) {
        giveBack();
        throw new UploadStopped("envoi annulé");
      }
      // L'éditeur est parti, la page reste (session recréée : droits baissés, corbeille, schéma) : ce
      // qu'il aurait posé ne le serait plus, et c'est la page relue qui dit pourquoi (R684).
      if (own.signal.aborted) {
        void afterCut(file);
        throw new UploadStopped("éditeur parti");
      }
      if (!(error instanceof ApiError) || error.status === 0 || error.status >= 500 || isSessionLost(error)) {
        // Une coupure, ou l'erreur d'un mandataire : c'est aussi ce qu'un navigateur voit d'un `413`
        // que le serveur rend sans lire le corps (R505). La limite gardée est peut-être périmée.
        if (!isSessionLost(error)) forgetFileLimits();
        void afterCut(file);
      } else {
        if (error.code === "file_too_large") forgetFileLimits();
        onRefused(notSent(file, error.message));
        // Un droit, ou la page, ont changé : la page relue le montre.
        void options.reread().catch(() => undefined);
      }
      giveBack();
      throw error;
    } finally {
      signal.removeEventListener("abort", stop);
      end(pageId, id);
    }
    if (signal.aborted) {
      void afterCut(file);
      throw new UploadStopped("éditeur parti");
    }
    // Ce que dit le serveur n'est posé que si c'est un fichier de cette page.
    if (filePath(info.url)?.pageId !== pageId || typeof info.name !== "string" || typeof info.mime !== "string") {
      onRefused(notSent(file, "le serveur a répondu autre chose qu'un fichier de cette page."));
      giveBack();
      throw new UploadStopped("réponse inattendue");
    }
    const props = { url: info.url, name: info.name };
    if (blockId === undefined) return { props };
    const block = options.blockType(blockId);
    // Le bloc n'est plus là, ou un autre en a fait autre chose qu'un bloc de fichier : `updateBlock` n'y
    // poserait rien, et le fichier, attaché à la page, n'y serait montré nulle part (R677).
    if (block === undefined || !FILE_BLOCKS.has(block)) {
      onRefused(
        `${quoted(file.name)} a été envoyé, mais son bloc a été supprimé ou changé entre-temps : il n'est pas dans la page.`,
      );
      throw new UploadStopped("bloc parti");
    }
    const type = blockFor(block, info.mime);
    return type === undefined ? { props } : { type, props };
  };
}
