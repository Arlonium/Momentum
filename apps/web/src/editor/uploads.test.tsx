// Ce que l'éditeur d'une page fait d'un envoi de fichier (`uploads.ts`, tour de correction 1 de la
// Task C8) : qui reçoit de quoi envoyer, et ce que devient le bloc d'un envoi qui n'aboutit pas.
import { act } from "react";
import { expect, test } from "vitest";
import type { Seed } from "../test/editors";
import { mountEditor } from "../test/richEditor";
import { discardUpload, sessionUploader } from "./uploads";

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

/** Le bloc qu'un dépôt de `photo.png` a posé, en attente de son envoi. */
const dropped = { id: "f", type: "image", props: { name: "photo.png" } };
const page = (...blocks: unknown[]): Seed =>
  [{ id: "x", type: "paragraph", content: "Bloc X" }, ...blocks, { id: "z", type: "paragraph", content: "Bloc Z" }] as Seed;
const ids = (m: Awaited<ReturnType<typeof mountEditor>>) => m.editor.document.map((b) => b.id);

test("the_block_of_an_upload_that_did_not_end_well_is_removed_when_it_is_as_the_upload_left_it", async () => {
  for (const type of ["image", "video", "audio", "file"]) {
    const m = await mountEditor({ blocks: page({ ...dropped, type }), editable: true });
    const before = m.writes.length;
    await act(async () => discardUpload(m.editor, "f", "photo.png"));
    expect(ids(m), type).toEqual(["x", "z"]);
    // Le nom local du fichier n'est plus dans le document.
    expect(JSON.stringify(m.editor.document), type).not.toContain("photo.png");
    expect(m.writes.length, type).toBeGreaterThan(before);
  }
});

test("the_only_block_of_a_page_becomes_an_empty_paragraph_again", async () => {
  // Le dépôt sur le paragraphe vide d'une page vide en a changé le type : il le retrouve.
  const m = await mountEditor({ blocks: [dropped] as Seed, editable: true });
  await act(async () => discardUpload(m.editor, "f", "photo.png"));
  expect(m.editor.document).toHaveLength(1);
  expect(m.editor.document[0].type).toBe("paragraph");
  expect(JSON.stringify(m.editor.document)).not.toContain("photo.png");
});

test("a_block_that_changed_since_the_upload_began_is_left_alone", async () => {
  const kept: [what: string, block: Record<string, unknown>, fileName?: string][] = [
    ["il a reçu une adresse", { ...dropped, props: { name: "photo.png", url: PATH } }],
    ["il a une légende", { ...dropped, props: { name: "photo.png", caption: "Vue du port" } }],
    ["son nom n'est pas celui du fichier", { ...dropped, props: { name: "autre.png" } }],
    // Un bloc posé par le menu « / », où l'on a choisi un fichier : il n'a pas de nom, et reste.
    ["il était là avant l'envoi", { ...dropped, props: {} }],
    ["il n'est plus un bloc de fichier", { id: "f", type: "paragraph", content: "photo.png" }],
    ["le fichier n'a pas de nom", { ...dropped, props: { name: "" } }, ""],
  ];
  for (const [what, block, fileName = "photo.png"] of kept) {
    const m = await mountEditor({ blocks: page(block), editable: true });
    const before = m.writes.length;
    await act(async () => discardUpload(m.editor, "f", fileName));
    expect(ids(m), what).toEqual(["x", "f", "z"]);
    expect(m.writes.length, what).toBe(before);
  }
  // Un bloc parti, ou un éditeur qui n'écrit plus : rien, et aucune erreur.
  const gone = await mountEditor({ blocks: page(dropped), editable: true });
  await act(async () => discardUpload(gone.editor, "absent", "photo.png"));
  expect(ids(gone)).toEqual(["x", "f", "z"]);
  const reader = await mountEditor({ blocks: page(dropped), editable: false });
  const before = reader.writes.length;
  await act(async () => discardUpload(reader.editor, "f", "photo.png"));
  expect(ids(reader)).toEqual(["x", "f", "z"]);
  expect(reader.writes.length).toBe(before);
  expect([...gone.errors, ...reader.errors]).toEqual([]);
});
