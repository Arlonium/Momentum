// Ce que l'éditeur d'une page fait d'un envoi de fichier (`uploads.ts`, tours de correction de la
// Task C8) : qui reçoit de quoi envoyer, et ce que devient le bloc d'un envoi qui n'aboutit pas : il
// reste, et perd le nom du fichier que l'envoi y avait mis (R685, récrit au tour 2).
import { act } from "react";
import { expect, test } from "vitest";
import { duo, exchange, stored, storedProps, tab, typeAtEnd, xml, type Seed } from "../test/editors";
import { mountEditor } from "../test/richEditor";
import { forgetUploadName, sessionDrop, sessionPaste, sessionUploader } from "./uploads";

const PAGE = "0192f0c1-0000-7000-8000-000000000001";
const PATH = `/api/v1/pages/${PAGE}/files/0192f0c1-0000-7000-8000-0000000000f1`;

const files = { onRefused: () => {}, reread: async () => undefined };
const session = (readOnly: boolean) => ({ readOnly, pageId: PAGE });

test("a_read_only_session_is_given_nothing_to_upload_with", () => {
  const signal = new AbortController().signal;
  const editor = () => null;
  // Une session en lecture seule (lecteur, corbeille, autre schéma) : rien, que la page donne de quoi ou non.
  expect(sessionUploader(session(true), () => files, editor, signal)).toBeUndefined();
  expect(sessionUploader(session(true), () => null, editor, signal)).toBeUndefined();
  // Une page qui ne donne rien pour envoyer (un test) : rien non plus.
  expect(sessionUploader(session(false), () => null, editor, signal)).toBeUndefined();
  // Témoin : une session qui écrit reçoit de quoi envoyer.
  expect(typeof sessionUploader(session(false), () => files, editor, signal)).toBe("function");
});

test("a_read_only_session_pastes_as_blocknote_does_and_copies_no_file", () => {
  // Le collage qui rattache les fichiers d'autres pages (Task C9) : jamais pour une session en lecture seule.
  const signal = new AbortController().signal;
  expect(sessionPaste(session(true), () => files, signal)).toBeUndefined();
  expect(sessionPaste(session(false), () => null, signal)).toBeUndefined();
  expect(typeof sessionPaste(session(false), () => files, signal)).toBe("function");  // De même pour le dépôt d'un bloc glissé d'un autre onglet (R714).
  expect(sessionDrop(session(true), () => files, () => null, signal)).toBeUndefined();
  expect(sessionDrop(session(false), () => null, () => null, signal)).toBeUndefined();
  expect(sessionDrop(session(false), () => files, () => null, signal)).toBeDefined();
});

/** Le bloc qu'un dépôt de `photo.png` a posé, en attente de son envoi. */
const dropped = { id: "f", type: "image", props: { name: "photo.png" } };
const page = (...blocks: unknown[]): Seed =>
  [{ id: "x", type: "paragraph", content: "Bloc X" }, ...blocks, { id: "z", type: "paragraph", content: "Bloc Z" }] as Seed;
type Mounted = Awaited<ReturnType<typeof mountEditor>>;
const ids = (m: Mounted) => m.editor.document.map((b) => b.id);
const propsOf = (m: Mounted, id: string) => m.editor.getBlock(id)!.props as Record<string, unknown>;

test("the_name_of_a_file_whose_upload_did_not_end_well_leaves_the_document_and_its_block_stays", async () => {
  for (const type of ["image", "video", "audio", "file"]) {
    const m = await mountEditor({ blocks: page({ ...dropped, type }), editable: true });
    const before = m.writes.length;
    await act(async () => forgetUploadName(m.editor, "f", "photo.png"));
    // Le bloc reste, vide, de la même sorte ; le nom local du fichier n'est plus dans le document.
    expect(ids(m), type).toEqual(["x", "f", "z"]);
    expect(m.editor.getBlock("f")!.type, type).toBe(type);
    expect(propsOf(m, "f").name, type).toBe("");
    expect(JSON.stringify(m.editor.document), type).not.toContain("photo.png");
    expect(storedProps(m.doc, "f").name, type).toBe("");
    expect(m.writes.length, type).toBeGreaterThan(before);
  }
});

// N1 de la re-relecture : retirer le bloc emportait ce qu'un autre y avait mis sans que l'onglet l'ait
// encore reçu. Mona et Tom, chacun son éditeur ; Tom agit sur le bloc de l'envoi de Mona, qui échoue
// avant qu'elle ait reçu ce que Tom a fait.
const ORDERS = [
  [5, 9],
  [9, 5],
] as const;

test("a_block_another_put_under_the_block_of_the_upload_meanwhile_is_kept", async () => {
  for (const [mona, tom] of ORDERS) {
    const s = await duo(page(dropped), mona, tom);
    typeAtEnd(s.tom, "z", " tapé par Tom");
    tab(s.tom, "z");
    forgetUploadName(s.mona.editor, "f", "photo.png");
    await exchange(s.server, [s.mona, s.tom]);
    for (const doc of [s.server, s.mona.doc, s.tom.doc]) {
      expect(xml(doc), `${mona} ${tom}`).toContain("Bloc Z tapé par Tom");
      const block = stored(doc).find((b) => b.id === "f")!;
      expect(block.children.map((child) => child.id), `${mona} ${tom}`).toEqual(["z"]);
      expect(storedProps(doc, "f").name, `${mona} ${tom}`).toBe("");
    }
  }
});

test("a_caption_or_an_address_another_set_meanwhile_is_kept", async () => {
  for (const [mona, tom] of ORDERS) {
    for (const props of [{ caption: "Vue du port" }, { url: PATH }]) {
      const s = await duo(page(dropped), mona, tom);
      s.tom.editor.updateBlock("f", { props } as never);
      forgetUploadName(s.mona.editor, "f", "photo.png");
      await exchange(s.server, [s.mona, s.tom]);
      for (const doc of [s.server, s.mona.doc, s.tom.doc]) {
        expect(storedProps(doc, "f"), `${mona} ${tom}`).toMatchObject({ ...props, name: "" });
      }
    }
  }
});

test("only_the_name_this_upload_put_there_is_taken_off", async () => {
  // Un bloc qui était là avant l'envoi garde ce qu'il portait (sa légende, sa sorte, ses sous-blocs),
  // sauf un nom qui est celui du fichier : rien ne dit s'il y était avant l'envoi, et sans adresse il
  // ne nomme rien.
  const m = await mountEditor({
    blocks: page({
      ...dropped,
      props: { name: "photo.png", caption: "Vue du port" },
      children: [{ id: "c", type: "paragraph", content: "Dessous" }],
    }),
    editable: true,
  });
  await act(async () => forgetUploadName(m.editor, "f", "photo.png"));
  expect(propsOf(m, "f")).toMatchObject({ name: "", caption: "Vue du port", url: "" });
  expect(m.editor.getBlock("f")!.children.map((child) => child.id)).toEqual(["c"]);
});

test("a_block_that_is_not_as_the_upload_left_it_keeps_its_name", async () => {
  const kept: [what: string, block: Record<string, unknown>, name: unknown][] = [
    // Un autre envoi a abouti entre-temps (ou l'adresse a été intégrée) : le nom est celui du fichier montré.
    ["il a reçu une adresse", { ...dropped, props: { name: "photo.png", url: PATH } }, "photo.png"],
    ["son nom n'est pas celui du fichier", { ...dropped, props: { name: "autre.png" } }, "autre.png"],
  ];
  for (const [what, block, name] of kept) {
    const m = await mountEditor({ blocks: page(block), editable: true });
    const before = m.writes.length;
    await act(async () => forgetUploadName(m.editor, "f", "photo.png"));
    expect(propsOf(m, "f").name, what).toBe(name);
    expect(m.writes.length, what).toBe(before);
  }
  // Un bloc changé en autre chose, ou parti : rien, et aucune erreur.
  const other = await mountEditor({ blocks: page({ id: "f", type: "paragraph", content: "photo.png" }), editable: true });
  const gone = await mountEditor({ blocks: page(dropped), editable: true });
  const writes = [other.writes.length, gone.writes.length];
  await act(async () => {
    forgetUploadName(other.editor, "f", "photo.png");
    forgetUploadName(gone.editor, "absent", "photo.png");
  });
  expect([other.writes.length, gone.writes.length]).toEqual(writes);
  expect(propsOf(gone, "f").name).toBe("photo.png");
  expect([...other.errors, ...gone.errors]).toEqual([]);
});

test("an_editor_that_no_longer_writes_takes_no_name_off", async () => {
  const reader = await mountEditor({ blocks: page(dropped), editable: false });
  const before = reader.writes.length;
  await act(async () => forgetUploadName(reader.editor, "f", "photo.png"));
  expect(propsOf(reader, "f").name).toBe("photo.png");
  expect(reader.writes.length).toBe(before);
});

test("ctrl_z_after_a_name_was_taken_off_brings_it_back_then_takes_the_block_away", async () => {
  // L'annulation de Yjs groupe ce qui s'écrit en moins de 500 ms : chaque geste attend que ce délai
  // soit passé (une durée, pas une condition : c'est le délai lui-même qui est attendu).
  const apart = () => new Promise((done) => setTimeout(done, 600));
  const m = await mountEditor({ blocks: page(), editable: true });
  await act(async () => {
    m.editor.focus();
    m.editor.setTextCursorPosition("x", "end");
    m.editor.insertInlineContent(" tapé");
  });
  await apart();
  await act(async () => {
    m.editor.insertBlocks([dropped as never], "x", "after");
  });
  await apart();
  await act(async () => forgetUploadName(m.editor, "f", "photo.png"));
  expect(propsOf(m, "f").name).toBe("");
  // Première annulation : le nom revient (limite écrite) ; seconde : le bloc du dépôt part.
  await act(async () => m.editor.undo());
  expect(propsOf(m, "f").name).toBe("photo.png");
  await act(async () => m.editor.undo());
  expect(ids(m)).toEqual(["x", "z"]);
  expect(m.text()).toContain("Bloc X tapé");
});
