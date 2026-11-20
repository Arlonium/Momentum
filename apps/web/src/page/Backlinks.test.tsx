// « Pages qui mènent ici », sous le texte d'une page (lot 4, Task C3) : ce qui est montré de la
// liste que rend `useBacklinks`. Le titre de chaque page vient de l'arbre publié, jamais de la
// réponse (qui n'en porte pas) ni d'une lecture d'avant ; rien ne bouge sous le pointeur ni sous
// le focus.
import type { Backlink } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { blockPath } from "../router";
import { publishPages } from "../space/pageDirectory";
import "../test/hooks";
import { page } from "../test/pages";
import { backlinkHref, Backlinks } from "./Backlinks";
import { arriveAt } from "./Breadcrumb";

vi.mock("./Breadcrumb", () => ({ arriveAt: vi.fn() }));

const A = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const B = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const C = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(arriveAt).mockClear();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  publishPages(null, null);
});

const link = (pageId: string, excerpt = "", blockId: string | null = null): Backlink => ({
  pageId,
  blockId,
  excerpt,
});

/** L'arbre de l'espace, tel que `App` le publie. */
const tree = (titles: Record<string, string>) =>
  act(async () =>
    publishPages(
      "space",
      Object.entries(titles).map(([id, title]) => page(id, { title })),
    ),
  );

const show = (pages: Backlink[] | null, error: string | null = null) =>
  act(async () => root.render(<Backlinks pages={pages} error={error} />));

const section = () => host.querySelector("nav");
const rows = () => [...host.querySelectorAll<HTMLAnchorElement>("[data-backlink-page]")];
const names = () => rows().map((row) => row.querySelector(".mm-backlink-name")?.textContent);

test("no_section_without_a_backlink", async () => {
  await tree({ [A]: "Alpha" });
  // Pas encore lus, puis lus et vides : rien, pas même un titre de section.
  await show(null);
  expect(host.textContent).toBe("");
  await show([]);
  expect(host.textContent).toBe("");
  // Témoin : avec une page, la section est là.
  await show([link(A)]);
  expect(section()).not.toBeNull();
});

test("the_section_is_a_list_of_links", async () => {
  await tree({ [A]: "Alpha", [B]: "Bêta" });
  await show([link(A, "voir la page Cible", "bloc-1"), link(B)]);
  const nav = section()!;
  // Un repère nommé par son titre, visible.
  const heading = nav.querySelector("h2")!;
  expect(heading.textContent).toBe("Pages qui mènent ici");
  expect(nav.getAttribute("aria-labelledby")).toBe(heading.id);
  expect(heading.id).not.toBe("");
  // Une liste, un élément par page, chacun un vrai lien vers la page.
  const list = nav.querySelector("ul")!;
  expect([...list.children].map((item) => item.tagName)).toEqual(["LI", "LI"]);
  const [first, second] = rows();
  expect(first.closest("li")?.parentElement).toBe(list);
  expect(first.tagName).toBe("A");
  expect(first.getAttribute("href")).toBe(backlinkHref(A, "bloc-1"));
  expect(second.getAttribute("href")).toBe(backlinkHref(B, null));
  expect(first.dataset.backlinkPage).toBe(A);
  // `data-page-id` est réservé aux lignes de l'arbre.
  expect(nav.querySelector("[data-page-id]")).toBeNull();
  expect(names()).toEqual(["Alpha", "Bêta"]);
  // L'extrait : du texte seul, dans sa direction ; absent quand il est vide.
  const excerpt = first.querySelector(".mm-backlink-excerpt")!;
  expect(excerpt.textContent).toBe("voir la page Cible");
  expect(excerpt.getAttribute("dir")).toBe("auto");
  expect(first.querySelector(".mm-backlink-name")?.getAttribute("dir")).toBe("auto");
  expect(second.querySelector(".mm-backlink-excerpt")).toBeNull();
});

test("backlink_href_is_the_address_of_the_page", () => {
  // Le seul endroit où un rétrolien devient une adresse : celle de la page, à son bloc (`blockPath`,
  // C2). L'identifiant d'un bloc est un texte venu d'ailleurs : toujours échappé.
  expect(backlinkHref(A, null)).toBe(`/p/${A}`);
  expect(backlinkHref(A, "bloc-1")).toBe(`/p/${A}#b=bloc-1`);
  expect(backlinkHref(A, "un bloc/#?&é")).toBe(blockPath(A, "un bloc/#?&é"));
  expect(backlinkHref(A, "un bloc/#?&é")).toBe(`/p/${A}#b=un%20bloc%2F%23%3F%26%C3%A9`);
});

test("only_a_backlink_without_a_block_notes_its_arrival_for_the_breadcrumb", async () => {
  // Le lien suivi part avec la page quittée. Sans bloc, rien d'autre ne prendrait le focus : le fil
  // d'Ariane de la page d'arrivée le prend (`arriveAt`). Avec un bloc, c'est l'arrivée sur le bloc
  // (C2) qui le prend : le fil ne doit pas s'en mêler. Un clic que le routeur ne suit pas ne note rien.
  await tree({ [A]: "Alpha", [B]: "Bêta" });
  await show([link(A, "", "bloc-1"), link(B)]);
  // jsdom ne navigue pas : le clic s'arrête ici, après le lien.
  host.addEventListener("click", (event) => event.preventDefault());
  const [withBlock, withoutBlock] = rows();
  const click = (row: HTMLElement, init: MouseEventInit = {}) =>
    act(async () => {
      row.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ...init }));
    });
  await click(withBlock);
  expect(arriveAt).not.toHaveBeenCalled();
  await click(withoutBlock, { ctrlKey: true });
  await click(withoutBlock, { button: 1 });
  expect(arriveAt).not.toHaveBeenCalled();
  await click(withoutBlock);
  expect(arriveAt).toHaveBeenCalledTimes(1);
  expect(arriveAt).toHaveBeenCalledWith(B);
});

test("an_excerpt_is_shown_as_text_never_as_markup", async () => {
  await tree({ [A]: "Alpha" });
  await show([link(A, '<img src=x onerror="alert(1)"> <b>gras</b>')]);
  const excerpt = host.querySelector(".mm-backlink-excerpt")!;
  expect(excerpt.textContent).toBe('<img src=x onerror="alert(1)"> <b>gras</b>');
  expect(excerpt.children).toHaveLength(0);
});

test("an_excerpt_cut_by_the_server_says_so", async () => {
  await tree({ [A]: "Alpha", [B]: "Bêta" });
  // 240 points de code : la borne du serveur, qui coupe sans marque. Un emoji compte pour un.
  const full = "😀".repeat(240);
  await show([link(A, full), link(B, "😀".repeat(239))]);
  const [cut, whole] = [...host.querySelectorAll(".mm-backlink-excerpt")];
  expect(cut.textContent).toBe(`${full}…`);
  expect(whole.textContent).toBe("😀".repeat(239));
});

test("the_title_is_the_one_of_the_tree_and_follows_it", async () => {
  await tree({ [A]: "Alpha", [B]: "" });
  await show([link(A), link(B)]);
  // Le serveur ne rend pas de titre vide, mais une ligne ne reste jamais muette.
  expect(names()).toEqual(["Alpha", "Sans titre"]);
  // Renommée ailleurs : la ligne suit l'arbre, sans relecture des rétroliens.
  await tree({ [A]: "Alpha renommée", [B]: "Bêta" });
  expect(names()).toEqual(["Alpha renommée", "Bêta"]);
});

test("a_source_the_tree_does_not_hold_is_not_shown", async () => {
  // La réponse et l'arbre ne sont pas lus au même instant : une source que l'arbre ne porte pas
  // (ou plus) n'est pas montrée, et surtout pas sous le titre qu'elle avait.
  await tree({ [A]: "Alpha" });
  await show([link(A), link(B, "un extrait")]);
  expect(rows().map((row) => row.dataset.backlinkPage)).toEqual([A]);
  expect(host.textContent).not.toContain("un extrait");

  await tree({ [A]: "Alpha", [B]: "Bêta" });
  expect(names()).toEqual(["Alpha", "Bêta"]);
  // Elle quitte l'arbre (accès retiré, corbeille) : sa ligne part, son titre avec.
  await tree({ [A]: "Alpha" });
  expect(names()).toEqual(["Alpha"]);
  expect(host.textContent).not.toContain("Bêta");
  // Plus aucune source montrée : plus de section.
  await tree({ [C]: "Autre" });
  expect(host.textContent).toBe("");
});

test("the_list_is_held_under_the_pointer_and_the_focus", async () => {
  await tree({ [A]: "Alpha", [B]: "Bêta", [C]: "Gamma" });
  await show([link(B), link(C)]);
  const nav = section()!;
  const over = (type: "pointerover" | "pointerout") =>
    act(async () => {
      nav.dispatchEvent(new MouseEvent(type, { bubbles: true, relatedTarget: document.body }));
    });

  // Sous le pointeur : une relecture qui ajoute une page en tête et en retire une ne change rien.
  await over("pointerover");
  await show([link(A), link(C)]);
  expect(names()).toEqual(["Bêta", "Gamma"]);
  // Le focus y entre, le pointeur en sort : toujours tenue.
  await act(async () => rows()[1].focus());
  await over("pointerout");
  await show([link(A)]);
  expect(names()).toEqual(["Bêta", "Gamma"]);
  expect(document.activeElement).toBe(rows()[1]);
  // D'un lien à l'autre, le focus reste dedans.
  await act(async () => rows()[0].focus());
  expect(names()).toEqual(["Bêta", "Gamma"]);
  // Le focus en sort : la liste suit enfin ce qui a été lu.
  await act(async () => (document.activeElement as HTMLElement).blur());
  expect(names()).toEqual(["Alpha"]);
});

test("a_list_emptied_while_held_stays_until_released", async () => {
  await tree({ [A]: "Alpha" });
  await show([link(A)]);
  await act(async () => rows()[0].focus());
  await show([]);
  expect(names()).toEqual(["Alpha"]);
  expect(document.activeElement).toBe(rows()[0]);
  await act(async () => (document.activeElement as HTMLElement).blur());
  expect(host.textContent).toBe("");
});

test("a_source_gone_from_the_tree_while_held_keeps_its_place_without_its_title", async () => {
  // Tenue sous le focus, une ligne ne part pas ; mais son titre n'est plus celui de l'arbre : elle
  // passe sous un libellé neutre, sans son extrait, jamais sous le titre gardé.
  await tree({ [A]: "Alpha", [B]: "Bêta" });
  await show([link(A, "extrait de A"), link(B)]);
  await act(async () => rows()[0].focus());
  await tree({ [B]: "Bêta" });
  expect(names()).toEqual(["Page indisponible", "Bêta"]);
  expect(host.textContent).not.toContain("Alpha");
  expect(host.textContent).not.toContain("extrait de A");
  expect(document.activeElement).toBe(rows()[0]);
  await act(async () => (document.activeElement as HTMLElement).blur());
  expect(names()).toEqual(["Bêta"]);
});

test("a_failed_read_is_said_under_the_list", async () => {
  await tree({ [A]: "Alpha" });
  await show([link(A)], "page introuvable");
  const said = host.querySelector('[role="status"]')!;
  expect(said.textContent).toContain("page introuvable");
  // Sous la liste : rien ne descend.
  const list = host.querySelector("ul")!;
  expect(list.compareDocumentPosition(said) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  // Sans liste, la section le dit quand même.
  await show(null, "Le serveur Mementum ne répond pas.");
  expect(section()).not.toBeNull();
  expect(host.querySelector('[role="status"]')?.textContent).toContain("ne répond pas");
  expect(host.querySelector("ul")).toBeNull();
});
