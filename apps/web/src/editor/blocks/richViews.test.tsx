// Les vues des blocs riches dessinées sans React (le callout, le signet, le lien de page ; R625 à R627,
// R653 à R657, `richView.ts`) : ce qui les rendait lentes à l'ouverture d'une page ne revient pas, et
// ce que faisait la vue React tient toujours (le focus d'un bouton gardé quand une prop change
// ailleurs, une prop qui n'est pas du texte, le curseur gardé, les îlots non modifiables, la sélection
// d'un bloc, rien d'écouté par une vue qui n'est plus affichée, la copie, les icônes de l'interface).
import { ReadOnlyExtension } from "@blocknote/core/extensions";
import { NodeSelection } from "prosemirror-state";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { BookmarkIcon, InfoIcon, PageIcon } from "../../icons";
import { publishPages, watchersForTests } from "../../space/pageDirectory";
import { REMOTE, seedState } from "../../test/editors";
import { page } from "../../test/pages";
import { blockOf, flush, foreignStyles, mountEditor, type Mounted } from "../../test/richEditor";
import { FRAGMENT } from "../collab";
import { BOOKMARK_ICON, iconElement, INFO_ICON, PAGE_ICON, stopEventsOutside } from "./richView";

afterEach(() => publishPages(null, null));

/** Ce que tiptap ou Mementum lisent de la sélection du document. */
const GETTERS = ["anchorNode", "anchorOffset", "focusNode", "focusOffset", "rangeCount", "type", "isCollapsed"] as const;

/** Combien de fois la sélection du document est lue pendant `run`. */
async function selectionReads(run: () => Promise<unknown>): Promise<number> {
  let reads = 0;
  const spies = GETTERS.map((name) => {
    const read = Object.getOwnPropertyDescriptor(Selection.prototype, name)!.get!;
    return vi.spyOn(Selection.prototype, name, "get").mockImplementation(function (this: Selection) {
      reads += 1;
      return read.call(this);
    });
  });
  try {
    await run();
  } finally {
    for (const spy of spies) spy.mockRestore();
  }
  return reads;
}

/** Un paragraphe, où est la sélection de l'éditeur, puis `count` callouts. */
function callouts(count: number) {
  return [
    { id: "p", type: "paragraph" as const, content: "Avant" },
    ...Array.from({ length: count }, (_, i) => ({
      id: `c${i}`,
      type: "callout" as const,
      props: { icon: "⚠️" },
      content: `Callout ${i}`,
    })),
  ];
}

test("callouts_are_mounted_without_reading_the_document_selection", async () => {
  // Chromium refait la mise en page de toute la page pour dire où est la sélection : la vue React d'un
  // callout la lisait avant d'y mettre son texte, une mise en page par callout à l'ouverture (R625).
  const one = await selectionReads(() => mountEditor({ blocks: callouts(1) }));
  let m: Mounted | null = null;
  const many = await selectionReads(async () => {
    m = await mountEditor({ blocks: callouts(40) });
  });
  // Les lectures ne suivent pas le nombre de callouts (avant : une de plus par callout).
  expect(many - one, `${one} lectures pour un callout, ${many} pour quarante`).toBeLessThan(5);
  // Chaque texte est à sa place, dans le contenu de son callout.
  for (let i = 0; i < 40; i += 1) {
    expect(blockOf(m!, `c${i}`).querySelector(".mm-callout-content")?.textContent).toBe(`Callout ${i}`);
  }
});

test("the_caret_stays_in_a_callout_drawn_anew_under_it", async () => {
  const m = await mountEditor({
    blocks: [
      { id: "x", type: "paragraph", content: "Avant" },
      { id: "c", type: "paragraph", content: "Attention" },
      { id: "z", type: "paragraph", content: "Après" },
    ],
  });
  m.editor.focus();
  m.editor.setTextCursorPosition("c", "end");
  await flush();
  // Le bloc où est le curseur devient un callout : sa vue est neuve, sous le curseur.
  m.editor.updateBlock("c", { type: "callout" });
  await flush();
  const content = blockOf(m, "c").querySelector(".mm-callout-content")!;
  expect(content.textContent).toBe("Attention");
  // Le curseur est resté à la fin du texte (un texte déplacé sous lui le faisait passer au début).
  const selection = document.getSelection()!;
  expect(selection.anchorNode !== null && content.contains(selection.anchorNode), "la sélection du document").toBe(true);
  expect(selection.anchorOffset).toBe("Attention".length);
  expect(m.editor.getTextCursorPosition().block.id).toBe("c");
  expect(m.editor.prosemirrorState.selection.$anchor.parentOffset).toBe("Attention".length);
});

const PLAN = "0190f5a0-0000-7000-8000-0000000000a1";
const NOTES = "0190f5a0-0000-7000-8000-0000000000a2";

/** Le bouton du bloc `id` qui ouvre sa fenêtre. */
const anchorOf = (m: Mounted, id: string) => blockOf(m, id).querySelector<HTMLButtonElement>("[data-block-window-anchor]")!;

test("a_rich_block_changed_elsewhere_keeps_the_focus_on_its_button_or_link", async () => {
  const m = await mountEditor({
    blocks: [
      { id: "c", type: "callout", props: { icon: "⚠️", color: "blue" }, content: "Attention" },
      { id: "b", type: "bookmark", props: { url: "https://exemple.fr/" } },
      { id: "l", type: "pageLink", props: { pageId: PLAN, title: "Plan" } },
    ],
  });
  for (const [id, change, shown] of [
    ["c", { icon: "📌", color: "red" }, "📌"],
    ["b", { url: "https://ailleurs.example/page" }, "https://ailleurs.example/page"],
    ["l", { pageId: NOTES, title: "Notes" }, "Notes"],
  ] as const) {
    const button = id === "l" ? blockOf(m, id).querySelector<HTMLElement>("a")! : anchorOf(m, id);
    button.focus();
    expect(document.activeElement).toBe(button);
    // Un autre change la prop : la vue suit, et le bouton qui avait le focus le garde.
    m.editor.updateBlock(id, { props: change });
    await flush();
    expect(blockOf(m, id).textContent, id).toContain(shown);
    expect(document.activeElement, id).toBe(button);
    expect(id === "l" ? blockOf(m, id).querySelector("a") : anchorOf(m, id), id).toBe(button);
  }
  expect(blockOf(m, "l").querySelector("a")!.getAttribute("href")).toBe(`/p/${NOTES}`);
  const callout = blockOf(m, "c");
  expect(callout.querySelector(".mm-callout")!.classList.contains("mm-callout-red")).toBe(true);
  expect(callout.querySelector(".mm-callout")!.classList.contains("mm-callout-blue")).toBe(false);
  // Les attributs de l'élément du bloc suivent aussi ses props.
  expect(callout.querySelector(".bn-block-content")!.getAttribute("data-color")).toBe("red");
  expect(callout.querySelector(".bn-block-content")!.getAttribute("data-icon")).toBe("📌");
  expect(blockOf(m, "b").querySelector(".bn-block-content")!.getAttribute("data-url")).toBe("https://ailleurs.example/page");
  expect(blockOf(m, "l").querySelector(".bn-block-content")!.getAttribute("data-page-id")).toBe(NOTES);
  expect(blockOf(m, "l").querySelector(".bn-block-content")!.getAttribute("data-title")).toBe("Notes");
  // Le texte du callout n'a pas bougé.
  expect(callout.querySelector(".mm-callout-content")!.textContent).toBe("Attention");
});

test("a_key_pressed_on_the_button_of_a_rich_block_is_left_to_the_button", async () => {
  const m = await mountEditor({
    blocks: [
      { id: "p", type: "paragraph", content: "Avant" },
      { id: "c", type: "callout", props: { icon: "⚠️" }, content: "Attention" },
      { id: "b", type: "bookmark", props: { url: "https://exemple.fr/" } },
    ],
  });
  m.editor.setTextCursorPosition("p", "end");
  const before = JSON.stringify(m.editor.document);
  for (const id of ["c", "b"]) {
    const button = anchorOf(m, id);
    button.focus();
    // Entrée sur le bouton l'actionne ; ProseMirror ne doit pas la prendre pour une frappe dans le
    // document (elle couperait le paragraphe où est le curseur de l'éditeur).
    const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    button.dispatchEvent(event);
    await flush();
    expect(event.defaultPrevented, id).toBe(false);
    expect(JSON.stringify(m.editor.document), id).toBe(before);
  }
});

test("a_rich_view_keeps_the_events_a_react_view_kept", () => {
  // L'élément du bloc (posé par BlockNote), la vue, un bouton, un lien, et le texte.
  const wrapper = document.createElement("div");
  wrapper.className = "bn-block-content";
  const dom = document.createElement("div");
  const button = document.createElement("button");
  const glyph = document.createElement("span");
  button.append(glyph);
  const link = document.createElement("a");
  const content = document.createElement("div");
  const text = document.createElement("span");
  content.append(text);
  dom.append(button, link, content);
  wrapper.append(dom);
  const stop = stopEventsOutside(dom, content);
  const on = (target: Element, type: string) => {
    const event = new Event(type, { bubbles: true, cancelable: true });
    target.dispatchEvent(event);
    Object.defineProperty(event, "target", { value: target });
    return stop(event);
  };
  // Un bouton garde tout, sauf le glisser-déposer. Ce qui est dans le bouton (l'icône) est traité comme
  // le reste de la vue, comme tiptap le faisait (il ne regarde que l'élément visé).
  for (const type of ["mousedown", "mouseup", "click", "keydown"]) expect(on(button, type), `bouton ${type}`).toBe(true);
  expect(on(glyph, "mousedown")).toBe(false);
  for (const type of ["mouseup", "click", "keydown"]) expect(on(glyph, type), `dans le bouton ${type}`).toBe(true);
  // Le dépôt et le survol d'un glisser vont à ProseMirror, même sur un bouton.
  expect(on(button, "drop")).toBe(false);
  expect(on(button, "dragover")).toBe(false);
  // Ailleurs dans la vue : ProseMirror voit le bouton enfoncé (il sélectionne le bloc), la copie, le
  // collage, la coupe, le dépôt ; le reste est gardé (relâcher sur un lien ne le suit pas par lui).
  for (const type of ["mousedown", "copy", "paste", "cut", "drop", "dragover", "dragenter"]) {
    expect(on(link, type), `lien ${type}`).toBe(false);
  }
  for (const type of ["mouseup", "click", "keydown"]) expect(on(link, type), `lien ${type}`).toBe(true);
  // L'élément du bloc lui-même compte ; le texte et le dehors sont à ProseMirror.
  expect(on(wrapper, "click")).toBe(true);
  for (const type of ["mousedown", "mouseup", "keydown"]) expect(on(text, type), `texte ${type}`).toBe(false);
  expect(on(document.body, "click")).toBe(false);
});

/** Les abonnés à l'état « modifiable » de l'éditeur. */
const editableListeners = (m: Mounted) => m.editor.getExtension(ReadOnlyExtension)!.store.listeners.size;

test("a_rich_view_listens_only_while_it_is_shown", async () => {
  const m = await mountEditor({ blocks: [{ id: "p", type: "paragraph", content: "Seul" }] });
  const before = editableListeners(m);
  const pagesBefore = watchersForTests();
  m.editor.insertBlocks(
    [
      { id: "c1", type: "callout", content: "Un" },
      { id: "c2", type: "callout", content: "Deux" },
      { id: "b1", type: "bookmark", props: { url: "https://exemple.fr/" } },
      { id: "l1", type: "pageLink", props: { pageId: PLAN, title: "Plan" } },
    ],
    "p",
    "after",
  );
  await flush();
  expect(editableListeners(m)).toBe(before + 3);
  expect(watchersForTests()).toBe(pagesBefore + 1);
  // Un rendu hors de l'éditeur (copie, export) n'écoute rien.
  await m.editor.blocksToFullHTML(m.editor.document);
  m.editor.blocksToHTMLLossy(m.editor.document);
  expect(editableListeners(m)).toBe(before + 3);
  expect(watchersForTests()).toBe(pagesBefore + 1);
  // Les blocs partis, leurs vues n'écoutent plus.
  m.editor.removeBlocks(["c1", "c2", "b1", "l1"]);
  await flush();
  expect(editableListeners(m)).toBe(before);
  expect(watchersForTests()).toBe(pagesBefore);
});

test("rich_blocks_copied_come_back_whole", async () => {
  // La copie passe par le rendu hors de l'éditeur (`renderType` « dom ») : ce qu'il écrit se relit.
  const blocks = [
    { id: "c", type: "callout" as const, props: { icon: "⚠️", color: "blue" }, content: "Attention" },
    { id: "b", type: "bookmark" as const, props: { url: "https://exemple.fr/" } },
    { id: "l", type: "pageLink" as const, props: { pageId: PLAN, title: "Plan" } },
  ];
  const m = await mountEditor({ blocks });
  const html = await m.editor.blocksToFullHTML(m.editor.document.slice(0, 3));
  const back = await m.editor.tryParseHTMLToBlocks(html);
  expect(back.map(({ type, props, content }) => ({ type, props, content }))).toEqual(
    m.editor.document.slice(0, 3).map(({ type, props, content }) => ({ type, props, content })),
  );
  expect(back.map((block) => block.type)).toEqual(["callout", "bookmark", "pageLink"]);
});

test("the_icons_drawn_without_react_are_those_of_the_interface", async () => {
  const drawn = (svg: Element) => [
    svg.tagName.toLowerCase(),
    Object.fromEntries([...svg.attributes].map((a) => [a.name, a.value])),
    [...svg.children].map((child) => [child.tagName, Object.fromEntries([...child.attributes].map((a) => [a.name, a.value]))]),
  ];
  for (const [Icon, size, shapes] of [
    [InfoIcon, 18, INFO_ICON],
    [BookmarkIcon, 16, BOOKMARK_ICON],
    [PageIcon, 15, PAGE_ICON],
  ] as const) {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Icon />));
    expect(drawn(iconElement(size, shapes))).toEqual(drawn(host.querySelector("svg")!));
    await act(async () => root.unmount());
  }
});

/** Un callout, un signet et un lien de page entre deux paragraphes. */
const RICH = [
  { id: "p", type: "paragraph" as const, content: "Avant" },
  { id: "c", type: "callout" as const, props: { icon: "⚠️", color: "blue" }, content: "Attention" },
  { id: "b", type: "bookmark" as const, props: { url: "https://exemple.fr/" } },
  { id: "l", type: "pageLink" as const, props: { pageId: PLAN, title: "Plan" } },
  { id: "z", type: "paragraph" as const, content: "Après" },
];

/** L'élément Yjs du contenu du bloc `id` dans `doc`. */
function contentOf(doc: Y.Doc, id: string): Y.XmlElement {
  const walk = (node: Y.XmlElement | Y.XmlFragment): Y.XmlElement | null => {
    for (const child of node.toArray()) {
      if (!(child instanceof Y.XmlElement)) continue;
      if (child.nodeName === "blockContainer" && child.getAttribute("id") === id) return child.get(0) as Y.XmlElement;
      const found = walk(child);
      if (found) return found;
    }
    return null;
  };
  const found = walk(doc.getXmlFragment(FRAGMENT));
  if (found === null) throw new Error(`le bloc ${id} n'est pas dans le document`);
  return found;
}

/** Écrit dans le document de `m` comme un autre client. */
function remote(m: Mounted, change: (doc: Y.Doc) => void): void {
  const other = new Y.Doc();
  Y.applyUpdate(other, Y.encodeStateAsUpdate(m.doc));
  const before = Y.encodeStateVector(other);
  other.transact(() => change(other));
  Y.applyUpdate(m.doc, Y.encodeStateAsUpdate(other, before), REMOTE);
  other.destroy();
}

test("a_prop_turned_by_another_into_something_else_than_text_breaks_nothing", async () => {
  // Une vue sans React lit une prop changée ailleurs dans `update`, pas au rendu d'un composant :
  // Yjs y garde toute valeur JSON, et une valeur lue sans `textProp` levait dans la mise à jour de la
  // vue de ProseMirror (l'éditeur de tous ceux qui ont la page ouverte) ou s'affichait « [object
  // Object] ».
  await act(async () => publishPages("space", [page(PLAN, { title: "Plan 2027" })]));
  const m = await mountEditor({ blocks: RICH });
  const views = ["c", "b", "l"].map((id) => blockOf(m, id).querySelector(".bn-block-content")!.firstElementChild);
  const odd: unknown[] = [{ a: 1 }, 42, true, ["x"], null, "<img src=x onerror=alert(1)>", "x".repeat(300_000)];
  for (const value of odd) {
    const label = JSON.stringify(value)?.slice(0, 30) ?? String(value);
    await act(async () =>
      remote(m, (doc) => {
        for (const [id, name] of [["c", "icon"], ["c", "color"], ["b", "url"], ["l", "pageId"], ["l", "title"]]) {
          contentOf(doc, id).setAttribute(name, value as string);
        }
      }),
    );
    await flush();
    expect(m.errors, label).toEqual([]);
    for (const id of ["c", "b", "l"]) {
      const view = blockOf(m, id);
      expect(view.querySelector("img"), `${label} ${id}`).toBeNull();
      expect(foreignStyles(view), `${label} ${id}`).toEqual([]);
      expect(view.querySelector('a[href^="javascript"]'), `${label} ${id}`).toBeNull();
      expect(view.textContent, `${label} ${id}`).not.toContain("[object");
    }
    expect(blockOf(m, "c").querySelector(".mm-callout-content")!.textContent, label).toBe("Attention");
    expect(blockOf(m, "c").querySelector(".mm-callout")!.className, label).toBe("mm-callout mm-callout-default");
    // Le lien de page : pas un id, pas de lien ; un titre copié qui n'est pas du texte est « Sans titre ».
    const title = typeof value === "string" ? value.slice(0, 500) : "Sans titre";
    expect(blockOf(m, "l").querySelector(".mm-page-link-title")!.textContent!.startsWith(title), label).toBe(true);
    // L'éditeur affiche encore ce qu'écrit un autre.
    await act(async () =>
      remote(m, (doc) => {
        const text = contentOf(doc, "z").get(0) as Y.XmlText;
        text.insert(text.length, "!");
      }),
    );
    await flush();
  }
  expect(blockOf(m, "z").textContent).toBe(`Après${"!".repeat(odd.length)}`);
  // Les vues n'ont pas été recréées.
  expect(["c", "b", "l"].map((id) => blockOf(m, id).querySelector(".bn-block-content")!.firstElementChild)).toEqual(views);
  // Une adresse `javascript:` n'est jamais un lien ; un titre au U+202E reste isolé.
  await act(async () =>
    remote(m, (doc) => {
      contentOf(doc, "b").setAttribute("url", "javascript:alert(1)");
      contentOf(doc, "l").setAttribute("pageId", "javascript:alert(1)");
      contentOf(doc, "l").setAttribute("title", "‮gnp.exe");
    }),
  );
  await flush();
  expect(blockOf(m, "b").querySelector("a")).toBeNull();
  expect(blockOf(m, "l").querySelector("a")).toBeNull();
  expect(blockOf(m, "l").querySelector(".mm-page-link-title")!.getAttribute("dir")).toBe("auto");
  // Un id connu revient : le titre à jour aussi ; une page que le compte ne voit pas : jamais lui.
  await act(async () => remote(m, (doc) => contentOf(doc, "l").setAttribute("pageId", PLAN)));
  await flush();
  expect(blockOf(m, "l").querySelector(".mm-page-link-title")!.textContent).toBe("Plan 2027");
  await act(async () => remote(m, (doc) => contentOf(doc, "l").setAttribute("pageId", NOTES)));
  await flush();
  expect(blockOf(m, "l").textContent).not.toContain("2027");
});

test("the_islands_of_a_rich_block_are_marked_not_editable", async () => {
  // `contentEditable = "false"` (la propriété) ne pose pas l'attribut dans jsdom : rien ne le voyait.
  const m = await mountEditor({ blocks: RICH });
  expect(blockOf(m, "c").querySelector(".mm-callout-icon")!.getAttribute("contenteditable")).toBe("false");
  expect(blockOf(m, "b").querySelector(".mm-bookmark")!.getAttribute("contenteditable")).toBe("false");
  expect(blockOf(m, "l").querySelector(".mm-page-link")!.getAttribute("contenteditable")).toBe("false");
  // Le texte du callout, lui, se modifie.
  expect(blockOf(m, "c").querySelector(".mm-callout-content")!.closest('[contenteditable="false"]')).toBeNull();
  // Pour un lecteur aussi.
  const reader = await mountEditor({ blocks: RICH, editable: false });
  expect(blockOf(reader, "c").querySelector(".mm-callout-icon")!.getAttribute("contenteditable")).toBe("false");
});

test("a_selected_rich_block_is_marked_like_a_react_view_marked_it", async () => {
  // ProseMirror, sans `selectNode` dans la vue, posait `draggable="true"` sur le bloc sélectionné ;
  // la vue React de tiptap ne posait que la classe.
  const m = await mountEditor({ blocks: RICH });
  const view = m.editor.prosemirrorView;
  for (const id of ["c", "b", "l"]) {
    const element = blockOf(m, id).querySelector<HTMLElement>(".bn-block-content")!;
    let at = -1;
    view.state.doc.descendants((node, pos) => {
      if (at < 0 && node.attrs.id === id) at = pos + 1;
      return at < 0;
    });
    act(() => view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, at))));
    expect(element.classList.contains("ProseMirror-selectednode"), id).toBe(true);
    expect(element.hasAttribute("draggable"), id).toBe(false);
    act(() => m.editor.setTextCursorPosition("p", "end"));
    expect(element.classList.contains("ProseMirror-selectednode"), id).toBe(false);
    expect(element.hasAttribute("draggable"), id).toBe(false);
  }
});

test("a_copy_of_a_rich_block_carries_no_button", async () => {
  // La copie et l'export passent par le rendu hors de l'éditeur : « Modifier », le bouton de l'icône
  // n'ont rien à y faire.
  const m = await mountEditor({ blocks: RICH });
  const blocks = m.editor.document.slice(1, 4);
  for (const html of [await m.editor.blocksToFullHTML(blocks), m.editor.blocksToHTMLLossy(blocks)]) {
    expect(html).not.toContain("<button");
    expect(html).not.toContain("Modifier");
    expect(html).toContain("Attention");
    expect(html).toContain("exemple.fr");
  }
});

test("a_starting_page_leaves_no_editor_listening", async () => {
  // Le banc écrit une page de départ par un éditeur à lui (`seedState`) : il ne reste pas monté.
  const before = watchersForTests();
  await seedState(RICH);
  expect(watchersForTests()).toBe(before);
});
