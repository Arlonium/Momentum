// Arriver sur un bloc (lot 4, Task C2) : un résultat de recherche ou un rétrolien ouvre la page à son
// bloc (`/p/<page>#b=<bloc>`). Ici, ce que fait l'éditeur d'une visée : aller au bloc une fois la page
// synchronisée et sa vue montée, pour qui modifie comme pour qui lit, une seule fois, sans rien écrire
// ni rien dire d'un bloc parti. Ce que la page en fait est dans `page/PageView.test.tsx`, l'adresse
// dans `router.test.ts`.
import { act, type ReactElement } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { MementumEditor } from "./debug";
import { EditorView } from "./Editor";
import { goToBlock, useArrival, type Arrival, type Synced } from "./goToBlock";
import { stored, type Seed } from "../test/editors";
import { blockOf, flush, mountEditor, renewSession, type Mounted } from "../test/richEditor";

/** Un identifiant comme un client peut en écrire : ni UUID, ni sûr dans un sélecteur ou du HTML. */
const ODD = 'a"]b\\ <img src=x onerror=alert(1)>#b=é';

const PAGE: Seed = [
  { id: "top", type: "paragraph", content: "En haut" },
  { id: "mid", type: "paragraph", content: "Au milieu", children: [{ id: "deep", type: "paragraph", content: "Dessous" }] },
  { id: ODD, type: "paragraph", content: "Étrange" },
  { id: "pic", type: "image" },
  { id: "end", type: "heading", props: { level: 2 }, content: "À la fin" },
];

/** Les défilements demandés : l'élément, et où il est amené. */
let scrolled: { element: Element; options: unknown }[] = [];
const original = Element.prototype.scrollIntoView;
beforeEach(() => {
  scrolled = [];
  Element.prototype.scrollIntoView = function (this: Element, options?: unknown) {
    scrolled.push({ element: this, options });
  };
});
afterEach(() => {
  Element.prototype.scrollIntoView = original;
  document.getSelection()?.removeAllRanges();
});

const scrolledTo = () => scrolled.map((s) => s.element);
const inEditor = (m: { editor: MementumEditor }) =>
  m.editor.prosemirrorView.dom.contains(document.activeElement);
/** Le bloc où est la sélection du document (celle qu'un lecteur d'écran et la frappe reprennent). */
const selectionBlock = () => {
  const node = document.getSelection()?.anchorNode ?? null;
  const element = node instanceof Element ? node : (node?.parentElement ?? null);
  return element?.closest('[data-node-type="blockOuter"]')?.getAttribute("data-id") ?? null;
};
/** `goToBlock`, sous `act` : la sélection qui change fait rendre la barre de mise en forme. */
function go(editor: MementumEditor, id: string, how?: Parameters<typeof goToBlock>[2]): boolean {
  let reached = false;
  act(() => {
    reached = goToBlock(editor, id, how);
  });
  return reached;
}
/** Tape `text` là où est le curseur de l'éditeur, comme une frappe. */
const type = (editor: MementumEditor, text: string) =>
  act(() => editor.prosemirrorView.dispatch(editor.prosemirrorState.tr.insertText(text)));

test("going_to_a_block_puts_the_cursor_the_focus_and_the_screen_on_it", async () => {
  const m = await mountEditor({ blocks: PAGE });
  for (const id of ["end", "deep", ODD]) {
    scrolled = [];
    expect(go(m.editor, id), id).toBe(true);
    expect(scrolledTo(), id).toEqual([contentOf(m, id)]);
    expect(scrolled[0].options).toMatchObject({ block: "center" });
    expect(m.editor.getTextCursorPosition().block.id, id).toBe(id);
    expect(inEditor(m), id).toBe(true);
  }
  expect(m.errors).toEqual([]);
});

/** `blockOf`, pour un identifiant quelconque (sans sélecteur CSS). */
function blockOf2(m: Mounted, id: string): Element {
  const found = [...m.host.querySelectorAll('[data-node-type="blockOuter"]')].filter(
    (element) => element.getAttribute("data-id") === id,
  );
  expect(found.length, id).toBe(1);
  return found[0];
}

/** Le contenu propre du bloc `id` (son texte, son image), sans ses sous-blocs : ce qui est amené à l'écran. */
function contentOf(m: Mounted, id: string): Element {
  const own = blockOf2(m, id).querySelector(":scope > .bn-block > .bn-block-content");
  expect(own, id).not.toBeNull();
  return own!;
}

test("a_block_with_children_is_shown_by_its_own_text_not_by_its_subtree", async () => {
  // Centré sur son sous-arbre, un bloc dont les sous-blocs dépassent la fenêtre n'aurait pas son
  // texte à l'écran.
  const m = await mountEditor({ blocks: PAGE });
  expect(go(m.editor, "mid")).toBe(true);
  expect(scrolledTo()).toEqual([contentOf(m, "mid")]);
  expect(scrolledTo()[0].contains(blockOf(m, "deep"))).toBe(false);
  expect(scrolledTo()[0].textContent).toBe("Au milieu");
  expect(m.editor.getTextCursorPosition().block.id).toBe("mid");
});

test("a_block_without_text_is_shown_without_an_error", async () => {
  const m = await mountEditor({ blocks: PAGE });
  expect(go(m.editor, "pic")).toBe(true);
  expect(scrolledTo()).toEqual([contentOf(m,"pic")]);
  expect(inEditor(m)).toBe(true);
  expect(m.errors).toEqual([]);
});

test("a_block_that_left_opens_the_page_at_its_top_and_says_nothing", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const before = m.editor.getTextCursorPosition().block.id;
  const html = document.body.innerHTML;
  for (const gone of ["parti", "", 'x"] *', "end ", "END"]) {
    expect(go(m.editor, gone), gone).toBe(false);
  }
  // Rien ne défile, le curseur et le focus restent où ils étaient, rien n'est dit ni écrit.
  expect(scrolled).toEqual([]);
  expect(m.editor.getTextCursorPosition().block.id).toBe(before);
  expect(inEditor(m)).toBe(false);
  expect(document.body.innerHTML).toBe(html);
  expect(document.querySelector('[role="alert"], [role="status"]')).toBeNull();
  expect(m.writes).toEqual([]);
  expect(m.errors).toEqual([]);
  // Témoin : un bloc qui est là.
  expect(go(m.editor, "end")).toBe(true);
});

test("a_reader_is_taken_to_the_block_too", async () => {
  const m = await mountEditor({ blocks: PAGE, editable: false });
  expect(m.editor.isEditable).toBe(false);
  expect(go(m.editor, "end")).toBe(true);
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  // Le focus est dans l'éditeur (`editor.focus()` seul n'y fait rien), et la sélection du document
  // au début du texte du bloc : ProseMirror ne la pose pas dans une vue en lecture seule.
  expect(inEditor(m)).toBe(true);
  expect(selectionBlock()).toBe("end");
  expect(document.getSelection()!.isCollapsed).toBe(true);
  expect(document.getSelection()!.anchorOffset).toBe(0);
  expect(m.writes).toEqual([]);
  expect(m.errors).toEqual([]);
});

test("going_without_the_focus_leaves_it_where_it_is", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const field = document.createElement("input");
  document.body.append(field);
  field.focus();
  expect(go(m.editor, "end", { focus: false })).toBe(true);
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  expect(document.activeElement).toBe(field);
  field.remove();
});

/** La session de synchro d'une page, vue de l'arrivée : sa première synchro faite ou non. */
class Session implements Synced {
  private readonly listeners = new Set<() => void>();
  constructor(public synced: boolean) {}
  subscribe(listener: () => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
  /** La première synchro arrive. */
  sync() {
    this.synced = true;
    for (const listener of [...this.listeners]) listener();
  }
}
const SYNCED = new Session(true);

/** Ce que `Editor` fait d'une visée, autour de la vraie vue. */
function Arriving({
  editor,
  editable = true,
  session = SYNCED,
  target,
  onDone,
  view = true,
}: {
  editor: MementumEditor;
  editable?: boolean;
  session?: Session;
  target: Arrival | null;
  onDone: (target: Arrival) => void;
  view?: boolean;
}): ReactElement | null {
  useArrival(editor, session, target, onDone);
  return view ? <EditorView editor={editor} editable={editable} pageId="page" /> : null;
}

test("the_block_is_reached_after_the_first_sync_only", async () => {
  const target = { blockId: "end" };
  const done = vi.fn();
  const session = new Session(false);
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} session={session} target={target} onDone={done} />,
  });
  await flush();
  // Pas encore synchronisée : rien, et la visée n'est pas tenue pour suivie.
  expect(scrolled).toEqual([]);
  expect(done).not.toHaveBeenCalled();
  expect(inEditor(m)).toBe(false);
  await act(async () => session.sync());
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  expect(m.editor.getTextCursorPosition().block.id).toBe("end");
  expect(inEditor(m)).toBe(true);
  expect(done).toHaveBeenCalledTimes(1);
  expect(done).toHaveBeenCalledWith(target);
});

test("a_view_mounted_after_the_sync_reaches_the_block_once_it_is_there", async () => {
  const target = { blockId: "end" };
  const done = vi.fn();
  // La synchro est faite, le code de l'éditeur arrive après : la vue n'est pas encore montée.
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} target={target} onDone={done} view={false} />,
  });
  expect(scrolled).toEqual([]);
  expect(done).not.toHaveBeenCalled();
  await act(async () => m.root.render(<Arriving editor={m.editor} target={target} onDone={done} />));
  await flush();
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  expect(done).toHaveBeenCalledTimes(1);
});

test("the_fragment_is_followed_once", async () => {
  const target = { blockId: "end" };
  const done = vi.fn();
  const view = (editor: MementumEditor, editable = true) => (
    <Arriving editor={editor} editable={editable} target={target} onDone={done} />
  );
  const m = await mountEditor({ blocks: PAGE, view });
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  expect(done).toHaveBeenCalledTimes(1);
  // Un rendu de plus, la même visée : rien de plus.
  go(m.editor, "top");
  scrolled = [];
  await act(async () => m.root.render(view(m.editor)));
  await flush();
  expect(scrolled).toEqual([]);
  expect(m.editor.getTextCursorPosition().block.id).toBe("top");
  expect(done).toHaveBeenCalledTimes(1);
  // La page l'a tenue pour suivie (`onDone`) et ne la donne plus : une session recréée, donc un
  // éditeur neuf, ne ramène pas au bloc.
  const renewed = await renewSession(m, { editable: false, pageId: "page" });
  await act(async () =>
    m.root.render(<Arriving editor={renewed.editor} editable={false} target={null} onDone={done} />),
  );
  await flush();
  expect(scrolled).toEqual([]);
  expect(done).toHaveBeenCalledTimes(1);
  // Une visée neuve du même bloc (la même adresse ouverte de nouveau) est suivie.
  const again = { ...target };
  await act(async () =>
    m.root.render(<Arriving editor={renewed.editor} editable={false} target={again} onDone={done} />),
  );
  await flush();
  expect(scrolled.length).toBe(1);
  expect(done).toHaveBeenLastCalledWith(again);
});

test("a_target_whose_block_left_is_done_with_too", async () => {
  const target = { blockId: "parti" };
  const done = vi.fn();
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} target={target} onDone={done} />,
  });
  expect(scrolled).toEqual([]);
  expect(inEditor(m)).toBe(false);
  // Suivie quand même : un bloc de cet identifiant qui arriverait plus tard n'emmène personne.
  expect(done).toHaveBeenCalledTimes(1);
  expect(m.errors).toEqual([]);
});

test("typing_after_arriving_is_not_lost", async () => {
  const target = { blockId: "mid" };
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} target={target} onDone={() => {}} />,
  });
  expect(m.editor.getTextCursorPosition().block.id).toBe("mid");
  // Arriver n'écrit rien dans le document.
  expect(m.writes).toEqual([]);
  type(m.editor, "Juste ");
  await flush();
  // La frappe va au début du bloc atteint, dans l'éditeur et dans le document.
  const texts = (blocks: ReturnType<typeof stored>): Record<string, string> =>
    Object.fromEntries(blocks.flatMap((b) => [[b.id, b.text], ...Object.entries(texts(b.children))]));
  expect(texts(stored(m.doc))).toMatchObject({ top: "En haut", mid: "Juste Au milieu", deep: "Dessous" });
  expect(m.writes.length).toBeGreaterThan(0);
  expect(m.errors).toEqual([]);
});

test("a_late_arrival_does_not_take_the_focus_from_a_field_being_typed_in", async () => {
  const target = { blockId: "end" };
  const done = vi.fn();
  const session = new Session(false);
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} session={session} target={target} onDone={done} />,
  });
  // Pendant que la page se synchronise, la personne s'est mise à écrire ailleurs (le titre).
  const field = document.createElement("input");
  document.body.append(field);
  field.focus();
  await act(async () => session.sync());
  // Le bloc est montré, le focus reste au champ.
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  expect(document.activeElement).toBe(field);
  expect(done).toHaveBeenCalledTimes(1);
  field.remove();
});

test("an_arrival_in_the_gesture_takes_the_focus_from_where_the_window_gave_it_back", async () => {
  // La page est déjà ouverte et synchronisée : la fenêtre de recherche vient de rendre le focus à
  // ce qui l'avait (ici un champ), et la visée arrive dans le même rendu.
  const done = vi.fn();
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} target={null} onDone={done} />,
  });
  const field = document.createElement("input");
  document.body.append(field);
  field.focus();
  const target = { blockId: "end" };
  await act(async () => m.root.render(<Arriving editor={m.editor} target={target} onDone={done} />));
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  expect(inEditor(m)).toBe(true);
  expect(m.editor.getTextCursorPosition().block.id).toBe("end");
  field.remove();
});

test("a_cold_arrival_does_not_take_the_focus_from_a_field_being_typed_in", async () => {
  // La page s'ouvre par une adresse : la synchro est déjà faite quand le code de l'éditeur arrive,
  // et la personne tape déjà dans le titre. Tout est prêt au premier passage, mais ce n'est pas le
  // geste : seule une visée reçue par un éditeur déjà monté l'est.
  const field = document.createElement("input");
  document.body.append(field);
  field.focus();
  const target = { blockId: "end" };
  const done = vi.fn();
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} target={target} onDone={done} />,
  });
  expect(scrolledTo()).toEqual([contentOf(m, "end")]);
  expect(document.activeElement).toBe(field);
  expect(done).toHaveBeenCalledTimes(1);
  field.remove();
  // Témoin : personne n'écrit ailleurs, l'arrivée à froid prend le focus.
  const other = { blockId: "top" };
  const cold = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} target={other} onDone={done} />,
  });
  expect(inEditor(cold)).toBe(true);
});

test("a_late_arrival_leaves_the_focus_to_a_select_and_to_an_open_menu", async () => {
  const busy: [string, () => HTMLElement][] = [
    [
      "une liste déroulante",
      () => {
        const select = document.createElement("select");
        document.body.append(select);
        return select;
      },
    ],
    ...['role="menu"', "data-sidebar-menu"].map((mark): [string, () => HTMLElement] => [
      mark,
      () => {
        const holder = document.createElement("div");
        holder.innerHTML = `<div ${mark}><button type="button">Entrée</button></div>`;
        document.body.append(holder);
        return holder.querySelector("button")!;
      },
    ]),
  ];
  for (const [what, make] of busy) {
    scrolled = [];
    const session = new Session(false);
    const m = await mountEditor({
      blocks: PAGE,
      view: (editor) => <Arriving editor={editor} session={session} target={{ blockId: "end" }} onDone={() => {}} />,
    });
    const held = make();
    held.focus();
    expect(document.activeElement, what).toBe(held);
    await act(async () => session.sync());
    expect(scrolledTo(), what).toEqual([contentOf(m, "end")]);
    expect(document.activeElement, what).toBe(held);
  }
  // Témoin : un bouton ordinaire (l'entrée « Rechercher » de la barre latérale) cède le focus.
  const session = new Session(false);
  const m = await mountEditor({
    blocks: PAGE,
    view: (editor) => <Arriving editor={editor} session={session} target={{ blockId: "end" }} onDone={() => {}} />,
  });
  const button = document.createElement("button");
  document.body.append(button);
  button.focus();
  await act(async () => session.sync());
  expect(inEditor(m)).toBe(true);
});
