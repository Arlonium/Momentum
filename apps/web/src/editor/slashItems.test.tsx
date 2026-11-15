// Le menu « / » propose les blocs riches, dans leur groupe, après ceux de BlockNote. Choisir l'un
// d'eux fait le geste de BlockNote (R308) : dans un bloc vide (ou qui ne porte que « / »), le bloc
// devient celui qu'on a choisi ; ailleurs, il est inséré après. Un signet ouvre sa fenêtre.
import { filterSuggestionItems } from "@blocknote/core";
import { expect, test } from "vitest";
import { cursor, storedProps, stored } from "../test/editors";
import { blockWindow, flush, mountEditor } from "../test/richEditor";
import { RICH_GROUP, richSlashItems, slashMenuItems } from "./slashItems";

const PAGE = [
  { id: "x", type: "paragraph" as const, content: "Du texte" },
  { id: "e", type: "paragraph" as const, content: "" },
  { id: "z", type: "paragraph" as const, content: "Après" },
];

test("slash_menu_offers_the_rich_blocks_in_order", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const all = slashMenuItems(m.editor);
  const rich = all.filter((item) => item.group === RICH_GROUP);
  expect(rich.map((item) => item.title)).toEqual(["Callout", "Signet"]);
  expect(richSlashItems(m.editor).map((item) => item.title)).toEqual(["Callout", "Signet"]);
  // Le groupe vient après ceux de BlockNote, d'un seul tenant.
  const groups = [...new Set(all.map((item) => item.group))];
  expect(groups[groups.length - 1]).toBe(RICH_GROUP);
  const first = all.findIndex((item) => item.group === RICH_GROUP);
  expect(all.slice(first).every((item) => item.group === RICH_GROUP)).toBe(true);
  // Des noms en français et en anglais : chacun retrouve son bloc.
  const found = (query: string) => filterSuggestionItems(rich, query).map((item) => item.title);
  for (const query of ["callout", "encadré", "encadre", "note", "astuce"]) expect(found(query), query).toContain("Callout");
  for (const query of ["signet", "bookmark", "lien", "link", "url"]) expect(found(query), query).toContain("Signet");
  // Chaque élément a son icône et sa ligne d'explication.
  for (const item of rich) {
    expect(item.icon, item.title).toBeTruthy();
    expect(item.subtext, item.title).toBeTruthy();
  }
});

test("a_rich_block_chosen_in_an_empty_block_replaces_it_and_after_text_is_inserted_after", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const callout = () => richSlashItems(m.editor).find((item) => item.title === "Callout")!;
  // Dans le paragraphe vide : il devient un callout, au même id (le geste de BlockNote, R308).
  cursor(m, "e", 0);
  callout().onItemClick();
  await flush();
  expect(stored(m.doc).map((b) => `${b.id} ${b.type}`)).toEqual(["x paragraph", "e callout", "z paragraph"]);
  expect(m.editor.getTextCursorPosition().block.id).toBe("e");
  // Dans un bloc qui a du texte : un callout est inséré après lui.
  cursor(m, "x", 3);
  callout().onItemClick();
  await flush();
  const blocks = stored(m.doc);
  expect(blocks.map((b) => b.type)).toEqual(["paragraph", "callout", "callout", "paragraph"]);
  expect(blocks[0]).toMatchObject({ id: "x", text: "Du texte" });
  expect(m.editor.getTextCursorPosition().block.id).toBe(blocks[1].id);
  expect(storedProps(m.doc, blocks[1].id)).toMatchObject({ icon: "💡", color: "default" });
});

test("a_bookmark_is_inserted_empty_and_opens_its_window", async () => {
  const m = await mountEditor({ blocks: PAGE });
  cursor(m, "x", 3);
  richSlashItems(m.editor).find((item) => item.title === "Signet")!.onItemClick();
  await flush();
  const blocks = stored(m.doc);
  expect(blocks.map((b) => b.type)).toEqual(["paragraph", "bookmark", "paragraph", "paragraph"]);
  expect(storedProps(m.doc, blocks[1].id).url ?? "").toBe("");
  const dialog = blockWindow("Adresse du signet");
  expect(dialog, "la fenêtre du signet inséré").not.toBeNull();
  expect(document.activeElement).toBe(dialog!.querySelector("input"));
});
