// Le menu « … » de la barre du haut d'une page : « Historique », à tout niveau (on lit l'historique
// dès « Peut lire »). Le code de la fenêtre n'est chargé qu'à la demande : rien de plus sur le
// chemin de l'ouverture d'une page. Fermée, la fenêtre rend le focus au bouton du menu.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { page } from "../test/pages";
import { PageMenu } from "./PageMenu";

const probe = vi.hoisted(() => ({ loaded: false }));

// La fenêtre elle-même est essayée à part : ici, elle prend le focus comme elle, et se ferme.
vi.mock("../history/HistoryDialog", () => {
  probe.loaded = true;
  return {
    HistoryDialog: ({ onClose, onRestored }: { onClose: () => void; onRestored: () => void }) => (
      <div role="dialog" aria-label="Historique">
        <button type="button" autoFocus onClick={onClose}>
          Fermer
        </button>
        <button
          type="button"
          onClick={() => {
            onRestored();
            onClose();
          }}
        >
          Restaurer
        </button>
      </div>
    ),
  };
});

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  document.body.replaceChildren();
});

const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });

const menuButton = () => host.querySelector<HTMLButtonElement>('button[aria-label="Actions de la page"]')!;
const menu = () => document.querySelector<HTMLElement>('[role="menu"]');

/** Attend que `check` soit vrai, en laissant React et les promesses suivre (borne large). */
async function until(check: () => boolean, what: string): Promise<void> {
  const end = Date.now() + 5_000;
  while (!check()) {
    if (Date.now() > end) throw new Error(`toujours pas : ${what}`);
    await act(async () => {
      await new Promise((done) => setTimeout(done, 10));
    });
  }
}

test("history_is_in_the_page_menu_at_every_level_and_loaded_on_demand", async () => {
  await act(async () =>
    root.render(
      <PageMenu
        node={page("p", { level: "read" })}
        session={null}
        readOnly={false}
        selfId="alice"
        onRefused={() => {}}
      />,
    ),
  );
  // Rien de l'historique n'est chargé avec la page.
  expect(probe.loaded).toBe(false);
  expect(menuButton().getAttribute("aria-haspopup")).toBe("menu");
  expect(menuButton().getAttribute("aria-expanded")).toBe("false");

  await click(menuButton());
  expect(menuButton().getAttribute("aria-expanded")).toBe("true");
  expect(menu()?.getAttribute("aria-label")).toBe("Actions de la page");
  const entries = [...menu()!.querySelectorAll('[role="menuitem"]')].map((item) => item.textContent);
  expect(entries).toEqual(["Historique"]);
  // Le menu prend le focus sur sa première entrée.
  await until(() => document.activeElement?.textContent === "Historique", "le focus dans le menu");

  await click(menu()!.querySelector('[role="menuitem"]')!);
  expect(menu()).toBeNull();
  await until(() => host.querySelector('[role="dialog"][aria-label="Historique"]') !== null, "la fenêtre");
  expect(probe.loaded).toBe(true);
  expect(document.activeElement?.textContent).toBe("Fermer");

  // Fermée, elle rend le focus au bouton du menu.
  await click(host.querySelector('[role="dialog"] button')!);
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  expect(document.activeElement).toBe(menuButton());
});

test("a_restored_version_is_announced", async () => {
  await act(async () =>
    root.render(
      <PageMenu
        node={page("p", { level: "edit" })}
        session={null}
        readOnly={false}
        selfId="alice"
        onRefused={() => {}}
      />,
    ),
  );
  // La région d'annonce est là avant l'annonce : un lecteur d'écran ne lit que ce qui y change.
  const region = () => host.querySelector('[role="status"]');
  expect(region()).not.toBeNull();
  expect(region()!.textContent).toBe("");
  await click(menuButton());
  await click(menu()!.querySelector('[role="menuitem"]')!);
  await until(() => host.querySelector('[role="dialog"]') !== null, "la fenêtre");
  const restore = [...host.querySelectorAll('[role="dialog"] button')].find(
    (button) => button.textContent === "Restaurer",
  )!;
  await click(restore);
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  expect(region()!.textContent).toBe("Version restaurée.");
  expect(document.activeElement).toBe(menuButton());
  // Rouvrir l'historique efface l'annonce d'avant.
  await click(menuButton());
  await click(menu()!.querySelector('[role="menuitem"]')!);
  expect(region()!.textContent).toBe("");
});
