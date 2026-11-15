// La fenêtre non modale où se règle un bloc riche : elle prend le focus en s'ouvrant, le garde (Tab
// n'en sort pas), se ferme sur Échap ou un clic dehors, et reste dans l'écran, au-dessous de son
// bloc s'il y a la place, au-dessus sinon, et jamais coupée en bas.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../../test/hooks";
import { BlockPopover, placeWindow } from "./BlockPopover";

const VIEWPORT = { width: 1280, height: 720 };
const SIZE = { width: 320, height: 200 };

/** Le rectangle d'un élément à (left, top), de 100 × 24. */
function rect(left: number, top: number, width = 100, height = 24) {
  return { left, top, right: left + width, bottom: top + height, width, height };
}

test("a_window_goes_under_its_block_when_there_is_room", () => {
  expect(placeWindow(rect(400, 100), SIZE, VIEWPORT)).toEqual({ top: 130, left: 400, maxHeight: 704 });
});

test("a_window_goes_above_its_block_near_the_bottom_of_the_screen", () => {
  // Au-dessous, elle finirait à 690 + 6 + 200 = 896 > 712 : elle passe au-dessus.
  expect(placeWindow(rect(400, 666), SIZE, VIEWPORT)).toEqual({ top: 460, left: 400, maxHeight: 704 });
});

test("a_window_never_leaves_the_screen", () => {
  // Ni la place au-dessous, ni au-dessus (un bloc au milieu d'un écran bas) : collée au bas, entière.
  const low = { width: 1280, height: 300 };
  const placed = placeWindow(rect(400, 120), SIZE, low);
  expect(placed.top).toBeGreaterThanOrEqual(8);
  expect(placed.top + SIZE.height).toBeLessThanOrEqual(low.height - 8);
  // Plus haute que l'écran : en haut, et bornée (elle défile en dedans).
  const tall = placeWindow(rect(400, 120), { width: 320, height: 900 }, VIEWPORT);
  expect(tall).toEqual({ top: 8, left: 400, maxHeight: 704 });
  // Un bloc au bord droit : la fenêtre recule pour tenir.
  expect(placeWindow(rect(1200, 100), SIZE, VIEWPORT).left).toBe(1280 - 8 - 320);
  // Un bloc sorti à gauche (défilement horizontal) : la fenêtre reste au bord.
  expect(placeWindow(rect(-50, 100), SIZE, VIEWPORT).left).toBe(8);
  // Un bloc défilé hors de l'écran, en haut ou en bas : la fenêtre reste dedans.
  for (const top of [-500, 2000]) {
    const placed = placeWindow(rect(400, top), SIZE, VIEWPORT);
    expect(placed.top, String(top)).toBeGreaterThanOrEqual(8);
    expect(placed.top + SIZE.height, String(top)).toBeLessThanOrEqual(VIEWPORT.height - 8);
  }
});

let host: HTMLElement;
let root: Root;
let anchor: HTMLButtonElement;
let outside: HTMLButtonElement;

beforeEach(() => {
  host = document.createElement("div");
  anchor = document.createElement("button");
  anchor.textContent = "Ancre";
  outside = document.createElement("button");
  outside.textContent = "Ailleurs";
  document.body.append(anchor, outside, host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  anchor.remove();
  outside.remove();
});

async function open(onClose: (how: string) => void) {
  await act(async () =>
    root.render(
      <BlockPopover label="Réglage du bloc" anchor={() => anchor} onClose={onClose}>
        <button type="button">Premier</button>
        <input aria-label="Champ" data-autofocus="" />
        <button type="button">Dernier</button>
      </BlockPopover>,
    ),
  );
  await act(async () => {
    await new Promise((done) => setTimeout(done, 0));
  });
  return host.querySelector<HTMLElement>('[role="dialog"]')!;
}

function key(target: Element, key: string, init: KeyboardEventInit = {}) {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

test("popover_is_a_named_non_modal_window_that_takes_the_focus", async () => {
  const dialog = await open(() => {});
  expect(dialog.getAttribute("aria-label")).toBe("Réglage du bloc");
  expect(dialog.getAttribute("aria-modal")).toBe("false");
  // Pas un élément `dialog` : une fenêtre ouverte ferait taire Ctrl+Alt+M (`underModal`).
  expect(dialog.tagName).toBe("DIV");
  expect(document.activeElement).toBe(host.querySelector('input[aria-label="Champ"]'));
});

test("tab_never_leaves_the_popover", async () => {
  const dialog = await open(() => {});
  const [first, , last] = [...dialog.querySelectorAll<HTMLElement>("button, input")];
  last.focus();
  expect(key(last, "Tab").defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(first);
  expect(key(first, "Tab", { shiftKey: true }).defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(last);
  // Au milieu, Tab suit son cours.
  const field = host.querySelector<HTMLElement>("input")!;
  field.focus();
  expect(key(field, "Tab").defaultPrevented).toBe(false);
});

test("escape_closes_the_popover_and_says_so", async () => {
  const onClose = vi.fn();
  const dialog = await open(onClose);
  const event = key(dialog.querySelector("input")!, "Escape");
  expect(event.defaultPrevented).toBe(true);
  expect(onClose).toHaveBeenCalledWith("escape");
});

test("a_click_outside_closes_the_popover_but_not_one_inside_or_on_its_anchor", async () => {
  const onClose = vi.fn();
  const dialog = await open(onClose);
  dialog.querySelector("button")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  anchor.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  expect(onClose).not.toHaveBeenCalled();
  outside.dispatchEvent(new Event("pointerdown", { bubbles: true }));
  expect(onClose).toHaveBeenCalledWith("outside");
});
