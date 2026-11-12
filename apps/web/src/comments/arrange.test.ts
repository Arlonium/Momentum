// Où va chaque fil dans le panneau : les fils ouverts des blocs, dans l'ordre des blocs dans la
// page, puis ceux de la page ; à part, ceux dont le bloc n'est plus là ; et les résolus.
import { expect, test } from "vitest";
import { author, thread } from "../test/comments";
import { arrangeThreads, openCount } from "./arrange";

const ids = (threads: { id: string }[]) => threads.map((t) => t.id);

test("open_threads_follow_the_blocks_then_the_page", () => {
  const threads = [
    thread("page-1"),
    thread("deux-a", { blockId: "deux" }),
    thread("un", { blockId: "un" }),
    thread("deux-b", { blockId: "deux" }),
    thread("page-2"),
  ];
  const arranged = arrangeThreads(threads, ["un", "deux"]);
  // Deux fils d'un même bloc gardent l'ordre où ils ont été ouverts.
  expect(ids(arranged.open)).toEqual(["un", "deux-a", "deux-b", "page-1", "page-2"]);
  expect(arranged.orphans).toEqual([]);
  expect(arranged.resolved).toEqual([]);
});

test("a_thread_whose_block_is_gone_is_an_orphan", () => {
  const threads = [thread("parti", { blockId: "parti" }), thread("un", { blockId: "un" })];
  const arranged = arrangeThreads(threads, ["un"]);
  expect(ids(arranged.open)).toEqual(["un"]);
  expect(ids(arranged.orphans)).toEqual(["parti"]);
  expect(arranged.deleted(threads[0])).toBe(true);
  expect(arranged.deleted(threads[1])).toBe(false);
});

test("an_unknown_document_makes_no_orphan", () => {
  const threads = [thread("page"), thread("b", { blockId: "b" }), thread("a", { blockId: "a" })];
  const arranged = arrangeThreads(threads, null);
  // Les fils des blocs, dans l'ordre où ils ont été ouverts, puis ceux de la page.
  expect(ids(arranged.open)).toEqual(["b", "a", "page"]);
  expect(arranged.orphans).toEqual([]);
  expect(arranged.deleted(threads[1])).toBe(false);
});

test("resolved_threads_are_apart_in_the_same_order", () => {
  const resolved = { resolvedAt: 5, resolvedBy: author("bruno") };
  const threads = [
    thread("page", resolved),
    thread("parti", { blockId: "parti", ...resolved }),
    thread("un", { blockId: "un", ...resolved }),
    thread("ouvert", { blockId: "un" }),
  ];
  const arranged = arrangeThreads(threads, ["un"]);
  expect(ids(arranged.open)).toEqual(["ouvert"]);
  expect(ids(arranged.resolved)).toEqual(["un", "page", "parti"]);
  expect(openCount(threads)).toBe(1);
});
