// Ce que KaTeX produit d'une formule, mesuré avant d'être attaché à la page. Une formule vient
// d'ailleurs (MMD, un agent, un autre client) : sans macro, une formule imbriquée faisait planter
// l'onglet de quiconque ouvrait la page (la mise en page de Chromium déborde sa pile sur des boîtes
// imbriquées, tables ou non) ; une définition (`\def`) faisait figer l'onglet. Les définitions sont
// refusées là où elles se produisent, pendant l'analyse ; la profondeur d'éléments est comptée dans
// ce que KaTeX produit.
import katex from "katex";
import { afterEach, expect, test, vi } from "vitest";
import { depthAllowance, MAX_LATEX_CHARS, MAX_TEX_NODES, refusal, typeset } from "./tex";

const B = String.fromCharCode(92);
/** `@` pour la barre oblique inverse, plus lisible dans une chaîne de JavaScript. */
const t = (s: string) => s.replace(/@/g, B);
const wrap = (open: string, close: string, n: number, core = "x") => t(open).repeat(n) + t(core) + t(close).repeat(n);
const frac = (n: number) => wrap("@frac{1}{", "}", n);
const sqrt = (n: number) => wrap("@sqrt{", "}", n);

afterEach(() => {
  vi.restoreAllMocks();
});

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

test("tags_and_unnumbered_lines_are_not_definitions", () => {
  // KaTeX écrit `\tag` sous la forme `\gdef\df@tag{…}`, `\nonumber` et `\notag` sous la forme
  // `\gdef\@eqnsw{0}` : ce sont ses définitions, pas celles de la formule (relecture de C2, I1).
  for (const raw of [
    "x @tag{1}",
    "x @tag*{a}",
    "@begin{equation} x @tag{2} @end{equation}",
    "@begin{align} a &= b @tag{1} @@ c &= d @notag @end{align}",
    "@begin{gather} a @@ b @nonumber @end{gather}",
  ]) {
    expect(typeset(t(raw), true).kind, raw).toBe("formula");
  }
});

test("a_command_of_katex_with_an_at_sign_written_in_the_formula_is_refused", () => {
  // Accepter `\df@tag` par son nom ne doit pas rouvrir la bombe : un `\tag` de 2 000 lettres, suivi
  // de mille `\df@tag`, en ferait écrire deux millions. Une commande dont le nom porte `@` n'est
  // écrite que par KaTeX : écrite dans la formule, elle est refusée.
  const bomb = `${t("@tag{")}${"a".repeat(2_000)}}${`${B}df@tag`.repeat(1_000)}`;
  for (const latex of [bomb, `${B}gdef${B}df@tag{x}`, `x ${B}df@tag`, `${B}@eqnsw`, `${B}current@color x`]) {
    expect(typeset(latex, true), latex.slice(0, 40)).toEqual({ kind: "refused", why: "internal" });
  }
  expect(refusal({ kind: "refused", why: "internal" })).toBe(
    "Les commandes internes de KaTeX (dont le nom contient @) ne sont pas permises dans une formule.",
  );
  // Témoin : un retour à la ligne suivi d'une arobase n'est pas une commande.
  expect(typeset("a \\\\@ b", true).kind).toBe("formula");
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
  // 400 fractions : 23 600 éléments, dont la mise en page prenait 120 à 450 ms d'un coup.
  const fractions = (n: number) => Array.from({ length: n }, (_, k) => `${B}frac{a_{${k}}}{b^{2}}`).join(" + ");
  expect(typeset(fractions(400), true)).toEqual({ kind: "refused", why: "size" });
  // La plus grande des budgets du navigateur, 200 fractions, passe.
  const largest = typeset(fractions(200), true);
  expect(largest.kind === "formula" && largest.nodes).toBeLessThanOrEqual(MAX_TEX_NODES);
  // Témoin : une matrice plus petite passe, et ses éléments sont comptés.
  const small = typeset(matrix(100), true);
  expect(small.kind).toBe("formula");
  if (small.kind === "formula") {
    expect(small.nodes).toBe(new DOMParser().parseFromString(small.html, "text/html").body.querySelectorAll("*").length);
    expect(small.nodes).toBeLessThan(MAX_TEX_NODES);
  }
});

test("sizes_written_in_a_formula_are_capped", () => {
  // Une formule de 24 caractères faisait un bloc de 33 millions de pixels (relecture de C2, I3).
  for (const raw of ["@rule{1em}{2000000em}", "@kern3000em x", "x @@[3000em] y", "@hspace{3000em} x", "@raisebox{3000em}{x}"]) {
    const outcome = typeset(t(raw), true);
    expect(outcome.kind, raw).toBe("formula");
    if (outcome.kind !== "formula") continue;
    const box = document.createElement("div");
    box.innerHTML = outcome.html;
    const shown = box.querySelector(".katex-html")!.outerHTML;
    const sizes = [...shown.matchAll(/(-?\d+(?:\.\d+)?)em/g)].map((m) => Number(m[1]));
    // 50 em, plus la hauteur d'une lettre au-dessus d'une boîte haussée de 50 em.
    expect(Math.max(...sizes), raw).toBeLessThanOrEqual(51);
  }
});

test("macro_expansions_are_capped_at_a_thousand", () => {
  // Sans définition, la seule façon de faire grossir une formule est d'appeler des macros de KaTeX :
  // mille expansions au plus (`maxExpand`).
  expect(typeset(t("@bgroup}").repeat(1_000), true).kind).toBe("formula");
  const over = typeset(t("@bgroup}").repeat(1_001), true);
  expect(over.kind).toBe("invalid");
  if (over.kind === "invalid") expect(over.message).toMatch(/Too many expansions/);
});

test("message_errmessage_and_show_write_nothing_to_the_console", () => {
  // Ce sont des macros de KaTeX : elles écrivaient dans la console de chaque lecteur.
  const log = vi.spyOn(console, "log").mockImplementation(() => {});
  const error = vi.spyOn(console, "error").mockImplementation(() => {});
  for (const raw of ["@message{bonjour} x", "@errmessage{bonjour} x", "@show@frac x"]) {
    expect(typeset(t(raw), true).kind, raw).toBe("formula");
  }
  expect(log).not.toHaveBeenCalled();
  expect(error).not.toHaveBeenCalled();
});

test("the_katex_measured_is_the_one_installed", () => {
  // Les seuils de plantage sont mesurés avec la feuille de style de cette version, et `typeset`
  // passe par une fonction interne de KaTeX : toute montée de version se refait mesurer.
  expect(katex.version, "KaTeX a changé de version : refaire les mesures de plantage (registre, Task C2)").toBe("0.19.1");
  expect(
    typeof (katex as unknown as { __renderToDomTree?: unknown }).__renderToDomTree,
    "KaTeX n'a plus `__renderToDomTree` : `typeset` ne peut plus mesurer une formule avant de l'attacher",
  ).toBe("function");
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
