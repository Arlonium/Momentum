// Le menu « … » d'une ligne de l'arbre : pour qui peut modifier la page, les déplacements, puis le
// favori (R254), à tout niveau, puis la corbeille, en dernier (R412). Un menu ouvert qui perd une
// entrée se ferme, et rend le focus à son bouton.
import type { PageNode } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { page } from "../test/pages";
import { SectionList } from "./SectionList";

let host: HTMLDivElement;
let root: Root;
/** La zone qui défile : l'arbre n'y rend que les lignes de sa fenêtre (`treeWindow.ts`). */
let scroller: HTMLElement;

beforeEach(() => {
  // jsdom n'a pas d'observateur de taille ; l'arbre n'en a besoin que pour suivre le défilement.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  host = document.createElement("div");
  document.body.append(host);
  scroller = host;
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  document.body.replaceChildren();
  localStorage.clear();
  vi.unstubAllGlobals();
});

type Options = {
  favorites?: string[];
  onFavorite?: (id: string) => void;
  readOnly?: boolean;
  onTrash?: (id: string) => void;
};

async function show(nodes: PageNode[], { favorites, onFavorite, readOnly = false, onTrash = () => {} }: Options = {}) {
  await act(async () =>
    root.render(
      <SectionList
        spaceId="space"
        role="member"
        nodes={nodes}
        unavailable={false}
        viewport={{ scroller, top: null, bottom: null }}
        activeId={null}
        readOnly={readOnly}
        onCreate={() => {}}
        onTrash={onTrash}
        onMove={() => {}}
        favorites={favorites}
        onFavorite={onFavorite}
      />,
    ),
  );
}

const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });
const moreOf = (title: string) =>
  host.querySelector<HTMLButtonElement>(`button[aria-label="Actions de « ${title} »"]`);
const addUnder = (title: string) =>
  host.querySelector(`button[aria-label="Ajouter une sous-page à « ${title} »"]`);
const menu = () => document.querySelector<HTMLElement>('[role="menu"]');
const entries = () =>
  [...(menu()?.querySelectorAll('[role="menuitem"]') ?? [])].map((item) => item.textContent);
/** Les entrées du menu de la ligne `title`, ouvert puis refermé. */
async function entriesOf(title: string): Promise<string[] | null> {
  const button = moreOf(title);
  if (button === null) return null;
  await click(button);
  const shown = entries() as string[];
  await click(button);
  return shown;
}

const MOVES = [
  "Déplacer vers le haut",
  "Déplacer vers le bas",
  "Imbriquer dans la page précédente",
  "Sortir de la page parente",
];

test("every_row_offers_its_favorite_and_only_editors_the_rest", async () => {
  const nodes = [page("a", { level: "full" }), page("r", { level: "read", position: "a1" })];
  await show(nodes, { favorites: [], onFavorite: () => {} });
  // Qui modifie : les déplacements en tête (le premier geste au clavier), le favori, puis la
  // corbeille, en dernier ; et le « + » d'une sous-page.
  expect(await entriesOf("A")).toEqual([...MOVES, "Ajouter aux favoris", "Mettre à la corbeille"]);
  expect(addUnder("A")).not.toBeNull();
  // Qui lit : le seul favori, ni « + », ni saisie pour glisser.
  expect(await entriesOf("R")).toEqual(["Ajouter aux favoris"]);
  expect(addUnder("R")).toBeNull();
  expect(host.querySelector('[data-page-id="r"]')?.getAttribute("draggable")).toBeNull();
  // Depuis un onglet d'un autre schéma (R35) : le favori, qui ne s'écrit pas dans le document.
  await show(nodes, { favorites: [], onFavorite: () => {}, readOnly: true });
  expect(await entriesOf("A")).toEqual(["Ajouter aux favoris"]);
  expect(addUnder("A")).toBeNull();
  // Sans favori donné, la ligne d'un lecteur n'a pas de menu, comme avant.
  await show(nodes);
  expect(moreOf("R")).toBeNull();
  expect(await entriesOf("A")).toEqual([...MOVES, "Mettre à la corbeille"]);
});

test("row_favorite_says_what_it_does_and_names_its_page", async () => {
  const onFavorite = vi.fn();
  const nodes = [page("a", { level: "read" }), page("b", { level: "read", position: "a1" })];
  await show(nodes, { favorites: ["a"], onFavorite });
  expect(await entriesOf("A")).toEqual(["Retirer des favoris"]);
  expect(await entriesOf("B")).toEqual(["Ajouter aux favoris"]);
  await click(moreOf("B")!);
  await click(menu()!.querySelector('[role="menuitem"]')!);
  expect(onFavorite.mock.calls).toEqual([["b"]]);
  expect(menu()).toBeNull();
  // Le menu rend le focus à son bouton.
  expect(document.activeElement).toBe(moreOf("B"));
});

test("an_open_row_menu_that_loses_an_entry_closes_and_gives_the_focus_to_its_button", async () => {
  // Le niveau tombe sous « Peut modifier » (une relecture de l'arbre), menu ouvert, le focus sur
  // « Mettre à la corbeille » : l'entrée part, le menu se ferme, le focus revient au « … ».
  const onFavorite = () => {};
  await show([page("a", { level: "edit" })], { favorites: [], onFavorite });
  await click(moreOf("A")!);
  const trash = [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')].at(-1)!;
  trash.focus();
  expect(document.activeElement?.textContent).toBe("Mettre à la corbeille");
  await show([page("a", { level: "read" })], { favorites: [], onFavorite });
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(moreOf("A"));
  // Le favori qui change de libellé (relu) ne le ferme pas.
  await click(moreOf("A")!);
  expect(entries()).toEqual(["Ajouter aux favoris"]);
  await show([page("a", { level: "read" })], { favorites: ["a"], onFavorite });
  expect(entries()).toEqual(["Retirer des favoris"]);
});
