// Dupliquer des blocs (Task C5, R253) : Ctrl+D (Cmd+D) et « Dupliquer » dans le menu de la
// poignée. Une copie ne reprend aucun identifiant de bloc, à aucune profondeur : avec celui de
// l'original, le dédoublonnage du serveur la supprimerait (preuve 5 du plan). Ce qu'une copie
// garde est relu dans le document (Yjs), pas seulement dans l'éditeur : un éditeur donne un id neuf
// à un bloc dont l'id revient, sans l'écrire (`stored`, `test/editors.ts`). Ce que le geste fait à
// la frappe d'un autre est mesuré dans `gestures.test.ts` (« dupliquer un bloc »).
import { SideMenuExtension } from "@blocknote/core/extensions";
import { NodeSelection } from "prosemirror-state";
import { act, createElement } from "react";
import { afterEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { cursorOf, elementIds, peer, seedState, selectBlock, selectContent, selectFrom, shown, X, XYZ, Y_, Z, block, type Peer, type Seed } from "../test/editors";
import { until } from "../test/hooks";
import { flush, mountEditor, type Mounted } from "../test/richEditor";
import { FRAGMENT } from "./collab";
import type { MementumEditor } from "./debug";
import {
  duplicateBlocks,
  isDuplicateShortcut,
  MAX_DUPLICATED_BLOCKS,
  selectedBlockIds,
  TOO_MANY_BLOCKS,
  type DuplicateKey,
} from "./duplicate";
import { dismissNotice } from "./editorNotice";
import { EditorView } from "./Editor";
import allBlocks from "./fixtures/all-blocks.input.json";
import nonDefault from "./fixtures/non-default.input.json";

type Block = MementumEditor["document"][number];

afterEach(() => dismissNotice("mm-copy-refused"));

/** Une page de départ ouverte par Mona, seule. */
async function alone(blocks: Seed): Promise<Peer> {
  return peer("Mona", 2, await seedState(blocks));
}

/** Les ids d'un bloc et de tout ce qu'il contient (enfants, colonnes), dans l'ordre. */
function idsOf(b: Block): string[] {
  return [b.id, ...b.children.flatMap(idsOf)];
}

/** `b` sans aucun id de bloc, à aucune profondeur : ce qu'une copie doit porter. */
function stripped(b: Block): unknown {
  const { id: _id, children, ...rest } = b;
  return { ...rest, children: children.map(stripped) };
}

/** Les ids que le document porte plus d'une fois (`elementIds` joint un double par « ; »). */
function repeatedInDoc(doc: Y.Doc): string[] {
  return [...elementIds(doc)].filter(([, value]) => value.includes(" ; ")).map(([id]) => id);
}

// ---------------------------------------------------------------------------
// Des ids neufs, à chaque profondeur
// ---------------------------------------------------------------------------

/** Un bloc à enfants et petits-enfants, une liste de colonnes, un tableau. */
const DEEP: Seed = [
  {
    id: "a",
    type: "paragraph",
    content: "Bloc A",
    children: [
      {
        id: "a1",
        type: "bulletListItem",
        content: "Enfant A1",
        children: [{ id: "a11", type: "checkListItem", props: { checked: true }, content: "Petit-enfant A11" }],
      },
    ],
  },
  {
    id: "cols",
    type: "columnList",
    children: [
      { id: "col1", type: "column", children: [{ id: "c1", type: "paragraph", content: "Colonne 1" }] },
      {
        id: "col2",
        type: "column",
        children: [
          { id: "c2", type: "heading", props: { level: 2 }, content: "Colonne 2" },
          { id: "c3", type: "paragraph", content: "Suite" },
        ],
      },
    ],
  },
  {
    id: "t",
    type: "table",
    content: {
      type: "tableContent",
      rows: [{ cells: ["Nom", "Rôle"] }, { cells: ["Mona", "Autrice"] }],
    },
  },
  { id: "z", type: "paragraph", content: "Bloc Z" },
] as Seed;

test.each([
  { what: "un bloc à enfants et petits-enfants", id: "a" },
  { what: "une liste de colonnes", id: "cols" },
  { what: "un tableau", id: "t" },
])("duplicated_block_has_fresh_ids_at_every_depth ($what)", async ({ id }) => {
  const mona = await alone(DEEP);
  const original = mona.editor.getBlock(id)!;
  const before = new Set(elementIds(mona.doc).keys());
  const made = duplicateBlocks(mona.editor, [id]);
  expect(made, "une copie").toHaveLength(1);
  const copy = mona.editor.getBlock(made[0])!;
  // Dans l'éditeur : la copie suit l'original, et porte ce qu'il porte, hors ids.
  const top = mona.editor.document.map((b) => b.id);
  expect(top.indexOf(made[0])).toBe(top.indexOf(id) + 1);
  expect(stripped(copy)).toEqual(stripped(original));
  // Dans le document : aucun id deux fois, et les ids neufs sont exactement ceux de la copie, à toutes
  // les profondeurs (blocs, colonnes, liste de colonnes).
  expect(repeatedInDoc(mona.doc)).toEqual([]);
  const fresh = [...elementIds(mona.doc).keys()].filter((known) => !before.has(known));
  expect(fresh.sort()).toEqual(idsOf(copy).sort());
  expect(idsOf(copy).length, "autant de blocs que l'original").toBe(idsOf(original).length);
});

// ---------------------------------------------------------------------------
// Une copie garde tout, hors les ids
// ---------------------------------------------------------------------------

/**
 * Un élément Yjs tel quel, sans l'id des blocs (conteneur, liste de colonnes, colonne) : ses
 * attributs, et ses enfants ; un texte, par ses morceaux et leurs marques. L'`id` d'une mention reste.
 */
function shape(node: Y.XmlElement | Y.XmlText): unknown {
  if (node instanceof Y.XmlText) return node.toDelta();
  const attributes: Record<string, unknown> = { ...node.getAttributes() };
  if (["blockContainer", "columnList", "column"].includes(node.nodeName)) delete attributes.id;
  return { name: node.nodeName, attributes, children: node.toArray().map((child) => shape(child as Y.XmlElement)) };
}

test.each([
  { fixture: "all-blocks", blocks: allBlocks },
  { fixture: "non-default", blocks: nonDefault },
])("duplicated_blocks_keep_everything_but_their_ids ($fixture)", async ({ blocks }) => {
  const mona = await alone(blocks as Seed);
  const originals = mona.editor.document.map((b) => b.id);
  const made = duplicateBlocks(mona.editor, originals);
  expect(made).toHaveLength(originals.length);
  // Le document porte chaque bloc puis, après le dernier, sa copie, élément pour élément, attribut
  // pour attribut, texte et marques compris.
  const group = mona.doc.getXmlFragment(FRAGMENT).get(0) as Y.XmlElement;
  const nodes = group.toArray() as Y.XmlElement[];
  expect(nodes).toHaveLength(2 * originals.length);
  for (let i = 0; i < originals.length; i += 1) {
    expect(shape(nodes[originals.length + i]), `la copie du bloc ${i + 1}`).toEqual(shape(nodes[i]));
  }
  expect(repeatedInDoc(mona.doc)).toEqual([]);
  expect(mona.editor.document.slice(originals.length).map((b) => b.id)).toEqual(made);
});

test("a_copy_keeps_the_page_its_mention_names_and_the_address_of_its_image", async () => {
  const mona = await alone([
    {
      id: "m",
      type: "paragraph",
      content: [
        "Voir ",
        { type: "mention", props: { kind: "page", id: "0193a5b0-0000-7000-8000-000000000001", label: "Plan" } },
        " et ",
        { type: "math", props: { latex: "x^2" } },
      ],
    },
    { id: "i", type: "image", props: { url: "/api/v1/pages/p/files/f", name: "photo.png", caption: "Légende" } },
  ] as Seed);
  const [mention, image] = duplicateBlocks(mona.editor, ["m", "i"]);
  const copied = mona.editor.getBlock(mention)!;
  expect(Array.isArray(copied.content) && copied.content[1]).toMatchObject({
    type: "mention",
    props: { kind: "page", id: "0193a5b0-0000-7000-8000-000000000001", label: "Plan" },
  });
  expect(mona.editor.getBlock(image)!.props).toMatchObject({ url: "/api/v1/pages/p/files/f", name: "photo.png", caption: "Légende" });
});

// ---------------------------------------------------------------------------
// Plusieurs blocs, la place, le curseur
// ---------------------------------------------------------------------------

test("several_selected_blocks_are_copied_in_order_after_the_last", async () => {
  const mona = await alone(XYZ);
  // La sélection, du texte de X à celui de Y : les deux blocs, dans l'ordre.
  selectFrom(mona, "x", "y");
  expect(selectedBlockIds(mona.editor)).toEqual(["x", "y"]);
  const [x2, y2] = duplicateBlocks(mona.editor, selectedBlockIds(mona.editor));
  expect(shown(mona)).toEqual([X, Y_, block(x2, "Bloc X"), block(y2, "Bloc Y"), Z]);
  // Un id répété est copié une fois ; un id qui n'est plus dans la page est passé.
  const again = duplicateBlocks(mona.editor, ["z", "z", "parti"]);
  expect(again).toHaveLength(1);
  expect(shown(mona).map((b) => b.text)).toEqual(["Bloc X", "Bloc Y", "Bloc X", "Bloc Y", "Bloc Z", "Bloc Z"]);
  // Rien à copier : rien n'est écrit.
  const writes = mona.writes.length;
  expect(duplicateBlocks(mona.editor, ["parti"])).toEqual([]);
  expect(mona.writes).toHaveLength(writes);
});

test("a_text_cursor_or_a_clicked_image_names_its_own_block", async () => {
  const mona = await alone([...XYZ, { id: "i", type: "image", props: { url: "https://exemple.fr/a.png" } }] as Seed);
  mona.editor.setTextCursorPosition("y", "end");
  expect(selectedBlockIds(mona.editor)).toEqual(["y"]);
  // Un clic sur une image la sélectionne comme un nœud (`selectContent`).
  selectContent(mona, "i");
  expect(mona.editor.prosemirrorState.selection).toBeInstanceOf(NodeSelection);
  expect(selectedBlockIds(mona.editor)).toEqual(["i"]);
});

test("cursor_goes_to_the_first_copy", async () => {
  const mona = await alone(DEEP);
  mona.editor.setTextCursorPosition("z", "start");
  // Un bloc à texte : à la fin de son texte.
  const [a2] = duplicateBlocks(mona.editor, ["a", "t"]);
  expect(cursorOf(mona)).toEqual({ id: a2, offset: "Bloc A".length });
  // Une liste de colonnes, qui n'a pas de texte à elle : à la fin du dernier texte qu'elle porte (la
  // fin de la copie, comme la pose BlockNote).
  const [cols2] = duplicateBlocks(mona.editor, ["cols"]);
  const lastInCopy = mona.editor.getBlock(cols2)!.children[1].children[1].id;
  expect(cursorOf(mona)).toEqual({ id: lastInCopy, offset: "Suite".length });
});

test("ctrl_z_undoes_the_whole_copy", async () => {
  const mona = await alone(DEEP);
  duplicateBlocks(mona.editor, ["a", "cols"]);
  expect(mona.editor.document).toHaveLength(6);
  mona.editor.undo();
  expect(mona.editor.document.map((b) => b.id)).toEqual(["a", "cols", "t", "z"]);
});

// ---------------------------------------------------------------------------
// Le raccourci : la lettre tapée, Ctrl ou Cmd, ni Alt, ni Maj, ni AltGr
// ---------------------------------------------------------------------------

function key(init: Partial<Omit<DuplicateKey, "getModifierState">> & { altGraph?: boolean }): DuplicateKey {
  const { altGraph = false, ...rest } = init;
  return {
    key: "",
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...rest,
    getModifierState: (modifier: string) => modifier === "AltGraph" && altGraph,
  };
}

test("isDuplicateShortcut_takes_ctrl_d_and_cmd_d", () => {
  expect(isDuplicateShortcut(key({ key: "d", ctrlKey: true }))).toBe(true);
  expect(isDuplicateShortcut(key({ key: "d", metaKey: true }))).toBe(true);
  // Verrouillage des majuscules : la lettre est « D », sans Maj.
  expect(isDuplicateShortcut(key({ key: "D", ctrlKey: true }))).toBe(true);
});

test("isDuplicateShortcut_leaves_every_other_key_alone", () => {
  // Ctrl+Maj+D, Ctrl+Alt+D, Cmd+Option+D.
  expect(isDuplicateShortcut(key({ key: "D", ctrlKey: true, shiftKey: true }))).toBe(false);
  expect(isDuplicateShortcut(key({ key: "d", ctrlKey: true, altKey: true }))).toBe(false);
  expect(isDuplicateShortcut(key({ key: "∂", metaKey: true, altKey: true }))).toBe(false);
  // AltGr+D tape un caractère (« đ » sur un clavier croate), et se signale comme Ctrl+Alt sous
  // Windows ; même si la lettre rendue est « d ».
  expect(isDuplicateShortcut(key({ key: "đ", ctrlKey: true, altKey: true, altGraph: true }))).toBe(false);
  expect(isDuplicateShortcut(key({ key: "d", ctrlKey: true, altKey: true, altGraph: true }))).toBe(false);
  expect(isDuplicateShortcut(key({ key: "d", ctrlKey: true, altGraph: true }))).toBe(false);
  // « d » seul, ou une autre lettre avec Ctrl.
  expect(isDuplicateShortcut(key({ key: "d" }))).toBe(false);
  expect(isDuplicateShortcut(key({ key: "s", ctrlKey: true }))).toBe(false);
  // La touche marquée D d'un QWERTY tape une autre lettre sur un autre clavier (« e » en Dvorak,
  // « в » en russe) : ce n'est pas le raccourci. La position de la touche n'est pas lue.
  expect(isDuplicateShortcut({ ...key({ key: "e", ctrlKey: true }), code: "KeyD" } as DuplicateKey)).toBe(false);
  expect(isDuplicateShortcut(key({ key: "в", ctrlKey: true }))).toBe(false);
  // Ctrl+Cmd+D : « Rechercher » sur Mac.
  expect(isDuplicateShortcut(key({ key: "d", ctrlKey: true, metaKey: true }))).toBe(false);
});

// ---------------------------------------------------------------------------
// Dans l'éditeur de la page : le raccourci, le menu de la poignée
// ---------------------------------------------------------------------------

/** Ctrl+D (ou Cmd+D) reçu par `target`, par défaut le texte de l'éditeur. */
function ctrlD(m: Mounted, { meta = false, target }: { meta?: boolean; target?: Element } = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key: "d", ctrlKey: !meta, metaKey: meta, bubbles: true, cancelable: true });
  (target ?? m.editor.prosemirrorView.dom).dispatchEvent(event);
  return event;
}

test("ctrl_d_in_the_text_duplicates_and_keeps_the_key_from_the_browser", async () => {
  const m = await mountEditor({ blocks: XYZ });
  m.editor.focus();
  m.editor.setTextCursorPosition("y", "end");
  const event = ctrlD(m);
  expect(event.defaultPrevented).toBe(true);
  const page = m.editor.document.map((b) => b.id);
  expect(page).toHaveLength(4);
  expect(shown(m).map((b) => b.text)).toEqual(["Bloc X", "Bloc Y", "Bloc Y", "Bloc Z"]);
  expect(cursorOf(m)).toEqual({ id: page[2], offset: "Bloc Y".length });
  // Cmd+D, sur Mac.
  expect(ctrlD(m, { meta: true }).defaultPrevented).toBe(true);
  expect(m.editor.document).toHaveLength(5);
});

test("ctrl_d_is_left_to_the_browser_outside_the_text_of_an_editable_editor", async () => {
  const m = await mountEditor({ blocks: XYZ });
  m.editor.setTextCursorPosition("y", "end");
  // Le focus ailleurs (un champ hors de l'éditeur) : rien n'est pris, rien n'est copié, même si la
  // touche arrive au texte.
  const elsewhere = document.createElement("input");
  document.body.append(elsewhere);
  elsewhere.focus();
  expect(ctrlD(m).defaultPrevented).toBe(false);
  expect(ctrlD(m, { target: elsewhere }).defaultPrevented).toBe(false);
  // Un bouton posé dans la vue de l'éditeur, qui a le focus.
  const button = document.createElement("button");
  m.host.querySelector(".bn-container")!.append(button);
  button.focus();
  expect(ctrlD(m, { target: button }).defaultPrevented).toBe(false);
  expect(m.editor.document).toHaveLength(3);
  elsewhere.remove();
  button.remove();
  // Témoin : le focus dans le texte, la touche est prise.
  m.editor.focus();
  expect(ctrlD(m).defaultPrevented).toBe(true);
  expect(m.editor.document).toHaveLength(4);
});

test("ctrl_d_in_a_read_only_editor_is_left_to_the_browser", async () => {
  const m = await mountEditor({ blocks: XYZ, editable: false });
  act(() => {
    m.editor.isEditable = false;
  });
  // ProseMirror ne focalise pas une vue en lecture seule : son élément, si.
  m.editor.prosemirrorView.dom.focus();
  expect(ctrlD(m).defaultPrevented).toBe(false);
  expect(m.editor.document).toHaveLength(3);
  expect(m.writes).toEqual([]);
});

/** Les entrées du menu ouvert. */
function menuItems(): string[] {
  return [...document.querySelectorAll('[role="menuitem"]')].map((item) => item.textContent ?? "");
}

/**
 * Ouvre le menu de la poignée du bloc `id`, comme un survol (l'état du menu de côté de BlockNote) puis
 * un clic sur la poignée. Le menu de Mantine s'ouvre et se ferme en fondu : on attend que le
 * précédent soit parti, puis que celui-ci montre ses entrées, jusqu'à « Supprimer » (de BlockNote).
 */
async function openHandleMenu(m: Mounted, id: string): Promise<string[]> {
  await until(m.host, () => menuItems().length === 0, "le menu précédent fermé");
  const sideMenu = m.editor.getExtension(SideMenuExtension)!;
  act(() => sideMenu.store.setState({ show: true, referencePos: new DOMRect(), block: m.editor.getBlock(id)! }));
  await flush();
  const handle = m.host.ownerDocument.querySelector<HTMLElement>('[aria-label="Ouvrir le menu du bloc"]');
  expect(handle, "la poignée").not.toBeNull();
  act(() => handle!.click());
  await until(m.host, () => menuItems().includes("Supprimer"), "le menu de la poignée ouvert");
  return menuItems();
}

function menuItem(name: string): HTMLElement {
  const item = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((i) => i.textContent === name);
  if (!item) throw new Error(`aucune entrée « ${name} »`);
  return item;
}

test("duplicate_is_in_the_handle_menu_and_comment_only_for_who_can_comment", async () => {
  // Sans le droit de commenter : « Dupliquer », puis les entrées de BlockNote.
  const m = await mountEditor({ blocks: XYZ });
  expect(await openHandleMenu(m, "y")).toEqual(["Dupliquer", "Supprimer", "Couleurs"]);
  act(() => menuItem("Dupliquer").click());
  await flush();
  expect(shown(m).map((b) => b.text)).toEqual(["Bloc X", "Bloc Y", "Bloc Y", "Bloc Z"]);
  expect(cursorOf(m)?.id).toBe(m.editor.document[2].id);
  // Le menu fermé, le focus est au texte de l'éditeur (BlockNote l'y rend), où va la frappe.
  await until(m.host, () => menuItems().length === 0, "le menu fermé");
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
});

test("comment_comes_first_in_the_handle_menu_for_who_can_comment", async () => {
  const commented: string[] = [];
  const m = await mountEditor({
    blocks: XYZ,
    view: (editor) => createElement(EditorView, { editor, editable: true, onComment: (id: string) => commented.push(id) }),
  });
  expect(await openHandleMenu(m, "x")).toEqual(["Commenter", "Dupliquer", "Supprimer", "Couleurs"]);
  act(() => menuItem("Commenter").click());
  await flush();
  expect(commented).toEqual(["x"]);
});

test("duplicate_from_the_handle_copies_the_selection_that_holds_its_block", async () => {
  const m = await mountEditor({ blocks: XYZ });
  m.editor.focus();
  selectFrom(m, "x", "y");
  await openHandleMenu(m, "y");
  act(() => menuItem("Dupliquer").click());
  await flush();
  expect(shown(m).map((b) => b.text)).toEqual(["Bloc X", "Bloc Y", "Bloc X", "Bloc Y", "Bloc Z"]);
  // Hors de la sélection, le bloc de la poignée seul.
  selectFrom(m, "x", "y");
  await openHandleMenu(m, "z");
  act(() => menuItem("Dupliquer").click());
  await flush();
  expect(shown(m).map((b) => b.text)).toEqual(["Bloc X", "Bloc Y", "Bloc X", "Bloc Y", "Bloc Z", "Bloc Z"]);
});

// ---------------------------------------------------------------------------
// Des blocs qui ne sont pas frères : la copie remonte à leur parent commun (R640)
// ---------------------------------------------------------------------------

/** X, une liste de deux colonnes (C1, C2), Z. */
const COLUMNS: Seed = [
  { id: "x", type: "paragraph", content: "Bloc X" },
  {
    id: "cols",
    type: "columnList",
    children: [
      { id: "col1", type: "column", children: [{ id: "c1", type: "paragraph", content: "Colonne 1" }] },
      { id: "col2", type: "column", children: [{ id: "c2", type: "paragraph", content: "Colonne 2" }] },
    ],
  },
  { id: "z", type: "paragraph", content: "Bloc Z" },
] as Seed;

test.each([
  {
    what: "de la colonne 1 à la colonne 2",
    last: "c2",
    // BlockNote rend le bloc de départ, puis la colonne voisine.
    selected: ["c1", "col2"],
    // Une colonne devient sa liste de colonnes : la liste entière est copiée.
    copied: ["cols"],
    page: (made: string[]) => ["x", "cols", made[0], "z"],
  },
  {
    what: "de la colonne 1 au paragraphe qui suit",
    last: "z",
    selected: ["c1", "col2", "z"],
    copied: ["cols", "z"],
    page: (made: string[]) => ["x", "cols", "z", made[0], made[1]],
  },
])("a_selection_leaving_a_column_is_duplicated ($what)", async ({ last, selected, copied, page }) => {
  const mona = await alone(COLUMNS);
  selectFrom(mona, "c1", last);
  expect(selectedBlockIds(mona.editor), "ce que rend la sélection de BlockNote").toEqual(selected);
  const made = duplicateBlocks(mona.editor, selectedBlockIds(mona.editor));
  expect(made).toHaveLength(copied.length);
  expect(mona.editor.document.map((b) => b.id)).toEqual(page(made));
  copied.forEach((id, i) => {
    expect(stripped(mona.editor.getBlock(made[i])!), `la copie de ${id}`).toEqual(stripped(mona.editor.getBlock(id)!));
  });
  expect(repeatedInDoc(mona.doc)).toEqual([]);
});

test("a_block_in_a_column_or_under_another_is_copied_where_it_is", async () => {
  // Témoin de R640 : des frères ne remontent pas. Un bloc d'une colonne est copié dans sa colonne ;
  // un sous-bloc, sous son bloc parent.
  const mona = await alone([
    ...COLUMNS,
    { id: "p", type: "paragraph", content: "Parent", children: [{ id: "p1", type: "paragraph", content: "Enfant" }] },
  ] as Seed);
  const [c1b] = duplicateBlocks(mona.editor, ["c1"]);
  expect(mona.editor.getBlock("col1")!.children.map((b) => b.id)).toEqual(["c1", c1b]);
  const [p1b] = duplicateBlocks(mona.editor, ["p1"]);
  expect(mona.editor.getBlock("p")!.children.map((b) => b.id)).toEqual(["p1", p1b]);
  expect(mona.editor.document.map((b) => b.id)).toEqual(["x", "cols", "z", "p"]);
});

test("a_selection_from_a_child_to_a_later_block_copies_from_their_common_parent", async () => {
  const mona = await alone([
    { id: "x", type: "paragraph", content: "Bloc X", children: [{ id: "x1", type: "paragraph", content: "Enfant X1" }] },
    { id: "y", type: "paragraph", content: "Bloc Y" },
    { id: "z", type: "paragraph", content: "Bloc Z" },
  ] as Seed);
  selectFrom(mona, "x1", "y");
  expect(selectedBlockIds(mona.editor)).toEqual(["x1", "y"]);
  // X1 remonte à X, l'enfant direct du parent commun (la page) : X, son enfant compris, puis Y.
  const [x2, y2] = duplicateBlocks(mona.editor, selectedBlockIds(mona.editor));
  expect(mona.editor.document.map((b) => b.id)).toEqual(["x", "y", x2, y2, "z"]);
  expect(stripped(mona.editor.getBlock(x2)!)).toEqual(stripped(mona.editor.getBlock("x")!));
  expect(repeatedInDoc(mona.doc)).toEqual([]);
});

test("a_copy_that_blocknote_refuses_leaves_the_key_to_the_browser_and_closes_the_menu", async () => {
  const m = await mountEditor({ blocks: XYZ });
  m.editor.focus();
  m.editor.setTextCursorPosition("y", "end");
  const refuse = vi.spyOn(m.editor, "insertBlocks").mockImplementation(() => {
    throw new Error("copie refusée pour le test");
  });
  const writes = m.writes.length;
  // Ctrl+D : rien n'est copié, la touche n'est pas annulée, et l'erreur est dite, pas levée.
  expect(ctrlD(m).defaultPrevented).toBe(false);
  expect(m.editor.document).toHaveLength(3);
  expect(m.writes).toHaveLength(writes);
  expect(m.errors.some((line) => line.includes("copie refusée pour le test"))).toBe(true);
  // « Dupliquer » du menu de la poignée : le menu se ferme, et le focus revient au texte.
  await openHandleMenu(m, "y");
  act(() => menuItem("Dupliquer").click());
  await until(m.host, () => menuItems().length === 0, "le menu fermé");
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
  expect(m.editor.document).toHaveLength(3);
  refuse.mockRestore();
});

// ---------------------------------------------------------------------------
// Le coût : un parcours de la page, et au plus 1 000 blocs par geste (R641)
// ---------------------------------------------------------------------------

test("a_copy_reads_the_page_in_one_walk", async () => {
  // Garde : relire chaque bloc par son id (`getBlock`, `getParentBlock`) reparcourt la page à chaque
  // fois, et la copie de toute une page de 10 000 blocs figeait l'onglet 6 s de plus.
  const mona = await alone(DEEP);
  const getBlock = vi.spyOn(mona.editor, "getBlock");
  const getParentBlock = vi.spyOn(mona.editor, "getParentBlock");
  expect(duplicateBlocks(mona.editor, ["a", "cols", "t", "z"])).toHaveLength(4);
  expect(getBlock).not.toHaveBeenCalled();
  expect(getParentBlock).not.toHaveBeenCalled();
});

/** `n` blocs, chacun avec un enfant : `2 × n` blocs. */
function parents(n: number): Seed {
  return Array.from({ length: n }, (_, i) => ({
    id: `p${i}`,
    type: "paragraph",
    content: `Bloc ${i}`,
    children: [{ id: `e${i}`, type: "paragraph", content: `Enfant ${i}` }],
  })) as Seed;
}

/** Le message d'une copie refusée, ou `null`. */
function copyRefused(): string | null {
  return document.querySelector(".mm-copy-refused[role=alert]")?.textContent ?? null;
}

test(
  "more_than_1000_blocks_are_not_copied_and_it_is_said",
  async () => {
    expect(MAX_DUPLICATED_BLOCKS).toBe(1_000);
    const mona = await alone([...parents(MAX_DUPLICATED_BLOCKS / 2), { id: "z", type: "paragraph", content: "Bloc Z" }] as Seed);
    const all = mona.editor.document.map((b) => b.id);
    // 1 001 blocs, sous-blocs compris : rien n'est copié ni écrit, et le message le dit.
    const writes = mona.writes.length;
    expect(duplicateBlocks(mona.editor, all)).toEqual([]);
    expect(mona.writes).toHaveLength(writes);
    expect(mona.editor.document).toHaveLength(all.length);
    expect(copyRefused()).toBe(TOO_MANY_BLOCKS);
    dismissNotice("mm-copy-refused");
    // Témoin : 1 000 blocs (500 et leurs enfants) sont copiés, sans message.
    expect(duplicateBlocks(mona.editor, all.slice(0, -1))).toHaveLength(MAX_DUPLICATED_BLOCKS / 2);
    expect(copyRefused()).toBeNull();
  },
  60_000,
);

test(
  "ctrl_d_on_too_many_blocks_takes_the_key_and_says_why_nothing_is_copied",
  async () => {
    const m = await mountEditor({ blocks: [...parents(MAX_DUPLICATED_BLOCKS / 2), { id: "z", type: "paragraph", content: "Bloc Z" }] as Seed });
    m.editor.focus();
    m.editor.setSelection("p0", "z");
    const writes = m.writes.length;
    expect(ctrlD(m).defaultPrevented).toBe(true);
    expect(m.writes).toHaveLength(writes);
    expect(copyRefused()).toBe(TOO_MANY_BLOCKS);
    dismissNotice("mm-copy-refused");
  },
  60_000,
);

// ---------------------------------------------------------------------------
// Échap sur le menu de la poignée (R642)
// ---------------------------------------------------------------------------

test("escape_on_the_handle_menu_gives_the_focus_back_to_the_text", async () => {
  const m = await mountEditor({ blocks: XYZ });
  m.editor.focus();
  m.editor.setTextCursorPosition("y", "end");
  await openHandleMenu(m, "y");
  const item = menuItem("Dupliquer");
  act(() => item.focus());
  act(() => {
    item.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
  });
  await until(m.host, () => menuItems().length === 0, "le menu fermé");
  await until(m.host, () => document.activeElement !== document.body, "le focus quitte `body`");
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
  expect(m.editor.document).toHaveLength(3);
});

// Les ids de la sélection sont lus sans convertir ses blocs : `getSelection()` de BlockNote convertit
// chaque bloc choisi, sous-blocs compris (une page de 10 000 blocs sélectionnée : près d'une demi-
// seconde avant même de compter). Ce sont les mêmes ids que les siens.
test("selected_ids_are_those_of_the_blocknote_selection_read_without_converting_blocks", async () => {
  const mona = await alone([
    { id: "x", type: "paragraph", content: "Bloc X", children: [{ id: "x1", type: "paragraph", content: "Enfant X1" }, { id: "x2", type: "paragraph", content: "Enfant X2" }] },
    ...COLUMNS.slice(1, 2),
    { id: "y", type: "paragraph", content: "Bloc Y" },
    { id: "t", type: "table", content: { type: "tableContent", rows: [{ cells: ["A", "B"] }] } },
    { id: "i", type: "image", props: { url: "https://exemple.fr/a.png" } },
    { id: "z", type: "paragraph", content: "Bloc Z" },
  ] as Seed);
  const blockNote = () => mona.editor.getSelection()?.blocks.map((b) => b.id) ?? [mona.editor.getTextCursorPosition().block.id];
  const ours = () => {
    const getSelection = vi.spyOn(mona.editor, "getSelection");
    try {
      return selectedBlockIds(mona.editor);
    } finally {
      expect(getSelection, "lu sans `getSelection`").not.toHaveBeenCalled();
      getSelection.mockRestore();
    }
  };
  const pairs: [string, string][] = [
    ["x", "y"],
    ["x1", "y"],
    ["x", "x1"],
    ["x1", "x2"],
    ["x2", "c1"],
    ["c1", "c2"],
    ["c1", "y"],
    ["c2", "z"],
    ["y", "z"],
  ];
  for (const [first, last] of pairs) {
    selectFrom(mona, first, last);
    expect(ours(), `de ${first} à ${last}`).toEqual(blockNote());
  }
  // Un curseur, une image cliquée, un curseur dans un tableau : le bloc du curseur.
  mona.editor.setTextCursorPosition("x1", "end");
  expect(ours()).toEqual(["x1"]);
  selectContent(mona, "i");
  expect(ours()).toEqual(blockNote());
  // Un bloc entier sélectionné comme un nœud (son conteneur) : celui du curseur, comme pour BlockNote.
  selectBlock(mona, "y");
  expect(ours()).toEqual(blockNote());
  mona.editor.setTextCursorPosition("t", "start");
  expect(ours()).toEqual(["t"]);
});
