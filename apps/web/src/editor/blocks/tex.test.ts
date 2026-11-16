// Ce que KaTeX produit d'une formule, mesuré avant d'être attaché à la page. Une formule vient
// d'ailleurs (MMD, un agent, un autre client) : sans macro, une formule imbriquée faisait planter
// l'onglet de quiconque ouvrait la page (la mise en page de Chromium déborde sa pile sur des boîtes
// imbriquées, tables ou non) ; une définition (`\def`) faisait figer l'onglet. Les définitions sont
// refusées là où elles se produisent, pendant l'analyse ; la profondeur d'éléments est comptée dans
// ce que KaTeX produit.
import { expect, test } from "vitest";
import { depthAllowance, MAX_LATEX_CHARS, MAX_TEX_NODES, typeset } from "./tex";

const B = String.fromCharCode(92);
/** `@` pour la barre oblique inverse, plus lisible dans une chaîne de JavaScript. */
const t = (s: string) => s.replace(/@/g, B);
const wrap = (open: string, close: string, n: number, core = "x") => t(open).repeat(n) + t(core) + t(close).repeat(n);
const frac = (n: number) => wrap("@frac{1}{", "}", n);
const sqrt = (n: number) => wrap("@sqrt{", "}", n);

test("a_formula_is_typeset_and_never_follows_a_link", () => {
  const plain = typeset("E = mc^2", false);
  expect(plain.kind).toBe("formula");
  if (plain.kind !== "formula") return;
  expect(plain.html).toContain('class="katex"');
  for (const latex of ["\\href{javascript:alert(1)}{x}", "\\url{https://a.example/}", "\\href{https://a.example/}{x}"]) {
    const outcome = typeset(latex, true);
    const html = outcome.kind === "formula" ? outcome.html : "";
    expect(html, latex).not.toMatch(/<a[\s>]/);
    // L'adresse peut se lire, en texte (KaTeX montre la commande refusée) ; jamais en attribut.
    expect(html, latex).not.toMatch(/href=|src=|onclick|onerror/i);
  }
});

test("definitions_are_refused_where_they_occur", () => {
  for (const latex of [
    "\\def\\a{b}\\a",
    "\\gdef\\a{b}",
    "\\edef\\a{b}",
    "\\xdef\\a{b}",
    "\\global\\def\\a{b}",
    "\\let\\a\\b",
    "\\futurelet\\a\\b\\c",
    "\\newcommand{\\a}{b}\\a",
    "\\renewcommand{\\frac}{b}",
    "\\providecommand{\\a}{b}",
    // `\verb` ne cache rien : la définition qui le suit est vue là où elle se produit.
    "\\verb|x|\\def\\a{\\a}\\a",
    "x^{\\text{\\def\\a{b}}}",
  ]) {
    expect(typeset(latex, true), latex).toEqual({ kind: "refused", why: "definition" });
  }
});

test("what_katex_defines_for_itself_is_not_a_definition", () => {
  // Les tableaux posent `\cr`, la couleur `\current@color`, `\middle` redéfinit `|` : KaTeX, pas
  // la formule. Et « \def » écrit dans `\verb` n'est que du texte.
  for (const latex of [
    "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
    "\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}",
    "\\color{red} x",
    "\\left( a \\middle| b \\right)",
    "\\verb|\\def\\a{b}|",
    "\\neq \\leq \\iff",
  ]) {
    expect(typeset(latex, true).kind, latex).toBe("formula");
  }
});

/** La profondeur d'éléments du HTML, lue par l'analyseur de la page (jsdom). */
function parsedDepth(html: string): number {
  const box = document.createElement("div");
  box.innerHTML = html;
  let deepest = 0;
  const stack: [Element, number][] = [...box.children].map((child) => [child, 1] as [Element, number]);
  while (stack.length > 0) {
    const [element, depth] = stack.pop()!;
    deepest = Math.max(deepest, depth);
    for (const child of element.children) stack.push([child, depth + 1]);
  }
  return deepest;
}

/**
 * Des formes qui faisaient planter l'onglet en haut d'une page (une équation à 20 éléments du haut),
 * sans aucune table pour la plupart : le plus petit nombre de niveaux qui plante, mesuré dans le
 * Chromium de Playwright (registre, Task C2, tour de correction 1).
 */
const CRASHED_AT_THE_TOP: [string, string][] = [
  ["mathllap", wrap("@mathllap{", "}", 13)],
  ["llap", wrap("@llap{", "}", 13)],
  ["reflectbox", wrap("@mathreflectbox{", "}", 30)],
  ["vphantom", wrap("a@vphantom{", "}", 34)],
  ["smash", wrap("a@smash{", "}", 93)],
  ["color", wrap("@color{red}{a", "}", 131)],
  ["group", wrap("{", "a}", 263)],
  ["frac", frac(13)],
  ["sqrt", sqrt(13)],
];

test("element_depth_is_counted_in_what_katex_produces", () => {
  // La profondeur d'éléments de tout l'arbre (HTML et MathML), celle que l'analyseur de la page
  // construit, pas seulement les tables (relecture de C2, C1).
  for (const [name, latex] of [...CRASHED_AT_THE_TOP, ["simple", "a + b"], ["matrix", t("@begin{pmatrix} a @@ b @end{pmatrix}")]]) {
    for (const display of [true, false]) {
      const outcome = typeset(latex, display);
      expect(outcome.kind, name).toBe("formula");
      if (outcome.kind === "formula") expect(outcome.depth, `${name} ${display}`).toBe(parsedDepth(outcome.html));
    }
  }
});

test("a_formula_that_crashed_the_tab_never_fits_at_the_top_of_a_page", () => {
  for (const [name, latex] of CRASHED_AT_THE_TOP) {
    const outcome = typeset(latex, true);
    expect(outcome.kind === "formula" && outcome.depth > depthAllowance(20), name).toBe(true);
  }
});

test("the_depth_allowed_shrinks_with_the_depth_of_the_page_around_the_formula", () => {
  // Mesuré dans Chromium : l'onglet plante quand profondeur de la page + 2,64 × profondeur
  // d'éléments de la formule dépasse 187 environ ; on compte 3 par élément et on s'arrête à 150.
  expect(depthAllowance(0)).toBe(50);
  expect(depthAllowance(20)).toBe(43);
  expect(depthAllowance(49)).toBe(33);
  expect(depthAllowance(94)).toBe(18);
  expect(depthAllowance(150)).toBe(0);
  expect(depthAllowance(400)).toBe(0);
  // Les formules ordinaires tiennent en haut d'une page : la plus profonde mesurée, une fraction
  // continue de quatre niveaux, a 35 éléments de profondeur.
  for (const raw of [
    "@frac{1}{1+@frac{1}{1+@frac{1}{1+@frac{1}{x}}}}",
    "@sqrt{@frac{a^{2^{n}}}{b_{i_{j}}}}",
    "x = @frac{-b @pm @sqrt{b^2-4ac}}{2a}",
    "@begin{bmatrix} @frac{1}{2} & @sqrt{2} @@ @alpha & @beta @end{bmatrix}",
  ]) {
    const outcome = typeset(t(raw), true);
    expect(outcome.kind === "formula" && outcome.depth <= depthAllowance(20), raw).toBe(true);
  }
});

test("a_formula_too_long_is_refused_and_one_at_the_limit_is_not", () => {
  expect(typeset("x".repeat(MAX_LATEX_CHARS + 1), true)).toEqual({ kind: "refused", why: "length" });
  expect(typeset("x".repeat(MAX_LATEX_CHARS), true).kind).toBe("formula");
});

test("a_formula_too_big_is_refused", () => {
  // Une matrice de 4 900 colonnes : 9 800 caractères, plus de 80 000 éléments.
  const matrix = (columns: number) => `${t("@begin{matrix}")}${"a&".repeat(columns)}a${t("@end{matrix}")}`;
  expect(typeset(matrix(4_900), true)).toEqual({ kind: "refused", why: "size" });
  // Témoin : une matrice plus petite passe, et ses éléments sont comptés.
  const small = typeset(matrix(100), true);
  expect(small.kind).toBe("formula");
  if (small.kind === "formula") {
    expect(small.nodes).toBe(new DOMParser().parseFromString(small.html, "text/html").body.querySelectorAll("*").length);
    expect(small.nodes).toBeLessThan(MAX_TEX_NODES);
  }
});

test("an_invalid_formula_says_why", () => {
  const outcome = typeset("\\frac{1}{", true);
  expect(outcome.kind).toBe("invalid");
  if (outcome.kind === "invalid") expect(outcome.message).toMatch(/Expected '}'|end of input/i);
  expect(typeset("\\nosuchcommand", true).kind).toBe("invalid");
});

test("an_empty_formula_is_empty", () => {
  expect(typeset("", true)).toEqual({ kind: "empty" });
  expect(typeset("   ", false)).toEqual({ kind: "empty" });
});

test("a_formula_is_typeset_once_and_kept", () => {
  const a = typeset("\\sum_{k=1}^{n} k", true);
  expect(typeset("\\sum_{k=1}^{n} k", true)).toBe(a);
  // Pas la même chose en ligne.
  expect(typeset("\\sum_{k=1}^{n} k", false)).not.toBe(a);
});
