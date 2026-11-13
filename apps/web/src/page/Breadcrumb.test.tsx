// Le fil d'Ariane de la barre du haut (R256) : les pages parentes que le compte voit, puis la
// page ; au-delà de quatre niveaux, la première, « … » (un menu des autres) et les deux
// dernières. Les titres viennent d'autres comptes : du texte isolé, infobulle comprise.
import type { PageNode } from "@mementum/protocol";
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test } from "vitest";
import "../test/hooks";
import { page } from "../test/pages";
import { Breadcrumb } from "./Breadcrumb";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  history.replaceState(null, "", "/");
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  document.body.replaceChildren();
});

/** Une chaîne de pages `ids[0] > ids[1] > …`, chacune sous la précédente. */
function chain(...ids: string[]): PageNode[] {
  return ids.map((id, index) => page(id, { parentId: index === 0 ? null : ids[index - 1] }));
}

async function show(node: PageNode, ancestors: PageNode[]): Promise<void> {
  await act(async () => root.render(<Breadcrumb node={node} ancestors={ancestors} />));
}

const nav = () => host.querySelector<HTMLElement>('nav[aria-label="Fil d\'Ariane"]');
const items = () => [...nav()!.querySelectorAll<HTMLElement>(":scope > ol > li")];
const menu = () => document.querySelector<HTMLElement>('[role="menu"]');
const more = () => nav()!.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]');

/** Ce que montre chaque élément du fil : le texte d'un lien, « … », ou la page en cours. */
function shown(): string[] {
  return items().map((item) => {
    const link = item.querySelector("a");
    if (link) return `lien ${link.getAttribute("href")} ${link.textContent}`;
    if (item.querySelector('button[aria-haspopup="menu"]')) return "…";
    const current = item.querySelector('[aria-current="page"]');
    return `page ${current?.textContent}`;
  });
}

const press = (key: string) =>
  act(async () => {
    document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });

test("breadcrumb_is_a_list_of_links_ending_with_the_page", async () => {
  const [a, b, c] = chain("a", "b", "c");
  await show(c, [a, b]);
  expect(nav()).not.toBeNull();
  expect(nav()!.querySelector(":scope > ol")).not.toBeNull();
  expect(shown()).toEqual(["lien /p/a A", "lien /p/b B", "page C"]);
  // La page en cours n'est pas un lien.
  expect(items()[2].querySelector("a")).toBeNull();
  // Les séparateurs ne se lisent pas.
  const separators = [...nav()!.querySelectorAll(".mm-crumb-separator")];
  expect(separators).toHaveLength(2);
  for (const separator of separators) expect(separator.getAttribute("aria-hidden")).toBe("true");

  // Une page à la racine : son titre seul.
  await show(a, []);
  expect(shown()).toEqual(["page A"]);
  expect(nav()!.querySelectorAll(".mm-crumb-separator")).toHaveLength(0);
});

test("four_levels_are_shown_in_full", async () => {
  const [a, b, c, d] = chain("a", "b", "c", "d");
  await show(d, [a, b, c]);
  expect(shown()).toEqual(["lien /p/a A", "lien /p/b B", "lien /p/c C", "page D"]);
  expect(more()).toBeNull();
});

test("deep_breadcrumb_folds_its_middle_into_a_menu", async () => {
  const [a, b, c, d, e, f] = chain("a", "b", "c", "d", "e", "f");
  await show(f, [a, b, c, d, e]);
  expect(shown()).toEqual(["lien /p/a A", "…", "lien /p/e E", "page F"]);
  const button = more()!;
  expect(button.getAttribute("aria-expanded")).toBe("false");
  expect(button.getAttribute("aria-label")).toBe("Pages parentes masquées");

  // Au clavier : le bouton (Entrée, un clic sans `detail`) ouvre le menu, qui prend le focus sur
  // la première page masquée ; les flèches passent de l'une à l'autre.
  button.focus();
  await act(async () => button.click());
  expect(button.getAttribute("aria-expanded")).toBe("true");
  expect(menu()?.getAttribute("aria-label")).toBe("Pages parentes masquées");
  const entries = [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')];
  expect(entries.map((entry) => entry.textContent)).toEqual(["B", "C", "D"]);
  expect(document.activeElement).toBe(entries[0]);
  await press("ArrowDown");
  expect(document.activeElement).toBe(entries[1]);
  await press("End");
  expect(document.activeElement).toBe(entries[2]);

  // Échap ferme le menu et rend le focus au bouton.
  await press("Escape");
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(button);
  expect(button.getAttribute("aria-expanded")).toBe("false");

  // Une entrée choisie mène à sa page.
  await act(async () => button.click());
  await act(async () => menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')[1].click());
  expect(location.pathname).toBe("/p/c");
  expect(menu()).toBeNull();
});

test("menu_closes_when_the_hidden_pages_change_under_it", async () => {
  // Une page déplacée ailleurs pendant que le menu est ouvert : ses entrées ne valent plus. Il
  // se ferme, et le focus ne tombe pas sur `body`.
  const [a, b, c, d, e, f] = chain("a", "b", "c", "d", "e", "f");
  await show(f, [a, b, c, d, e]);
  await act(async () => more()!.click());
  expect(document.activeElement?.getAttribute("role")).toBe("menuitem");
  // Toujours replié, d'autres pages masquées : le focus revient au bouton « … ».
  const x = page("x", { parentId: "a" });
  await show(f, [a, x, c, d, e]);
  expect(menu()).toBeNull();
  expect(document.activeElement).toBe(more());

  // Le fil n'est plus replié : le focus va à la page.
  await act(async () => more()!.click());
  expect(menu()).not.toBeNull();
  await show(f, [a, e]);
  expect(menu()).toBeNull();
  expect(more()).toBeNull();
  expect(document.activeElement).toBe(items()[2].querySelector('[aria-current="page"]'));

  // Témoin : un titre renommé ne ferme pas le menu, il le met à jour.
  await show(f, [a, b, c, d, e]);
  await act(async () => more()!.click());
  await show(f, [a, { ...b, title: "B2" }, c, d, e]);
  expect(menu()).not.toBeNull();
  expect(menu()!.querySelector('[role="menuitem"]')!.textContent).toBe("B2");
});

test("five_levels_fold_two_pages", async () => {
  const [a, b, c, d, e] = chain("a", "b", "c", "d", "e");
  await show(e, [a, b, c, d]);
  expect(shown()).toEqual(["lien /p/a A", "…", "lien /p/d D", "page E"]);
  await act(async () => more()!.click());
  const entries = [...menu()!.querySelectorAll('[role="menuitem"]')];
  expect(entries.map((entry) => entry.textContent)).toEqual(["B", "C"]);
});

test("titles_are_isolated_text", async () => {
  const arabic = "مرحبا بالعالم";
  const tags = '<img src="x" onerror="alert(1)">Plan';
  const reversed = "‮abc";
  const [a, b, c, d, e] = [
    page("a", { title: arabic }),
    page("b", { parentId: "a", title: reversed }),
    page("c", { parentId: "b", title: "C" }),
    page("d", { parentId: "c", title: "D" }),
    page("e", { parentId: "d", title: tags }),
  ];
  await show(e, [a, b, c, d]);
  // Du texte, jamais du HTML.
  expect(host.querySelector("img")).toBeNull();
  expect(items()[3].textContent).toContain(tags);

  // Chaque titre est un élément isolé (`dir="auto"`, `unicode-bidi: isolate` par sa classe),
  // et son infobulle l'entoure de U+2068 et U+2069.
  const first = items()[0].querySelector<HTMLElement>("a")!;
  const firstText = first.querySelector<HTMLElement>(".mm-isolate")!;
  expect(firstText.textContent).toBe(arabic);
  expect(firstText.getAttribute("dir")).toBe("auto");
  expect(first.getAttribute("title")).toBe(`⁨${arabic}⁩`);

  const current = items()[3].querySelector<HTMLElement>('[aria-current="page"]')!;
  expect(current.getAttribute("dir")).toBe("auto");
  expect(current.classList.contains("mm-isolate")).toBe(true);
  expect(current.getAttribute("title")).toBe(`⁨${tags}⁩`);

  // Dans le menu aussi.
  await act(async () => more()!.click());
  const entry = menu()!.querySelector<HTMLElement>('[role="menuitem"]')!;
  const entryText = entry.querySelector<HTMLElement>(".mm-isolate")!;
  expect(entryText.textContent).toBe(reversed);
  expect(entryText.getAttribute("dir")).toBe("auto");
  expect(entry.getAttribute("title")).toBe(`⁨${reversed}⁩`);
});

/**
 * Une page de l'application, réduite à ce qui compte ici : la barre du haut est recréée à
 * chaque page ouverte (`PageView` est rendu sous la clé de la page), comme dans `App`.
 */
function Pages({ pages }: { pages: PageNode[] }) {
  const [openId, setOpenId] = useState(pages[pages.length - 1].id);
  const index = pages.findIndex((p) => p.id === openId);
  const open = (event: MouseEvent) => {
    const link = (event.target as Element).closest("a");
    if (!link) return;
    event.preventDefault();
    setOpenId(link.getAttribute("href")!.slice("/p/".length));
  };
  return (
    <div onClickCapture={(event) => open(event.nativeEvent)}>
      <button type="button">ailleurs</button>
      <Breadcrumb key={openId} node={pages[index]} ancestors={pages.slice(0, index)} />
    </div>
  );
}

test("following_a_crumb_puts_the_focus_on_the_page_reached", async () => {
  const pages = chain("a", "b", "c");
  await act(async () => root.render(<Pages pages={pages} />));
  const link = items()[1].querySelector<HTMLElement>("a")!;
  link.focus();
  // Le lien suivi quitte le document avec la barre du haut : le focus ne tombe pas sur `body`,
  // il va à la page atteinte, dans le fil de sa barre du haut.
  await act(async () => link.click());
  expect(shown()).toEqual(["lien /p/a A", "page B"]);
  const current = items()[1].querySelector<HTMLElement>('[aria-current="page"]')!;
  expect(document.activeElement).toBe(current);
});

test("a_page_opened_otherwise_leaves_the_focus_where_it_is", async () => {
  // Témoin : sans lien suivi dans le fil, une page qui s'ouvre ne prend pas le focus.
  const elsewhere = document.createElement("button");
  document.body.append(elsewhere);
  elsewhere.focus();
  const [a, b] = chain("a", "b");
  await show(b, [a]);
  expect(document.activeElement).toBe(elsewhere);
  await act(async () => root.render(<Breadcrumb key="a" node={a} ancestors={[]} />));
  expect(document.activeElement).toBe(elsewhere);
});

test("a_crumb_opened_elsewhere_does_not_take_the_focus_later", async () => {
  // Un clic que le routeur ne suit pas (Ctrl, Maj, Cmd ou Alt : un autre onglet, une autre
  // fenêtre, un téléchargement ; le bouton du milieu) ne marque pas la page : quand elle
  // s'ouvre plus tard autrement (la barre latérale), sa barre du haut ne vole pas le focus.
  const [a, b, c] = chain("a", "b", "c");
  await show(c, [a, b]);
  // Le navigateur ne suit pas ces liens ici (jsdom ne navigue pas).
  host.addEventListener("click", (event) => event.preventDefault(), { capture: true });
  const link = items()[1].querySelector<HTMLElement>("a")!;
  const clicks: MouseEventInit[] = [
    { ctrlKey: true },
    { shiftKey: true },
    { metaKey: true },
    { altKey: true },
    { button: 1 },
  ];
  for (const init of clicks) {
    await act(async () => {
      link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...init }));
    });
  }
  const elsewhere = document.createElement("button");
  document.body.append(elsewhere);
  elsewhere.focus();
  await act(async () => root.render(<Breadcrumb key="b" node={b} ancestors={[a]} />));
  expect(document.activeElement).toBe(elsewhere);
});

test("hidden_pages_with_the_same_title_keep_their_entries", async () => {
  // Deux pages masquées du même titre, puis l'une renommée : chaque entrée reste celle de sa
  // page, sans clé en double pour React.
  const errors: unknown[][] = [];
  const original = console.error;
  console.error = (...args: unknown[]) => errors.push(args);
  try {
    const [a, b, c, d, e, f] = chain("a", "b", "c", "d", "e", "f");
    const same = (node: PageNode, title: string) => ({ ...node, title });
    await show(f, [a, same(b, "Même"), same(c, "Même"), d, e]);
    await act(async () => more()!.click());
    const entries = () => [...menu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    expect(entries().map((entry) => entry.textContent)).toEqual(["Même", "Même", "D"]);
    await show(f, [a, same(b, "Même"), same(c, "Autre"), d, e]);
    expect(entries().map((entry) => entry.textContent)).toEqual(["Même", "Autre", "D"]);
    await act(async () => entries()[1].click());
    expect(location.pathname).toBe("/p/c");
    expect(errors.filter((args) => String(args[0]).includes("same key"))).toEqual([]);
  } finally {
    console.error = original;
  }
});
