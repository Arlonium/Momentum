// La barre latérale et ses favoris (R254) : « Favoris » est sa première section, absente quand
// elle est vide ; elle ne montre que les pages de l'arbre, et ne bouge pas sous le pointeur quand
// une relecture la change (R411), sauf pour ce qu'on y fait soi-même.
import type { Me, PageNode } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SessionContext } from "../session/session";
import "../test/hooks";
import { page, space } from "../test/pages";
import { Sidebar } from "./Sidebar";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  // jsdom n'a pas d'observateur de taille ; la barre n'en a besoin que pour ses bords collés.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  document.body.replaceChildren();
  localStorage.clear();
  vi.unstubAllGlobals();
});

const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("space")],
};

const NODES = [page("a"), page("b", { position: "a1" }), page("c", { position: "a2" })];

async function show(
  favorites: string[],
  {
    nodes = NODES,
    onFavorite = () => {},
    loaded = true,
    toggles = new Map(),
  }: {
    nodes?: PageNode[];
    onFavorite?: (id: string) => void;
    loaded?: boolean;
    toggles?: ReadonlyMap<string, number>;
  } = {},
) {
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me, refresh: async () => {} }}>
        <Sidebar
          space={me.spaces[0]}
          nodes={nodes}
          activeId={null}
          settingsOpen={false}
          trashOpen={false}
          readOnly={false}
          onCreate={() => {}}
          onTrash={() => {}}
          onMove={() => {}}
          notice={null}
          readError={null}
          onRetry={() => {}}
          favorites={{ pages: favorites, loaded, toggles }}
          onFavorite={onFavorite}
        />
      </SessionContext.Provider>,
    ),
  );
}

const nav = () => host.querySelector("nav")!;
const favoritesList = () => host.querySelector<HTMLElement>('[role="group"][data-group="favorites"]');
const shown = () =>
  [...(favoritesList()?.querySelectorAll("a") ?? [])].map((link) => link.textContent);
/** Le pointeur entre dans la barre latérale, ou en sort (React les lit de `pointerover`/`pointerout`). */
const pointer = (into: boolean) =>
  act(async () => {
    const outside = document.body;
    const target = into ? nav() : outside;
    const from = into ? outside : nav();
    from.dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: target }));
    target.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, relatedTarget: from }));
  });

test("favorites_are_the_first_section_and_show_only_pages_of_the_tree", async () => {
  await show([]);
  // Vide : absente.
  expect(favoritesList()).toBeNull();
  await show(["c", "x", "a"]);
  expect(favoritesList()?.querySelector(".mm-section-title")?.textContent).toBe("Favoris");
  // Dans l'ordre donné, sans la page que l'arbre n'a pas.
  expect(shown()).toEqual(["C", "A"]);
  // Avant les sections de l'espace : la première de ce qui défile sous la tête.
  const sections = host.querySelector(".mm-sections")!;
  expect(favoritesList()!.compareDocumentPosition(sections) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(favoritesList()!.previousElementSibling?.classList.contains("mm-side-head")).toBe(true);
  // Les liens ne portent pas `data-page-id`, réservé aux lignes de l'arbre.
  expect(favoritesList()!.querySelector("[data-page-id]")).toBeNull();
});

test("favorites_do_not_move_under_the_pointer_when_a_reread_changes_them", async () => {
  await show(["a"]);
  await pointer(true);
  // Une relecture ajoute « b » (posé dans un autre onglet) : rien ne grandit sous le pointeur.
  await show(["a", "b"]);
  expect(shown()).toEqual(["A"]);
  // « a » quitte l'arbre (corbeille ailleurs) : la liste garde sa taille.
  await show(["b"], { nodes: NODES.slice(1) });
  expect(shown()).toEqual(["A"]);
  // Le pointeur sort : la liste reçue.
  await pointer(false);
  expect(shown()).toEqual(["B"]);
});

test("a_favorite_set_from_a_row_shows_at_once_under_the_pointer", async () => {
  const onFavorite = vi.fn();
  await show(["a"], { onFavorite });
  await pointer(true);
  // Par le menu de la ligne de « c » : c'est le geste de la personne, il se voit tout de suite.
  await act(async () => host.querySelector<HTMLElement>('button[aria-label="Actions de « C »"]')!.click());
  const item = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(
    (entry) => entry.textContent === "Ajouter aux favoris",
  )!;
  await act(async () => item.click());
  expect(onFavorite.mock.calls).toEqual([["c"]]);
  // `App` pose le favori (`useFavorites`), qui dit le rang du geste.
  const toggles = new Map([["c", 1]]);
  await show(["a", "c"], { onFavorite, toggles });
  expect(shown()).toEqual(["A", "C"]);
  // Une arrivée d'ailleurs, elle, attend toujours que le pointeur sorte.
  await show(["a", "c", "b"], { onFavorite, toggles });
  expect(shown()).toEqual(["A", "C"]);
  await pointer(false);
  expect(shown()).toEqual(["A", "C", "B"]);
});

test("favorites_are_not_held_before_their_first_read", async () => {
  // Le pointeur est déjà dans la barre (le sélecteur d'espace y est, la souris y reposait au
  // chargement) quand la liste arrive : elle se montre, et c'est elle qui est gardée ensuite.
  await show([], { loaded: false });
  await pointer(true);
  await show(["a"]);
  expect(shown()).toEqual(["A"]);
  await show(["a", "b"]);
  expect(shown()).toEqual(["A"]);
});

test("a_favorite_set_from_the_page_menu_shows_at_once_under_the_pointer", async () => {
  // La souris repose sur la barre ; au clavier, le menu « … » de la page pose un favori. C'est
  // aussi le geste de la personne : il se voit tout de suite.
  await show(["a"]);
  await pointer(true);
  await show(["a", "b"], { toggles: new Map([["b", 1]]) });
  expect(shown()).toEqual(["A", "B"]);
  // Témoin : sans geste d'ici, la même arrivée attend.
  await show(["a", "b", "c"], { toggles: new Map([["b", 1]]) });
  expect(shown()).toEqual(["A", "B"]);
});

/** Le menu de la ligne `title`, ouvert. */
async function openRowMenuOf(title: string) {
  await act(async () =>
    host.querySelector<HTMLElement>(`button[aria-label="Actions de « ${title} »"]`)!.click(),
  );
  return document.querySelector<HTMLElement>('[role="menu"]')!;
}
/** Le pointeur passe sur `target`, venu de `from` (Chromium : `pointerout` puis `pointerover`). */
const moveOnto = (target: Element, from: Element | null) =>
  act(async () => {
    from?.dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: target }));
    target.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, relatedTarget: from }));
  });

test("favorites_thaw_once_a_row_menu_closes_under_the_pointer_over_the_content", async () => {
  // I1 : le menu d'une ligne, posé dans `body`, déborde sur le contenu. On y clique une entrée là
  // où il déborde : il disparaît sous le pointeur, et Chromium n'envoie aucun `pointerout` depuis
  // lui, seulement `pointerover` à ce qui est dessous (`body`, puis le contenu).
  await show(["a"]);
  await pointer(true);
  const menu = await openRowMenuOf("C");
  const item = [...menu.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(
    (entry) => entry.textContent === "Ajouter aux favoris",
  )!;
  await moveOnto(item, nav());
  // Sur le menu, le pointeur est encore dans la barre : une arrivée d'ailleurs attend.
  await show(["a", "b"]);
  expect(shown()).toEqual(["A"]);
  await act(async () => item.click());
  expect(document.querySelector('[role="menu"]')).toBeNull();
  const content = document.createElement("main");
  document.body.append(content);
  await act(async () => {
    document.body.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, relatedTarget: null }));
    content.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, relatedTarget: document.body }));
  });
  // Le pointeur est sur le contenu : la liste reçue se montre.
  expect(shown()).toEqual(["A", "B"]);
});

test("favorites_thaw_once_the_pointer_leaves_the_window_from_the_sidebar", async () => {
  await show(["a"]);
  await pointer(true);
  await show(["a", "b"]);
  expect(shown()).toEqual(["A"]);
  // Hors de la fenêtre : `pointerout` sans élément d'arrivée.
  await act(async () => {
    nav().dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: null }));
  });
  expect(shown()).toEqual(["A", "B"]);
});

test("favorites_thaw_at_the_first_pointer_move_after_a_drag_that_says_no_end", async () => {
  // N2 : la ligne tenue a quitté le document pendant le glisser (mise à la corbeille ailleurs) :
  // `dragend` va à la ligne détachée, et rien n'arrive au document. Le premier `pointerover`
  // d'après, sur le contenu, dit que le glisser est fini (R419).
  await show(["a"]);
  await pointer(true);
  const title = host.querySelector(".mm-sections .mm-section-title")!;
  await act(async () => {
    title.dispatchEvent(new Event("dragstart", { bubbles: true }));
    nav().dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: null }));
  });
  await show(["a", "b"]);
  expect(shown()).toEqual(["A"]);
  const content = document.createElement("main");
  document.body.append(content);
  await act(async () => {
    content.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, relatedTarget: null }));
  });
  expect(shown()).toEqual(["A", "B"]);
});

test("favorites_hold_still_while_a_page_is_dragged_in_the_tree", async () => {
  // I2 : au début d'un glisser HTML5, Chromium envoie `pointercancel`, puis `pointerout` et
  // `pointerleave` ; plus aucun événement de pointeur jusqu'à la fin du glisser. Le pointeur tient
  // une ligne : rien ne doit bouger sous lui.
  await show(["a"]);
  await pointer(true);
  const title = host.querySelector(".mm-sections .mm-section-title")!;
  await act(async () => {
    title.dispatchEvent(new Event("dragstart", { bubbles: true }));
    nav().dispatchEvent(new MouseEvent("pointercancel", { bubbles: true }));
    nav().dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: null }));
  });
  await show(["a", "b"]);
  expect(shown()).toEqual(["A"]);
  // Le glisser finit (lâché hors de la barre) : la liste reçue arrive.
  await act(async () => {
    title.dispatchEvent(new Event("dragend", { bubbles: true }));
  });
  expect(shown()).toEqual(["A", "B"]);
});

test("a_gesture_made_before_the_hold_and_undone_during_it_shows_at_once", async () => {
  // M3 : « b » est posé par le menu de la page, le pointeur hors de la barre ; il y entre avant la
  // réponse, puis le serveur refuse. `useFavorites` défait le geste et lui redonne un rang : la
  // barre le montre défait tout de suite, comme le geste lui-même.
  await show(["a", "b"], { toggles: new Map([["b", 1]]) });
  await pointer(true);
  expect(shown()).toEqual(["A", "B"]);
  await show(["a"], { toggles: new Map([["b", 2]]) });
  expect(shown()).toEqual(["A"]);
});
