// Le fenêtrage d'un arbre de la barre latérale : les lignes à rendre pour la partie visible,
// et de combien défiler pour montrer la page active.
import { expect, test } from "vitest";
import { OVERSCAN, ROW_HEIGHT } from "./rows";
import { revealDelta, windowRange } from "./treeWindow";

test("window_is_the_visible_rows_plus_overscan", () => {
  expect(ROW_HEIGHT).toBe(30);
  expect(OVERSCAN).toBe(10);
  // L'arbre commence en haut de la partie visible, haute de 20 lignes.
  expect(windowRange(0, 600, 1_000)).toEqual({ start: 0, end: 30 });
  // Défilé de 100 lignes.
  expect(windowRange(3_000, 600, 1_000)).toEqual({ start: 90, end: 130 });
  // Tout en bas : la fin est bornée au nombre de lignes.
  expect(windowRange(29_400, 600, 1_000)).toEqual({ start: 970, end: 1_000 });
  // Un petit arbre est rendu en entier.
  expect(windowRange(0, 600, 3)).toEqual({ start: 0, end: 3 });
});

test("tree_out_of_view_renders_no_row", () => {
  // L'arbre commence bien plus bas que la partie visible (une autre section, au-dessus,
  // prend la place) : rien à rendre.
  expect(windowRange(-5_000, 600, 1_000)).toEqual({ start: 0, end: 0 });
  // L'arbre est entièrement au-dessus de la partie visible.
  expect(windowRange(40_000, 600, 1_000)).toEqual({ start: 1_000, end: 1_000 });
  // Il commence juste sous la partie visible : ses premières lignes sont prêtes.
  expect(windowRange(-700, 600, 1_000)).toEqual({ start: 0, end: 7 });
});

test("reveal_leaves_a_visible_row_where_it_is", () => {
  expect(revealDelta(0, 600)).toBe(0);
  expect(revealDelta(285, 600)).toBe(0);
  expect(revealDelta(600 - ROW_HEIGHT, 600)).toBe(0);
});

test("reveal_scrolls_a_cut_row_just_enough", () => {
  // Coupée par le haut de 10 px : on remonte de 10 px.
  expect(revealDelta(-10, 600)).toBe(-10);
  // Coupée par le bas : sa dernière ligne de pixels arrive au bas de la partie visible.
  expect(revealDelta(600 - 20, 600)).toBe(10);
  expect(revealDelta(600 - 1, 600)).toBe(ROW_HEIGHT - 1);
});

test("reveal_centers_a_row_that_is_far", () => {
  // Hors de la partie visible : la ligne arrive au milieu.
  expect(revealDelta(3_000, 600)).toBe(3_000 - (600 - ROW_HEIGHT) / 2);
  expect(revealDelta(-3_000, 600)).toBe(-3_000 - (600 - ROW_HEIGHT) / 2);
  // Juste au bord, sans un pixel visible : elle est loin.
  expect(revealDelta(600, 600)).toBe(600 - (600 - ROW_HEIGHT) / 2);
  expect(revealDelta(-ROW_HEIGHT, 600)).toBe(-ROW_HEIGHT - (600 - ROW_HEIGHT) / 2);
});
