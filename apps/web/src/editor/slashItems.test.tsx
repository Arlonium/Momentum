// Le menu « / » propose les blocs riches, dans leur groupe, après ceux de BlockNote. Choisir l'un
// d'eux fait le geste de BlockNote (R308) : dans un bloc vide (ou qui ne porte que « / »), le bloc
// devient celui qu'on a choisi ; ailleurs, il est inséré après. Un signet ouvre sa fenêtre.
import { filterSuggestionItems } from "@blocknote/core";
import { expect, test } from "vitest";
import { cursor, storedProps, stored } from "../test/editors";
import { blockWindow, fill, flush, mountEditor, press } from "../test/richEditor";
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
  const order = ["Callout", "Équation", "Équation en ligne", "Signet", "Table des matières", "Lien vers une page"];
  expect(rich.map((item) => item.title)).toEqual(order);
  expect(richSlashItems(m.editor).map((item) => item.title)).toEqual(order);
  // Le groupe vient après ceux de BlockNote, d'un seul tenant.
  const groups = [...new Set(all.map((item) => item.group))];
  expect(groups[groups.length - 1]).toBe(RICH_GROUP);
  const first = all.findIndex((item) => item.group === RICH_GROUP);
  expect(all.slice(first).every((item) => item.group === RICH_GROUP)).toBe(true);
  // Des noms en français et en anglais : chacun retrouve son bloc.
  const found = (query: string) => filterSuggestionItems(rich, query).map((item) => item.title);
  for (const query of ["callout", "encadré", "encadre", "note", "astuce"]) expect(found(query), query).toContain("Callout");
  for (const query of ["signet", "bookmark", "lien", "link", "url"]) expect(found(query), query).toContain("Signet");
  for (const query of ["équation", "equation", "formule", "latex", "math"]) expect(found(query), query).toContain("Équation");
  for (const query of ["en ligne", "inline", "formule en ligne"]) expect(found(query), query).toContain("Équation en ligne");
  for (const query of ["table des matières", "matieres", "sommaire", "toc", "contents", "outline"]) {
    expect(found(query), query).toContain("Table des matières");
  }
  for (const query of ["lien vers une page", "page", "sous-page", "lien", "link"]) {
    expect(found(query), query).toContain("Lien vers une page");
  }
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

test("a_bookmark_set_from_the_slash_menu_gives_the_focus_back_to_the_text", async () => {
  // Posé depuis le texte, le signet rend le focus au texte, dans le paragraphe qui le suit : la
  // frappe reprend (R474).
  const m = await mountEditor({ blocks: PAGE });
  cursor(m, "x", 3);
  richSlashItems(m.editor).find((item) => item.title === "Signet")!.onItemClick();
  await flush();
  const field = blockWindow("Adresse du signet")!.querySelector("input")!;
  fill(field, "https://mementum.dev");
  field.form!.requestSubmit();
  await flush();
  expect(blockWindow("Adresse du signet")).toBeNull();
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
  const blocks = stored(m.doc);
  expect(blocks[1].type).toBe("bookmark");
  expect(m.editor.getTextCursorPosition().block.id).toBe(blocks[2].id);
});

test("an_equation_is_inserted_empty_and_opens_its_field_at_once", async () => {
  const m = await mountEditor({ blocks: PAGE });
  cursor(m, "x", 3);
  richSlashItems(m.editor).find((item) => item.title === "Équation")!.onItemClick();
  await flush();
  const blocks = stored(m.doc);
  expect(blocks.map((b) => b.type)).toEqual(["paragraph", "equation", "paragraph", "paragraph"]);
  expect(storedProps(m.doc, blocks[1].id).latex ?? "").toBe("");
  const dialog = blockWindow("Formule de l'équation");
  expect(dialog, "la fenêtre de l'équation insérée").not.toBeNull();
  expect(document.activeElement).toBe(dialog!.querySelector("textarea"));
  // Fermée, elle rend le focus au texte (R474).
  fill(dialog!.querySelector("textarea")!, "x^2");
  press(dialog!.querySelector("textarea")!, "Enter", { ctrlKey: true });
  await flush();
  expect(storedProps(m.doc, blocks[1].id).latex).toBe("x^2");
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
});

test("a_toc_is_inserted_by_the_slash_menu_and_the_typing_goes_on_after_it", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const toc = () => richSlashItems(m.editor).find((item) => item.title === "Table des matières")!;
  // Après un texte : insérée après lui ; le curseur passe au bloc qui la suit (R308).
  cursor(m, "x", 3);
  toc().onItemClick();
  await flush();
  expect(stored(m.doc).map((b) => b.type)).toEqual(["paragraph", "toc", "paragraph", "paragraph"]);
  expect(m.editor.getTextCursorPosition().block.id).toBe("e");
  // Dans un bloc vide : il la devient, au même id.
  toc().onItemClick();
  await flush();
  expect(stored(m.doc).map((b) => `${b.type}${b.id === "e" ? " e" : ""}`)).toEqual([
    "paragraph",
    "toc",
    "toc e",
    "paragraph",
  ]);
  expect(m.host.querySelectorAll('nav[aria-label="Table des matières"]')).toHaveLength(2);
});
