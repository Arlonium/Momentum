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
// La règle est celle du serveur, pour ce qu'un navigateur peut écrire : `TYPED` nomme les
// attributs qui ne sont pas des chaînes (un test le compare au registre, `attrBound.test.ts`) ;
// tout autre attribut est une chaîne, un booléen ou un nombre fini. `null` et `undefined` passent
// partout : y-prosemirror n'écrit pas le premier, et le serveur garde le second.
import { Plugin, PluginKey, type Transaction } from "prosemirror-state";
import { ySyncPluginKey } from "y-prosemirror";
import { announce, dismissNotice } from "./editorNotice";

/** Colonnes qu'une cellule fusionne au plus : `MAX_WIDTH_COLUMNS` du serveur, la borne du MMD. */
export const MAX_COLSPAN = 100;

/** Ce que l'éditeur dit d'un contenu refusé. */
export const ATTR_REFUSED =
  "Ce contenu porte une valeur que Mementum ne garde pas (une fusion de cellules de plus de " +
  "100 colonnes, par exemple) : il n'a pas été inséré.";

/** La zone d'alerte du message (`editorNotice.ts`). */
const ATTR_REFUSED_NOTICE = "mm-attr-refused";

/** Ce qu'un attribut qui n'est pas une chaîne doit être. */
export type Kind = "bool" | "int" | "float" | "colspan" | "rowspan" | "widths";

const CELL: Record<string, Kind> = { colspan: "colspan", rowspan: "rowspan", colwidth: "widths" };

/** Les attributs qui ne sont pas des chaînes, par nœud : ceux du registre des blocs
 * (`spec/schema/blocks.json`), et ceux d'une cellule de tableau. */
export const TYPED: Record<string, Record<string, Kind>> = {
  heading: { level: "int", isToggleable: "bool" },
  numberedListItem: { start: "int" },
  checkListItem: { checked: "bool" },
  image: { showPreview: "bool", previewWidth: "float" },
  video: { showPreview: "bool", previewWidth: "float" },
  audio: { showPreview: "bool" },
  column: { width: "float" },
  unknown: { foreign: "bool" },
  tableCell: CELL,
  tableHeader: CELL,
};

const finite = (value: unknown): boolean => typeof value === "number" && Number.isFinite(value);

function admits(kind: Kind | undefined, value: unknown): boolean {
  if (value === null || value === undefined) return true;
  switch (kind) {
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
      return (
        Array.isArray(value) &&
        value.length <= MAX_COLSPAN &&
        value.every((width) => width === null || width === undefined || finite(width))
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

function unsound(node: NodeLike): boolean {
  for (const name in node.attrs) {
    if (!soundAttr(node.type.name, name, node.attrs[name])) return true;
  }
  return false;
}

/** Vrai si une étape de la transaction pose un attribut que le serveur refuserait : un contenu
 * inséré (collage, remplacement, un nœud dont les attributs changent), ou un attribut posé seul. */
export function addsUnsoundAttr(tr: Transaction): boolean {
  const steps = tr.steps as unknown as {
    slice?: { content: NodeLike };
    attr?: string;
    value?: unknown;
    pos?: number;
  }[];
  for (const [index, step] of steps.entries()) {
    if (step.slice !== undefined) {
      let found = false;
      step.slice.content.descendants((node) => {
        if (!found && unsound(node)) found = true;
        return !found;
      });
      if (found) return true;
    }
    if (typeof step.attr === "string" && typeof step.pos === "number") {
      const node = tr.docs[index]?.nodeAt(step.pos);
      if (node && !soundAttr(node.type.name, step.attr, step.value)) return true;
    }
  }
  return false;
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
      if (!tr.docChanged || fromDocument(tr) || !addsUnsoundAttr(tr)) return true;
      announce(ATTR_REFUSED_NOTICE, ATTR_REFUSED);
      return false;
    },
    view: () => ({ destroy: () => dismissNotice(ATTR_REFUSED_NOTICE) }),
  });
}
