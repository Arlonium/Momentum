// La provenance d'un bloc copié ou glissé (relecture finale de la partie B, R746 à R752, R776 à R780).
//
// Un fichier d'une autre page n'est rattaché à la page où l'on pose un bloc (`adopt.ts`) que si ce bloc
// a été **copié ou glissé depuis la page de ce fichier, dans ce navigateur, sous ce compte**. Sans cela,
// la recopie partait pour tout bloc collé qui nommait une autre page, avec les droits de qui colle : un
// bloc planté dans une page partagée par quelqu'un qui ne lit pas la page du fichier, puis copié et
// collé par quelqu'un qui la lit, en sortait le fichier ; un presse-papiers écrit par un autre site
// aussi (le type `blocknote/html` s'écrit depuis toute page web).
//
// - **La marque** : à la copie, à la coupe et au début d'un glisser, si ce que BlockNote a écrit nomme
//   des fichiers de la page d'où l'on copie, la page y joint le type `ORIGIN` : un JSON `{v, page, mac}`.
//   `mac` est un HMAC-SHA-256, sous le secret du navigateur, du compte, de la page, du **jeton de ce
//   geste** et de la liste de ces fichiers. Le presse-papiers de BlockNote (`blocknote/html`,
//   `text/html`, `text/plain`) n'est pas touché ; une autre application ignore ce type.
// - **Le secret ne sort jamais du navigateur** : ni dans la marque, ni vers le serveur, ni dans une
//   page. Tout site où l'on colle lit tout le presse-papiers : un secret écrit tel quel y serait lu, et
//   resservi.
// - **Le jeton de la dernière copie, et celui de la dernière prise au glisser** (R776) : tirés au hasard
//   chaque fois que l'éditeur écrit des blocs dans le presse-papiers ou dans un glisser (`taking`),
//   gardés à côté du secret, jamais écrits dans la marque. Au collage, la marque n'est admise que sous le
//   jeton de la dernière copie ; au dépôt, sous celui de la dernière prise. Le presse-papiers ne tient
//   qu'une copie : un collage honnête porte toujours la dernière, autant de fois qu'on veut, d'un onglet
//   à l'autre. Un site qui a reçu un collage ne peut resservir sa marque que tant que la personne n'a
//   copié aucun autre bloc dans Mementum (limite écrite, R750 récrit).
// - **Le secret et les jetons** : par compte, dans `localStorage` (ils valent pour tous les onglets du
//   navigateur). Effacés à la déconnexion, et à l'entrée d'un autre compte (la règle de R297) ; créés
//   seulement par un onglet où le compte est entré (`writesFor`) ; **neufs à chaque session que l'onglet
//   ouvre par un formulaire** (`renewClipboardSecret`, R777 : une session perdue sans déconnexion ne
//   laisse pas son secret au compte qui revient). Stockage refusé : en mémoire, pour l'onglet seul.
// - **La vérification** (`vouches`) : la marque doit porter le HMAC de **toute** la liste des fichiers
//   de sa page que nomme le presse-papiers, sous le secret et le jeton d'ici. Un fichier d'une autre page
//   que celle de la marque n'est jamais recopié. Une marque d'une autre version (`v: 1`) n'est pas lue.
//
// Le HMAC est calculé d'un seul tenant (la copie n'attend rien) : `crypto.subtle` est asynchrone, et
// absent d'une instance servie en HTTP hors de la boucle locale.
import { digest } from "lib0/hash/sha256";
import { writesFor } from "../space/recents";

/** Le type de la marque, dans un presse-papiers ou un glisser. */
export const ORIGIN = "application/x-mementum-origin";

/** La version de la marque : la seule lue. */
const VERSION = 2;

const PREFIX = "mementum.clipboard.";
const HEX64 = /^[0-9a-f]{64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
/** Au-delà, une marque n'est pas lue. */
const MAX_MARK_CHARS = 512;

/** Par où un bloc voyage : le presse-papiers (copier, couper, coller) ou un glisser (prendre, déposer). */
export type Way = "copy" | "drag";

/** Ce que le navigateur garde pour un compte : le secret, et le jeton du dernier geste de chaque sorte. */
type Kept = { secret: string; copy: string | null; drag: string | null };

/** Ce qui est gardé en mémoire quand le stockage est refusé, par compte. */
const volatile = new Map<string, Kept>();

const hex = (bytes: Uint8Array) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

function random(): string {
  return hex(crypto.getRandomValues(new Uint8Array(32)));
}

const token = (value: unknown): string | null => (typeof value === "string" && HEX64.test(value) ? value : null);

/** Ce que dit la valeur gardée `raw` ; `null` : rien de lisible (une autre forme, une valeur abîmée). */
function parse(raw: string | null): Kept | null {
  if (raw === null) return null;
  try {
    const kept: unknown = JSON.parse(raw);
    if (typeof kept !== "object" || kept === null) return null;
    const { secret, copy, drag } = kept as Record<string, unknown>;
    if (token(secret) === null) return null;
    return { secret: secret as string, copy: token(copy), drag: token(drag) };
  } catch {
    return null;
  }
}

/**
 * Ce que le navigateur garde pour le compte `userId`, changé par `change` s'il est donné ; créé s'il
 * manque et que le compte est entré dans l'onglet. `null` : rien (un onglet resté ouvert après la
 * déconnexion ne recrée ni n'écrit rien).
 */
function kept(userId: string, change?: (kept: Kept) => Kept): Kept | null {
  const key = PREFIX + userId;
  const writes = writesFor(userId);
  try {
    let stored = parse(localStorage.getItem(key));
    if (stored === null || change !== undefined) {
      if (!writes) return change === undefined ? stored : null;
      const next = stored ?? { secret: random(), copy: null, drag: null };
      localStorage.setItem(key, JSON.stringify(change === undefined ? next : change(next)));
      // Relu : deux onglets qui l'écrivent au même instant gardent ce qui est resté.
      stored = parse(localStorage.getItem(key));
    }
    if (stored !== null) return stored;
  } catch {
    // stockage refusé : ne vaut que dans l'onglet
  }
  if (!writes) return null;
  let own = volatile.get(userId) ?? { secret: random(), copy: null, drag: null };
  if (change !== undefined) own = change(own);
  volatile.set(userId, own);
  return own;
}

/** Le secret du navigateur pour le compte `userId` ; `null` : il n'y en a pas. */
export function clipboardSecret(userId: string): string | null {
  return kept(userId)?.secret ?? null;
}

function removeKeys(doomed: (key: string) => boolean): void {
  try {
    const keys: string[] = [];
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key !== null && key.startsWith(PREFIX) && doomed(key)) keys.push(key);
    }
    for (const key of keys) localStorage.removeItem(key);
  } catch {
    // stockage refusé : rien n'y était gardé
  }
}

/** Le compte `userId` entre dans l'onglet : ce qui est gardé pour les autres comptes quitte le navigateur. */
export function forgetOtherClipboardSecrets(userId: string): void {
  removeKeys((key) => key !== PREFIX + userId);
  for (const other of [...volatile.keys()]) if (other !== userId) volatile.delete(other);
}

/** Le compte `userId` se déconnecte : son secret est effacé, et ce qu'il a copié n'est plus reconnu. */
export function forgetClipboardSecret(userId: string): void {
  removeKeys((key) => key === PREFIX + userId);
  volatile.delete(userId);
}

/**
 * L'onglet ouvre une session du compte `userId` par un formulaire (connexion, inscription, session
 * rouverte par-dessus l'application) : son secret est tiré à neuf au prochain usage, pour tous les
 * onglets du navigateur (R777). Ce qui a été copié avant n'est plus reconnu.
 */
export function renewClipboardSecret(userId: string): void {
  forgetClipboardSecret(userId);
}

const encoder = new TextEncoder();
const BLOCK = 64;

/** HMAC-SHA-256 (RFC 2104) de `message` sous `key`, en hexadécimal. */
export function hmac(key: Uint8Array, message: Uint8Array): string {
  const padded = new Uint8Array(BLOCK);
  padded.set(key.length > BLOCK ? digest(key) : key);
  const inner = new Uint8Array(BLOCK + message.length);
  const outer = new Uint8Array(BLOCK + 32);
  for (let i = 0; i < BLOCK; i += 1) {
    inner[i] = padded[i] ^ 0x36;
    outer[i] = padded[i] ^ 0x5c;
  }
  inner.set(message, BLOCK);
  outer.set(digest(inner), BLOCK);
  return hex(digest(outer));
}

/** Qui copie ou colle : le compte de l'application, le secret de son navigateur, et le jeton du dernier geste. */
export type Copier = { account: string; secret: string; token: string };

/** Ce que la marque signe : le compte, la page, le jeton du geste, et les chemins des fichiers, sans doublon, triés. */
function signed(by: Copier, pageId: string, paths: Iterable<string>): string {
  const files = [...new Set(Array.from(paths, (path) => path.toLowerCase()))].sort();
  const message = [`mementum-origin-${VERSION}`, by.account.toLowerCase(), pageId.toLowerCase(), by.token, ...files].join("\n");
  return hmac(encoder.encode(by.secret), encoder.encode(message));
}

/** La marque d'un bloc copié depuis `pageId`, qui nomme les fichiers `paths` de cette page. */
export function originMark(by: Copier, pageId: string, paths: Iterable<string>): string {
  return JSON.stringify({ v: VERSION, page: pageId.toLowerCase(), mac: signed(by, pageId, paths) });
}

/** La page que nomme la marque `raw`, en minuscules ; `null` : ce n'est pas une marque. */
export function markedPage(raw: string): string | null {
  return read(raw)?.page ?? null;
}

function read(raw: string): { page: string; mac: string } | null {
  if (raw === "" || raw.length > MAX_MARK_CHARS) return null;
  let mark: unknown;
  try {
    mark = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof mark !== "object" || mark === null) return null;
  const { v, page, mac } = mark as Record<string, unknown>;
  if (v !== VERSION || typeof page !== "string" || typeof mac !== "string") return null;
  if (!UUID.test(page) || !HEX64.test(mac)) return null;
  return { page, mac };
}

/**
 * La marque `raw` est-elle celle de `by` pour exactement les fichiers `paths` de sa page ? Faux pour
 * une marque d'un autre navigateur, d'un autre compte, d'avant une déconnexion, d'un geste plus ancien
 * que le dernier, ou d'une autre liste.
 */
export function vouches(raw: string, by: Copier, paths: Iterable<string>): boolean {
  const mark = read(raw);
  if (mark === null) return false;
  return signed(by, mark.page, paths) === mark.mac;
}

/**
 * Qui **pose** dans cet onglet ce qui est venu par `way` : le compte `account` (`null` : aucun), son
 * secret, et le jeton de son dernier geste de cette sorte ; `null` : personne, ou aucun geste encore.
 */
export function copier(account: string | null, way: Way): Copier | null {
  if (account === null) return null;
  const own = kept(account);
  const last = own?.[way] ?? null;
  return own === null || last === null ? null : { account, secret: own.secret, token: last };
}

/**
 * L'éditeur vient d'écrire des blocs dans le presse-papiers ou dans un glisser (`way`) : un jeton neuf
 * pour ce geste, gardé à la place de celui d'avant (ce qui a été copié avant n'est plus reconnu), et qui
 * le signe. `null` : personne (aucun compte, ou un onglet qui n'écrit pas pour lui).
 */
export function taking(account: string | null, way: Way): Copier | null {
  if (account === null) return null;
  const fresh = random();
  const own = kept(account, (before) => ({ ...before, [way]: fresh }));
  const last = own?.[way] ?? null;
  // Relu : le jeton gardé est celui qui est resté (un autre onglet a pu copier au même instant).
  return own === null || last === null ? null : { account, secret: own.secret, token: last };
}
