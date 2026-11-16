// Les vues des blocs riches dessinées sans React (le callout, le signet ; R625, R626, `richView.ts`) :
// ce qui les rendait lentes à l'ouverture d'une page ne revient pas, et ce que faisait la vue React
// tient toujours (le focus d'un bouton gardé quand une prop change ailleurs, le curseur gardé, rien
// d'écouté par une vue qui n'est plus affichée, les icônes de l'interface).
import { ReadOnlyExtension } from "@blocknote/core/extensions";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { BookmarkIcon, InfoIcon, PageIcon } from "../../icons";
import { watchersForTests } from "../../space/pageDirectory";
import { blockOf, flush, mountEditor, type Mounted } from "../../test/richEditor";
import { BOOKMARK_ICON, iconElement, INFO_ICON, PAGE_ICON, stopEventsOutside } from "./richView";

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
