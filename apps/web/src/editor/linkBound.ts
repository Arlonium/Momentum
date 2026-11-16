// Un lien de plus de 16 Kio n'est pas posé (Ruling R495, constat N3 de la re-relecture du tour 5
// de la piste S).
//
// La garde des mises à jour du serveur (`MARKS`, `crates/mementum-crdt/src/guard.rs`, R431) refuse
// une marque `link` dont l'adresse dépasse `MAX_HREF_BYTES` (16 Kio) : la connexion est fermée,
// y-websocket se reconnecte et renvoie les mêmes éléments, refusés de nouveau, sans fin, et tout
// ce que l'onglet tape ensuite reste local. L'éditeur refuse donc lui-même toute transaction qui
// pose un tel lien (la barre du lien, un collage, un lien reconnu en tapant), et le dit. Ce qui
// arrive du document partagé (y-prosemirror) n'est jamais refusé : le serveur l'a déjà jugé.
import { createExtension } from "@blocknote/core";
import { Plugin, PluginKey, type Transaction } from "prosemirror-state";
import { ySyncPluginKey } from "y-prosemirror";

/** Longueur maximale, en octets, de l'adresse d'un lien : `MAX_HREF_BYTES` du serveur. */
export const MAX_HREF_BYTES = 16 * 1024;

/** Ce que l'éditeur dit d'un lien refusé. */
export const LINK_TOO_LONG =
  "Lien trop long : une adresse s'écrit en 16 Kio au plus. Le lien n'a pas été posé.";

/** Durée d'affichage du message, en millisecondes. */
const NOTICE_MS = 6_000;

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

/** Le message, dans une zone d'alerte posée sur la page (hors de l'arbre de React). */
let notice: { element: HTMLElement; timer: ReturnType<typeof setTimeout> } | null = null;

function announce(): void {
  if (typeof document === "undefined") return;
  if (notice === null) {
    const element = document.createElement("div");
    element.className = "mm-link-refused";
    element.setAttribute("role", "alert");
    element.style.cssText =
      "position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:1000;" +
      "padding:8px 14px;border-radius:6px;background:#2f2e2b;color:#fff;font-size:14px;";
    element.textContent = LINK_TOO_LONG;
    document.body.append(element);
    notice = { element, timer: setTimeout(dismiss, NOTICE_MS) };
  } else {
    clearTimeout(notice.timer);
    notice.timer = setTimeout(dismiss, NOTICE_MS);
  }
}

function dismiss(): void {
  if (notice === null) return;
  clearTimeout(notice.timer);
  notice.element.remove();
  notice = null;
}

const linkBoundKey = new PluginKey("mementum-link-bound");

/** L'extension de l'éditeur de la page. */
export const linkBound = createExtension({
  key: "linkBound",
  prosemirrorPlugins: [
    new Plugin({
      key: linkBoundKey,
      filterTransaction(tr) {
        if (!tr.docChanged || fromDocument(tr) || !addsTooLongLink(tr)) return true;
        announce();
        return false;
      },
      view: () => ({ destroy: dismiss }),
    }),
  ],
});
