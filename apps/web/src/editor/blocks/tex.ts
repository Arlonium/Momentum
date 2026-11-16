// Ce que KaTeX produit d'une formule, mesuré **avant** d'être attaché à la page (R420, R421, R422).
// Une formule vient d'ailleurs (MMD, un agent, un autre client) : tout ce qu'une formule peut faire
// à l'onglet de qui ouvre la page se décide ici.
//
// - **Les définitions sont refusées là où elles se produisent** (`\def`, `\gdef`, `\edef`, `\let`,
//   `\newcommand`…) : KaTeX range chaque définition dans `settings.macros`, que voici sous la forme
//   d'un objet qui refuse toute définition de la formule, au moment où l'analyse la fait. Rien ne
//   relit les lexèmes : `\verb` ne cache rien (la relecture perdue de la Task C2 le contournait).
//   Une définition récursive ou en cascade figeait l'onglet. KaTeX range aussi ses propres valeurs
//   (`\cr` dans un tableau, `\current@color`, `|` sous `\middle`) : des chaînes, des fonctions, jamais
//   une définition de la formule, qui est un objet porteur de `tokens`.
// - **L'imbrication est comptée dans ce que KaTeX produit** : chaque fraction, racine, exposant ou
//   tableau imbriqué ajoute une table de mise en page (`vlist-t`, en `display: inline-table`), et
//   la mise en page de Chromium déborde sa pile sur des tables imbriquées : l'onglet plantait, sans
//   macro, pour une formule de treize niveaux en haut d'une page, de sept dans une liste imbriquée
//   de vingt-cinq niveaux (mesuré, registre, R421). La formule ne s'attache que si son imbrication
//   tient dans ce que la profondeur de la page autour d'elle laisse (`nestingAllowance`) ; sinon,
//   elle est montrée en texte, avec un message (`Formula`).
// - Une formule de plus de `MAX_LATEX_CHARS` caractères, ou dont KaTeX produirait plus de
//   `MAX_TEX_NODES` nœuds, n'est pas mise en forme.
//
// KaTeX reste à `trust: false` : ni lien, ni image, ni attribut. Le résultat est gardé, par formule,
// dans un cache borné : une formule déjà vue s'affiche sans être recalculée.
import katex from "katex";

/** Au-delà, une formule n'est pas mise en forme (le champ n'en accepte pas plus). */
export const MAX_LATEX_CHARS = 10_000;

/** Au-delà, ce que KaTeX produirait est trop grand pour la page (une formule de 10 000 caractères en fait 33 000). */
export const MAX_TEX_NODES = 60_000;

/**
 * La mise en page de Chromium plante quand la profondeur de la page autour de la formule, plus
 * `TABLE_COST` fois son imbrication de tables, dépasse 169 environ (mesuré : registre, R421). On
 * s'arrête à `DEPTH_BUDGET`, trois tables plus tôt, et jamais au-delà de `MAX_TEX_NESTING`.
 */
const DEPTH_BUDGET = 150;
const TABLE_COST = 12;
export const MAX_TEX_NESTING = 10;

/** Ce qu'une formule donne. */
export type TexOutcome =
  | { kind: "formula"; html: string; nesting: number; nodes: number }
  | { kind: "empty" }
  | { kind: "invalid"; message: string }
  | { kind: "refused"; why: "definition" | "length" | "size" };

/**
 * L'imbrication de tables qu'une formule peut avoir, à `depth` éléments du haut du document : dix en
 * haut d'une page, moins dans une liste imbriquée, aucune au-delà de 138 (une page aussi profonde
 * plante déjà sans formule, vers 180 : registre).
 */
export function nestingAllowance(depth: number): number {
  return Math.max(0, Math.min(MAX_TEX_NESTING, Math.floor((DEPTH_BUDGET - Math.max(0, depth)) / TABLE_COST)));
}

/** Une définition de la formule, refusée pendant l'analyse. */
class DefinitionRefused extends Error {}

/** Ce que KaTeX range lui-même dans `macros` ; une définition de la formule y est un objet à `tokens`. */
const isDefinition = (value: unknown) =>
  typeof value === "object" && value !== null && Array.isArray((value as { tokens?: unknown }).tokens);

/** Les macros de la formule : KaTeX y range toute définition, qui est refusée là. */
function guardedMacros(): Record<string, unknown> {
  return new Proxy<Record<string, unknown>>(
    {},
    {
      set(target, name, value) {
        if (isDefinition(value)) throw new DefinitionRefused(String(name));
        target[name as string] = value;
        return true;
      },
      defineProperty() {
        throw new DefinitionRefused("defineProperty");
      },
    },
  );
}

/** L'arbre que KaTeX construit avant d'en écrire le HTML (sans type public). */
type TexNode = { classes?: string[]; children?: TexNode[]; toMarkup(): string };
const renderToDomTree = (
  katex as unknown as { __renderToDomTree(latex: string, settings: katex.KatexOptions): TexNode }
).__renderToDomTree;

/** L'imbrication de tables (`vlist-t`) et le nombre de nœuds de l'arbre, parcouru sans récursion. */
function measure(tree: TexNode): { nesting: number; nodes: number } {
  let nodes = 0;
  let nesting = 0;
  const stack: [TexNode, number][] = [[tree, 0]];
  while (stack.length > 0) {
    const [node, above] = stack.pop()!;
    nodes += 1;
    const here = above + (node.classes?.includes("vlist-t") ? 1 : 0);
    if (here > nesting) nesting = here;
    for (const child of node.children ?? []) stack.push([child, here]);
  }
  return { nesting, nodes };
}

function compute(latex: string, display: boolean): TexOutcome {
  if (latex.trim() === "") return { kind: "empty" };
  if (latex.length > MAX_LATEX_CHARS) return { kind: "refused", why: "length" };
  let tree: TexNode;
  try {
    tree = renderToDomTree(latex, {
      displayMode: display,
      throwOnError: true,
      trust: false,
      strict: "ignore",
      output: "htmlAndMathml",
      macros: guardedMacros(),
    });
  } catch (error) {
    if (error instanceof DefinitionRefused) return { kind: "refused", why: "definition" };
    if (error instanceof katex.ParseError) return { kind: "invalid", message: error.rawMessage ?? error.message };
    return { kind: "invalid", message: String(error) };
  }
  const { nesting, nodes } = measure(tree);
  if (nodes > MAX_TEX_NODES) return { kind: "refused", why: "size" };
  return { kind: "formula", html: tree.toMarkup(), nesting, nodes };
}

/** Le cache : par formule et par mode, borné en entrées et en taille de HTML. */
const MAX_CACHED = 400;
const MAX_CACHED_CHARS = 8_000_000;
const cache = new Map<string, TexOutcome>();
let cachedChars = 0;

const weight = (outcome: TexOutcome) => (outcome.kind === "formula" ? outcome.html.length : 64);

/** Ce que la formule `latex` donne, en bloc (`display`) ou en ligne ; calculé une fois, puis gardé. */
export function typeset(latex: string, display: boolean): TexOutcome {
  const key = `${display ? "B" : "L"}${latex}`;
  const kept = cache.get(key);
  if (kept !== undefined) {
    cache.delete(key);
    cache.set(key, kept);
    return kept;
  }
  const outcome = compute(latex, display);
  cache.set(key, outcome);
  cachedChars += weight(outcome);
  for (const [oldest, value] of cache) {
    if (cache.size <= MAX_CACHED && cachedChars <= MAX_CACHED_CHARS) break;
    cache.delete(oldest);
    cachedChars -= weight(value);
  }
  return outcome;
}

/** La formule est-elle déjà mise en forme (son affichage ne coûte alors rien) ? */
export function isTypeset(latex: string, display: boolean): boolean {
  return cache.has(`${display ? "B" : "L"}${latex}`);
}

/** Ce qu'on dit d'une formule qui n'est pas montrée mise en forme. */
export function refusal(outcome: TexOutcome): string | null {
  switch (outcome.kind) {
    case "refused":
      return outcome.why === "definition"
        ? "Les définitions (\\def, \\newcommand…) ne sont pas permises dans une formule."
        : outcome.why === "length"
          ? `Formule trop longue : ${MAX_LATEX_CHARS.toLocaleString("fr-FR")} caractères au plus.`
          : "Formule trop grande pour être affichée.";
    case "invalid":
      return `Formule invalide : ${outcome.message}`;
    default:
      return null;
  }
}

/** Ce qu'on dit d'une formule trop imbriquée pour la place où elle est. */
export const TOO_NESTED = "Formule trop imbriquée pour être affichée ici.";
