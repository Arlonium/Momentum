// Les plages à surligner d'un extrait (lot 4, Task C1) : des places en unités UTF-16, données par
// le serveur, relues ici avant de couper le texte. Une plage qui ne tient pas est ignorée, jamais
// corrigée ; le texte, lui, est toujours rendu en entier.
import { expect, test } from "vitest";
import { markedParts } from "./marks";

/** Le texte remis bout à bout : rien n'en est jamais perdu. */
const whole = (excerpt: string, marks: [number, number][]) =>
  markedParts(excerpt, marks)
    .map((part) => part.text)
    .join("");

test("the_ranges_cut_the_text_in_marked_and_plain_parts", () => {
  expect(markedParts("le chat dort", [[3, 7]])).toEqual([
    { text: "le ", marked: false },
    { text: "chat", marked: true },
    { text: " dort", marked: false },
  ]);
  // Au début, à la fin, deux plages qui se touchent.
  expect(markedParts("chat chien", [[0, 4], [5, 10]])).toEqual([
    { text: "chat", marked: true },
    { text: " ", marked: false },
    { text: "chien", marked: true },
  ]);
  expect(markedParts("abcd", [[0, 2], [2, 4]])).toEqual([
    { text: "ab", marked: true },
    { text: "cd", marked: true },
  ]);
});

test("an_excerpt_without_range_is_one_plain_part_and_an_empty_one_has_none", () => {
  expect(markedParts("rien à surligner", [])).toEqual([{ text: "rien à surligner", marked: false }]);
  expect(markedParts("", [])).toEqual([]);
  expect(markedParts("", [[0, 3]])).toEqual([]);
});

test("ranges_count_utf16_units", () => {
  // « 😀 » tient sur deux unités : « chat » commence à la place 3.
  const excerpt = "😀 chat";
  expect(markedParts(excerpt, [[3, 7]])).toEqual([
    { text: "😀 ", marked: false },
    { text: "chat", marked: true },
  ]);
  expect(markedParts(excerpt, [[0, 2]])).toEqual([
    { text: "😀", marked: true },
    { text: " chat", marked: false },
  ]);
});

test("a_range_outside_the_text_is_ignored", () => {
  const plain = [{ text: "le chat", marked: false }];
  expect(markedParts("le chat", [[3, 8]])).toEqual(plain);
  expect(markedParts("le chat", [[7, 9]])).toEqual(plain);
  expect(markedParts("le chat", [[-1, 2]])).toEqual(plain);
  // Vide, à l'envers, ou qui n'est pas faite de places entières.
  expect(markedParts("le chat", [[3, 3]])).toEqual(plain);
  expect(markedParts("le chat", [[5, 3]])).toEqual(plain);
  expect(markedParts("le chat", [[0.5, 2]])).toEqual(plain);
  expect(markedParts("le chat", [[Number.NaN, 2]])).toEqual(plain);
  expect(markedParts("le chat", [[0, Number.POSITIVE_INFINITY]])).toEqual(plain);
  // Une forme que le type ne permet pas, mais que rien n'empêche d'arriver.
  expect(markedParts("le chat", [["0", "2"]] as unknown as [number, number][])).toEqual(plain);
  expect(markedParts("le chat", [[0]] as unknown as [number, number][])).toEqual(plain);
  expect(markedParts("le chat", null as unknown as [number, number][])).toEqual(plain);
});

test("a_range_that_cuts_a_surrogate_pair_is_ignored", () => {
  const excerpt = "a😀b";
  const plain = [{ text: excerpt, marked: false }];
  // La place 2 est entre les deux unités de « 😀 ».
  expect(markedParts(excerpt, [[0, 2]])).toEqual(plain);
  expect(markedParts(excerpt, [[2, 4]])).toEqual(plain);
  // Témoin : autour de la paire entière, la plage tient.
  expect(markedParts(excerpt, [[1, 3]])).toEqual([
    { text: "a", marked: false },
    { text: "😀", marked: true },
    { text: "b", marked: false },
  ]);
});

test("a_range_that_overlaps_or_precedes_the_one_before_is_ignored_and_the_others_kept", () => {
  expect(markedParts("le chat dort", [[3, 7], [5, 9], [8, 12]])).toEqual([
    { text: "le ", marked: false },
    { text: "chat", marked: true },
    { text: " ", marked: false },
    { text: "dort", marked: true },
  ]);
  expect(markedParts("le chat dort", [[8, 12], [3, 7]])).toEqual([
    { text: "le chat ", marked: false },
    { text: "dort", marked: true },
  ]);
});

test("no_text_is_ever_lost", () => {
  const excerpt = "<img src=x onerror=alert(1)> 😀 et la suite";
  const cases: [number, number][][] = [
    [],
    [[0, 4]],
    [[0, 4], [2, 9], [100, 200], [30, 31]],
    [[29, 30], [31, 33]],
    [[5, 1], [-3, 4], [6, 12]],
  ];
  for (const marks of cases) expect(whole(excerpt, marks)).toBe(excerpt);
});
