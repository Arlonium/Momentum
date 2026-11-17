// Un lien de plus de 16 Kio n'est pas posé (Ruling R495, constat N3 de la re-relecture du tour 5
// de la piste S).
//
// La garde des mises à jour du serveur (`MARKS`, `crates/mementum-crdt/src/guard.rs`, R431) refuse
// une marque `link` dont l'adresse dépasse `MAX_HREF_BYTES` (16 Kio) : la connexion est fermée,
// y-websocket se reconnecte et renvoie les mêmes éléments, refusés de nouveau, sans fin, et tout
// ce que l'onglet tape ensuite reste local. L'éditeur refuse donc lui-même toute transaction qui
// pose un tel lien (la barre du lien, un collage, un lien reconnu en tapant), et le dit. Ce qui
// arrive du document partagé (y-prosemirror) n'est jamais refusé : le serveur l'a déjà jugé.
//
// Limite connue (re-relecture du tour 6, écrite au registre du lot 3, tour de correction 7) : le
// filtre juge les marques ajoutées et les contenus insérés, pas une coupure de paragraphe (Entrée)
// au milieu d'un lien trop long déjà présent. ProseMirror n'y ajoute aucune marque et n'insère
// aucun contenu marqué, mais y-prosemirror écrit le texte d'après la coupure dans un texte neuf,
// marque comprise : le serveur la refuse, et l'onglet reste dans la boucle dite plus haut. Ce
// chemin ne s'atteint plus que par un lien trop long reçu avant la relecture du stockage qui le
// retire (Rulings R431, R490, R493) : une page servie n'en porte plus.
import { createExtension } from "@blocknote/core";
import { Plugin, PluginKey, type Transaction } from "prosemirror-state";
import { ySyncPluginKey } from "y-prosemirror";
import { attrBoundPlugin } from "./attrBound";
import { announce, dismissNotice } from "./editorNotice";

/** Longueur maximale, en octets, de l'adresse d'un lien : `MAX_HREF_BYTES` du serveur. */
export const MAX_HREF_BYTES = 16 * 1024;

/** Ce que l'éditeur dit d'un lien refusé. */
export const LINK_TOO_LONG =
  "Lien trop long : une adresse s'écrit en 16 Kio au plus. Le lien n'a pas été posé.";

/** La zone d'alerte du message (`editorNotice.ts`). */
const LINK_REFUSED = "mm-link-refused";

const encoder = new TextEncoder();

type MarkLike = { type: { name: string }; attrs: Record<string, unknown> };
type NodeLike = {
  marks: readonly MarkLike[];
  descendants(f: (node: NodeLike) => boolean | void): void;
};

function tooLong(mark: MarkLike): boolean {
  const href = mark.attrs.href;
  return mark.type.name === "link" && typeof href === "string" && encoder.encode(href).length > MAX_HREF_BYTES;
}

/** Vrai si une étape de la transaction pose un lien trop long : une marque ajoutée, ou un contenu
 * inséré (collage, remplacement) qui en porte un. */
export function addsTooLongLink(tr: Transaction): boolean {
  for (const step of tr.steps as unknown as { mark?: MarkLike; slice?: { content: NodeLike } }[]) {
    if (step.mark !== undefined && tooLong(step.mark)) return true;
    if (step.slice !== undefined) {
      let found = false;
      step.slice.content.descendants((node) => {
        if (!found && node.marks.some(tooLong)) found = true;
        return !found;
      });
      if (found) return true;
    }
  }
  return false;
}

/** Une transaction qui vient du document partagé (y-prosemirror) : déjà jugée par le serveur. */
function fromDocument(tr: Transaction): boolean {
  const meta = tr.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined;
  return meta?.isChangeOrigin === true;
}

const linkBoundKey = new PluginKey("mementum-link-bound");

/** L'extension de l'éditeur de la page : les bornes de la garde du serveur qu'un navigateur peut
 * atteindre, un lien trop long (R495) et un attribut d'un autre type que le sien (`attrBound.ts`,
 * R695). */
export const linkBound = createExtension({
  key: "linkBound",
  prosemirrorPlugins: [
    attrBoundPlugin(),
    new Plugin({
      key: linkBoundKey,
      filterTransaction(tr) {
        if (!tr.docChanged || fromDocument(tr) || !addsTooLongLink(tr)) return true;
        announce(LINK_REFUSED, LINK_TOO_LONG);
        return false;
      },
      view: () => ({ destroy: () => dismissNotice(LINK_REFUSED) }),
    }),
  ],
});
