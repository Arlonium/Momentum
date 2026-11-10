// Ce qui crée quelque chose à chaque clic ne compte pas le second clic d'un double clic.
import { expect, test, vi } from "vitest";
import { firstClick } from "./firstClick";

test("second_click_of_a_double_click_does_nothing", () => {
  const create = vi.fn();
  const onClick = firstClick(create);
  // Un clic simple.
  onClick({ detail: 1 });
  expect(create).toHaveBeenCalledTimes(1);
  // Le second clic d'un double clic, le troisième d'un triple : rien de plus, que le premier
  // ait déjà répondu ou non.
  onClick({ detail: 2 });
  onClick({ detail: 3 });
  expect(create).toHaveBeenCalledTimes(1);
  // Le clic suivant, simple, compte de nouveau.
  onClick({ detail: 1 });
  expect(create).toHaveBeenCalledTimes(2);
});

test("keyboard_press_counts", () => {
  // Entrée ou Espace sur un bouton : un clic sans pointeur, `detail` vaut 0.
  const create = vi.fn();
  firstClick(create)({ detail: 0 });
  expect(create).toHaveBeenCalledTimes(1);
});
