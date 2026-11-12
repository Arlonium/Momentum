// Les pastilles des blocs commentés, et le suivi qui dit quels blocs commentés sont dans la page
// et dans quel ordre. Une page peut avoir dix mille blocs : le suivi ne regarde que les blocs
// qui ont un fil, et ne parcourt pas la page quand rien n'a bougé.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { BlockMarks, BlockTracker } from "./BlockMarks";

/** Un bloc de l'éditeur, tel que BlockNote le rend (`blockOuter` > `blockContainer` > contenu). */
function block(id: string, text = id, children: HTMLElement[] = []): HTMLElement {
  const outer = document.createElement("div");
  outer.className = "bn-block-outer";
  outer.setAttribute("data-node-type", "blockOuter");
  outer.setAttribute("data-id", id);
  const container = document.createElement("div");
  container.className = "bn-block";
  container.setAttribute("data-node-type", "blockContainer");
  container.setAttribute("data-id", id);
  const content = document.createElement("div");
  content.className = "bn-block-content";
  content.textContent = text;
  container.append(content);
  if (children.length > 0) {
    const group = document.createElement("div");
    group.className = "bn-block-group";
    group.append(...children);
    container.append(group);
  }
  outer.append(container);
  return outer;
}

/** Un éditeur (`.bn-editor`) qui porte ces blocs, dans un cadre. */
function editorWith(blocks: HTMLElement[]): { frame: HTMLDivElement; editor: HTMLElement } {
  const frame = document.createElement("div");
  const editor = document.createElement("div");
  editor.className = "bn-editor";
  editor.append(...blocks);
  frame.append(editor);
  document.body.append(frame);
  return { frame, editor };
}

afterEach(() => {
  document.body.replaceChildren();
});

/** Compte ce que le cadre parcourt (`querySelector`, `querySelectorAll`). */
function countScans(frame: HTMLElement) {
  const one = vi.spyOn(frame, "querySelector");
  const all = vi.spyOn(frame, "querySelectorAll");
  return () => one.mock.calls.length + all.mock.calls.length;
}

test("only_blocks_with_a_thread_are_looked_at_and_once", () => {
  const blocks = Array.from({ length: 2_000 }, (_, i) => block(`b${i}`));
  const { frame } = editorWith(blocks);
  const tracker = new BlockTracker(() => frame);
  const scans = countScans(frame);
  const found = tracker.locate(["b10", "b1500"], false);
  expect([...found.keys()]).toEqual(["b10", "b1500"]);
  expect(found.get("b1500")).toBe(blocks[1500]);
  const first = scans();
  expect(first).toBeLessThanOrEqual(2);
  // Rien n'a bougé : les éléments gardés servent tels quels, la page n'est pas parcourue.
  for (let i = 0; i < 50; i += 1) tracker.locate(["b10", "b1500"], false);
  expect(scans()).toBe(first);
});

test("a_block_rendered_again_is_found_again", () => {
  const { frame, editor } = editorWith([block("a"), block("b")]);
  const tracker = new BlockTracker(() => frame);
  const before = tracker.locate(["b"], false).get("b");
  // ProseMirror recrée l'élément d'un bloc déplacé ou retapé.
  const again = block("b");
  editor.replaceChild(again, before!);
  expect(tracker.locate(["b"], false).get("b")).toBe(again);
});

test("an_absent_block_is_looked_for_again_only_when_asked", () => {
  const { frame, editor } = editorWith([block("a")]);
  const tracker = new BlockTracker(() => frame);
  expect(tracker.locate(["parti"], false).has("parti")).toBe(false);
  const scans = countScans(frame);
  tracker.locate(["parti"], false);
  expect(scans()).toBe(0);
  // Revenu (une annulation) : vu à la prochaine vérification demandée.
  editor.append(block("parti"));
  expect(tracker.locate(["parti"], false).has("parti")).toBe(false);
  expect(tracker.locate(["parti"], true).has("parti")).toBe(true);
});

test("many_missing_blocks_are_found_in_a_single_pass", () => {
  const blocks = Array.from({ length: 100 }, (_, i) => block(`b${i}`));
  const { frame } = editorWith(blocks);
  const tracker = new BlockTracker(() => frame);
  const scans = countScans(frame);
  const ids = Array.from({ length: 40 }, (_, i) => `b${i * 2}`);
  expect(tracker.locate(ids, false).size).toBe(40);
  expect(scans()).toBe(1);
});

test("order_follows_the_document_nested_blocks_included", () => {
  const child = block("enfant");
  const { frame } = editorWith([block("premier"), block("parent", "parent", [child]), block("dernier")]);
  const tracker = new BlockTracker(() => frame);
  const found = tracker.locate(["dernier", "enfant", "premier", "parent"], false);
  expect(BlockTracker.order(found)).toEqual(["premier", "parent", "enfant", "dernier"]);
});

test("an_identifier_with_quotes_is_found_as_written", () => {
  const odd = 'b"\\]\n';
  const { frame } = editorWith([block(odd)]);
  const tracker = new BlockTracker(() => frame);
  expect(tracker.locate([odd], false).has(odd)).toBe(true);
});

// Le composant : les pastilles, l'ordre rendu à la page.

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
});

/** Une session réduite à ce que les pastilles lisent : la synchro, et les mises à jour du document. */
function fakeSession() {
  const updates = new Set<() => void>();
  return {
    synced: true,
    subscribe: () => () => {},
    doc: {
      on: (_: "update", listener: () => void) => updates.add(listener),
      off: (_: "update", listener: () => void) => updates.delete(listener),
    },
    /** Le document change (une frappe, une écriture d'un autre). */
    change() {
      for (const listener of [...updates]) listener();
    },
  };
}

/** Laisse passer une image (les pastilles se placent au rythme de l'affichage). */
const frameTick = () =>
  act(async () => {
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  });

test("each_block_with_an_open_thread_has_a_mark_that_opens_it", async () => {
  const { frame } = editorWith([block("un", "Premier"), block("deux"), block("trois", "Troisième")]);
  frame.append(host);
  const session = fakeSession();
  const onOrder = vi.fn();
  const onOpenThread = vi.fn();
  const marks = new Map([
    ["trois", ["t3"]],
    ["un", ["t1", "t1b"]],
  ]);
  await act(async () =>
    root.render(
      <BlockMarks
        frame={{ current: frame }}
        session={session}
        blockIds={["trois", "un", "parti"]}
        marks={marks}
        onOrder={onOrder}
        onOpenThread={onOpenThread}
        highlight={null}
        onComment={null}
      />,
    ),
  );
  await frameTick();
  expect(onOrder).toHaveBeenLastCalledWith(["un", "trois"]);
  const list = host.querySelector('ul[aria-label="Blocs commentés"]')!;
  const items = [...list.children];
  expect(items.every((item) => item.tagName === "LI")).toBe(true);
  const buttons = [...list.querySelectorAll("button")];
  // Dans l'ordre de la page, nommées par le début de leur bloc.
  expect(buttons.map((b) => b.getAttribute("aria-label"))).toEqual([
    "2 fils de commentaires sur « Premier »",
    "1 fil de commentaires sur « Troisième »",
  ]);
  await act(async () => buttons[0].click());
  expect(onOpenThread).toHaveBeenCalledWith("t1");
});

test("the_order_is_told_again_only_when_it_changes", async () => {
  const { frame, editor } = editorWith([block("un"), block("deux")]);
  frame.append(host);
  const session = fakeSession();
  const onOrder = vi.fn();
  await act(async () =>
    root.render(
      <BlockMarks
        frame={{ current: frame }}
        session={session}
        blockIds={["deux", "un"]}
        marks={new Map()}
        onOrder={onOrder}
        onOpenThread={() => {}}
        highlight={null}
        onComment={null}
      />,
    ),
  );
  await frameTick();
  expect(onOrder).toHaveBeenCalledTimes(1);
  // Une frappe qui ne déplace rien : rien de neuf à dire.
  await act(async () => session.change());
  await frameTick();
  expect(onOrder).toHaveBeenCalledTimes(1);
  // Le bloc « deux » passe devant.
  const deux = editor.lastElementChild!;
  editor.insertBefore(block("deux"), editor.firstElementChild);
  deux.remove();
  await act(async () => session.change());
  await frameTick();
  expect(onOrder).toHaveBeenLastCalledWith(["deux", "un"]);
});

test("nothing_is_known_before_the_first_sync", async () => {
  const { frame } = editorWith([block("un")]);
  frame.append(host);
  const session = { ...fakeSession(), synced: false };
  const onOrder = vi.fn();
  await act(async () =>
    root.render(
      <BlockMarks
        frame={{ current: frame }}
        session={session}
        blockIds={["un"]}
        marks={new Map([["un", ["t"]]])}
        onOrder={onOrder}
        onOpenThread={() => {}}
        highlight={null}
        onComment={null}
      />,
    ),
  );
  await frameTick();
  expect(onOrder).not.toHaveBeenCalledWith(["un"]);
  expect(host.querySelector("button")).toBeNull();
});
