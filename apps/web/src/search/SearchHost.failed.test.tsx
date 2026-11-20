// Le code de la fenêtre de recherche introuvable (réseau coupé, application mise à jour depuis) :
// un message, au lieu de rien, et l'application reste en place. Seul dans son fichier : le module
// chargé à la demande n'est demandé qu'une fois par processus.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { space } from "../test/pages";
import { useSearchWindow, type SearchWindow } from "./SearchHost";

vi.mock("./loadSearch", () => ({
  loadSearch: () => Promise.reject(new TypeError("Failed to fetch dynamically imported module")),
}));

let host: HTMLDivElement;
let root: Root;
let current: SearchWindow;

function Probe() {
  current = useSearchWindow(space("s1"), null);
  return (
    <>
      <p>le reste de l'application</p>
      {current.window}
    </>
  );
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

test("a_window_whose_code_cannot_be_loaded_says_so_and_leaves_the_application", async () => {
  // React et la barrière écrivent l'erreur dans la console : c'est voulu.
  const logged = vi.spyOn(console, "error").mockImplementation(() => {});
  await act(async () => root.render(<Probe />));
  await act(async () => current.open());
  const dialog = host.querySelector<HTMLDialogElement>("dialog")!;
  expect(dialog.open).toBe(true);
  expect(dialog.getAttribute("role")).toBe("alertdialog");
  expect(dialog.textContent).toContain("La recherche n'a pas pu s'afficher. Recharge la page pour réessayer.");
  expect(host.textContent).toContain("le reste de l'application");
  expect(logged).toHaveBeenCalled();
  // « Fermer » la retire.
  const close = [...dialog.querySelectorAll("button")].find((b) => b.textContent === "Fermer")!;
  await act(async () => close.click());
  expect(host.querySelector("dialog")).toBeNull();
  expect(host.textContent).toContain("le reste de l'application");
});
