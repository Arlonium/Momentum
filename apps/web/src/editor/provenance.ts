// La provenance d'un bloc copié ou glissé (relecture finale de la partie B, R746 à R752).
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
//   `mac` est un HMAC-SHA-256, sous le secret du navigateur, du compte, de la page et de la liste de
//   ces fichiers. Le presse-papiers de BlockNote (`blocknote/html`, `text/html`, `text/plain`) n'est
//   pas touché ; une autre application ignore ce type.
// - **Le secret ne sort jamais du navigateur** : ni dans la marque, ni vers le serveur, ni dans une
//   page. Tout site où l'on colle lit tout le presse-papiers : un secret écrit tel quel y serait lu, et
//   resservi. Un site qui a reçu un collage ne peut que rejouer la marque reçue, pour les fichiers
//   qu'elle nomme (limite écrite, R750).
// - **Le secret** : 32 octets tirés au hasard, par compte, dans `localStorage` (il vaut pour tous les
//   onglets du navigateur). Effacé à la déconnexion, et à l'entrée d'un autre compte (la règle de
//   R297) ; créé seulement par un onglet où le compte est entré (`writesFor`). Stockage refusé : un
//   secret en mémoire, qui ne vaut que dans l'onglet.
// - **La vérification** (`vouchedPage`) : au collage et au dépôt, la marque doit porter le HMAC de
//   **toute** la liste des fichiers de sa page que nomme le presse-papiers, sous le secret d'ici.
//   Un fichier d'une autre page que celle de la marque n'est jamais recopié.
//
// Le HMAC est calculé d'un seul tenant (la copie n'attend rien) : `crypto.subtle` est asynchrone, et
// absent d'une instance servie en HTTP hors de la boucle locale.
import { digest } from "lib0/hash/sha256";
import { writesFor } from "../space/recents";

/** Le type de la marque, dans un presse-papiers ou un glisser. */
export const ORIGIN = "application/x-mementum-origin";

const PREFIX = "mementum.clipboard.";
const SECRET = /^[0-9a-f]{64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
/** Au-delà, une marque n'est pas lue. */
const MAX_MARK_CHARS = 512;

/** Les secrets gardés en mémoire quand le stockage est refusé, par compte. */
const volatile = new Map<string, string>();

const hex = (bytes: Uint8Array) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

function random(): string {
  return hex(crypto.getRandomValues(new Uint8Array(32)));
}

/**
 * Le secret du navigateur pour le compte `userId` ; créé s'il manque et que le compte est entré dans
 * l'onglet. `null` : il n'y en a pas (un onglet resté ouvert après la déconnexion n'en recrée pas).
 */
export function clipboardSecret(userId: string): string | null {
  const key = PREFIX + userId;
  try {
    const stored = localStorage.getItem(key);
    if (stored !== null && SECRET.test(stored)) return stored;
    if (!writesFor(userId)) return null;
    localStorage.setItem(key, random());
    // Relu : deux onglets qui le créent au même instant gardent celui qui est resté.
    const kept = localStorage.getItem(key);
    if (kept !== null && SECRET.test(kept)) return kept;
  } catch {
    // stockage refusé : le secret ne vaut que dans l'onglet
  }
  if (!writesFor(userId)) return null;
  let secret = volatile.get(userId);
  if (secret === undefined) volatile.set(userId, (secret = random()));
  return secret;
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

/** Le compte `userId` entre dans l'onglet : les secrets des autres comptes quittent le navigateur. */
export function forgetOtherClipboardSecrets(userId: string): void {
  removeKeys((key) => key !== PREFIX + userId);
  for (const other of [...volatile.keys()]) if (other !== userId) volatile.delete(other);
}

/** Le compte `userId` se déconnecte : son secret est effacé, et ce qu'il a copié n'est plus reconnu. */
export function forgetClipboardSecret(userId: string): void {
  removeKeys((key) => key === PREFIX + userId);
  volatile.delete(userId);
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

/** Ce que la marque signe : le compte, la page, et les chemins des fichiers, sans doublon, dans l'ordre. */
function signed(secret: string, account: string, pageId: string, paths: Iterable<string>): string {
  const files = [...new Set(Array.from(paths, (path) => path.toLowerCase()))].sort();
  const message = ["mementum-origin-1", account.toLowerCase(), pageId.toLowerCase(), ...files].join("\n");
  return hmac(encoder.encode(secret), encoder.encode(message));
}

/** Qui copie : le compte de l'application, et le secret de son navigateur. */
export type Copier = { account: string; secret: string };

/** La marque d'un bloc copié depuis `pageId`, qui nomme les fichiers `paths` de cette page. */
export function originMark(by: Copier, pageId: string, paths: Iterable<string>): string {
  return JSON.stringify({ v: 1, page: pageId.toLowerCase(), mac: signed(by.secret, by.account, pageId, paths) });
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
  if (v !== 1 || typeof page !== "string" || typeof mac !== "string") return null;
  if (!UUID.test(page) || !SECRET.test(mac)) return null;
  return { page, mac };
}

/**
 * La marque `raw` est-elle celle de `by` pour exactement les fichiers `paths` de sa page ? Faux pour
 * une marque d'un autre navigateur, d'un autre compte, d'avant une déconnexion, ou d'une autre liste.
 */
export function vouches(raw: string, by: Copier, paths: Iterable<string>): boolean {
  const mark = read(raw);
  if (mark === null) return false;
  return signed(by.secret, by.account, mark.page, paths) === mark.mac;
}

/** Qui copie dans cet onglet : le compte `account` (`null` : aucun) et son secret ; `null` : personne. */
export function copier(account: string | null): Copier | null {
  if (account === null) return null;
  const secret = clipboardSecret(account);
  return secret === null ? null : { account, secret };
}
