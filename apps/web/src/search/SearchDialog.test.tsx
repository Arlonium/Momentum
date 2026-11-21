// La fenêtre de recherche (lot 4, Task C1). Champ vide : les pages récentes. Dès la frappe : les
// pages de l'arbre dont le titre contient ce qui est tapé, tout de suite et sans réseau, puis les
// pages que le serveur trouve, ajoutées dessous. Une page n'a qu'une ligne ; une ligne montrée ne
// bouge plus ; la ligne active est tenue par l'identifiant de sa page. Tout se fait au clavier, le
// champ garde le focus, et le focus ne tombe jamais sur `body`. Un extrait est du texte, jamais du
// HTML.
//
// Le serveur est `searchSpace`, que le test fait répondre quand il veut ; l'anti-rebond passe sous
// des minuteurs simulés : aucun test n'attend une durée.
import type { Me, PageNode, SearchHit, SearchRequest, SearchResults } from "@mementum/protocol";
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError } from "../api";
import { SessionContext } from "../session/session";
import { publishPages } from "../space/pageDirectory";
import { recentsKey } from "../space/recents";
import { deferred, settle, type Deferred } from "../test/hooks";
import { page, space, unauthenticated } from "../test/pages";
import { Message } from "../ui/Message";
import { SearchDialog } from "./SearchDialog";
import { SEARCH_DEBOUNCE_MS } from "./useSearch";

type Asked = { spaceId: string; body: SearchRequest; answer: Deferred<SearchResults> };
const asked: Asked[] = [];

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  searchSpace: (spaceId: string, body: SearchRequest) => {
    const answer = deferred<SearchResults>();
    asked.push({ spaceId, body, answer });
    return answer.promise;
  },
}));

let host: HTMLDivElement;
let root: Root;
const onClose = vi.fn();

const alice = (): Me => ({
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("space")],
});

/** L'arbre de l'espace : « Chats » sous « Animaux », et d'autres pages. */
const NODES: PageNode[] = [
  page("animaux", { title: "Animaux" }),
  page("chats", { title: "Chats", parentId: "animaux" }),
  page("chatons", { title: "Les chatons", parentId: "chats" }),
  page("chiens", { title: "Chiens", parentId: "animaux" }),
  page("recettes", { title: "Recettes" }),
];

beforeEach(() => {
  asked.length = 0;
  onClose.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  publishPages("space", NODES);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
});

afterEach(async () => {
  vi.useRealTimers();
  await act(async () => root.unmount());
  document.body.replaceChildren();
  publishPages(null, null);
  localStorage.clear();
  history.replaceState(null, "", "/");
  vi.restoreAllMocks();
});

/** Ce qui rend la fenêtre : il tient ce qui est tapé, comme `App`. */
function Harness({ me, nodes, start }: { me: Me; nodes: PageNode[] | null; start: string }) {
  const [query, setQuery] = useState(start);
  return (
    <SessionContext.Provider value={{ me, refresh: async () => {} }}>
      <SearchDialog space={space("space")} nodes={nodes} query={query} onQuery={setQuery} onClose={onClose} />
    </SessionContext.Provider>
  );
}

async function show({
  me = alice(),
  nodes = NODES,
  start = "",
}: { me?: Me; nodes?: PageNode[] | null; start?: string } = {}) {
  await act(async () => root.render(<Harness me={me} nodes={nodes} start={start} />));
}

const dialog = () => host.querySelector<HTMLDialogElement>("dialog.mm-search")!;
const field = () => dialog().querySelector<HTMLInputElement>('input[role="combobox"]')!;
const rows = () => [...dialog().querySelectorAll<HTMLAnchorElement>('a[role="option"]')];
const shown = () => rows().map((row) => row.dataset.searchPage);
const active = () => rows().find((row) => row.getAttribute("aria-selected") === "true")?.dataset.searchPage;
const row = (id: string) => rows().find((r) => r.dataset.searchPage === id)!;
const said = () => [...dialog().querySelectorAll('[role="status"]')].map((s) => s.textContent).join(" | ");
const alerts = () => [...dialog().querySelectorAll('[role="alert"]')].map((s) => s.textContent);
const more = () =>
  [...dialog().querySelectorAll("button")].find((b) => b.textContent === "Afficher plus de résultats");

/** Tape `text` dans le champ : sa valeur entière, comme après une frappe. */
async function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(field(), text);
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function press(key: string, init: KeyboardEventInit = {}, on: Element = field()) {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  await act(async () => {
    on.dispatchEvent(event);
  });
  return event;
}

/** Le temps de l'anti-rebond passe : la requête part. */
const debounced = () =>
  act(async () => {
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);
  });

const hit = (pageId: string, overrides: Partial<SearchHit> = {}): SearchHit => ({
  pageId,
  title: `Page ${pageId}`,
  path: [],
  blockId: `bloc-${pageId}`,
  excerpt: `le chat de ${pageId}`,
  marks: [[3, 7]],
  ...overrides,
});

const found = (hits: SearchHit[], overrides: Partial<SearchResults> = {}): SearchResults => ({
  results: hits,
  more: false,
  indexing: false,
  ...overrides,
});

const answer = (results: SearchResults, at = asked.length - 1) => settle(() => asked[at].answer.resolve(results));

test("the_window_opens_with_the_focus_in_its_field", async () => {
  await show();
  expect(dialog().open).toBe(true);
  expect(dialog().getAttribute("aria-label")).toBe("Rechercher");
  expect(document.activeElement).toBe(field());
  expect(field().getAttribute("aria-label")).toBe("Rechercher dans l'espace");
});

test("an_empty_field_shows_the_recent_pages_and_asks_nothing", async () => {
  // La plus récente d'abord ; « partie » n'est plus dans l'arbre.
  localStorage.setItem(recentsKey("alice", "space"), JSON.stringify(["chiens", "partie", "recettes"]));
  await show();
  expect(shown()).toEqual(["chiens", "recettes"]);
  expect(dialog().querySelector(".mm-search-heading")?.textContent).toBe("Récents");
  expect(active()).toBe("chiens");
  // Des espaces seules ne sont pas une recherche.
  await type("   ");
  expect(shown()).toEqual(["chiens", "recettes"]);
  await debounced();
  await debounced();
  expect(asked).toEqual([]);
  // Rien n'est dit : ni recherche en cours, ni « aucun résultat ».
  expect(said()).toBe("");
});

test("without_recent_page_an_empty_field_shows_no_list", async () => {
  await show();
  expect(rows()).toEqual([]);
  expect(dialog().querySelector(".mm-search-heading")).toBeNull();
  expect(said()).toBe("");
  expect(field().getAttribute("aria-expanded")).toBe("false");
});

test("titles_of_the_tree_come_first_and_at_once", async () => {
  await show();
  await type("cha");
  // Tout de suite, sans réseau : les titres qui commencent par ce qui est tapé, puis ceux qui le
  // contiennent, sans accents ni casse.
  expect(shown()).toEqual(["chats", "chatons"]);
  expect(asked).toEqual([]);
  expect(rows().map((r) => r.dataset.from)).toEqual(["tree", "tree"]);
  expect(row("chats").querySelector(".mm-search-title")?.textContent).toBe("Chats");
  // Le fil d'Ariane vient de l'arbre, de la plus haute à la page parente.
  expect([...row("chatons").querySelectorAll(".mm-search-ancestor")].map((a) => a.textContent)).toEqual([
    "Animaux",
    "Chats",
  ]);
  expect(said()).toBe("Recherche…");
  // Puis le serveur, sous elles, dans son ordre.
  await debounced();
  expect(asked.map((a) => [a.spaceId, a.body])).toEqual([["space", { q: "cha" }]]);
  await answer(found([hit("x"), hit("y", { path: [{ id: "animaux", title: "Animaux" }] })]));
  expect(shown()).toEqual(["chats", "chatons", "x", "y"]);
  expect(rows().map((r) => r.dataset.from)).toEqual(["tree", "tree", "server", "server"]);
  expect([...row("y").querySelectorAll(".mm-search-ancestor")].map((a) => a.textContent)).toEqual(["Animaux"]);
  expect(said()).toBe("");
});

test("a_page_found_twice_has_one_line", async () => {
  await show();
  await type("chats");
  expect(shown()).toEqual(["chats"]);
  await debounced();
  // Le serveur la trouve aussi, par un bloc ; et rend deux fois une autre page.
  await answer(found([hit("x"), hit("chats", { title: "Chats" }), hit("x")]));
  expect(shown()).toEqual(["chats", "x"]);
  // Sa ligne reste celle de l'arbre, telle qu'elle était : sans extrait.
  expect(row("chats").dataset.from).toBe("tree");
  expect(row("chats").querySelector(".mm-search-excerpt")).toBeNull();
});

test("the_active_line_is_held_by_its_id_when_results_arrive", async () => {
  await show();
  await type("cha");
  expect(active()).toBe("chats");
  await press("ArrowDown");
  expect(active()).toBe("chatons");
  expect(field().getAttribute("aria-activedescendant")).toBe(row("chatons").id);
  await debounced();
  await answer(found([hit("x"), hit("y")], { more: true }));
  // Les résultats arrivés, puis leur suite : la même page, à la même ligne.
  expect(active()).toBe("chatons");
  await act(async () => more()!.click());
  await answer(found([hit("z")]));
  expect(active()).toBe("chatons");
  expect(field().getAttribute("aria-activedescendant")).toBe(row("chatons").id);
  // Entrée ouvre cette page-là.
  await press("Enter");
  expect(location.pathname).toBe("/p/chatons");
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("without_tree_line_the_first_result_becomes_the_active_line_and_stays_it", async () => {
  await show();
  await type("zèbre");
  expect(rows()).toEqual([]);
  // Entrée sans aucune ligne : rien.
  await press("Enter");
  expect(onClose).not.toHaveBeenCalled();
  await debounced();
  await answer(found([hit("x"), hit("y")], { more: true }));
  expect(active()).toBe("x");
  await press("ArrowDown");
  await act(async () => more()!.click());
  await answer(found([hit("z")]));
  expect(active()).toBe("y");
  // Une autre recherche : la première ligne de la liste neuve.
  await type("cha");
  expect(active()).toBe("chats");
});

test("the_arrows_stop_at_the_ends_and_home_and_end_are_left_to_the_field", async () => {
  await show();
  await type("ch");
  expect(shown()).toEqual(["chats", "chiens", "chatons"]);
  expect((await press("ArrowUp")).defaultPrevented).toBe(true);
  expect(active()).toBe("chats");
  await press("ArrowDown");
  await press("ArrowDown");
  await press("ArrowDown");
  expect(active()).toBe("chatons");
  // Le curseur du champ garde ses touches.
  expect((await press("Home")).defaultPrevented).toBe(false);
  expect((await press("End")).defaultPrevented).toBe(false);
  expect(active()).toBe("chatons");
});

test("nothing_moves_under_the_pointer_when_results_arrive", async () => {
  await show();
  await type("cha");
  const before = rows().map((r) => r.outerHTML);
  await debounced();
  await answer(found([hit("x"), hit("chatons"), hit("y")], { more: true, indexing: true }));
  // Les lignes déjà montrées : au même rang, et telles quelles (aucune ne gagne un extrait, qui
  // la ferait grandir et pousserait les suivantes).
  expect(rows().slice(0, 2).map((r) => r.outerHTML)).toEqual(before);
  const withFirst = rows().map((r) => r.outerHTML);
  await act(async () => more()!.click());
  await answer(found([hit("z")]));
  expect(rows().slice(0, 4).map((r) => r.outerHTML)).toEqual(withFirst);
  expect(shown()).toEqual(["chats", "chatons", "x", "y", "z"]);
  // L'arbre relu pendant que la fenêtre est ouverte (une page renommée ailleurs, une page arrivée
  // qui passerait en tête) : la liste montrée ne change pas.
  const now = rows().map((r) => r.outerHTML);
  await act(async () =>
    publishPages("space", [
      page("chameau", { title: "Chameau" }),
      ...NODES.map((n) => (n.id === "chats" ? { ...n, title: "Félins" } : n)),
    ]),
  );
  expect(rows().map((r) => r.outerHTML)).toEqual(now);
  // Rien n'est jamais dit au-dessus de la liste : ce qui se dit ne la pousse pas.
  const list = dialog().querySelector('[role="listbox"]')!;
  for (const status of dialog().querySelectorAll('[role="status"], [role="alert"]')) {
    expect(list.compareDocumentPosition(status) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  }
});

test("the_recent_pages_shown_do_not_follow_the_tree_while_the_window_is_open", async () => {
  localStorage.setItem(recentsKey("alice", "space"), JSON.stringify(["chiens", "recettes"]));
  await show();
  expect(shown()).toEqual(["chiens", "recettes"]);
  const before = rows().map((r) => r.outerHTML);
  // Une autre page ouverte dans un autre onglet passe en tête des récents ; l'arbre est relu.
  localStorage.setItem(recentsKey("alice", "space"), JSON.stringify(["chats", "chiens", "recettes"]));
  await act(async () => window.dispatchEvent(new StorageEvent("storage", { key: recentsKey("alice", "space") })));
  await act(async () => root.render(<Harness me={alice()} nodes={NODES.filter((n) => n.id !== "chiens")} start="" />));
  expect(rows().map((r) => r.outerHTML)).toEqual(before);
});

test("marks_wrap_text_nodes_only", async () => {
  await show();
  await type("img");
  await debounced();
  const hostile = '<img src=x onerror="alert(1)"> et <b>gras</b> 😀 fin';
  const pair = hostile.indexOf("😀");
  await answer(
    found([
      // Une plage juste, une qui coupe « 😀 » en deux, une hors du texte.
      hit("x", {
        title: "<script>alert(2)</script>",
        excerpt: hostile,
        marks: [[1, 4], [pair + 1, pair + 3], [pair + 3, 400]],
      }),
      hit("y", { excerpt: "le chat dort", marks: [[3, 7]] }),
      // Trouvée par son titre : pas d'extrait.
      hit("z", { excerpt: "", marks: [], blockId: null }),
    ]),
  );
  const excerpt = row("x").querySelector(".mm-search-excerpt")!;
  // Du texte, tel quel : aucun élément n'est né de l'extrait ni du titre.
  expect(excerpt.textContent).toBe(hostile);
  expect(row("x").querySelector(".mm-search-title")!.textContent).toBe("<script>alert(2)</script>");
  expect(dialog().querySelector("img, b, script")).toBeNull();
  // Un `<mark>` par plage qui tient, autour d'un nœud de texte, et de lui seul.
  const marks = [...excerpt.querySelectorAll("mark")];
  expect(marks.map((m) => m.textContent)).toEqual(["img"]);
  for (const node of excerpt.childNodes) {
    if (node instanceof Element) {
      expect(node.tagName).toBe("MARK");
      expect([...node.childNodes].every((child) => child.nodeType === Node.TEXT_NODE)).toBe(true);
      expect(node.getAttribute("style")).toBeNull();
    } else expect(node.nodeType).toBe(Node.TEXT_NODE);
  }
  expect([...row("y").querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["chat"]);
  expect(row("z").querySelector(".mm-search-excerpt")).toBeNull();
  // Du texte venu d'ailleurs : isolé, dans le sens de son écriture.
  expect(excerpt.getAttribute("dir")).toBe("auto");
  expect(row("x").querySelector(".mm-search-title")!.getAttribute("dir")).toBe("auto");
  expect(dialog().innerHTML).not.toContain("style=");
});

test("each_line_is_a_real_link_to_its_page", async () => {
  await show();
  await type("cha");
  expect(rows().map((r) => r.getAttribute("href"))).toEqual(["/p/chats", "/p/chatons"]);
  // Le champ garde le focus : les lignes ne sont pas des arrêts de Tab.
  expect(rows().every((r) => r.tabIndex === -1)).toBe(true);
  // Un clic simple ouvre la page dans l'onglet, et ferme la fenêtre.
  const click = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
  await act(async () => {
    row("chatons").dispatchEvent(click);
  });
  expect(click.defaultPrevented).toBe(true);
  expect(location.pathname).toBe("/p/chatons");
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a_line_found_by_a_block_is_a_link_to_that_block", async () => {
  // Task C2 : l'identifiant d'un bloc est un texte venu d'ailleurs, échappé dans le fragment ; il
  // n'entre ni dans le chemin ni dans du HTML.
  const odd = 'a"b <img onerror=x>#b=é/?&';
  const opened = vi.spyOn(window, "open").mockReturnValue(null);
  await show();
  await type("cha");
  await debounced();
  await settle(() =>
    asked[0].answer.resolve(found([hit("ailleurs", { blockId: odd }), hit("titre", { blockId: null }), hit("chats")])),
  );
  const href = `/p/ailleurs#b=${encodeURIComponent(odd)}`;
  expect(row("ailleurs").getAttribute("href")).toBe(href);
  expect(dialog().querySelector("img")).toBeNull();
  // Une page trouvée par son titre, et une ligne venue de l'arbre (R1080) : la page, sans bloc.
  expect(row("titre").getAttribute("href")).toBe("/p/titre");
  expect(row("chats").getAttribute("href")).toBe("/p/chats");
  // Ctrl+Entrée ouvre la même adresse ; Entrée y mène dans l'onglet, bloc compris.
  await press("ArrowDown");
  await press("ArrowDown");
  expect(active()).toBe("ailleurs");
  await press("Enter", { ctrlKey: true });
  expect(opened).toHaveBeenCalledWith(href, "_blank", "noopener");
  await press("Enter");
  expect(location.pathname + location.hash).toBe(href);
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a_click_with_a_modifier_is_left_to_the_browser_and_the_window_stays", async () => {
  await show();
  await type("cha");
  for (const init of [{ ctrlKey: true }, { metaKey: true }, { shiftKey: true }, { button: 1 }]) {
    const click = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ...init });
    await act(async () => {
      row("chats").dispatchEvent(click);
    });
    expect(click.defaultPrevented).toBe(false);
  }
  expect(location.pathname).toBe("/");
  expect(onClose).not.toHaveBeenCalled();
  // Le focus revient au champ : on continue de taper.
  expect(document.activeElement).toBe(field());
});

test("ctrl_enter_opens_the_active_page_in_a_new_tab_and_the_window_stays", async () => {
  const opened = vi.spyOn(window, "open").mockReturnValue(null);
  await show();
  await type("cha");
  await press("ArrowDown");
  expect((await press("Enter", { ctrlKey: true })).defaultPrevented).toBe(true);
  expect(opened).toHaveBeenCalledWith("/p/chatons", "_blank", "noopener");
  await press("Enter", { metaKey: true });
  expect(opened).toHaveBeenCalledTimes(2);
  expect(location.pathname).toBe("/");
  expect(onClose).not.toHaveBeenCalled();
  // Entrée qui valide une composition (IME) appartient au champ.
  await press("Enter", { isComposing: true });
  expect(location.pathname).toBe("/");
  expect(onClose).not.toHaveBeenCalled();
});

test("escape_closes_and_gives_the_focus_back", async () => {
  const opener = document.createElement("button");
  const entry = document.createElement("button");
  entry.setAttribute("data-search-entry", "");
  document.body.append(opener, entry);
  opener.focus();
  await show();
  expect(document.activeElement).toBe(field());
  await type("un texte tapé");
  // Échap : le navigateur envoie `cancel` à la fenêtre.
  const cancel = new Event("cancel", { cancelable: true });
  await act(async () => {
    dialog().dispatchEvent(cancel);
  });
  expect(onClose).toHaveBeenCalledTimes(1);
  // C'est qui la rend qui la retire : le focus revient d'où il venait, jamais sur `body`.
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(opener);
  // Ce qui avait le focus est parti entre-temps (la page a changé) : l'entrée de la barre latérale.
  opener.focus();
  await show();
  opener.remove();
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(entry);
});

test("the_selection_of_the_editor_is_given_back_with_the_focus", async () => {
  // Ouverte depuis le texte d'une page : le curseur y était au milieu d'un mot.
  const editor = document.createElement("div");
  editor.setAttribute("contenteditable", "true");
  editor.tabIndex = 0;
  const text = document.createTextNode("le chat dort");
  editor.append(text);
  document.body.append(editor);
  editor.focus();
  const selection = document.getSelection()!;
  selection.setBaseAndExtent(text, 5, text, 5);
  await show();
  // Le focus passé au champ, le navigateur retire la sélection du texte ; rendu à l'éditeur par
  // `focus()`, il la poserait à son début.
  selection.removeAllRanges();
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(editor);
  expect([selection.anchorNode, selection.anchorOffset, selection.isCollapsed]).toEqual([text, 5, true]);
  // Une sélection étendue de même, et elle suit ce qu'un autre écrit avant elle pendant ce temps.
  selection.setBaseAndExtent(text, 3, text, 7);
  await show();
  selection.removeAllRanges();
  text.insertData(0, "et ");
  await act(async () => root.render(null));
  expect(String(selection)).toBe("chat");
  // Le texte parti entre-temps : le focus revient, sans sélection forcée dans ce qui n'est plus là.
  selection.setBaseAndExtent(text, 3, text, 7);
  await show();
  selection.removeAllRanges();
  text.remove();
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(editor);
});

test("the_focus_goes_to_the_screen_that_replaced_the_page_before_the_sidebar_entry", async () => {
  // Recette E5 : la page sous la fenêtre a été remplacée par un écran qui dit pourquoi (un
  // `Message`), et ce qui avait le focus est parti avec elle. Son titre prend le focus, pas l'entrée
  // de la barre latérale ; sans entrée (l'espace parti avec l'accès), jamais `body`.
  const opener = document.createElement("button");
  const entry = document.createElement("button");
  entry.setAttribute("data-search-entry", "");
  document.body.append(opener, entry);
  opener.focus();
  await show();
  opener.remove();
  const screen = document.createElement("div");
  document.body.append(screen);
  const screenRoot = createRoot(screen);
  await act(async () => screenRoot.render(<Message title="Tu n'as plus accès à cette page" />));
  entry.remove();
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(screen.querySelector("h1"));
  // Un écran de passage (« Chargement… ») ne le prend pas : l'entrée, s'il y en a une.
  document.body.append(entry);
  await act(async () => screenRoot.render(<Message title="Chargement…" transient />));
  document.body.append(opener);
  opener.focus();
  await show();
  opener.remove();
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(entry);
  await act(async () => screenRoot.unmount());
});

test("opened_with_nothing_focused_the_focus_goes_back_to_the_sidebar_entry", async () => {
  const entry = document.createElement("button");
  entry.setAttribute("data-search-entry", "");
  document.body.append(entry);
  expect(document.activeElement).toBe(document.body);
  await show();
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(entry);
});

test("escape_in_the_window_closes_nothing_under_it", async () => {
  await show();
  // `cancel` et `close` d'une autre fenêtre remontent dans l'arbre de React : seuls ceux de la
  // fenêtre de recherche la concernent.
  const inner = document.createElement("dialog");
  dialog().append(inner);
  for (const name of ["cancel", "close"]) {
    await act(async () => {
      inner.dispatchEvent(new Event(name, { bubbles: true, cancelable: true }));
    });
  }
  expect(onClose).not.toHaveBeenCalled();
  // Témoin : les siens la ferment (Chrome la ferme d'elle-même au second Échap : `close`).
  await act(async () => {
    dialog().dispatchEvent(new Event("close"));
  });
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("what_was_typed_is_given_back_selected_when_the_window_opens_again", async () => {
  await show({ start: "chats" });
  expect(field().value).toBe("chats");
  expect([field().selectionStart, field().selectionEnd]).toEqual([0, 5]);
  // Et la recherche repart d'elle-même.
  expect(shown()).toEqual(["chats"]);
  await debounced();
  expect(asked.map((a) => a.body)).toEqual([{ q: "chats" }]);
});

/**
 * Un appui puis un clic de la souris, chacun sur son élément. jsdom donne au cadre de la fenêtre une
 * boîte nulle en (0, 0) : (500, 500) est sur le voile, (0, 0) dans le cadre.
 */
async function mouse(down: Element, up: Element, at: number, detail = 1) {
  const init = { bubbles: true, cancelable: true, clientX: at, clientY: at, detail };
  await act(async () => {
    down.dispatchEvent(new MouseEvent("mousedown", init));
    up.dispatchEvent(new MouseEvent("click", init));
  });
}

test("a_click_on_the_backdrop_closes_and_a_click_in_the_window_does_not", async () => {
  await show();
  await mouse(dialog(), dialog(), 0);
  expect(onClose).not.toHaveBeenCalled();
  await mouse(field(), field(), 500);
  expect(onClose).not.toHaveBeenCalled();
  await mouse(dialog(), dialog(), 500);
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a_selection_started_in_the_field_and_released_on_the_backdrop_does_not_close", async () => {
  await show({ start: "un texte tapé" });
  // On sélectionne le texte du champ en glissant, et la souris est relâchée hors du cadre : le
  // navigateur envoie le clic à la fenêtre elle-même.
  await mouse(field(), dialog(), 500);
  expect(onClose).not.toHaveBeenCalled();
  // Un appui dans le cadre, hors de tout élément, relâché sur le voile : de même.
  await act(async () => {
    dialog().dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientX: 0, clientY: 0 }));
    dialog().dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: 500, clientY: 500 }));
  });
  expect(onClose).not.toHaveBeenCalled();
  // L'appui d'avant ne vaut pas pour le clic d'après.
  await mouse(dialog(), dialog(), 500);
  await mouse(field(), dialog(), 500);
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("the_second_click_of_a_double_click_on_the_backdrop_does_not_close", async () => {
  // Un double clic sur « Rechercher » : le premier ouvre la fenêtre, le second tombe sur son voile.
  await show();
  await mouse(dialog(), dialog(), 500, 2);
  await mouse(dialog(), dialog(), 500, 3);
  expect(onClose).not.toHaveBeenCalled();
  await mouse(dialog(), dialog(), 500, 1);
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("the_second_click_of_a_double_click_opens_no_line", async () => {
  // Un double clic sur « Afficher plus de résultats » : le second clic tombe sur une ligne arrivée
  // sous le pointeur. Il n'ouvre rien, et le navigateur ne suit pas le lien non plus.
  await show();
  await type("cha");
  for (const detail of [2, 3]) {
    const click = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, detail });
    await act(async () => {
      row("chatons").dispatchEvent(click);
    });
    expect(click.defaultPrevented).toBe(true);
  }
  expect(location.pathname).toBe("/");
  expect(onClose).not.toHaveBeenCalled();
  expect(field().value).toBe("cha");
});

test("the_field_takes_no_more_than_the_route_does", async () => {
  await show();
  expect(field().maxLength).toBe(200);
});

test("opened_after_its_shell_the_window_takes_what_its_host_noted", async () => {
  // L'hôte a relevé, à l'ouverture, ce qui avait le focus ; la coquille a tenu le champ le temps que
  // le code arrive, et dit où était son curseur.
  const opener = document.createElement("button");
  document.body.append(opener);
  const elsewhere = document.createElement("button");
  document.body.append(elsewhere);
  elsewhere.focus();
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me: alice(), refresh: async () => {} }}>
        <SearchDialog
          space={space("space")}
          nodes={NODES}
          query="chats"
          onQuery={() => {}}
          onClose={onClose}
          before={{ opener, range: null }}
          caret={{ current: { start: 2, end: 2 } }}
        />
      </SessionContext.Provider>,
    ),
  );
  // Le curseur là où il était, pas tout le texte sélectionné.
  expect(document.activeElement).toBe(field());
  expect([field().selectionStart, field().selectionEnd]).toEqual([2, 2]);
  // Fermée, elle rend le focus à ce que l'hôte a relevé, pas à ce qui l'avait à son montage.
  await act(async () => root.render(null));
  expect(document.activeElement).toBe(opener);
});

test("the_states_are_said_under_the_list", async () => {
  await show();
  await type("zèbre");
  // Tant que le serveur n'a pas répondu, rien ne dit « aucun résultat ».
  expect(said()).toBe("Recherche…");
  await debounced();
  expect(said()).toBe("Recherche…");
  await answer(found([]));
  expect(said()).toBe("Aucun résultat.");
  // L'index en retard : une réponse vide n'est pas sûre, et la fenêtre le dit (recette B10).
  await type("zèbres");
  await debounced();
  await answer(found([], { indexing: true }));
  expect(said()).toBe("Aucun résultat pour l'instant. | L'indexation est en cours : des pages peuvent manquer.");
  // Avec des résultats aussi.
  await type("zèbre rayé");
  await debounced();
  await answer(found([hit("x")], { indexing: true }));
  expect(said()).toBe("L'indexation est en cours : des pages peuvent manquer.");
  // Des pages de l'arbre trouvées, rien de plus du serveur : pas d'« aucun résultat ».
  await type("cha");
  await debounced();
  await answer(found([]));
  expect(shown()).toEqual(["chats", "chatons"]);
  expect(said()).toBe("");
  expect(alerts()).toEqual([]);
});

test("a_refusal_of_the_server_is_said_as_it_says_it", async () => {
  await show();
  await type("cha");
  await debounced();
  await settle(() =>
    asked[0].answer.reject(new ApiError(422, "invalid_query", "Ta recherche compte plus de 8 mots : retires-en.")),
  );
  expect(alerts()).toEqual(["Ta recherche compte plus de 8 mots : retires-en."]);
  // Les pages de l'arbre restent, et rien ne dit « aucun résultat ».
  expect(shown()).toEqual(["chats", "chatons"]);
  expect(said()).toBe("");
  // La frappe suivante efface le refus.
  await type("chat");
  expect(alerts()).toEqual([]);
});

test("search_busy_is_not_shown_for_a_replaced_query", async () => {
  await show();
  await type("zèb");
  await debounced();
  await type("zèbre");
  await debounced();
  await settle(() =>
    asked[0].answer.reject(new ApiError(429, "search_busy", "une recherche plus récente a pris la place de celle-ci")),
  );
  expect(alerts()).toEqual([]);
  expect(said()).toBe("Recherche…");
  await answer(found([hit("x")]), 1);
  expect(shown()).toEqual(["x"]);
  expect(alerts()).toEqual([]);
});

test("a_late_answer_of_an_older_query_is_dropped", async () => {
  await show();
  await type("zèb");
  await debounced();
  await type("zèbre");
  // La réponse de « zèb » arrive pendant l'anti-rebond de « zèbre », puis après son départ.
  await answer(found([hit("vieux")]), 0);
  expect(rows()).toEqual([]);
  await debounced();
  expect(rows()).toEqual([]);
  expect(said()).toBe("Recherche…");
  await answer(found([hit("neuf")]), 1);
  expect(shown()).toEqual(["neuf"]);
});

test("a_lost_session_asks_to_log_in_and_searches_again", async () => {
  await show();
  await type("zèbre");
  await debounced();
  // `api.ts` demande la connexion par-dessus la fenêtre (`onSessionLost`, prouvé dans
  // `api.test.ts`) ; ici, rien n'est dit comme une erreur, et la recherche attend.
  await settle(() => asked[0].answer.reject(unauthenticated()));
  expect(alerts()).toEqual([]);
  expect(said()).toBe("Recherche…");
  expect(field().value).toBe("zèbre");
  await debounced();
  expect(asked).toHaveLength(1);
  // Le compte revient (`me` relu) : la même recherche repart d'elle-même.
  await act(async () => root.render(<Harness me={alice()} nodes={NODES} start="" />));
  expect(asked.map((a) => a.body)).toEqual([{ q: "zèbre" }, { q: "zèbre" }]);
  await answer(found([hit("x")]));
  expect(shown()).toEqual(["x"]);
  expect(field().value).toBe("zèbre");
});

test("more_results_are_asked_by_a_button_that_never_leaves_the_focus_on_body", async () => {
  await show();
  await type("zèbre");
  await debounced();
  await answer(found([hit("x")]));
  expect(more()).toBeUndefined();
  await type("zèbres");
  await debounced();
  await answer(found([hit("x")], { more: true }));
  const button = more()!;
  button.focus();
  await act(async () => button.click());
  // En route : grisé sans être retiré, il garde le focus.
  expect(more()!.getAttribute("aria-disabled")).toBe("true");
  expect(document.activeElement).toBe(more());
  expect(asked.at(-1)!.body).toEqual({ q: "zèbres", after: 20 });
  // La dernière réponse le retire : le focus revient au champ. À sa place, dans le même pied, une
  // phrase qu'aucun clic ne prend : rien de cliquable ne vient sous le pointeur (recette B9).
  await answer(found([hit("y")]));
  expect(more()).toBeUndefined();
  expect(document.activeElement).toBe(field());
  expect(shown()).toEqual(["x", "y"]);
  const foot = dialog().querySelector(".mm-search-foot")!;
  expect(foot.textContent).toBe("Tous les résultats sont affichés.");
  expect(foot.querySelectorAll("button, a, [tabindex]")).toHaveLength(0);
  // Une autre recherche : le pied part avec ses résultats ; sans suite, il ne vient pas.
  await type("zèbre");
  expect(dialog().querySelector(".mm-search-foot")).toBeNull();
  await debounced();
  await answer(found([hit("x")]));
  expect(dialog().querySelector(".mm-search-foot")).toBeNull();
});

/** Le texte du pied de la fenêtre, ou `null` sans pied. */
const footText = () => dialog().querySelector(".mm-search-foot")?.textContent ?? null;

test("at_the_cap_the_foot_says_to_refine_and_never_that_everything_is_shown", async () => {
  // Cinq réponses de vingt pages, le serveur disant toujours qu'il en a d'autres : à la centième page,
  // la fenêtre ne demande plus rien (R911), et le pied ne dit pas que tout est là.
  await show();
  await type("zèbre");
  await debounced();
  const page = (n: number) => Array.from({ length: 20 }, (_, i) => hit(`p${n * 20 + i}`));
  await answer(found(page(0), { more: true }));
  for (let n = 1; n < 5; n += 1) {
    await act(async () => more()!.click());
    expect(asked.at(-1)!.body).toEqual({ q: "zèbre", after: n * 20 });
    await answer(found(page(n), { more: true }));
  }
  expect(shown()).toHaveLength(100);
  expect(more()).toBeUndefined();
  expect(footText()).toBe("Les 100 premières pages sont affichées : précise ta recherche.");
  expect(dialog().querySelectorAll(".mm-search-foot button, .mm-search-foot a")).toHaveLength(0);
});

test("with_the_index_behind_the_foot_says_everything_found_for_now", async () => {
  // L'index en retard : la dernière réponse ne garantit pas que tout est là (la phrase de
  // l'indexation le dit sous la liste) ; le pied ne le contredit pas.
  await show();
  await type("zèbre");
  await debounced();
  await answer(found([hit("x")], { more: true, indexing: true }));
  await act(async () => more()!.click());
  await answer(found([hit("y")], { indexing: true }));
  expect(footText()).toBe("Tous les résultats trouvés pour l'instant sont affichés.");
  expect(said()).toBe("L'indexation est en cours : des pages peuvent manquer.");
});

test("ctrl_k_in_the_window_is_taken_and_changes_nothing", async () => {
  await show({ start: "cha" });
  const again = await press("k", { ctrlKey: true });
  // Le navigateur en ferait autre chose (sa barre de recherche).
  expect(again.defaultPrevented).toBe(true);
  expect(onClose).not.toHaveBeenCalled();
  expect(field().value).toBe("cha");
  // Témoin : « k » seul se tape.
  expect((await press("k")).defaultPrevented).toBe(false);
});

test("the_list_is_a_listbox_driven_from_the_field", async () => {
  await show();
  await type("cha");
  const list = dialog().querySelector('[role="listbox"]')!;
  expect(field().getAttribute("aria-controls")).toBe(list.id);
  expect(field().getAttribute("aria-expanded")).toBe("true");
  expect(field().getAttribute("aria-autocomplete")).toBe("list");
  expect(rows().map((r) => r.getAttribute("aria-selected"))).toEqual(["true", "false"]);
  // Des identifiants à elles, jamais `data-page-id`, réservé aux lignes de l'arbre.
  expect(dialog().querySelector("[data-page-id]")).toBeNull();
  expect(new Set(rows().map((r) => r.id)).size).toBe(2);
});
