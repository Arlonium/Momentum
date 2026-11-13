// La fenêtre de confirmation (`Confirm`) : un clic sur le voile annule, sauf le second clic d'un
// double clic (`detail` 2 et plus), commencé sur le bouton qui a ouvert la fenêtre ; Échap et les
// boutons, au clavier comme à la souris, ne changent pas.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { Confirm } from "./Confirm";

let host: HTMLDivElement;
let root: Root;
const onConfirm = vi.fn();
const onCancel = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onConfirm.mockClear();
  onCancel.mockClear();
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function show() {
  await act(async () =>
    root.render(
      <Confirm title="Supprimer ?" confirmLabel="Supprimer" onConfirm={onConfirm} onCancel={onCancel}>
        Cela ne se défait pas.
      </Confirm>,
    ),
  );
  return host.querySelector("dialog")!;
}

/**
 * Un clic sur la fenêtre elle-même : le voile, ou son cadre. jsdom donne au cadre une boîte nulle
 * en (0, 0) : un clic en (500, 500) est hors du cadre, sur le voile.
 */
const clickOn = (dialog: HTMLDialogElement, detail: number, at = 500) =>
  act(async () => {
    dialog.dispatchEvent(
      new MouseEvent("click", { bubbles: true, detail, clientX: at, clientY: at }),
    );
  });

const button = (name: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === name)!;

test("a_click_on_the_backdrop_cancels", async () => {
  const dialog = await show();
  // Témoin : un clic dans le cadre, hors des boutons, ne fait rien.
  await clickOn(dialog, 1, 0);
  expect(onCancel).not.toHaveBeenCalled();
  await clickOn(dialog, 1);
  expect(onCancel).toHaveBeenCalledTimes(1);
  expect(onConfirm).not.toHaveBeenCalled();
});

test("the_second_click_of_a_double_click_on_the_backdrop_does_nothing", async () => {
  // Le premier clic a ouvert la fenêtre ; le second tombe sur le voile, qui couvre maintenant le
  // bouton.
  const dialog = await show();
  await clickOn(dialog, 2);
  await clickOn(dialog, 3);
  expect(onCancel).not.toHaveBeenCalled();
  // Un clic simple, ensuite, annule comme toujours.
  await clickOn(dialog, 1);
  expect(onCancel).toHaveBeenCalledTimes(1);
});

test("the_keyboard_and_the_buttons_are_untouched", async () => {
  const dialog = await show();
  // Échap : l'événement `cancel` de la fenêtre.
  await act(async () => {
    dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
  });
  expect(onCancel).toHaveBeenCalledTimes(1);
  // Entrée ou Espace sur un bouton : un clic de `detail` 0.
  await act(async () => {
    button("Annuler").dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 0 }));
  });
  expect(onCancel).toHaveBeenCalledTimes(2);
  await act(async () => {
    button("Supprimer").dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 0 }));
  });
  expect(onConfirm).toHaveBeenCalledTimes(1);
  // À la souris, les boutons répondent à chaque clic, le second d'un double clic compris.
  await act(async () => {
    button("Annuler").dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 2 }));
  });
  expect(onCancel).toHaveBeenCalledTimes(3);
});
