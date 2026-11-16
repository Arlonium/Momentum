// La table des matières (Task C3) : les titres de niveau 1 à 4 de la page, en liens, dans l'ordre,
// chacun à sa profondeur. Elle suit la page sans la relire à chaque frappe (les recalculs sont
// groupés), et seulement quand une table des matières est affichée. Un lien fait défiler la page
// jusqu'à son titre et y met le curseur. Rien ne bouge sous le pointeur ni sous le focus.
import { act } from "react";
import { afterEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { exchange, peer, REMOTE, seedState, selectBlock, typeAtEnd, type Seed } from "../../test/editors";
import { until } from "../../test/hooks";
import { blockOf, flush, foreignStyles, mountEditor, press, type Mounted } from "../../test/richEditor";
import { contrast } from "../../page/avatarInk";
import { readHeadings } from "./headings";

// Chaque lecture des titres passe par `readHeadings` : on les compte.
vi.mock("./headings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./headings")>();
  return { ...actual, readHeadings: vi.fn(actual.readHeadings) };
});
const reads = () => vi.mocked(readHeadings).mock.calls.length;

// jsdom ne fait pas défiler : un lien de la table appelle `scrollIntoView`, qu'il n'a pas.
Element.prototype.scrollIntoView ??= () => {};

afterEach(() => {
  vi.useRealTimers();
  vi.mocked(readHeadings).mockClear();
});

const TOC = { id: "toc", type: "toc" } as const;

const PAGE: Seed = [
  TOC,
  { id: "h1", type: "heading", props: { level: 1 }, content: "Introduction" },
  {
    id: "p",
    type: "paragraph",
    content: "Du texte",
    children: [{ id: "h2", type: "heading", props: { level: 2 }, content: "Contexte" }],
  },
  { id: "h5", type: "heading", props: { level: 5 }, content: "Détail" },
  { id: "h6", type: "heading", props: { level: 6 }, content: "Plus fin" },
  {
    id: "h3",
    type: "heading",
    props: { level: 3, isToggleable: true },
    content: "Repli",
    children: [{ id: "h4", type: "heading", props: { level: 4 }, content: "Fin" }],
  },
  { id: "empty", type: "heading", props: { level: 2 }, content: "" },
  {
    id: "cols",
    type: "columnList",
    children: [
      { id: "c1", type: "column", children: [{ id: "hc", type: "heading", props: { level: 2 }, content: "Colonne" }] },
      { id: "c2", type: "column", children: [{ id: "pc", type: "paragraph", content: "À côté" }] },
    ],
  },
  { id: "z", type: "paragraph", content: "Après" },
];

/** La table des matières affichée de `m`. */
function tocOf(m: Mounted): HTMLElement | null {
  return m.host.querySelector<HTMLElement>('nav[aria-label="Table des matières"]');
}

/** Ses entrées : niveau et texte. */
function entries(m: Mounted): { level: string | undefined; text: string | null }[] {
  return [...(tocOf(m)?.querySelectorAll("li") ?? [])].map((li) => ({
    level: li.dataset.level,
    text: li.textContent,
  }));
}

const texts = (m: Mounted) => entries(m).map((entry) => entry.text);

function link(m: Mounted, text: string): HTMLAnchorElement {
  const found = [...(tocOf(m)?.querySelectorAll("a") ?? [])].find((a) => a.textContent === text);
  if (found === undefined) throw new Error(`pas de lien « ${text} » dans la table des matières`);
  return found;
}

/** Une frappe à la fin du bloc `id`, comme au clavier. */
function typeIn(m: Mounted, id: string, text = "x"): void {
  act(() => {
    m.editor.setTextCursorPosition(id, "end");
    m.editor.insertInlineContent([text]);
  });
}

/** L'horloge de la page, avancée à la main (les relectures groupées), sans toucher au reste. */
function stopTheClock(): void {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
}
const advance = (ms: number) => act(() => void vi.advanceTimersByTime(ms));

test("toc_lists_headings_1_to_4_in_order_with_their_depth", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const toc = tocOf(m);
  expect(toc, "un nav nommé « Table des matières »").not.toBeNull();
  expect(blockOf(m, "toc").contains(toc)).toBe(true);
  // Une liste de liens, dans l'ordre du document, enfants, repli et colonnes compris ; ni les niveaux
  // 5 et 6, ni un titre vide.
  expect(toc!.querySelector(":scope > ol")).not.toBeNull();
  expect(entries(m)).toEqual([
    { level: "1", text: "Introduction" },
    { level: "2", text: "Contexte" },
    { level: "3", text: "Repli" },
    { level: "4", text: "Fin" },
    { level: "2", text: "Colonne" },
  ]);
  const hrefs = [...toc!.querySelectorAll("li > a")].map((a) => a.getAttribute("href"));
  expect(hrefs).toEqual(["#h1", "#h2", "#h3", "#h4", "#hc"]);
});

test("toc_follows_the_page_without_reading_it_at_each_keystroke", async () => {
  const m = await mountEditor({
    blocks: [TOC, { id: "h", type: "heading", props: { level: 1 }, content: "Un" }],
  });
  expect(texts(m)).toEqual(["Un"]);
  stopTheClock();
  const before = reads();
  // Dix frappes en 100 ms (la dernière à 90 ms) : aucune lecture pendant la rafale.
  for (let i = 0; i < 10; i++) {
    typeIn(m, "h");
    advance(10);
  }
  expect(reads()).toBe(before);
  // 250 ms après la dernière, une seule.
  advance(239);
  expect(reads()).toBe(before);
  expect(texts(m)).toEqual(["Un"]);
  advance(1);
  expect(reads()).toBe(before + 1);
  expect(texts(m)).toEqual(["Unxxxxxxxxxx"]);
  advance(5_000);
  expect(reads()).toBe(before + 1);

  // Une frappe toutes les 200 ms ne laisse jamais 250 ms de calme : la lecture part quand même une
  // seconde après la première frappe qu'elle traite.
  const start = reads();
  for (let i = 0; i < 4; i++) {
    typeIn(m, "h", "y");
    advance(200);
  }
  typeIn(m, "h", "y"); // 800 ms
  advance(199);
  expect(reads()).toBe(start);
  advance(1);
  expect(reads()).toBe(start + 1);
  expect(texts(m)).toEqual(["Unxxxxxxxxxxyyyyy"]);
});

test("page_without_a_toc_reads_no_heading", async () => {
  // Témoin d'abord : la même page avec une table des matières lit ses titres.
  for (const withToc of [true, false]) {
    vi.mocked(readHeadings).mockClear();
    const heading = { id: "h", type: "heading", props: { level: 1 }, content: "Un" } as const;
    const m = await mountEditor({ blocks: withToc ? [TOC, heading] : [heading] });
    const documentReads = vi.spyOn(m.editor, "document", "get");
    stopTheClock();
    for (let i = 0; i < 5; i++) {
      typeIn(m, "h");
      advance(100);
    }
    advance(2_000);
    vi.useRealTimers();
    // `editor.document` n'est jamais lu par la table des matières (garde : elle lit le document
    // ProseMirror) ; ni le DOM, que rien ne lit pour elle.
    expect(documentReads, withToc ? "avec" : "sans").not.toHaveBeenCalled();
    if (withToc) expect(reads()).toBeGreaterThan(0);
    else expect(reads()).toBe(0);
    documentReads.mockRestore();
  }

  // Une page dont on retire la seule table des matières ne lit plus rien.
  const m = await mountEditor({
    blocks: [TOC, { id: "h", type: "heading", props: { level: 1 }, content: "Un" }],
  });
  act(() => m.editor.removeBlocks(["toc"]));
  await flush();
  expect(tocOf(m)).toBeNull();
  vi.mocked(readHeadings).mockClear();
  stopTheClock();
  for (let i = 0; i < 5; i++) {
    typeIn(m, "h");
    advance(100);
  }
  advance(2_000);
  expect(reads()).toBe(0);
});

test("toc_entry_scrolls_to_its_heading_and_focuses_it", async () => {
  const scrolled: { element: Element; options: unknown }[] = [];
  const original = Element.prototype.scrollIntoView;
  Element.prototype.scrollIntoView = function (this: Element, options?: unknown) {
    scrolled.push({ element: this, options });
  };
  try {
    for (const editable of [true, false]) {
      scrolled.length = 0;
      const m = await mountEditor({ blocks: PAGE, editable });
      const hash = location.hash;
      act(() => link(m, "Contexte").click());
      await flush();
      expect(scrolled.map((s) => s.element), editable ? "modifiable" : "lecture").toEqual([blockOf(m, "h2")]);
      expect(scrolled[0].options).toMatchObject({ block: "start" });
      // Le curseur est au début du titre, et l'éditeur a le focus.
      expect(m.editor.getTextCursorPosition().block.id, editable ? "modifiable" : "lecture").toBe("h2");
      expect(m.editor.prosemirrorView.dom.contains(document.activeElement)).toBe(true);
      // Le lien n'est pas suivi : l'adresse ne change pas.
      expect(location.hash).toBe(hash);
    }
  } finally {
    Element.prototype.scrollIntoView = original;
  }
});

test("empty_toc_says_so", async () => {
  const m = await mountEditor({
    blocks: [
      TOC,
      { id: "p", type: "paragraph", content: "Rien" },
      { id: "h5", type: "heading", props: { level: 5 }, content: "Cinq" },
    ],
  });
  expect(tocOf(m)!.textContent).toBe("Aucun titre dans la page.");
  expect(tocOf(m)!.querySelector("ol")).toBeNull();
});

test("heading_text_is_shown_as_text", async () => {
  const m = await mountEditor({
    blocks: [
      TOC,
      {
        id: "h",
        type: "heading",
        props: { level: 1 },
        content: [
          { type: "text", text: '<img src=x onerror="alert(1)"> ‮Énergie ', styles: { bold: true } },
          { type: "mention", props: { kind: "user", id: "u", label: "<b>Alice</b>" } },
          { type: "text", text: " et ", styles: {} },
          { type: "math", props: { latex: "E=mc^2" } },
        ],
      },
    ],
  });
  expect(texts(m)).toEqual(['<img src=x onerror="alert(1)"> ‮Énergie @<b>Alice</b> et E=mc^2']);
  const toc = tocOf(m)!;
  expect(toc.querySelector("img, b")).toBeNull();
  // Isolé : `dir="auto"` sur le texte de l'entrée ; aucun `style`.
  expect(toc.querySelector("li a [dir='auto'], li a[dir='auto']")).not.toBeNull();
  expect(foreignStyles(blockOf(m, "toc"))).toEqual([]);
});

test("toc_follows_a_heading_typed_by_someone_else", async () => {
  const base = await seedState([TOC, { id: "h", type: "heading", props: { level: 2 }, content: "Plan" }]);
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  expect(texts(mona)).toEqual(["Plan"]);
  typeAtEnd(tom, "h", " du lot");
  await act(() => exchange(server, [tom, mona]));
  await until(mona.host, () => texts(mona)[0] === "Plan du lot", "le titre tapé par Tom, dans la table de Mona");
});

/** Le pointeur entre dans `target` depuis `from` (React lit `pointerover` et `pointerout`). */
const pointer = (from: Element, target: Element) =>
  act(async () => {
    from.dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: target }));
    target.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, relatedTarget: from }));
  });

test("toc_holds_still_under_the_pointer_and_follows_once_it_leaves", async () => {
  const base = await seedState([
    TOC,
    { id: "a", type: "heading", props: { level: 1 }, content: "Alpha" },
    { id: "b", type: "heading", props: { level: 1 }, content: "Bêta" },
  ]);
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  const list = tocOf(mona)!.querySelector("ol")!;
  await pointer(document.body, link(mona, "Bêta"));
  // Tom pose un titre avant « Alpha » : la table de Mona est relue, mais ne bouge pas sous son
  // pointeur (« Bêta » reste sous lui, où il était).
  const before = reads();
  tom.editor.insertBlocks([{ type: "heading", props: { level: 1 }, content: "Zéro" }], "a", "before");
  await act(() => exchange(server, [tom, mona]));
  await until(mona.host, () => reads() > before, "la table relue");
  expect(texts(mona)).toEqual(["Alpha", "Bêta"]);
  expect(list.children[1]).toBe(link(mona, "Bêta").parentElement);
  // Le pointeur sorti, elle suit la page.
  await pointer(list, document.body);
  expect(texts(mona)).toEqual(["Zéro", "Alpha", "Bêta"]);
});

test("a_focused_entry_holds_still_and_the_focus_never_falls_on_the_body", async () => {
  const base = await seedState([
    TOC,
    { id: "a", type: "heading", props: { level: 1 }, content: "Alpha" },
    { id: "b", type: "heading", props: { level: 1 }, content: "Bêta" },
    { id: "z", type: "paragraph", content: "Après" },
  ]);
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  act(() => link(mona, "Alpha").focus());
  // Tom supprime le titre qui a le focus chez Mona : l'entrée reste, et le focus avec elle.
  const before = reads();
  tom.editor.removeBlocks(["a"]);
  await act(() => exchange(server, [tom, mona]));
  await until(mona.host, () => reads() > before, "la table relue");
  expect(document.activeElement).toBe(link(mona, "Alpha"));
  // Un lien suivi rend la main à la page : la table la suit de nouveau.
  act(() => link(mona, "Bêta").click());
  await flush();
  expect(texts(mona)).toEqual(["Bêta"]);
  // Tom supprime la table des matières où Mona a le focus : il va à l'éditeur, jamais sur `body`.
  act(() => link(mona, "Bêta").focus());
  tom.editor.removeBlocks(["toc"]);
  await act(() => exchange(server, [tom, mona]));
  await until(mona.host, () => tocOf(mona) === null, "la table partie");
  await flush();
  expect(document.activeElement).not.toBe(document.body);
  expect(mona.editor.prosemirrorView.dom.contains(document.activeElement)).toBe(true);
});

test("ctrl_alt_enter_on_a_toc_puts_the_focus_on_its_first_entry", async () => {
  const m = await mountEditor({ blocks: PAGE });
  // Témoin : depuis un paragraphe, rien ne va à la table.
  act(() => {
    m.editor.setTextCursorPosition("z", "end");
    m.editor.focus();
  });
  const fromText = press(m.editor.prosemirrorView.dom, "Enter", { ctrlKey: true, altKey: true });
  expect(fromText.defaultPrevented).toBe(false);
  expect(tocOf(m)!.contains(document.activeElement)).toBe(false);
  // La table sélectionnée (les flèches la sélectionnent) : le focus va à sa première entrée.
  act(() => {
    selectBlock(m, "toc");
    m.editor.focus();
  });
  const onToc = press(m.editor.prosemirrorView.dom, "Enter", { ctrlKey: true, altKey: true });
  expect(onToc.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(link(m, "Introduction"));
});

test("the_colours_of_the_toc_are_readable", () => {
  // Une entrée (texte secondaire) sur la page, et survolée (texte) sur son fond.
  expect(contrast("#66655F", "#FFFFFF")).toBeGreaterThanOrEqual(4.5);
  expect(contrast("#2F2E2B", "#F4F3EF")).toBeGreaterThanOrEqual(4.5);
});
