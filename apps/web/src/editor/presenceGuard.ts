// La garde du navigateur contre un état de présence mal formé (report du lot 2, Ruling R129).
//
// Le serveur ne relaie un état de présence que s'il a la forme de ceux que le client écrit
// (Rulings R119 et R123, `crates/mementum-sync/src/presence.rs`). Jusqu'ici le navigateur s'y
// fiait en entier : un état mal formé qui l'atteignait quand même (serveur plus ancien, règle
// du serveur qui lirait un état autrement que lui) cassait son éditeur. La couleur d'un
// curseur est posée telle quelle dans un attribut `style`, par BlockNote pour le curseur et
// par y-prosemirror pour la sélection ; et la position d'un curseur est décodée sans garde à
// chaque changement venu du réseau (y-prosemirror, `createDecorations`). Mal formée, elle lève
// une exception dans la transaction qui applique ce changement : l'éditeur n'affiche plus ce
// qu'écrivent les autres, et la frappe suivante récrit le document d'après ce qu'il affiche,
// en effaçant ce qu'ils avaient écrit entre-temps (e2e
// `malformed_presence_that_reaches_a_browser_is_not_rendered`).
//
// L'éditeur ne reçoit donc pas l'awareness de y-websocket, mais une vue qui ne montre des
// autres clients que les états en règle. La règle est celle du serveur, relue ici sur l'état
// déjà décodé : **un champ ajouté à la présence de l'éditeur se déclare aux deux endroits**,
// sinon l'état entier est ignoré, et plus personne ne voit le curseur de personne.
import type { Awareness } from "y-protocols/awareness";
import { FRAGMENT } from "./collab";

/** Longueur maximale du nom annoncé, en caractères : celle d'un nom de compte. */
const MAX_NAME_CHARS = 100;
/** Plus grande horloge de Yjs : 32 bits. */
const MAX_CLOCK = 0xffff_ffff;
/** `#` et six chiffres hexadécimaux : la seule forme que y-prosemirror tient pour une couleur. */
const COLOUR = /^#[0-9a-fA-F]{6}$/;
/** Un caractère de contrôle (catégorie Cc), comme `char::is_control` côté serveur. */
const CONTROL = /\p{Cc}/u;

type Fields = Record<string, unknown>;

/** L'objet `value`, s'il en est un et n'a pas d'autre clé que `allowed`. */
function onlyKeys(value: unknown, allowed: readonly string[]): Fields | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return Object.keys(value).every((key) => allowed.includes(key)) ? (value as Fields) : null;
}

/** La clé `key` de `fields` est absente, ou sa valeur passe `check`. */
function absentOr(fields: Fields, key: string, check: (value: unknown) => boolean): boolean {
  return !Object.hasOwn(fields, key) || check(fields[key]);
}

function isIntegerIn(value: unknown, min: number, max: number): boolean {
  return typeof value === "number" && Number.isInteger(value) && value >= min && value <= max;
}

function isName(value: unknown): boolean {
  if (typeof value !== "string" || CONTROL.test(value)) return false;
  const chars = [...value].length;
  return chars >= 1 && chars <= MAX_NAME_CHARS;
}

function isColour(value: unknown): boolean {
  // `$` admet un saut de ligne final : la longueur l'écarte.
  return typeof value === "string" && value.length === 7 && COLOUR.test(value);
}

function isUser(value: unknown): boolean {
  const user = onlyKeys(value, ["name", "color"]);
  return user !== null && absentOr(user, "name", isName) && absentOr(user, "color", isColour);
}

/** Un identifiant de Yjs : `{client, clock}`, sans autre clé. */
function isId(value: unknown): boolean {
  const id = onlyKeys(value, ["client", "clock"]);
  return (
    id !== null &&
    isIntegerIn(id.client, 0, Number.MAX_SAFE_INTEGER) &&
    isIntegerIn(id.clock, 0, MAX_CLOCK)
  );
}

/** Une position relative de Yjs, telle que `relativePositionToJSON` l'écrit. */
function isRelativePosition(value: unknown): boolean {
  const position = onlyKeys(value, ["type", "tname", "item", "assoc"]);
  if (position === null) return false;
  const { type, tname, item } = position;
  // Yjs ne sait rien faire d'une position qui ne nomme ni élément, ni type : il lève une
  // exception (`createAbsolutePositionFromRelativePosition`, `unexpectedCase`).
  if (type == null && tname == null && item == null) return false;
  return (
    (type == null || isId(type)) &&
    (item == null || isId(item)) &&
    (tname == null || tname === FRAGMENT) &&
    absentOr(position, "assoc", (assoc) => isIntegerIn(assoc, -(2 ** 31), 2 ** 31 - 1))
  );
}

function isCursor(value: unknown): boolean {
  if (value === null) return true;
  const cursor = onlyKeys(value, ["anchor", "head"]);
  // Les deux : y-prosemirror lit `anchor` puis `head`, sans regarder s'ils sont là.
  return cursor !== null && isRelativePosition(cursor.anchor) && isRelativePosition(cursor.head);
}

/**
 * `state`, l'état de présence d'un autre client une fois décodé, a la forme de ceux que le
 * client écrit : un objet dont les seules clés possibles sont `user` (`name` et `color`,
 * chacun facultatif) et `cursor` (`null`, ou `{anchor, head}`, deux positions relatives de
 * Yjs). C'est la règle du serveur (`known_shape`), sans sa borne de taille.
 */
export function knownShape(state: unknown): boolean {
  const fields = onlyKeys(state, ["user", "cursor"]);
  return fields !== null && absentOr(fields, "user", isUser) && absentOr(fields, "cursor", isCursor);
}

/** Les états de `awareness` que l'éditeur peut lire : le sien, et ceux des autres en règle. */
function shownStates(awareness: Awareness): Map<number, Fields> {
  const shown = new Map<number, Fields>();
  for (const [client, state] of awareness.getStates()) {
    if (client === awareness.clientID || knownShape(state)) shown.set(client, state);
  }
  return shown;
}

/**
 * `awareness`, vue par l'éditeur : la même, sauf que `getStates()` et `states` ne montrent
 * des autres clients que les états en règle. Un état mal formé y est comme absent : ni
 * curseur, ni sélection. Ce que l'éditeur écrit (son nom, son curseur) et ce à quoi il
 * s'abonne vont à l'awareness de y-websocket, inchangés.
 */
export function guardedAwareness(awareness: Awareness): Awareness {
  return new Proxy(awareness, {
    get(target, property) {
      if (property === "getStates") return () => shownStates(target);
      if (property === "states") return shownStates(target);
      const value: unknown = Reflect.get(target, property, target);
      // Liées à la vraie awareness : ses méthodes lisent `this.states`, sans filtre.
      return typeof value === "function" ? value.bind(target) : value;
    },
  });
}
