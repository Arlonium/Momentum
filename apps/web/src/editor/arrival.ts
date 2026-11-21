// Le bloc où l'on arrive est surligné (correction 11 de la recette du lot 4) : à chaque arrivée par
// `goToBlock` (un résultat de recherche, un rétrolien à un bloc, un lien de la table des matières,
// une adresse `#b=` ouverte), le contenu propre du bloc atteint reçoit un fond d'accent qui s'efface
// en deux secondes (`.mm-arrived`, `theme.css`). On voit où l'on est arrivé sans le chercher.
//
// Une décoration de ProseMirror, jamais une classe posée à la main : la vue d'un bloc est à
// ProseMirror, qui relit tout attribut changé sous lui (BlockNote ne fait taire que ceux de Dark
// Reader) et recréerait le bloc. La décoration n'écrit rien dans le document : la transaction qui la
// pose ou la retire ne porte qu'une méta (y-prosemirror n'y trouve rien à écrire, comme pour une
// sélection), hors de l'historique de l'annulation.
//
// - Le contenu propre du bloc (son premier enfant, `blockContent`), pas le bloc entier, qui porte
//   ses sous-blocs (R1113) ; un bloc sans conteneur (une colonne) l'est en entier.
// - Rien ne bouge : un fond seulement, sans taille ni marge (`theme.css`).
// - Une seconde arrivée relance l'animation : la classe alterne (`mm-arrived-0`, `mm-arrived-1`),
//   et son animation change de nom.
// - La décoration suit le document : un bloc qu'un autre déplace (supprimé puis recréé au même id,
//   R220) la perd, sans erreur.
// - Elle part au bout de `ARRIVAL_MS` ; sous `prefers-reduced-motion`, le fond reste fixe jusque-là.
import { createExtension } from "@blocknote/core";
import { Plugin, PluginKey, type EditorState, type Transaction } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";
import type { MementumEditor } from "./debug";

/** Le temps du surlignage (ms) : celui de son animation (`theme.css`). */
export const ARRIVAL_MS = 2_000;

type PmNode = EditorState["doc"];

/** Le surlignage en cours : sa décoration, et le rang de l'arrivée (une classe sur deux). */
type Lit = { set: DecorationSet; turn: number };

/** Ce qu'une transaction demande : surligner `from..to`, ou éteindre l'arrivée `off`. */
type Ask = { at: { from: number; to: number } } | { off: number };

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

/** Une transaction qui ne porte que la demande `ask` : rien du document, rien à annuler. */
function asking(tr: Transaction, ask: Ask): Transaction {
  return tr.setMeta(arrivalKey, ask).setMeta("addToHistory", false);
}

/** L'extension de l'éditeur (`EDITOR_OPTIONS`). */
export const arrival = createExtension({
  key: "arrival",
  prosemirrorPlugins: [
    new Plugin<Lit>({
      key: arrivalKey,
      state: {
        init: () => ({ set: DecorationSet.empty, turn: 0 }),
        apply(tr, lit) {
          const ask = tr.getMeta(arrivalKey) as Ask | undefined;
          if (ask !== undefined && "at" in ask) {
            const turn = lit.turn + 1;
            const decoration = Decoration.node(ask.at.from, ask.at.to, {
              class: `mm-arrived mm-arrived-${turn % 2}`,
            });
            return { turn, set: DecorationSet.create(tr.doc, [decoration]) };
          }
          if (ask !== undefined) return ask.off === lit.turn ? { turn: lit.turn, set: DecorationSet.empty } : lit;
          return tr.docChanged ? { turn: lit.turn, set: lit.set.map(tr.mapping, tr.doc) } : lit;
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
            timer = setTimeout(() => {
              if (!current.isDestroyed) current.dispatch(asking(current.state.tr, { off: turn }));
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

/** Surligne le contenu propre du bloc `id` de `editor` ; rien s'il n'y est pas. */
export function lightBlock(editor: MementumEditor, id: string): void {
  const view = editor.prosemirrorView;
  const at = ownContent(view.state.doc, id);
  if (at !== null) view.dispatch(asking(view.state.tr, { at }));
}
