// Ce que KaTeX produit d'une formule, mesuré **avant** d'être attaché à la page (R420, R421, R422,
// R525 à R529). Une formule vient d'ailleurs (MMD, un agent, un autre client) : tout ce qu'une
// formule peut faire à l'onglet de qui ouvre la page se décide ici.
//
// - **Les définitions sont refusées là où elles se produisent** (`\def`, `\gdef`, `\edef`, `\let`,
//   `\newcommand`…) : KaTeX range chaque définition dans `settings.macros`, que voici sous la forme
//   d'un objet qui refuse toute définition de la formule, au moment où l'analyse la fait. Rien ne
//   relit les lexèmes : `\verb` ne cache rien (la relecture perdue de la Task C2 le contournait).
//   Une définition récursive ou en cascade figeait l'onglet. KaTeX range aussi ses propres valeurs
//   (`\cr` dans un tableau, `\current@color`, `|` sous `\middle`) : des chaînes, des fonctions ; et
//   deux définitions à lui, `\df@tag` (`\tag`) et `\@eqnsw` (`\nonumber`, `\notag`), seules acceptées
//   par leur nom. Une commande dont le nom porte `@` n'est écrite que par KaTeX : écrite dans la
//   formule, elle est refusée (sinon mille `\df@tag` copieraient mille fois un `\tag`).
// - **La profondeur d'éléments est comptée dans ce que KaTeX produit** (HTML et MathML, balises
//   comptées dans le HTML écrit) : la mise en page de Chromium déborde sa pile sur des boîtes
//   imbriquées, tables ou non (`\frac`, `\mathllap`, `\mathreflectbox`, des accolades…), et l'onglet
//   plantait d'autant plus tôt que la formule était profonde dans la page. La formule ne s'attache
//   que si sa profondeur tient dans ce que la profondeur de la page autour d'elle laisse
//   (`depthAllowance`) ; sinon, elle est montrée en texte, avec un message (`Formula`).
// - Une formule de plus de `MAX_LATEX_CHARS` caractères, ou dont KaTeX produirait plus de
//   `MAX_TEX_NODES` éléments, n'est pas mise en forme. Les tailles écrites (`\rule`, `\kern`,
//   `\\[…]`) sont bornées à `MAX_SIZE_EM`.
//
// KaTeX reste à `trust: false` : ni lien, ni image, ni attribut. Le résultat est gardé, par formule,
// dans un cache borné : une formule déjà vue s'affiche sans être recalculée.
import katex from "katex";

/** Au-delà, une formule n'est pas mise en forme (le champ n'en accepte pas plus). */
export const MAX_LATEX_CHARS = 10_000;

/** Au-delà, ce que KaTeX produirait est trop grand pour la page (en éléments). */
export const MAX_TEX_NODES = 60_000;

/** Les tailles écrites dans une formule, au plus (en em) : `\rule{1em}{2000000em}` faisait 33 millions de pixels. */
const MAX_SIZE_EM = 50;

/**
 * La mise en page de Chromium plante quand la profondeur de la page autour de la formule, plus
 * 2,64 fois sa profondeur d'éléments (au pire des formes mesurées, `\mathreflectbox`), dépasse 187
 * environ (mesuré : registre, R525). On compte `ELEMENT_COST` par élément, et on s'arrête à
 * `DEPTH_BUDGET`.
 */
const DEPTH_BUDGET = 150;
const ELEMENT_COST = 3;

/** Ce qu'une formule donne. */
export type TexOutcome =
  | { kind: "formula"; html: string; depth: number; nodes: number }
  | { kind: "empty" }
  | { kind: "invalid"; message: string }
  | { kind: "refused"; why: "definition" | "internal" | "length" | "size" };

/**
 * La profondeur d'éléments qu'une formule peut avoir, à `depth` éléments du haut du document : 43
 * en haut d'une page (une équation y est à 20), moins dans une liste imbriquée, aucune au-delà de
 * 147 (une page aussi profonde plante déjà sans formule, vers 180 : registre).
 */
export function depthAllowance(depth: number): number {
  return Math.max(0, Math.floor((DEPTH_BUDGET - Math.max(0, depth)) / ELEMENT_COST));
}

/** Une définition de la formule, refusée pendant l'analyse. */
class DefinitionRefused extends Error {}

/** Les définitions que KaTeX écrit lui-même : `\tag` et `\tag*`, puis `\nonumber` et `\notag`. */
const OWN_DEFINITIONS = new Set(["\\df@tag", "\\@eqnsw"]);

/** Ce que KaTeX range lui-même dans `macros` ; une définition y est un objet à `tokens`. */
const isDefinition = (value: unknown) =>
  typeof value === "object" && value !== null && Array.isArray((value as { tokens?: unknown }).tokens);

type Macros = NonNullable<katex.KatexOptions["macros"]>;
/** Les macros de la formule : KaTeX y range toute définition, qui est refusée là. */
function guardedMacros(): Macros {
  return new Proxy<Macros>(
    {},
    {
      set(target, name, value) {
        if (isDefinition(value) && !OWN_DEFINITIONS.has(String(name))) throw new DefinitionRefused(String(name));
        target[name as string] = value as string;
        return true;
      },
      defineProperty() {
        throw new DefinitionRefused("defineProperty");
      },
    },
  );
}

/**
 * Une commande dont le nom porte `@` écrite dans la formule (le lexeur de KaTeX tient `@` pour une
 * lettre dans un nom de commande ; `\\@`, un retour à la ligne suivi d'une arobase, n'en est pas une).
 */
function writesAnInternalCommand(latex: string): boolean {
  for (const [command] of latex.matchAll(/\\(?:[A-Za-z@]+|[^])/g)) if (command.includes("@")) return true;
  return false;
}

/** L'arbre que KaTeX construit avant d'en écrire le HTML (sans type public). */
type TexNode = { toMarkup(): string };
const renderToDomTree = (
  katex as unknown as { __renderToDomTree(latex: string, settings: katex.KatexOptions): TexNode }
).__renderToDomTree;

/**
 * La profondeur d'éléments et le nombre d'éléments du HTML de KaTeX, tels que l'analyseur de la page
 * les construira : KaTeX échappe `<` et `>` dans le texte et les attributs, et ferme tout élément,
 * sauf ceux qu'il écrit `<… />` (les chemins d'un SVG).
 */
function shape(html: string): { depth: number; nodes: number } {
  let depth = 0;
  let deepest = 0;
  let nodes = 0;
  for (let at = html.indexOf("<"); at !== -1; at = html.indexOf("<", at + 1)) {
    if (html.charCodeAt(at + 1) === 47 /* "/" */) {
      depth -= 1;
      continue;
    }
    nodes += 1;
    depth += 1;
    if (depth > deepest) deepest = depth;
    const end = html.indexOf(">", at);
    if (html.charCodeAt(end - 1) === 47) depth -= 1;
  }
  return { depth: deepest, nodes };
}

function compute(latex: string, display: boolean): TexOutcome {
  if (latex.trim() === "") return { kind: "empty" };
  if (latex.length > MAX_LATEX_CHARS) return { kind: "refused", why: "length" };
  if (writesAnInternalCommand(latex)) return { kind: "refused", why: "internal" };
  let html: string;
  try {
    html = renderToDomTree(latex, {
      displayMode: display,
      throwOnError: true,
      trust: false,
      strict: "ignore",
      output: "htmlAndMathml",
      maxSize: MAX_SIZE_EM,
      macros: guardedMacros(),
    }).toMarkup();
  } catch (error) {
    if (error instanceof DefinitionRefused) return { kind: "refused", why: "definition" };
    if (error instanceof katex.ParseError) return { kind: "invalid", message: error.rawMessage ?? error.message };
    return { kind: "invalid", message: String(error) };
  }
  const { depth, nodes } = shape(html);
  if (nodes > MAX_TEX_NODES) return { kind: "refused", why: "size" };
  return { kind: "formula", html, depth, nodes };
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
      switch (outcome.why) {
        case "definition":
          return "Les définitions (\\def, \\newcommand…) ne sont pas permises dans une formule.";
        case "internal":
          return "Les commandes internes de KaTeX (dont le nom contient @) ne sont pas permises dans une formule.";
        case "length":
          return `Formule trop longue : ${MAX_LATEX_CHARS.toLocaleString("fr-FR")} caractères au plus.`;
        default:
          return "Formule trop grande pour être affichée.";
      }
    case "invalid":
      return `Formule invalide : ${outcome.message}`;
    default:
      return null;
  }
}

/** Ce qu'on dit d'une formule trop imbriquée pour la place où elle est. */
export const TOO_NESTED = "Formule trop imbriquée pour être affichée ici.";
