// Le menu « … » de la barre du haut d'une page : « Historique », à partir de « Peut modifier »
// (le serveur l'exige : un lecteur ne lit pas ce qui a été effacé avant qu'on partage la page
// avec lui), à la corbeille aussi. Sans entrée pour son niveau, pas de menu. Le code de la
// fenêtre n'est chargé qu'à la demande : rien de plus sur le chemin de l'ouverture d'une page.
// Fermée, la fenêtre rend le focus au bouton du menu, ou à la page s'il n'est plus là.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { until } from "../test/hooks";
import { page } from "../test/pages";
import { PageMenu } from "./PageMenu";

const probe = vi.hoisted(() => ({ loaded: false, afterLogin: undefined as unknown }));

// La fenêtre elle-même est essayée à part : ici, elle prend le focus comme elle, et se ferme.
vi.mock("../history/HistoryDialog", () => {
  probe.loaded = true;
  return {
    HistoryDialog: ({
      onClose,
      onRestored,
      afterLogin,
    }: {
      onClose: () => void;
      onRestored: () => void;
      afterLogin?: () => Promise<void>;
    }) => (
      <div role="dialog" aria-label="Historique" ref={() => void (probe.afterLogin = afterLogin)}>
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
/** Le bouton du menu, s'il est là. */
const menuButtonIfAny = () =>
  host.querySelector<HTMLButtonElement>('button[aria-label="Actions de la page"]');
const menu = () => document.querySelector<HTMLElement>('[role="menu"]');

test("history_is_in_the_page_menu_from_edit_level_and_loaded_on_demand", async () => {
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
  await until(
    host,
    () => document.activeElement?.textContent === "Historique",
    "le focus dans le menu",
  );

  await click(menu()!.querySelector('[role="menuitem"]')!);
  expect(menu()).toBeNull();
  await until(
    host,
    () => host.querySelector('[role="dialog"][aria-label="Historique"]') !== null,
    "la fenêtre",
  );
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
  await until(host, () => host.querySelector('[role="dialog"]') !== null, "la fenêtre");
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

test("the_window_waits_for_the_account_given_by_the_page", async () => {
  // Une liste refusée par `401` se relit au retour du compte : la fenêtre l'attend par la page.
  const afterLogin = () => new Promise<void>(() => {});
  await act(async () =>
    root.render(
      <PageMenu
        node={page("p")}
        session={null}
        readOnly={false}
        selfId="alice"
        afterLogin={afterLogin}
        onRefused={() => {}}
      />,
    ),
  );
  await click(menuButton());
  await click([...menu()!.querySelectorAll('[role="menuitem"]')][0]);
  await until(host, () => document.querySelector('[role="dialog"]') !== null, "la fenêtre");
  expect(probe.afterLogin).toBe(afterLogin);
});

test("history_is_not_offered_below_edit_level", async () => {
  const shown = async (level: "read" | "comment" | "edit" | "full", trashed = false, readOnly = false) => {
    await act(async () =>
      root.render(
        <PageMenu
          node={page("p", { level, trashedAt: trashed ? 1 : null })}
          session={null}
          readOnly={readOnly}
          selfId="alice"
          onRefused={() => {}}
        />,
      ),
    );
    if (menuButtonIfAny() === null) return [];
    await click(menuButton());
    const entries = [...menu()!.querySelectorAll('[role="menuitem"]')].map((item) => item.textContent);
    await click(menuButton());
    return entries;
  };
  // Sous « Peut modifier », aucune entrée : pas de menu du tout.
  expect(await shown("read")).toEqual([]);
  expect(await shown("comment")).toEqual([]);
  expect(await shown("edit")).toEqual(["Historique"]);
  expect(await shown("full")).toEqual(["Historique"]);
  // À la corbeille, et depuis un onglet d'un autre schéma, l'historique se lit encore (la fenêtre
  // ne propose pas de restaurer).
  expect(await shown("edit", true)).toEqual(["Historique"]);
  expect(await shown("full", false, true)).toEqual(["Historique"]);
});

test("a_window_whose_right_is_gone_stays_readable_and_closes_onto_the_page", async () => {
  // La fenêtre est ouverte ; le niveau baisse sous « Peut modifier ». Elle reste ouverte (son
  // refus se lit), et, fermée, rend le focus à la page : le bouton du menu n'est plus là.
  const title = document.createElement("textarea");
  document.body.append(title);
  const render = (level: "read" | "edit") =>
    act(async () =>
      root.render(
        <PageMenu
          node={page("p", { level })}
          session={null}
          readOnly={false}
          selfId="alice"
          focusFallback={() => title.focus()}
          onRefused={() => {}}
        />,
      ),
    );
  await render("edit");
  await click(menuButton());
  await click(menu()!.querySelector('[role="menuitem"]')!);
  await until(host, () => host.querySelector('[role="dialog"]') !== null, "la fenêtre");
  await render("read");
  expect(host.querySelector('[role="dialog"]')).not.toBeNull();
  await click(host.querySelector('[role="dialog"] button')!);
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  expect(menuButtonIfAny()).toBeNull();
  expect(document.activeElement).toBe(title);
});

test("an_open_menu_closes_when_its_entries_go", async () => {
  const render = (level: "read" | "edit") =>
    act(async () =>
      root.render(
        <PageMenu node={page("p", { level })} session={null} readOnly={false} selfId="alice" onRefused={() => {}} />,
      ),
    );
  await render("edit");
  await click(menuButton());
  expect(menu()).not.toBeNull();
  await render("read");
  expect(menu()).toBeNull();
  // Le niveau revient : le menu ne se rouvre pas de lui-même.
  await render("edit");
  expect(menu()).toBeNull();
  expect(menuButton().getAttribute("aria-expanded")).toBe("false");
});

test("an_open_menu_that_loses_its_entries_gives_the_focus_to_the_page", async () => {
  // Le menu est ouvert au clavier (le focus sur « Historique ») ; le niveau tombe : le menu et son
  // bouton disparaissent, et le focus va à la page, pas sur `body`.
  const title = document.createElement("textarea");
  document.body.append(title);
  const render = (level: "read" | "edit") =>
    act(async () =>
      root.render(
        <PageMenu
          node={page("p", { level })}
          session={null}
          readOnly={false}
          selfId="alice"
          focusFallback={() => title.focus()}
          onRefused={() => {}}
        />,
      ),
    );
  await render("edit");
  await click(menuButton());
  await until(
    host,
    () => document.activeElement?.textContent === "Historique",
    "le focus dans le menu",
  );
  await render("read");
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(title);
});
