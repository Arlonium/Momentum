// Une liste courte de la barre latérale (« Récents », puis « Favoris ») : une section
// repliable de liens vers des pages, absente quand elle est vide.
import type { PageNode } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import "../test/hooks";
import { page } from "../test/pages";
import { readCollapsedGroup } from "./sections";
import { ShortList } from "./ShortList";

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
  localStorage.clear();
});

async function show(pages: PageNode[], activeId: string | null = null, after = false) {
  await act(async () =>
    root.render(
      <>
        <ShortList spaceId="s1" label="Récents" group="recents" pages={pages} activeId={activeId} />
        {/* Ce qui suit la liste dans la barre latérale : le titre d'une section. */}
        {after && (
          <button type="button" className="mm-section-title">
            Espace
          </button>
        )}
      </>,
    ),
  );
}

const group = () => host.querySelector<HTMLElement>('[role="group"]');
const title = () => group()!.querySelector<HTMLButtonElement>("button")!;
const links = () => [...host.querySelectorAll<HTMLAnchorElement>("ul > li > a")];

test("short_list_is_a_collapsible_list_of_links", async () => {
  await show([page("a"), page("b")], "b");
  // Une section nommée par son titre, qui la replie.
  expect(group()!.getAttribute("aria-labelledby")).toBe(title().id);
  expect(title().textContent).toBe("Récents");
  expect(title().getAttribute("aria-expanded")).toBe("true");
  expect(links().map((link) => link.getAttribute("href"))).toEqual(["/p/a", "/p/b"]);
  expect(links().map((link) => link.textContent)).toEqual(["A", "B"]);
  // La page ouverte est marquée.
  expect(links()[1].getAttribute("aria-current")).toBe("page");
  expect(links()[0].getAttribute("aria-current")).toBeNull();
  // Pas de `data-page-id` : les lignes de l'arbre restent seules à le porter.
  expect(host.querySelector("[data-page-id]")).toBeNull();

  // Repliée : plus de liens, et l'état est gardé pour l'espace.
  await act(async () => title().click());
  expect(title().getAttribute("aria-expanded")).toBe("false");
  expect(links()).toEqual([]);
  expect(readCollapsedGroup("s1", "recents")).toBe(true);

  // Remontée (une autre visite), elle reste repliée ; dépliée, elle le garde aussi.
  await act(async () => root.render(<></>));
  await show([page("a"), page("b")]);
  expect(title().getAttribute("aria-expanded")).toBe("false");
  await act(async () => title().click());
  expect(links()).toHaveLength(2);
  expect(readCollapsedGroup("s1", "recents")).toBe(false);
});

test("empty_short_list_is_absent", async () => {
  await show([]);
  expect(host.innerHTML).toBe("");
});

test("short_list_titles_are_isolated_text", async () => {
  const tags = '<img src="x" onerror="alert(1)">Plan';
  const arabic = "مرحبا بالعالم";
  await show([page("a", { title: tags }), page("b", { title: arabic })]);
  expect(host.querySelector("img")).toBeNull();
  const [first, second] = links();
  expect(first.textContent).toBe(tags);
  for (const [link, text] of [
    [first, tags],
    [second, arabic],
  ] as const) {
    const label = link.querySelector<HTMLElement>(".mm-isolate")!;
    expect(label.textContent).toBe(text);
    expect(label.getAttribute("dir")).toBe("auto");
    expect(link.getAttribute("title")).toBe(`⁨${text}⁩`);
  }
});

test("focus_of_a_page_that_leaves_the_list_goes_to_its_neighbour", async () => {
  await show([page("a"), page("b"), page("c")], null, true);
  links()[1].focus();
  // La page quitte la liste (à la corbeille, ou plus dans l'arbre) : le focus ne tombe pas sur
  // `body`, il va à celle qui prend sa place.
  await show([page("a"), page("c")], null, true);
  expect(document.activeElement).toBe(links()[1]);
  expect(links()[1].textContent).toBe("C");
  // La dernière : à celle d'avant.
  await show([page("a")], null, true);
  expect(document.activeElement).toBe(links()[0]);
  // La liste se vide et disparaît : au titre de ce qui la suit.
  await show([], null, true);
  expect(group()).toBeNull();
  expect(document.activeElement?.textContent).toBe("Espace");
});

test("focus_put_elsewhere_is_not_taken_back", async () => {
  // Témoin : le focus posé ailleurs entre-temps n'est pas repris.
  await show([page("a"), page("b")], null, true);
  links()[1].focus();
  const elsewhere = host.querySelector<HTMLElement>(".mm-section-title")!;
  elsewhere.focus();
  await show([page("a")], null, true);
  expect(document.activeElement).toBe(elsewhere);
});
