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
// - **La profondeur pondérée est comptée dans ce que KaTeX produit** (HTML et MathML, balises
//   comptées dans le HTML écrit) : la mise en page de Chromium déborde sa pile sur des boîtes
//   imbriquées, tables ou non (`\frac`, `\mathllap`, `\mathreflectbox`, des accolades…), et l'onglet
//   plantait d'autant plus tôt que la formule était profonde dans la page. Un élément pèse
//   `CHEAP_COST` sous les seules classes connues pour coûter peu (fractions, matrices, exposants,
//   délimiteurs), `DEAR_COST` ailleurs (R525, R551). La formule ne s'attache que si son poids tient
//   dans ce que la profondeur de la page autour d'elle laisse (`weightAllowance`) ; sinon, elle est
//   montrée en texte, avec un message (`Formula`).
// - Une formule de plus de `MAX_LATEX_CHARS` caractères, ou dont KaTeX produirait plus de
//   `MAX_TEX_NODES` éléments, n'est pas mise en forme. Les tailles écrites (`\rule`, `\kern`,
//   `\\[…]`) sont bornées à `MAX_SIZE_EM` ; les expansions de macros, à `MAX_EXPAND`.
//
// KaTeX reste à `trust: false` : ni lien, ni image, ni attribut. Le résultat est gardé, par formule,
// dans un cache borné : une formule déjà vue n'est pas recalculée (elle passe quand même par
// l'écran et la file de `Formula` pour être attachée).
import katex from "katex";

/** Au-delà, une formule n'est pas mise en forme (le champ n'en accepte pas plus). */
export const MAX_LATEX_CHARS = 10_000;

/**
 * Au-delà, ce que KaTeX produirait est trop grand pour la page (en éléments) : une formule
 * s'attache d'un coup, et la mise en page de 12 000 éléments prend déjà 60 à 150 ms selon ce que la
 * page porte (mesuré : registre, R526). Une formule de 10 000 lettres en fait 10 011.
 */
export const MAX_TEX_NODES = 12_000;

/** Les tailles écrites dans une formule, au plus (en em) : `\rule{1em}{2000000em}` faisait 33 millions de pixels. */
const MAX_SIZE_EM = 50;

/** Les expansions de macros d'une formule, au plus (la valeur par défaut de KaTeX, épinglée). */
const MAX_EXPAND = 1_000;

/**
 * La mise en page de Chromium plante quand la profondeur de la page autour de la formule, plus le
 * coût de sa profondeur d'éléments, dépasse 187 environ (mesuré : registre, R525, R551) : 2,64 par
 * élément au pire des boîtes (`\mathreflectbox`, les `\…lap`, `\raisebox`, `\vcenter`, les accents,
 * les racines), 1,76 au pire des fractions, matrices, exposants et délimiteurs. Un élément pèse
 * `DEAR_COST`, ou `CHEAP_COST` s'il n'a que des classes de `CHEAP_CLASSES` et n'est sous aucun
 * élément cher ; les enveloppes de KaTeX (`WRAPPERS`) pèsent `DEAR_COST` sans rendre cher ce qui est
 * sous elles. Au pire des boîtes, la marge est celle d'avant les deux poids ; les fractions et leurs
 * voisines n'en ont jamais moins (R551). On s'arrête à `DEPTH_BUDGET`.
 */
const DEPTH_BUDGET = 150;
const DEAR_COST = 3;
export const CHEAP_COST = 2.3;

/**
 * Les classes de KaTeX qui coûtent peu à la mise en page : celles des fractions, des matrices, des
 * exposants, des grands opérateurs, des délimiteurs et des polices. Toute autre classe, le MathML,
 * le SVG, et une table (`vlist-t`) qui n'est pas celle d'une fraction, d'un exposant, de limites ou
 * d'une colonne de matrice (`\raisebox`, `\vcenter`, les accents) coûtent cher.
 */
const CHEAP_CLASSES = new Set(
  (
    "katex katex-display katex-html katex-mathml katex-base katex-strut katex-sizing katex-root " +
    "mord mopen mclose mbin mrel mpunct minner mop mspace mtight mfrac frac-line nulldelimiter " +
    "mathnormal mathrm mathbf mathit mathbb mathcal mathfrak mathsf mathtt boldsymbol text textbf " +
    "pstrut vlist vlist-r vlist-s vlist-t vlist-t2 msupsub mtable arraycolsep vertical-separator " +
    "delimcenter delimsizing large-op op-symbol op-limits"
  ).split(" "),
);
const cheapClass = (name: string) =>
  CHEAP_CLASSES.has(name) || /^(reset-)?size\d+$/.test(name) || /^col-align-[lcr]$/.test(name);

/** Les enveloppes de chaque formule : au-dessus de tout, elles pèsent comme un élément cher. */
const WRAPPERS = new Set(["katex", "katex-display", "katex-html", "katex-base"]);

/** Les classes de la balise ouvrante `tag`. */
function classesOf(tag: string): string[] {
  const found = /class="([^"]*)"/.exec(tag);
  return found === null ? [] : found[1]!.split(" ").filter((name) => name !== "");
}

/**
 * Les parents sous lesquels une table de KaTeX (`vlist-t`) coûte peu : une fraction, un exposant,
 * des limites, une colonne de matrice. Celle de `\raisebox` ou de `\vcenter` coûte cher, une ou deux
 * lignes (KaTeX ajoute `vlist-t2` dès que le contenu descend sous la ligne de base : re-relecture
 * de C2, R1).
 */
const tableParent = (name: string) =>
  name === "mfrac" || name === "msupsub" || name === "op-limits" || /^col-align-[lcr]$/.test(name);

/**
 * Ce que coûte l'élément de la balise ouvrante `tag`, sous un parent qui a les classes `parent` :
 * cher (lui et ce qui est sous lui), une enveloppe, ou peu.
 */
function costOf(tag: string, parent: string[]): "dear" | "wrapper" | "cheap" {
  if (!tag.startsWith("<span")) return "dear";
  const classes = classesOf(tag);
  if (!classes.every(cheapClass)) return "dear";
  if (classes.includes("vlist-t") && !parent.some(tableParent)) return "dear";
  return classes.some((name) => WRAPPERS.has(name)) ? "wrapper" : "cheap";
}

/** Ce qu'une formule donne. */
export type TexOutcome =
  | { kind: "formula"; html: string; depth: number; weight: number; nodes: number }
  | { kind: "empty" }
  | { kind: "invalid"; message: string }
  | { kind: "refused"; why: "definition" | "internal" | "length" | "size" };

/**
 * Le poids qu'une formule peut avoir, à `depth` éléments du haut du document : 130 en haut d'une
 * page (une équation y est à 20 ; six fractions imbriquées pèsent 103), moins dans une liste
 * imbriquée, rien au-delà de 150 (une page aussi profonde plante déjà sans formule, vers 180).
 */
export function weightAllowance(depth: number): number {
  return Math.max(0, DEPTH_BUDGET - Math.max(0, depth));
}

/** Une définition de la formule, refusée pendant l'analyse. */
class DefinitionRefused extends Error {}

/** Les définitions que KaTeX écrit lui-même : `\tag` et `\tag*`, puis `\nonumber` et `\notag`. */
const OWN_DEFINITIONS = new Set(["\\df@tag", "\\@eqnsw"]);

/** Ce que KaTeX range lui-même dans `macros` ; une définition y est un objet à `tokens`. */
const isDefinition = (value: unknown) =>
  typeof value === "object" && value !== null && Array.isArray((value as { tokens?: unknown }).tokens);

type Macros = NonNullable<katex.KatexOptions["macros"]>;
type MacroContext = { consumeArgs(count: number): unknown; popToken(): unknown };

/**
 * `\message`, `\errmessage` et `\show` sont des macros de KaTeX qui écrivent dans la console de
 * chaque lecteur : elles prennent leur argument et n'écrivent rien.
 */
const SILENT: Macros = {
  "\\message": (context: object) => {
    (context as MacroContext).consumeArgs(1);
    return "";
  },
  "\\errmessage": (context: object) => {
    (context as MacroContext).consumeArgs(1);
    return "";
  },
  "\\show": (context: object) => {
    (context as MacroContext).popToken();
    return "";
  },
};

/** Les macros de la formule : KaTeX y range toute définition, qui est refusée là. */
function guardedMacros(): Macros {
  return new Proxy<Macros>(
    { ...SILENT },
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
 * La profondeur d'éléments, le poids (la profondeur pondérée la plus lourde) et le nombre d'éléments
 * du HTML de KaTeX, tels que l'analyseur de la page les construira : KaTeX échappe `<` et `>` dans
 * le texte et les attributs, et ferme tout élément, sauf ceux qu'il écrit `<… />` (les chemins d'un
 * SVG).
 */
function shape(html: string): { depth: number; weight: number; nodes: number } {
  let depth = 0;
  let deepest = 0;
  let heaviest = 0;
  let nodes = 0;
  const above: { weight: number; dear: boolean; classes: string[] }[] = [{ weight: 0, dear: false, classes: [] }];
  for (let at = html.indexOf("<"); at !== -1; at = html.indexOf("<", at + 1)) {
    if (html.charCodeAt(at + 1) === 47 /* "/" */) {
      depth -= 1;
      above.pop();
      continue;
    }
    nodes += 1;
    depth += 1;
    if (depth > deepest) deepest = depth;
    const end = html.indexOf(">", at);
    const parent = above[above.length - 1]!;
    const tag = html.slice(at, end + 1);
    const cost = parent.dear ? "dear" : costOf(tag, parent.classes);
    const dear = cost === "dear";
    const weight = parent.weight + (cost === "cheap" ? CHEAP_COST : DEAR_COST);
    if (weight > heaviest) heaviest = weight;
    if (html.charCodeAt(end - 1) === 47) depth -= 1;
    else above.push({ weight, dear, classes: dear ? [] : classesOf(tag) });
  }
  return { depth: deepest, weight: Math.round(heaviest * 10) / 10, nodes };
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
      maxExpand: MAX_EXPAND,
      macros: guardedMacros(),
    }).toMarkup();
  } catch (error) {
    if (error instanceof DefinitionRefused) return { kind: "refused", why: "definition" };
    if (error instanceof katex.ParseError) return { kind: "invalid", message: error.rawMessage ?? error.message };
    return { kind: "invalid", message: String(error) };
  }
  const { depth, weight, nodes } = shape(html);
  if (nodes > MAX_TEX_NODES) return { kind: "refused", why: "size" };
  return { kind: "formula", html, depth, weight, nodes };
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

/** La formule est-elle déjà calculée (`typeset` la rend alors sans rien recalculer) ? */
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
