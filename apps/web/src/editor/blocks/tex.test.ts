// Ce que KaTeX produit d'une formule, mesuré avant d'être attaché à la page. Une formule vient
// d'ailleurs (MMD, un agent, un autre client) : sans macro, une formule imbriquée faisait planter
// l'onglet de quiconque ouvrait la page (la mise en page de Chromium déborde sa pile sur des tables
// imbriquées) ; une définition (`\def`) faisait figer l'onglet. Les définitions sont refusées là où
// elles se produisent, pendant l'analyse ; l'imbrication est comptée dans ce que KaTeX produit.
import { expect, test } from "vitest";
import { MAX_LATEX_CHARS, nestingAllowance, typeset } from "./tex";

const frac = (n: number) => "\\frac{1}{".repeat(n) + "x" + "}".repeat(n);
const sqrt = (n: number) => "\\sqrt{".repeat(n) + "x" + "}".repeat(n);

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

test("nesting_is_counted_in_what_katex_produces", () => {
  // Chaque fraction, racine ou exposant imbriqué ajoute une table de mise en page (`vlist-t`).
  for (const n of [1, 5, 13]) {
    for (const make of [frac, sqrt]) {
      const outcome = typeset(make(n), true);
      expect(outcome.kind).toBe("formula");
      if (outcome.kind === "formula") expect(outcome.nesting, make(n).slice(0, 12)).toBe(n);
    }
  }
  const simple = typeset("a + b", true);
  expect(simple.kind === "formula" && simple.nesting).toBe(0);
});

test("the_nesting_allowed_shrinks_with_the_depth_of_the_page_around_the_formula", () => {
  // Mesuré dans Chromium (registre, R421) : l'onglet plante quand profondeur + 12 × tables dépasse
  // 169 environ ; on garde une marge de trois tables.
  expect(nestingAllowance(19)).toBe(10);
  expect(nestingAllowance(49)).toBe(8);
  expect(nestingAllowance(94)).toBe(4);
  expect(nestingAllowance(139)).toBe(0);
  expect(nestingAllowance(400)).toBe(0);
  expect(nestingAllowance(0)).toBe(10);
});

test("a_formula_too_long_is_refused_and_one_at_the_limit_is_not", () => {
  expect(typeset("x".repeat(MAX_LATEX_CHARS + 1), true)).toEqual({ kind: "refused", why: "length" });
  expect(typeset("x".repeat(MAX_LATEX_CHARS), true).kind).toBe("formula");
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
