// La couleur du texte d'un avatar : au moins 4,5:1 sur chacune des couleurs de présence.
import { expect, test } from "vitest";
import { PRESENCE_COLORS } from "../session/session";
import { contrast, inkOn, INK } from "./avatarInk";

test("every_presence_colour_gets_an_initial_of_at_least_4_5_to_1", () => {
  for (const colour of PRESENCE_COLORS) {
    expect(contrast(inkOn(colour), colour), colour).toBeGreaterThanOrEqual(4.5);
  }
});

test("white_is_kept_where_it_is_readable_and_the_ink_where_it_is_not", () => {
  expect(inkOn("#23704F")).toBe("#ffffff");
  expect(inkOn("#C27A12")).toBe(INK);
});
