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
        {/* Ce qui suit la liste dans la barre latérale : son pied. */}
        {after && (
          <div className="mm-side-footer">
            <p>Un message</p>
            <a href="/s/s1/trash">Corbeille</a>
          </div>
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
  // La liste se vide et disparaît : au premier lien ou bouton de ce qui la suit (le pied).
  await show([], null, true);
  expect(group()).toBeNull();
  expect(document.activeElement?.textContent).toBe("Corbeille");
});

test("focus_put_elsewhere_is_not_taken_back", async () => {
  // Témoin : le focus posé ailleurs entre-temps n'est pas repris, même quand la liste n'a pas
  // vu partir le focus (il l'a quittée pour `body`, sans élément d'arrivée, puis est allé
  // ailleurs hors d'elle).
  await show([page("a"), page("b")], null, true);
  links()[1].focus();
  links()[1].blur();
  const elsewhere = host.querySelector<HTMLElement>(".mm-side-footer a")!;
  elsewhere.focus();
  await show([page("a")], null, true);
  expect(document.activeElement).toBe(elsewhere);
});

// L'ordre montré est figé tant que le pointeur ou le focus est dans la liste (R298) : ouvrir
// une page récente la fait monter en tête du stockage, pas sous le pointeur.

const pages = (...ids: string[]) => ids.map((id) => page(id));
const titles = () => links().map((link) => link.textContent);
/** Ce qui suit la liste : où vont le pointeur et le focus qui en sortent. */
const outside = () => host.querySelector<HTMLElement>(".mm-side-footer a")!;
/**
 * Le pointeur passe de `from` à `to`. React tire l'entrée et la sortie d'un élément du seul
 * `pointerout` quand les deux sont dans son arbre (le `pointerover` qui suit est ignoré).
 */
const move = (from: Element, to: Element) =>
  act(async () => {
    from.dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: to }));
  });
const focus = (element: HTMLElement) => act(async () => element.focus());

test("order_stays_put_under_the_pointer", async () => {
  await show(pages("p5", "p4", "p3", "p2", "p1"), null, true);
  await move(outside(), links()[3]);
  // P2 ouverte d'un clic : elle passe en tête de ce qui est gardé, la liste ne bouge pas.
  await show(pages("p2", "p5", "p4", "p3", "p1"), "p2", true);
  expect(titles()).toEqual(["P5", "P4", "P3", "P2", "P1"]);
  expect(links()[3].getAttribute("aria-current")).toBe("page");
  // Le pointeur sort : l'ordre se remet à jour.
  await move(links()[3], outside());
  expect(titles()).toEqual(["P2", "P5", "P4", "P3", "P1"]);
});

test("order_stays_put_under_the_keyboard_focus", async () => {
  await show(pages("p5", "p4", "p3", "p2", "p1"), null, true);
  // Entrée sur le lien de P2 : la page s'ouvre, le focus reste sur le lien.
  await focus(links()[3]);
  await show(pages("p2", "p5", "p4", "p3", "p1"), "p2", true);
  expect(titles()).toEqual(["P5", "P4", "P3", "P2", "P1"]);
  // Entrée encore : la même page.
  expect(document.activeElement).toBe(links()[3]);
  expect(links()[3].getAttribute("href")).toBe("/p/p2");
  // Le focus sort : l'ordre se remet à jour, et le focus reste où il est allé.
  await focus(outside());
  expect(titles()).toEqual(["P2", "P5", "P4", "P3", "P1"]);
  expect(document.activeElement).toBe(outside());
});

test("order_is_put_back_once_pointer_and_focus_are_both_out", async () => {
  await show(pages("p5", "p4", "p3", "p2", "p1"), null, true);
  await focus(links()[3]);
  await move(outside(), links()[3]);
  await show(pages("p2", "p5", "p4", "p3", "p1"), "p2", true);
  // Le pointeur sort, le focus reste : figé.
  await move(links()[3], outside());
  expect(titles()).toEqual(["P5", "P4", "P3", "P2", "P1"]);
  // Le focus sort aussi : à jour.
  await focus(outside());
  expect(titles()).toEqual(["P2", "P5", "P4", "P3", "P1"]);
});

test("a_page_that_leaves_the_tree_goes_even_while_the_order_is_frozen", async () => {
  await show(pages("p5", "p4", "p3", "p2", "p1"), null, true);
  await focus(links()[2]);
  // P3 quitte l'arbre (à la corbeille) pendant que l'ordre est figé : elle part quand même, et
  // le focus va à celle qui prend sa place, jamais à `body`.
  await show(pages("p2", "p5", "p4", "p1"), "p2", true);
  expect(titles()).toEqual(["P5", "P4", "P2", "P1"]);
  expect(document.activeElement).toBe(links()[2]);
  // Une page qui arrive pendant le gel se range après les autres : rien ne bouge au-dessus.
  await show(pages("p6", "p2", "p5", "p4", "p1"), "p2", true);
  expect(titles()).toEqual(["P5", "P4", "P2", "P1", "P6"]);
});

test("freezing_renders_nothing_outside_the_list", async () => {
  // Le gel est un état de la liste seule : ni la barre latérale (un glisser dans l'arbre), ni la
  // page ouverte ne sont rendues de nouveau.
  let renders = 0;
  function Neighbour() {
    renders += 1;
    return <a href="/s/s1/trash">Corbeille</a>;
  }
  await act(async () =>
    root.render(
      <>
        <ShortList
          spaceId="s1"
          label="Récents"
          group="recents"
          pages={pages("p2", "p1")}
          activeId={null}
        />
        <Neighbour />
      </>,
    ),
  );
  const before = renders;
  const away = host.querySelector<HTMLElement>('a[href="/s/s1/trash"]')!;
  await move(away, links()[0]);
  await focus(links()[0]);
  await move(links()[0], away);
  await focus(away);
  expect(renders).toBe(before);
});
