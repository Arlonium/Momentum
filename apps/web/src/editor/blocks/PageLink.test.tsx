// Le lien vers une page (Task C3, R251). Il porte l'id de la page et le titre copié quand on l'a
// posé. Qui voit la page (elle est dans l'arbre de l'espace affiché, `pageDirectory.ts`) lit son
// titre à jour ; qui ne la voit pas ne lit que le titre copié dans le document, jamais un titre
// venu d'ailleurs (preuve 4 du plan). « Lien vers une page » du menu « / » ouvre un sélecteur ; le
// bloc n'est inséré qu'au choix d'une page.
import { act } from "react";
import { afterEach, expect, test } from "vitest";
import { cursor, selectBlock, stored, storedProps, type Seed } from "../../test/editors";
import { page } from "../../test/pages";
import {
  blockOf,
  blockWindow,
  fill,
  flush,
  foreignStyles,
  mountEditor,
  press,
  renewSession,
  type Mounted,
} from "../../test/richEditor";
import { contrast } from "../../page/avatarInk";
import { publishPages } from "../../space/pageDirectory";
import { richSlashItems } from "../slashItems";

const PLAN = "0190f5a0-0000-7000-8000-0000000000a1";
const NOTES = "0190f5a0-0000-7000-8000-0000000000a2";
const PICKER = "Lien vers une page";

afterEach(() => publishPages(null, null));

function linkPage(pageId: unknown, title: unknown = "Plan"): Seed {
  return [
    { id: "x", type: "paragraph", content: "Avant" },
    { id: "l", type: "pageLink", props: { pageId: pageId as string, title: title as string } },
    { id: "z", type: "paragraph", content: "Après" },
  ];
}

/** Ce que montre le lien de page `l` : son texte, et l'adresse de son lien s'il en est un. */
function shownLink(m: Mounted): { text: string | null; href: string | null } {
  const view = blockOf(m, "l");
  const anchor = view.querySelector("a");
  return {
    text: view.querySelector(".mm-page-link-title")?.textContent ?? null,
    href: anchor?.getAttribute("href") ?? null,
  };
}

const publish = (...args: Parameters<typeof publishPages>) => act(async () => publishPages(...args));

test("link_to_a_page_the_reader_sees_shows_its_current_title", async () => {
  await publish("space", [page(PLAN, { title: "Plan 2027" }), page(NOTES, { title: "Notes" })]);
  const m = await mountEditor({ blocks: linkPage(PLAN) });
  expect(shownLink(m)).toEqual({ text: "Plan 2027", href: `/p/${PLAN}` });
  // Renommée ailleurs, l'arbre relu : le titre suit, sans que l'éditeur ou le bloc soient refaits.
  const view = blockOf(m, "l");
  const before = m.writes.length;
  await publish("space", [page(PLAN, { title: "Plan 2028" }), page(NOTES, { title: "Notes" })]);
  expect(shownLink(m)).toEqual({ text: "Plan 2028", href: `/p/${PLAN}` });
  expect(blockOf(m, "l")).toBe(view);
  // Rien n'est écrit dans le document : le titre copié reste celui qu'on a posé.
  expect(m.writes.length).toBe(before);
  expect(storedProps(m.doc, "l")).toMatchObject({ pageId: PLAN, title: "Plan" });
});

test("link_to_a_page_the_reader_cannot_see_shows_only_what_the_document_holds", async () => {
  // L'arbre du lecteur porte d'autres pages, dont une au titre que la page liée a pris depuis : rien
  // ne vient d'ailleurs que du document.
  await publish("space", [page(NOTES, { title: "Plan 2027" })]);
  const m = await mountEditor({ blocks: linkPage(PLAN, "Plan") });
  expect(shownLink(m)).toEqual({ text: "Plan", href: `/p/${PLAN}` });
  expect(m.text()).not.toContain("2027");
  // Témoin : la même page publiée, son titre à jour paraît.
  await publish("space", [page(NOTES, { title: "Notes" }), page(PLAN, { title: "Plan 2027" })]);
  expect(shownLink(m).text).toBe("Plan 2027");
  // L'accès retiré (la page quitte l'arbre), on revient au titre copié.
  await publish("space", [page(NOTES, { title: "Notes" })]);
  expect(shownLink(m).text).toBe("Plan");
  // Un titre copié vide : « Sans titre ».
  const empty = await mountEditor({ blocks: linkPage(PLAN, "") });
  expect(shownLink(empty).text).toBe("Sans titre");
});

test("page_link_without_a_page_is_not_a_link", async () => {
  // Une page publiée sous un id qui n'en est pas un : jamais lue pour un lien.
  await publish("space", [page("plan", { title: "Secret" }), page(PLAN, { title: "Plan" })]);
  for (const pageId of ["", "plan", `../${PLAN}`, `${PLAN}/x`, "javascript:alert(1)", { id: PLAN }, 42]) {
    const m = await mountEditor({ blocks: linkPage(pageId, "Copié") });
    expect(shownLink(m), JSON.stringify(pageId)).toEqual({ text: "Copié", href: null });
    expect(blockOf(m, "l").querySelector("a")).toBeNull();
  }
  // Témoin : un UUID est un lien.
  const m = await mountEditor({ blocks: linkPage(PLAN, "Copié") });
  expect(shownLink(m)).toEqual({ text: "Plan", href: `/p/${PLAN}` });
});

test("page_link_title_is_shown_as_text", async () => {
  const title = '<img src=x onerror="alert(1)"> ‮Été';
  const m = await mountEditor({ blocks: linkPage(PLAN, title) });
  expect(shownLink(m).text).toBe(title);
  const view = blockOf(m, "l");
  expect(view.querySelector("img")).toBeNull();
  expect(view.querySelector(".mm-page-link-title")?.getAttribute("dir")).toBe("auto");
  expect(foreignStyles(view)).toEqual([]);
  // Un titre qui n'est pas du texte (un autre client) est un titre vide.
  const odd = await mountEditor({ blocks: linkPage(PLAN, { a: 1 }) });
  expect(shownLink(odd).text).toBe("Sans titre");
});

test("ctrl_alt_enter_on_a_page_link_puts_the_focus_on_it", async () => {
  await publish("space", [page(PLAN, { title: "Plan" })]);
  const m = await mountEditor({ blocks: linkPage(PLAN) });
  act(() => {
    selectBlock(m, "l");
    m.editor.focus();
  });
  const event = press(m.editor.prosemirrorView.dom, "Enter", { ctrlKey: true, altKey: true });
  expect(event.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(blockOf(m, "l").querySelector("a"));
});

test("the_colours_of_the_page_link_are_readable", () => {
  // Le lien (accent) sur le cadre d'un bloc.
  expect(contrast("#23704F", "#FAFAF8")).toBeGreaterThanOrEqual(4.5);
});

// Le menu « / » : « Lien vers une page » ouvre le sélecteur ; le bloc vient au choix.

const PAGE: Seed = [
  { id: "x", type: "paragraph", content: "Du texte" },
  { id: "e", type: "paragraph", content: "" },
  { id: "z", type: "paragraph", content: "Après" },
];

function openPicker(m: Mounted): HTMLInputElement {
  act(() => richSlashItems(m.editor).find((item) => item.title === PICKER)!.onItemClick());
  const dialog = blockWindow(PICKER);
  expect(dialog, "le sélecteur").not.toBeNull();
  const field = dialog!.querySelector<HTMLInputElement>('input[role="combobox"]')!;
  expect(document.activeElement).toBe(field);
  return field;
}

const options = () => [...(blockWindow(PICKER)?.querySelectorAll('[role="option"]') ?? [])].map((o) => o.textContent);

test("page_link_is_inserted_once_a_page_is_picked", async () => {
  await publish("space", [page(PLAN, { title: "Plan" }), page(NOTES, { title: "Notes" })]);
  const m = await mountEditor({ blocks: PAGE });
  cursor(m, "x", 3);
  const field = openPicker(m);
  // Rien n'est écrit tant qu'on cherche.
  expect(m.writes).toEqual([]);
  expect(options()).toEqual(["Plan", "Notes"]);
  await act(async () => fill(field, "not"));
  expect(options()).toEqual(["Notes"]);
  await act(async () => void press(field, "Enter"));
  await flush();
  expect(blockWindow(PICKER)).toBeNull();
  // Après un texte : inséré après lui, avec l'id et le titre de la page ; le curseur passe au bloc
  // qui le suit, et l'éditeur a le focus (la frappe reprend dans le texte).
  const blocks = stored(m.doc);
  expect(blocks.map((b) => b.type)).toEqual(["paragraph", "pageLink", "paragraph", "paragraph"]);
  expect(storedProps(m.doc, blocks[1].id)).toMatchObject({ pageId: NOTES, title: "Notes" });
  expect(m.editor.getTextCursorPosition().block.id).toBe("e");
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
  // Dans un bloc vide : il le devient, au même id.
  const again = openPicker(m);
  await act(async () => void press(again, "Enter"));
  await flush();
  expect(stored(m.doc).map((b) => `${b.type}${b.id === "e" ? " e" : ""}`)).toEqual([
    "paragraph",
    "pageLink",
    "pageLink e",
    "paragraph",
  ]);
  expect(storedProps(m.doc, "e")).toMatchObject({ pageId: PLAN, title: "Plan" });
});

test("closing_the_picker_inserts_nothing_and_keeps_what_was_typed", async () => {
  await publish("space", [page(PLAN, { title: "Plan" }), page(NOTES, { title: "Notes" })]);
  const m = await mountEditor({ blocks: PAGE });
  cursor(m, "x", 3);
  const field = openPicker(m);
  await act(async () => fill(field, "pla"));
  await act(async () => void press(field, "Escape"));
  await flush();
  expect(blockWindow(PICKER)).toBeNull();
  expect(m.writes).toEqual([]);
  expect(stored(m.doc).map((b) => b.id)).toEqual(["x", "e", "z"]);
  // Le focus revient au texte, où était le curseur.
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
  expect(m.editor.getTextCursorPosition().block.id).toBe("x");
  // Rouvert, il retrouve ce qu'on y avait tapé.
  expect(openPicker(m).value).toBe("pla");
  expect(options()).toEqual(["Plan"]);
  // Un clic dehors le ferme aussi, sans rien écrire.
  await act(async () => {
    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
  });
  await flush();
  expect(blockWindow(PICKER)).toBeNull();
  expect(m.writes).toEqual([]);
  expect(document.activeElement).not.toBe(document.body);
});

test("the_picker_survives_a_new_session_and_writes_nothing_in_read_only", async () => {
  await publish("space", [page(PLAN, { title: "Plan" })]);
  const m = await mountEditor({ blocks: PAGE, pageId: "page-1" });
  cursor(m, "x", 3);
  const field = openPicker(m);
  await act(async () => fill(field, "pl"));
  // La page passe en lecture seule (une baisse de droits recrée la session, donc l'éditeur) : le
  // sélecteur reste, avec ce qu'on y a tapé et le focus, et le dit.
  const reader = await renewSession(m, { editable: false, pageId: "page-1" });
  const dialog = blockWindow(PICKER);
  expect(dialog, "le sélecteur reste").not.toBeNull();
  expect(document.activeElement).toBe(dialog!.querySelector("input"));
  expect(dialog!.querySelector<HTMLInputElement>("input")!.value).toBe("pl");
  expect(dialog!.querySelector('[role="status"]')?.textContent).toBe(
    "Tu ne peux pas modifier cette page pour l'instant : aucun lien ne sera posé.",
  );
  await act(async () => void press(dialog!.querySelector("input")!, "Enter"));
  await flush();
  expect(reader.writes).toEqual([]);
  expect(stored(reader.doc).map((b) => b.type)).toEqual(["paragraph", "paragraph", "paragraph"]);
  // Le droit revenu, le choix pose le lien.
  const writer = await renewSession(m, { editable: true, pageId: "page-1" });
  await act(async () => void press(blockWindow(PICKER)!.querySelector("input")!, "Enter"));
  await flush();
  expect(stored(writer.doc).map((b) => b.type)).toEqual(["paragraph", "pageLink", "paragraph", "paragraph"]);
});

test("a_link_picked_once_its_block_is_gone_goes_after_the_selected_block_and_the_typing_goes_on_after_it", async () => {
  await publish("space", [page(PLAN, { title: "Plan" })]);
  for (const after of [true, false]) {
    const m = await mountEditor({
      blocks: [
        { id: "x", type: "paragraph", content: "Du texte" },
        { id: "s", type: "bookmark", props: { url: "https://mementum.dev/" } },
        ...(after ? [{ id: "z", type: "paragraph" as const, content: "Après" }] : []),
      ],
    });
    cursor(m, "x", 3);
    const field = openPicker(m);
    // Le bloc d'où l'on a ouvert le sélecteur part (un autre l'a supprimé), et le curseur est sur le
    // signet, sélectionné.
    act(() => {
      m.editor.removeBlocks(["x"]);
      selectBlock(m, "s");
    });
    await act(async () => void press(field, "Enter"));
    await flush();
    // Le lien va après le signet ; le curseur passe au texte qui suit (un paragraphe neuf s'il n'y en a
    // pas) : la frappe ne remplace ni le lien, ni le signet.
    const blocks = stored(m.doc);
    expect(blocks.map((b) => b.type), after ? "un texte après" : "rien après").toEqual(["bookmark", "pageLink", "paragraph"]);
    expect(m.editor.getTextCursorPosition().block.id).toBe(blocks[2].id);
    if (after) expect(blocks[2].id).toBe("z");
  }
});

const pointerDown = (target: Element) =>
  act(() => void target.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));

test("a_click_anywhere_in_the_editor_closes_the_picker", async () => {
  await publish("space", [page(PLAN, { title: "Plan" })]);
  const m = await mountEditor({ blocks: PAGE });
  // Témoin : un clic dans le sélecteur le laisse ouvert.
  cursor(m, "e", 0);
  pointerDown(openPicker(m));
  expect(blockWindow(PICKER)).not.toBeNull();
  // Un clic dans un autre bloc le ferme ; dans le bloc d'où on l'a ouvert aussi.
  pointerDown(blockOf(m, "x").querySelector(".bn-inline-content")!);
  expect(blockWindow(PICKER), "un autre bloc").toBeNull();
  cursor(m, "e", 0);
  openPicker(m);
  pointerDown(blockOf(m, "e").querySelector(".bn-inline-content")!);
  expect(blockWindow(PICKER), "le bloc d'origine").toBeNull();
  // Ce bloc supprimé par un autre (la fenêtre se pose alors près de l'éditeur) : un clic dans
  // l'éditeur la ferme encore.
  cursor(m, "e", 0);
  openPicker(m);
  act(() => m.editor.removeBlocks(["e"]));
  await flush();
  expect(blockWindow(PICKER)).not.toBeNull();
  pointerDown(blockOf(m, "x").querySelector(".bn-inline-content")!);
  expect(blockWindow(PICKER), "le bloc parti").toBeNull();
});
