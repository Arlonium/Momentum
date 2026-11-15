// La fenêtre de réglage d'un bloc est tenue par l'id du bloc et rendue hors du document
// (`BlockWindowHost`) : quand un autre recrée le bloc (un déplacement le supprime et le récrit au
// même id, R223) ou le supprime, elle ne se ferme pas sous les doigts, le focus reste où l'on tape,
// et la frappe qui suit ne part jamais dans le document. Ctrl+Alt+Entrée l'ouvre depuis le bloc où
// est le curseur.
import { NodeSelection, TextSelection } from "prosemirror-state";
import { expect, test } from "vitest";
import * as Y from "yjs";
import {
  containerOf,
  drag,
  exchange,
  peer,
  REMOTE,
  seedState,
  storedProps,
  stored,
  type Peer,
  type Seed,
} from "../../test/editors";
import { blockOf, blockWindow, fill, flush, mountEditor, press, type Mounted } from "../../test/richEditor";

const PAGE: Seed = [
  { id: "x", type: "paragraph", content: "Avant" },
  { id: "s", type: "bookmark", props: { url: "https://mementum.dev/" } },
  { id: "c", type: "callout", content: "Attention" },
  { id: "z", type: "paragraph", content: "Après" },
];

/** Mona, dans l'éditeur rendu, Tom dans un éditeur du banc, et le serveur, sur la page `PAGE`. */
async function duo(): Promise<{ mona: Mounted; tom: Peer; server: Y.Doc }> {
  const base = await seedState(PAGE);
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  return { mona, tom, server };
}

async function openBookmarkWindow(m: Mounted): Promise<HTMLInputElement> {
  blockOf(m, "s").querySelector<HTMLButtonElement>('button[aria-label="Modifier le signet"]')!.click();
  await flush();
  const field = blockWindow("Adresse du signet")!.querySelector<HTMLInputElement>("input")!;
  expect(document.activeElement).toBe(field);
  return field;
}

/** Ce que Mona a écrit et pas encore envoyé : rien, si la frappe n'est pas allée au document. */
const unsent = (m: Mounted) => m.writes.length;

test("the_window_stays_open_when_another_recreates_its_block", async () => {
  const { mona, tom, server } = await duo();
  const field = await openBookmarkWindow(mona);
  fill(field, "https://mementum.dev/do");
  const shownBefore = blockOf(mona, "s").querySelector(".mm-bookmark");

  // Tom glisse le signet sous « Après » : supprimé et récrit au même id.
  drag(tom, "s", { under: "z" });
  await exchange(server, [tom, mona]);
  await flush();
  expect(stored(server).map((b) => b.id), "le signet est déplacé").toEqual(["x", "c", "z", "s"]);
  // Précondition : l'élément affiché du bloc a été recréé chez Mona.
  expect(blockOf(mona, "s").querySelector(".mm-bookmark")).not.toBe(shownBefore);

  // La fenêtre est toujours là, le focus dans son champ, avec le texte tapé.
  expect(blockWindow("Adresse du signet")).not.toBeNull();
  expect(document.activeElement).toBe(field);
  expect(field.value).toBe("https://mementum.dev/do");
  // La frappe qui suit va au champ, pas au document.
  fill(field, "https://mementum.dev/doc");
  await flush();
  expect(unsent(mona)).toBe(0);

  // Enregistrée, l'adresse va au bloc, par son id, et Tom la reçoit.
  field.form!.requestSubmit();
  await flush();
  await exchange(server, [mona, tom]);
  expect(storedProps(server, "s").url).toBe("https://mementum.dev/doc");
  expect(storedProps(tom.doc, "s").url).toBe("https://mementum.dev/doc");
  // Le focus revient au bouton du signet, à sa nouvelle place.
  expect(document.activeElement).toBe(blockOf(mona, "s").querySelector('button[aria-label="Modifier le signet"]'));
});

test("the_window_of_a_deleted_block_says_so_and_keeps_what_was_typed", async () => {
  const { mona, tom, server } = await duo();
  const field = await openBookmarkWindow(mona);
  fill(field, "https://mementum.dev/tapé");

  tom.editor.removeBlocks(["s"]);
  await exchange(server, [tom, mona]);
  await flush();

  const dialog = blockWindow("Adresse du signet")!;
  expect(dialog, "la fenêtre reste").not.toBeNull();
  expect(dialog.querySelector('[role="status"]')?.textContent).toBe(
    "Ce signet n'est plus dans la page : quelqu'un l'a supprimé ou changé. Ton texte reste ici, à copier.",
  );
  expect(document.activeElement).toBe(field);
  expect(field.readOnly).toBe(true);
  expect(field.value).toBe("https://mementum.dev/tapé");
  // Rien ne s'écrit plus.
  field.form!.requestSubmit();
  await flush();
  expect(unsent(mona)).toBe(0);
  expect(stored(mona.doc).map((b) => b.id)).toEqual(["x", "c", "z"]);

  // Échap ferme ; le bouton du bloc est parti : le focus va à l'éditeur, jamais à `body`.
  press(field, "Escape");
  await flush();
  expect(blockWindow("Adresse du signet")).toBeNull();
  expect(document.activeElement).toBe(mona.editor.prosemirrorView.dom);
});

test("the_window_comes_back_to_life_when_its_block_comes_back", async () => {
  const { mona, tom, server } = await duo();
  const field = await openBookmarkWindow(mona);
  fill(field, "https://mementum.dev/retour");
  tom.editor.removeBlocks(["s"]);
  await exchange(server, [tom, mona]);
  await flush();
  expect(field.readOnly).toBe(true);
  // Tom annule : le signet revient, au même id.
  tom.editor.undo();
  await exchange(server, [tom, mona]);
  await flush();
  expect(stored(mona.doc).map((b) => b.id)).toEqual(["x", "s", "c", "z"]);
  expect(field.readOnly).toBe(false);
  expect(blockWindow("Adresse du signet")!.querySelector('[role="status"]')).toBeNull();
  field.form!.requestSubmit();
  await flush();
  expect(storedProps(mona.doc, "s").url).toBe("https://mementum.dev/retour");
});

/** Ctrl+Alt+Entrée reçu par l'éditeur de `m`, comme d'un clavier ; `altGraph` : tapé avec AltGr. */
function shortcut(m: Mounted, { altGraph = false, meta = false } = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key: "Enter",
    ctrlKey: !meta,
    metaKey: meta,
    altKey: true,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "getModifierState", {
    value: (modifier: string) => modifier === "AltGraph" && altGraph,
  });
  m.editor.prosemirrorView.dom.dispatchEvent(event);
  return event;
}

function cursorIn(m: Mounted, id: string): void {
  m.editor.focus();
  m.editor.transact((tr) => tr.setSelection(TextSelection.create(tr.doc, containerOf(tr.doc, id).pos + 3)));
}

function select(m: Mounted, id: string): void {
  m.editor.focus();
  m.editor.transact((tr) => tr.setSelection(NodeSelection.create(tr.doc, containerOf(tr.doc, id).pos + 1)));
}

test("ctrl_alt_enter_opens_the_window_of_the_rich_block_at_the_cursor", async () => {
  const m = await mountEditor({ blocks: PAGE });
  // Dans le texte d'un callout.
  cursorIn(m, "c");
  const event = shortcut(m);
  await flush();
  expect(event.defaultPrevented).toBe(true);
  const dialog = blockWindow("Icône et couleur du callout")!;
  expect(dialog).not.toBeNull();
  expect(dialog.contains(document.activeElement)).toBe(true);
  // Échap rend le focus à l'éditeur, d'où on l'a ouverte, avec son curseur.
  press(document.activeElement!, "Escape");
  await flush();
  expect(blockWindow("Icône et couleur du callout")).toBeNull();
  expect(document.activeElement).toBe(m.editor.prosemirrorView.dom);
  expect(m.editor.getTextCursorPosition().block.id).toBe("c");

  // Un signet sélectionné (il n'a pas de texte), avec Cmd+Option sur Mac.
  select(m, "s");
  expect(shortcut(m, { meta: true }).defaultPrevented).toBe(true);
  await flush();
  expect(blockWindow("Adresse du signet")).not.toBeNull();
  press(document.activeElement!, "Escape");
  await flush();
});

test("ctrl_alt_enter_takes_nothing_it_has_no_use_for", async () => {
  const m = await mountEditor({ blocks: PAGE });
  // Un paragraphe n'a pas de fenêtre : la touche suit son cours.
  cursorIn(m, "x");
  expect(shortcut(m).defaultPrevented).toBe(false);
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  // AltGr (Ctrl+Alt sous Windows) n'est jamais pris.
  cursorIn(m, "c");
  expect(shortcut(m, { altGraph: true }).defaultPrevented).toBe(false);
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  // Sous une fenêtre modale, le reste de la page est inerte.
  const modal = document.createElement("dialog");
  modal.setAttribute("open", "");
  document.body.append(modal);
  try {
    expect(shortcut(m).defaultPrevented).toBe(false);
    await flush();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  } finally {
    modal.remove();
  }
  // Témoin : la même touche, sans tout cela, ouvre la fenêtre.
  expect(shortcut(m).defaultPrevented).toBe(true);
});

test("ctrl_alt_enter_opens_nothing_for_a_reader", async () => {
  const m = await mountEditor({ blocks: PAGE, editable: false });
  select(m, "s");
  expect(shortcut(m).defaultPrevented).toBe(false);
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  // Témoin : le même geste, dans un éditeur modifiable, l'ouvre.
  m.editor.isEditable = true;
  await flush();
  select(m, "s");
  expect(shortcut(m).defaultPrevented).toBe(true);
  await flush();
  expect(blockWindow("Adresse du signet")).not.toBeNull();
});
