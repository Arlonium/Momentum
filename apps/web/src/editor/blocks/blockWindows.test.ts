// La fenêtre de réglage d'un bloc riche est tenue par l'éditeur et par l'id du bloc, jamais par
// l'élément affiché du bloc : un autre qui recrée le bloc (un déplacement le supprime et le récrit
// au même id, R223) ne la ferme pas. Ses brouillons vivent aussi par id de bloc, tant que l'éditeur
// vit (la page ouverte) : Échap ne détruit jamais un texte tapé.
import { expect, test } from "vitest";
import { renderHook, settle } from "../../test/hooks";
import {
  blockDraft,
  closeBlockWindow,
  openBlockWindow,
  openWindowOf,
  setBlockDraft,
  toggleBlockWindow,
  useBlockWindow,
  useBlockWindowPage,
  useOpenBlockWindow,
} from "./blockWindows";

test("one_window_at_a_time_per_editor_held_by_block_id", () => {
  const editor = {};
  const other = {};
  expect(openWindowOf(editor)).toBeNull();
  openBlockWindow(editor, { id: "a", type: "bookmark" });
  expect(openWindowOf(editor)).toEqual({ blockId: "a", kind: "bookmark", returnTo: "anchor" });
  // Un autre éditeur (l'aperçu d'une version, un autre onglet du test) n'est pas touché.
  expect(openWindowOf(other)).toBeNull();
  openBlockWindow(editor, { id: "b", type: "bookmark" }, "editor");
  expect(openWindowOf(editor)).toEqual({ blockId: "b", kind: "bookmark", returnTo: "editor" });
  closeBlockWindow(editor);
  expect(openWindowOf(editor)).toBeNull();
});

test("toggling_the_open_block_closes_it_and_another_opens_it", () => {
  const editor = {};
  toggleBlockWindow(editor, { id: "a", type: "bookmark" });
  expect(openWindowOf(editor)?.blockId).toBe("a");
  toggleBlockWindow(editor, { id: "b", type: "bookmark" });
  expect(openWindowOf(editor)?.blockId).toBe("b");
  toggleBlockWindow(editor, { id: "b", type: "bookmark" });
  expect(openWindowOf(editor)).toBeNull();
});

test("drafts_live_by_block_and_by_kind_until_cleared", () => {
  const editor = {};
  expect(blockDraft(editor, "a", "bookmark")).toBeUndefined();
  setBlockDraft(editor, "a", "bookmark", "https://exem");
  expect(blockDraft(editor, "a", "bookmark")).toBe("https://exem");
  // Fermer la fenêtre ne touche pas au brouillon.
  openBlockWindow(editor, { id: "a", type: "bookmark" });
  closeBlockWindow(editor);
  expect(blockDraft(editor, "a", "bookmark")).toBe("https://exem");
  // Un autre bloc, une autre sorte de fenêtre, un autre éditeur : rien.
  expect(blockDraft(editor, "b", "bookmark")).toBeUndefined();
  expect(blockDraft(editor, "a", "equation")).toBeUndefined();
  expect(blockDraft({}, "a", "bookmark")).toBeUndefined();
  setBlockDraft(editor, "a", "bookmark", undefined);
  expect(blockDraft(editor, "a", "bookmark")).toBeUndefined();
});

test("hooks_follow_the_open_window_without_being_rendered_again_by_hand", async () => {
  const editor = {};
  const open = await renderHook(() => useOpenBlockWindow(editor), undefined);
  const mine = await renderHook((id: string) => useBlockWindow(editor, id), "a");
  expect(open.current).toBeNull();
  expect(mine.current).toBe(false);
  await settle(() => openBlockWindow(editor, { id: "a", type: "bookmark" }));
  expect(open.current?.blockId).toBe("a");
  expect(mine.current).toBe(true);
  await settle(() => openBlockWindow(editor, { id: "b", type: "bookmark" }));
  expect(open.current?.blockId).toBe("b");
  expect(mine.current).toBe(false);
  await settle(() => closeBlockWindow(editor));
  expect(open.current).toBeNull();
});

test("editors_of_one_page_share_its_window_and_drafts_until_the_page_is_left", async () => {
  // Une session recréée apporte un éditeur neuf : la fenêtre et les brouillons de la page passent à
  // lui ; la dernière vue de la page partie, ils partent (R471).
  const first = {};
  const second = {};
  const view = await renderHook((editor: object) => useBlockWindowPage(editor, "page-x"), first);
  openBlockWindow(first, { id: "a", type: "bookmark" });
  setBlockDraft(first, "a", "bookmark", "https://exem");
  await view.rerender(second);
  await settle(() => new Promise((done) => setTimeout(done, 0)));
  expect(openWindowOf(second)?.blockId).toBe("a");
  expect(blockDraft(second, "a", "bookmark")).toBe("https://exem");
  // Un éditeur d'une autre page n'en voit rien.
  const elsewhere = {};
  await renderHook((editor: object) => useBlockWindowPage(editor, "page-y"), elsewhere);
  expect(openWindowOf(elsewhere)).toBeNull();
  // La page quittée : plus rien.
  await view.unmount();
  await settle(() => new Promise((done) => setTimeout(done, 0)));
  const later = {};
  await renderHook((editor: object) => useBlockWindowPage(editor, "page-x"), later);
  expect(openWindowOf(later)).toBeNull();
  expect(blockDraft(later, "a", "bookmark")).toBeUndefined();
});
