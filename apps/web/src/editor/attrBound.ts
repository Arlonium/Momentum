// Un attribut que la garde du serveur refuserait n'est pas écrit (Ruling R695, sur le modèle de
// R495 pour les liens).
//
// La garde des attributs du serveur (`crates/mementum-crdt/src/attrs.rs`, Rulings R689 à R692)
// refuse une mise à jour qui pose sur un nœud un attribut qui n'est pas du type que dit le
// registre des blocs : la connexion est fermée, y-websocket se reconnecte et renvoie les mêmes
// éléments, refusés de nouveau, sans fin, et tout ce que l'onglet tape ensuite reste local.
// Un navigateur atteint ces refus sans client hostile, par un collage : BlockNote 0.57 lit le
// `colspan` et le `rowspan` d'une cellule collée tels que le HTML les écrit (`colspan="abc"`
// reste une chaîne, `colspan="0"` et `colspan="5000"` des nombres), ses largeurs par `parseInt`
// (`colwidth="abc"` donne `[NaN]`), et le niveau d'un titre par `parseFloat` (`2.5`, `1e21`). Et
// par un geste : fusionner plus de cent colonnes.
//
// L'éditeur refuse donc lui-même toute transaction de l'onglet qui pose un tel attribut, et le
// dit. Ce qui arrive du document partagé (y-prosemirror) n'est jamais refusé : le serveur l'a
// déjà jugé.
//
// La règle est celle du serveur : `TYPED` est bâtie du registre des blocs, comme sa table ; un
// attribut hors du registre est une chaîne, un booléen ou un nombre fini ; une largeur `NaN` est
// une largeur absente (ce que BlockNote écrit pour une cellule fusionnée collée, R705). `null` et `undefined` passent
// partout : y-prosemirror n'écrit pas le premier, et le serveur garde le second.
import { Plugin, PluginKey, type Transaction } from "prosemirror-state";
import { ySyncPluginKey } from "y-prosemirror";
import registry from "../../../../spec/schema/blocks.json";
import { announce, dismissNotice } from "./editorNotice";

/** Colonnes qu'une cellule fusionne au plus : `MAX_WIDTH_COLUMNS` du serveur, la borne du MMD. */
export const MAX_COLSPAN = 100;

/** Ce que l'éditeur dit d'un contenu refusé pour une fusion de plus de cent colonnes. */
export const SPAN_REFUSED =
  "Une cellule fusionne 100 colonnes au plus : ce contenu n'a pas été inséré.";

/** Ce que l'éditeur dit d'un contenu refusé pour une autre valeur. */
export const ATTR_REFUSED =
  "Ce contenu porte une valeur que Mementum ne garde pas : il n'a pas été inséré.";

/** La zone d'alerte du message (`editorNotice.ts`). */
const ATTR_REFUSED_NOTICE = "mm-attr-refused";

/** Ce qu'un attribut du registre doit être. */
export type Kind = "string" | "bool" | "int" | "float" | "colspan" | "rowspan" | "widths";

const KINDS: Record<string, Kind> = { string: "string", bool: "bool", int: "int", float: "float" };

/** Ce que doit être chaque attribut de chaque nœud, par nœud : la table du serveur
 * (`crates/mementum-crdt/src/attrs.rs`), bâtie comme elle du registre des blocs, plus les
 * attributs qu'il ne porte pas (`id`, `colwidth`, mention, formule en ligne). Un attribut qui
 * n'y est pas est une chaîne, un booléen ou un nombre fini. */
export const TYPED: Record<string, Record<string, Kind>> = (() => {
  const table: Record<string, Record<string, Kind>> = {};
  for (const block of registry.blocks) {
    const attrs: Record<string, Kind> = {};
    for (const prop of block.props) attrs[prop.name] = KINDS[prop.type];
    table[block.name] = attrs;
  }
  for (const unwrapped of ["columnList", "column"]) table[unwrapped].id = "string";
  table.blockContainer = { id: "string" };
  const cell: Record<string, Kind> = {};
  for (const prop of registry.tableCell.props) cell[prop.name] = KINDS[prop.type];
  Object.assign(cell, { colspan: "colspan", rowspan: "rowspan", colwidth: "widths" });
  table.tableCell = cell;
  table.tableHeader = cell;
  table.mention = { kind: "string", id: "string", label: "string" };
  table.math = { latex: "string" };
  return table;
})();

const finite = (value: unknown): boolean => typeof value === "number" && Number.isFinite(value);

function admits(kind: Kind | undefined, value: unknown): boolean {
  if (value === null || value === undefined) return true;
  switch (kind) {
    case "string":
      return typeof value === "string";
    case "bool":
      return typeof value === "boolean";
    case "int":
      return Number.isSafeInteger(value);
    case "float":
      return finite(value);
    case "colspan":
      return Number.isSafeInteger(value) && (value as number) >= 1 && (value as number) <= MAX_COLSPAN;
    case "rowspan":
      return Number.isSafeInteger(value) && (value as number) >= 1;
    case "widths":
      // `NaN` est une largeur absente : BlockNote l'écrit pour toute cellule fusionnée collée
      // sans largeurs (Ruling R705).
      return (
        Array.isArray(value) &&
        value.length <= MAX_COLSPAN &&
        value.every((width) => width === null || width === undefined || finite(width) || Number.isNaN(width))
      );
    default:
      return typeof value === "string" || typeof value === "boolean" || finite(value);
  }
}

/** Vrai si le serveur garderait l'attribut `name` de valeur `value` sur un nœud `node`. */
export function soundAttr(node: string, name: string, value: unknown): boolean {
  return admits(TYPED[node]?.[name], value);
}

type NodeLike = {
  type: { name: string };
  attrs: Record<string, unknown>;
  descendants(f: (node: NodeLike) => boolean | void): void;
};

/** Le premier attribut de `node` que le serveur refuserait, « nœud.attribut ». */
function unsound(node: NodeLike): string | null {
  for (const name in node.attrs) {
    if (!soundAttr(node.type.name, name, node.attrs[name])) return `${node.type.name}.${name}`;
  }
  return null;
}

/** Vrai si une étape de la transaction pose un attribut que le serveur refuserait : un contenu
 * inséré (collage, remplacement, un nœud dont les attributs changent), ou un attribut posé seul. */
export function addsUnsoundAttr(tr: Transaction): boolean {
  return unsoundAttrOf(tr) !== null;
}

/** L'attribut (« nœud.attribut ») que pose une étape de la transaction et que le serveur
 * refuserait, s'il y en a un. */
export function unsoundAttrOf(tr: Transaction): string | null {
  const steps = tr.steps as unknown as {
    slice?: { content: NodeLike };
    attr?: string;
    value?: unknown;
    pos?: number;
  }[];
  for (const [index, step] of steps.entries()) {
    if (step.slice !== undefined) {
      let found: string | null = null;
      step.slice.content.descendants((node) => {
        found ??= unsound(node);
        return found === null;
      });
      if (found !== null) return found;
    }
    if (typeof step.attr === "string" && typeof step.pos === "number") {
      const node = tr.docs[index]?.nodeAt(step.pos);
      if (node && !soundAttr(node.type.name, step.attr, step.value)) {
        return `${node.type.name}.${step.attr}`;
      }
    }
  }
  return null;
}

/** Une transaction qui vient du document partagé (y-prosemirror) : déjà jugée par le serveur. */
function fromDocument(tr: Transaction): boolean {
  const meta = tr.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined;
  return meta?.isChangeOrigin === true;
}

const attrBoundKey = new PluginKey("mementum-attr-bound");

/** Le greffon de l'éditeur de la page, posé avec celui des liens (`linkBound.ts`). */
export function attrBoundPlugin(): Plugin {
  return new Plugin({
    key: attrBoundKey,
    filterTransaction(tr) {
      if (!tr.docChanged || fromDocument(tr)) return true;
      const refused = unsoundAttrOf(tr);
      if (refused === null) return true;
      announce(ATTR_REFUSED_NOTICE, refused.endsWith(".colspan") ? SPAN_REFUSED : ATTR_REFUSED);
      return false;
    },
    view: () => ({ destroy: () => dismissNotice(ATTR_REFUSED_NOTICE) }),
  });
}
