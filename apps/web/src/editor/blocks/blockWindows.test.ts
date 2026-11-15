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
  useOpenBlockWindow,
} from "./blockWindows";

test("one_window_at_a_time_per_editor_held_by_block_id", () => {
  const editor = {};
  const other = {};
  expect(openWindowOf(editor)).toBeNull();
  openBlockWindow(editor, "a");
  expect(openWindowOf(editor)).toEqual({ blockId: "a", returnTo: "anchor" });
  // Un autre éditeur (l'aperçu d'une version, un autre onglet du test) n'est pas touché.
  expect(openWindowOf(other)).toBeNull();
  openBlockWindow(editor, "b", "editor");
  expect(openWindowOf(editor)).toEqual({ blockId: "b", returnTo: "editor" });
  closeBlockWindow(editor);
  expect(openWindowOf(editor)).toBeNull();
});

test("toggling_the_open_block_closes_it_and_another_opens_it", () => {
  const editor = {};
  toggleBlockWindow(editor, "a");
  expect(openWindowOf(editor)?.blockId).toBe("a");
  toggleBlockWindow(editor, "b");
  expect(openWindowOf(editor)?.blockId).toBe("b");
  toggleBlockWindow(editor, "b");
  expect(openWindowOf(editor)).toBeNull();
});

test("drafts_live_by_block_and_by_kind_until_cleared", () => {
  const editor = {};
  expect(blockDraft(editor, "a", "bookmark")).toBeUndefined();
  setBlockDraft(editor, "a", "bookmark", "https://exem");
  expect(blockDraft(editor, "a", "bookmark")).toBe("https://exem");
  // Fermer la fenêtre ne touche pas au brouillon.
  openBlockWindow(editor, "a");
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
  await settle(() => openBlockWindow(editor, "a"));
  expect(open.current?.blockId).toBe("a");
  expect(mine.current).toBe(true);
  await settle(() => openBlockWindow(editor, "b"));
  expect(open.current?.blockId).toBe("b");
  expect(mine.current).toBe(false);
  await settle(() => closeBlockWindow(editor));
  expect(open.current).toBeNull();
});
