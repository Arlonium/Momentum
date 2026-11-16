// Le menu « … » d'une ligne de l'arbre : pour qui peut modifier la page, les déplacements, puis le
// favori (R254), à tout niveau, puis la corbeille, en dernier (R412). Un menu ouvert qui perd une
// entrée se ferme, et rend le focus à son bouton.
import type { PageNode, Role } from "@mementum/protocol";
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
  role?: Role;
  onDuplicate?: (id: string) => void;
  duplicating?: boolean;
};

async function show(
  nodes: PageNode[],
  {
    favorites,
    onFavorite,
    readOnly = false,
    onTrash = () => {},
    role = "member",
    onDuplicate,
    duplicating,
  }: Options = {},
) {
  await act(async () =>
    root.render(
      <SectionList
        spaceId="space"
        role={role}
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
        onDuplicate={onDuplicate}
        duplicating={duplicating}
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

test("an_open_row_menu_never_moves_its_entries_under_the_pointer", async () => {
  // N1 : le niveau monte pendant que le menu d'un lecteur est ouvert, le pointeur sur « Ajouter aux
  // favoris ». Les déplacements s'inséreraient avant lui : « Déplacer vers le haut » prendrait sa
  // place, et un clic au même endroit déplacerait la page. Le menu se ferme (R466 complété), et
  // rend le focus à son bouton ; rouvert, il offre tout.
  const onFavorite = () => {};
  await show([page("a", { level: "read" })], { favorites: [], onFavorite });
  await click(moreOf("A")!);
  const under = 0;
  expect(entries()[under]).toBe("Ajouter aux favoris");
  (menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')[under]).focus();
  await show([page("a", { level: "edit" })], { favorites: [], onFavorite });
  // Ce qui est sous le pointeur est toujours l'entrée qu'il visait, ou plus rien.
  expect(entries()[under] ?? null).not.toBe("Déplacer vers le haut");
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(moreOf("A"));
  await click(moreOf("A")!);
  expect(entries()).toEqual([...MOVES, "Ajouter aux favoris", "Mettre à la corbeille"]);
});

test("row_menu_offers_duplicate_where_the_server_allows_it", async () => {
  // « Dupliquer » (R252), après le favori, avant la corbeille, qui reste la dernière (R412).
  const onDuplicate = vi.fn();
  const onFavorite = () => {};
  const nodes = [
    page("a", { level: "full" }),
    page("r", { level: "read", position: "a1" }),
    page("r1", { level: "read", parentId: "r" }),
    page("e", { level: "edit", position: "a2" }),
    page("e1", { level: "edit", parentId: "e" }),
  ];
  // Un membre : à tout niveau, la copie allant au pire dans son « Privé ».
  await show(nodes, { favorites: [], onFavorite, onDuplicate });
  expect(await entriesOf("A")).toEqual([
    ...MOVES,
    "Ajouter aux favoris",
    "Dupliquer",
    "Mettre à la corbeille",
  ]);
  expect(await entriesOf("R")).toEqual(["Ajouter aux favoris", "Dupliquer"]);
  expect(await entriesOf("R1")).toEqual(["Ajouter aux favoris", "Dupliquer"]);
  // Choisie, l'entrée demande la copie de sa page ; le menu se ferme et rend le focus à son bouton.
  await click(moreOf("R")!);
  const duplicate = [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(
    (item) => item.textContent === "Dupliquer",
  )!;
  await click(duplicate);
  expect(onDuplicate.mock.calls).toEqual([["r"]]);
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(moreOf("R"));

  // Un invité : seulement sous une page parente qu'il voit et peut modifier ; ni pour une page
  // partagée à la racine, ni sous une page qu'il ne fait que lire.
  const shared = nodes.map((node) => ({ ...node, section: "shared" as const }));
  await show(shared, { favorites: [], onFavorite, onDuplicate, role: "guest" });
  expect(await entriesOf("E1")).toEqual([...MOVES, "Ajouter aux favoris", "Dupliquer", "Mettre à la corbeille"]);
  expect(await entriesOf("E")).toEqual([...MOVES, "Ajouter aux favoris", "Mettre à la corbeille"]);
  expect(await entriesOf("R1")).toEqual(["Ajouter aux favoris"]);

  // Depuis un onglet d'un autre schéma (R35) : aucune création, une copie pas plus.
  await show(nodes, { favorites: [], onFavorite, onDuplicate, readOnly: true });
  expect(await entriesOf("A")).toEqual(["Ajouter aux favoris"]);
  // Sans `onDuplicate`, rien, comme avant.
  await show(nodes, { favorites: [], onFavorite });
  expect(await entriesOf("R")).toEqual(["Ajouter aux favoris"]);
  // Sans favori, une ligne en lecture a quand même son menu, pour la copie.
  await show(nodes, { onDuplicate });
  expect(await entriesOf("R")).toEqual(["Dupliquer"]);
});

test("duplicate_entry_is_inactive_while_a_duplication_runs", async () => {
  // Pendant la requête (une duplication à la fois par compte, R605), l'entrée est grisée : un clic
  // n'en demande pas d'autre. Quand elle a répondu, l'entrée revient, menu ouvert, à sa place.
  const onDuplicate = vi.fn();
  const nodes = [page("r", { level: "read" })];
  await show(nodes, { favorites: [], onFavorite: () => {}, onDuplicate, duplicating: true });
  await click(moreOf("R")!);
  const item = () =>
    [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(
      (entry) => entry.textContent === "Dupliquer",
    )!;
  expect(item().getAttribute("aria-disabled")).toBe("true");
  await click(item());
  expect(onDuplicate).not.toHaveBeenCalled();
  expect(menu()).not.toBeNull();
  await show(nodes, { favorites: [], onFavorite: () => {}, onDuplicate, duplicating: false });
  expect(entries()).toEqual(["Ajouter aux favoris", "Dupliquer"]);
  expect(item().getAttribute("aria-disabled")).toBeNull();
  await click(item());
  expect(onDuplicate.mock.calls).toEqual([["r"]]);
});
