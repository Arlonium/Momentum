// Le bloc où l'on arrive est surligné (correction 11 de la recette du lot 4) : à chaque arrivée par
// `goToBlock` (un résultat de recherche, un rétrolien à un bloc, un lien de la table des matières,
// une adresse `#b=` ouverte), le contenu propre du bloc atteint reçoit un fond d'accent qui s'efface
// en deux secondes (`.mm-arrived`, `theme.css`). On voit où l'on est arrivé sans le chercher.
//
// Une décoration de ProseMirror, jamais une classe posée à la main : la vue d'un bloc est à
// ProseMirror, qui relit tout attribut changé sous lui (BlockNote ne fait taire que ceux de Dark
// Reader) et recréerait le bloc. La décoration n'écrit rien dans le document : la transaction qui la
// pose ou la retire ne porte qu'une méta (y-prosemirror n'y trouve rien à écrire, comme pour une
// sélection), et n'entre dans aucun historique d'annulation.
//
// - Le contenu propre du bloc (son premier enfant, `blockContent`), pas le bloc entier, qui porte
//   ses sous-blocs (R1113) ; un bloc sans conteneur (une colonne) l'est en entier.
// - Rien ne bouge : un fond seulement, sans taille ni marge (`theme.css`).
// - Une seconde arrivée relance l'animation : la classe alterne (`mm-arrived-0`, `mm-arrived-1`),
//   et son animation change de nom.
// - Le surlignage suit le bloc par son id (R1588) : y-prosemirror remplace tout le document à chaque
//   changement venu de Yjs (ce qu'un autre écrit, une annulation), si bien que la décoration,
//   reportée par les positions, partait au premier caractère tapé ailleurs. Après une telle
//   transaction, le bloc est cherché de nouveau par son id, pendant les deux secondes du surlignage
//   seulement : un bloc déplacé par un autre (supprimé puis recréé au même id, R220) le garde, un bloc
//   supprimé le perd, sans erreur. Une transaction d'ici (une frappe) la reporte par les positions.
// - Il part au bout de `ARRIVAL_MS` ; sous `prefers-reduced-motion`, le fond reste fixe jusque-là.
import { createExtension } from "@blocknote/core";
import { Plugin, PluginKey, type EditorState, type Transaction } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";
import { ySyncPluginKey } from "y-prosemirror";
import type { MementumEditor } from "./debug";

/** Le temps du surlignage (ms) : celui de son animation (`theme.css`). */
export const ARRIVAL_MS = 2_000;

type PmNode = EditorState["doc"];

/** Le surlignage en cours : sa décoration, le bloc surligné (`null` : aucun), le rang de l'arrivée. */
type Lit = { set: DecorationSet; id: string | null; turn: number };

/** Ce qu'une transaction demande : surligner le bloc `id`, à `from..to`, ou éteindre l'arrivée `off`. */
type Ask = { at: { from: number; to: number }; id: string } | { off: number };

const arrivalKey = new PluginKey<Lit>("mementum-arrival");

/** La place du contenu propre du bloc `id` dans `doc` ; `null` s'il n'y est pas. */
function ownContent(doc: PmNode, id: string): { from: number; to: number } | null {
  let found: { from: number; to: number } | null = null;
  doc.descendants((node, pos) => {
    if (found !== null) return false;
    if (!node.isBlock || node.attrs.id !== id) return true;
    const content = node.type.name === "blockContainer" ? node.firstChild : null;
    found = content === null ? { from: pos, to: pos + node.nodeSize } : { from: pos + 1, to: pos + 1 + content.nodeSize };
    return false;
  });
  return found;
}

/** Le surlignage du bloc `id` à `at`, à l'arrivée de rang `turn` ; éteint s'il n'est plus là. */
function lit(doc: PmNode, id: string, at: { from: number; to: number } | null, turn: number): Lit {
  if (at === null) return { set: DecorationSet.empty, id: null, turn };
  const decoration = Decoration.node(at.from, at.to, { class: `mm-arrived mm-arrived-${turn % 2}` });
  return { set: DecorationSet.create(doc, [decoration]), id, turn };
}

/** La transaction vient du document partagé, par y-prosemirror : il y a remplacé tout le document. */
function fromYjs(tr: Transaction): boolean {
  return (tr.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined)?.isChangeOrigin === true;
}

/** L'extension de l'éditeur (`EDITOR_OPTIONS`). */
export const arrival = createExtension({
  key: "arrival",
  prosemirrorPlugins: [
    new Plugin<Lit>({
      key: arrivalKey,
      state: {
        init: () => ({ set: DecorationSet.empty, id: null, turn: 0 }),
        apply(tr, current) {
          const ask = tr.getMeta(arrivalKey) as Ask | undefined;
          if (ask !== undefined && "at" in ask) return lit(tr.doc, ask.id, ask.at, current.turn + 1);
          if (ask !== undefined) {
            return ask.off === current.turn ? { set: DecorationSet.empty, id: null, turn: current.turn } : current;
          }
          if (!tr.docChanged || current.id === null) return current;
          if (fromYjs(tr)) return lit(tr.doc, current.id, ownContent(tr.doc, current.id), current.turn);
          const set = current.set.map(tr.mapping, tr.doc);
          return set.find().length === 0 ? { set, id: null, turn: current.turn } : { ...current, set };
        },
      },
      props: {
        decorations: (state) => arrivalKey.getState(state)?.set,
      },
      view: (view) => {
        let shown = arrivalKey.getState(view.state)?.turn ?? 0;
        let timer: ReturnType<typeof setTimeout> | undefined;
        return {
          update(current) {
            const turn = arrivalKey.getState(current.state)?.turn ?? 0;
            if (turn === shown) return;
            shown = turn;
            clearTimeout(timer);
            // La fin ne porte que sa méta : marquée `addToHistory: false`, y-prosemirror fermerait le pas
            // d'annulation en cours sur elle, et une frappe d'avant ne s'annulerait plus avec celle d'après
            // (R1590).
            timer = setTimeout(() => {
              if (!current.isDestroyed) current.dispatch(current.state.tr.setMeta(arrivalKey, { off: turn }));
            }, ARRIVAL_MS);
          },
          destroy() {
            clearTimeout(timer);
          },
        };
      },
    }),
  ],
});

/**
 * Surligne le contenu propre du bloc `id` de `editor` ; rien s'il n'y est pas. L'arrivée est une
 * navigation : sa transaction ferme le pas d'annulation en cours (`addToHistory: false`), si bien que
 * ce qui a été tapé avant elle ne s'annule pas avec ce qui est tapé après.
 */
export function lightBlock(editor: MementumEditor, id: string): void {
  const view = editor.prosemirrorView;
  const at = ownContent(view.state.doc, id);
  if (at !== null) view.dispatch(view.state.tr.setMeta(arrivalKey, { at, id }).setMeta("addToHistory", false));
}
