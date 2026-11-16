// La fenêtre de réglage d'un bloc est tenue par l'id du bloc et rendue hors du document
// (`BlockWindowHost`) : quand un autre recrée le bloc (un déplacement le supprime et le récrit au
// même id, R223) ou le supprime, elle ne se ferme pas sous les doigts, le focus reste où l'on tape,
// et la frappe qui suit ne part jamais dans le document. Ctrl+Alt+Entrée l'ouvre depuis le bloc où
// est le curseur.
import { NodeSelection, TextSelection } from "prosemirror-state";
import { act } from "react";
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
import {
  blockOf,
  blockWindow,
  fill,
  flush,
  mountEditor,
  press,
  renewSession,
  type Mounted,
} from "../../test/richEditor";

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

/**
 * Ctrl+Alt+Entrée reçu par l'éditeur de `m` (ou par `target`), comme d'un clavier ; `altGraph` :
 * le système dit AltGr ; `alt: false` : Ctrl+Entrée seul.
 */
function shortcut(
  m: Mounted,
  {
    altGraph = false,
    meta = false,
    alt = true,
    target,
  }: { altGraph?: boolean; meta?: boolean; alt?: boolean; target?: Element } = {},
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key: "Enter",
    ctrlKey: !meta,
    metaKey: meta,
    altKey: alt,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, "getModifierState", {
    value: (modifier: string) => modifier === "AltGraph" && altGraph,
  });
  (target ?? m.editor.prosemirrorView.dom).dispatchEvent(event);
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
  // Ctrl+Entrée sans Alt n'est pas le raccourci.
  cursorIn(m, "c");
  expect(shortcut(m, { alt: false }).defaultPrevented).toBe(false);
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  // Tapée ailleurs que dans le texte de l'éditeur (un bouton posé dans sa vue, qui a le focus), la
  // touche n'ouvre rien, même si le curseur de l'éditeur est dans un callout.
  const elsewhere = document.createElement("button");
  m.host.querySelector(".bn-container")!.append(elsewhere);
  elsewhere.focus();
  expect(shortcut(m, { target: elsewhere }).defaultPrevented).toBe(false);
  await flush();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  elsewhere.remove();
  cursorIn(m, "c");
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

test("ctrl_alt_enter_is_taken_even_when_the_system_says_altgr", async () => {
  // Entrée ne tape aucun caractère sous AltGr : il n'y a rien à protéger, et une disposition à AltGr
  // (l'AZERTY sous Windows) pourrait dire AltGr pour Ctrl+Alt gauches (R475).
  const m = await mountEditor({ blocks: PAGE });
  cursorIn(m, "c");
  expect(shortcut(m, { altGraph: true }).defaultPrevented).toBe(true);
  await flush();
  expect(blockWindow("Icône et couleur du callout")).not.toBeNull();
});

/** Ouvre la fenêtre du callout `c` par son bouton. */
async function openCalloutWindow(m: Mounted): Promise<HTMLElement> {
  blockOf(m, "c").querySelector<HTMLButtonElement>('button[aria-label="Icône et couleur du callout"]')!.click();
  await flush();
  const dialog = blockWindow("Icône et couleur du callout")!;
  expect(dialog.contains(document.activeElement)).toBe(true);
  return dialog;
}

test("escape_after_another_changed_the_bookmark_into_a_paragraph_leaves_the_focus_in_the_editor", async () => {
  const { mona, tom, server } = await duo();
  const field = await openBookmarkWindow(mona);
  fill(field, "https://mementum.dev/tapé");
  tom.editor.updateBlock("s", { type: "paragraph" });
  await exchange(server, [tom, mona]);
  await flush();
  expect(stored(mona.doc).find((b) => b.id === "s")?.type, "précondition").toBe("paragraph");
  // Le bloc n'est plus un signet : la fenêtre le dit, garde le texte, et le focus.
  expect(blockWindow("Adresse du signet")!.querySelector('[role="status"]')?.textContent).toContain(
    "n'est plus dans la page",
  );
  expect(field.readOnly).toBe(true);
  expect(document.activeElement).toBe(field);
  press(field, "Escape");
  await flush();
  expect(blockWindow("Adresse du signet")).toBeNull();
  expect(document.activeElement).toBe(mona.editor.prosemirrorView.dom);
  expect(unsent(mona)).toBe(0);
});

test("escape_after_another_changed_the_callout_into_a_heading_leaves_the_focus_in_the_editor", async () => {
  const { mona, tom, server } = await duo();
  await openCalloutWindow(mona);
  tom.editor.updateBlock("c", { type: "heading", props: { level: 2 } });
  await exchange(server, [tom, mona]);
  await flush();
  expect(blockWindow("Icône et couleur du callout")!.querySelector('[role="status"]')).not.toBeNull();
  expect(blockWindow("Icône et couleur du callout")!.contains(document.activeElement)).toBe(true);
  // Un choix fait dans la fenêtre d'un bloc qui n'est plus un callout n'écrit rien.
  press(document.activeElement!, "ArrowRight");
  press(document.activeElement!, "Enter");
  await flush();
  expect(unsent(mona)).toBe(0);
  expect(storedProps(mona.doc, "c").icon).toBeUndefined();
  press(document.activeElement!, "Escape");
  await flush();
  expect(document.activeElement).toBe(mona.editor.prosemirrorView.dom);
});

test("the_focus_never_goes_to_the_button_of_a_child_block", async () => {
  // Le signet a un callout pour enfant ; Tom change le signet en paragraphe : son bouton part, celui
  // de l'enfant reste. Échap rend le focus à l'éditeur, pas au bouton de l'enfant.
  const base = await seedState([
    { id: "x", type: "paragraph", content: "Avant" },
    {
      id: "s",
      type: "bookmark",
      props: { url: "https://mementum.dev/" },
      children: [{ id: "c", type: "callout", content: "Enfant" }],
    },
    { id: "z", type: "paragraph", content: "Après" },
  ]);
  const mona = await mountEditor({ base, clientID: 2 });
  const tom = await peer("Tom", 3, base);
  const server = new Y.Doc();
  Y.applyUpdate(server, base, REMOTE);
  await openBookmarkWindow(mona);
  tom.editor.updateBlock("s", { type: "paragraph" });
  await exchange(server, [tom, mona]);
  await flush();
  expect(blockOf(mona, "c").querySelector('button[aria-label="Icône et couleur du callout"]'), "précondition").not.toBeNull();
  press(document.activeElement!, "Escape");
  await flush();
  expect(document.activeElement).toBe(mona.editor.prosemirrorView.dom);
});

test("a_click_outside_leaves_the_focus_where_it_went", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const elsewhere = document.createElement("button");
  elsewhere.textContent = "Ailleurs";
  document.body.append(elsewhere);
  try {
    await openBookmarkWindow(m);
    // Le clic, puis le focus que le navigateur lui donne.
    elsewhere.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    elsewhere.focus();
    await flush();
    await flush();
    expect(blockWindow("Adresse du signet")).toBeNull();
    expect(document.activeElement).toBe(elsewhere);
    // Témoin : un clic qui ne met le focus nulle part le rend au bouton du signet.
    await openBookmarkWindow(m);
    document.body.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    await flush();
    await flush();
    expect(blockWindow("Adresse du signet")).toBeNull();
    expect(document.activeElement).toBe(blockOf(m, "s").querySelector('button[aria-label="Modifier le signet"]'));
  } finally {
    elsewhere.remove();
  }
});

test("escape_in_an_editor_that_turned_read_only_never_leaves_the_focus_on_body", async () => {
  const m = await mountEditor({ blocks: PAGE });
  const field = await openBookmarkWindow(m);
  fill(field, "https://mementum.dev/tapé");
  m.editor.isEditable = false;
  await flush();
  const dialog = blockWindow("Adresse du signet")!;
  expect(dialog).not.toBeNull();
  expect(dialog.querySelector('[role="status"]')?.textContent).toBe(
    "Tu ne peux pas modifier cette page pour l'instant : ton texte reste ici, à copier.",
  );
  expect(field.readOnly).toBe(true);
  expect(document.activeElement).toBe(field);
  press(field, "Escape");
  await flush();
  expect(document.activeElement).not.toBe(document.body);
  expect(m.host.contains(document.activeElement)).toBe(true);
});

/** Ce que dit la fenêtre quand l'éditeur ne peut pas écrire (R472). */
const READ_ONLY_NOTICE = "Tu ne peux pas modifier cette page pour l'instant : ton texte reste ici, à copier.";

test("the_window_and_its_text_survive_a_new_session_of_the_page", async () => {
  const m = await mountEditor({ blocks: PAGE, pageId: "page-1" });
  const field = await openBookmarkWindow(m);
  fill(field, "https://mementum.dev/tapé");

  // La page passe en lecture seule : la session est recréée, avec un éditeur neuf.
  const reader = await renewSession(m, { editable: false, pageId: "page-1" });
  let dialog = blockWindow("Adresse du signet")!;
  expect(dialog, "la fenêtre reste").not.toBeNull();
  let now = dialog.querySelector("input")!;
  expect(now.value).toBe("https://mementum.dev/tapé");
  expect(now.readOnly).toBe(true);
  expect(dialog.querySelector('[role="status"]')?.textContent).toBe(READ_ONLY_NOTICE);
  expect(dialog.contains(document.activeElement), "le focus reste dans la fenêtre").toBe(true);
  now.form!.requestSubmit();
  await flush();
  expect(reader.writes).toEqual([]);

  // Le droit revient : une autre session, modifiable ; le texte tapé est là, et s'enregistre.
  const writer = await renewSession(m, { editable: true, pageId: "page-1" });
  dialog = blockWindow("Adresse du signet")!;
  now = dialog.querySelector("input")!;
  expect(now.value).toBe("https://mementum.dev/tapé");
  expect(now.readOnly).toBe(false);
  expect(dialog.querySelector('[role="status"]')).toBeNull();
  now.form!.requestSubmit();
  await flush();
  expect(storedProps(writer.doc, "s").url).toBe("https://mementum.dev/tap%C3%A9");
});

test("the_typed_text_is_forgotten_once_the_page_is_left", async () => {
  const first = await mountEditor({ blocks: PAGE, pageId: "page-2" });
  fill(await openBookmarkWindow(first), "https://mementum.dev/oublié");
  // Tant que la page est ouverte, une session neuve retrouve le texte (test précédent) ; la page
  // quittée (plus aucun éditeur d'elle), il part.
  await act(async () => first.root.unmount());
  await flush();
  const again = await mountEditor({ blocks: PAGE, pageId: "page-2" });
  expect(blockWindow("Adresse du signet"), "aucune fenêtre ne revient").toBeNull();
  expect((await openBookmarkWindow(again)).value).toBe("https://mementum.dev/");
});

test("the_window_of_one_page_is_never_shown_in_another", async () => {
  const one = await mountEditor({ blocks: PAGE, pageId: "page-3" });
  await openBookmarkWindow(one);
  const other = await mountEditor({ blocks: PAGE, pageId: "page-4" });
  expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
  expect(other.host.querySelector('[role="dialog"]')).toBeNull();
});

test("a_choice_for_a_callout_another_deleted_writes_nothing_and_raises_nothing", async () => {
  const { mona, tom, server } = await duo();
  await openCalloutWindow(mona);
  tom.editor.removeBlocks(["c"]);
  await exchange(server, [tom, mona]);
  await flush();
  // Entrée sur une couleur : le bloc n'existe plus, rien ne s'écrit, rien ne lève.
  const colors = blockWindow("Icône et couleur du callout")!.querySelector('ul[aria-label="Couleur"]')!;
  const option = colors.querySelector<HTMLElement>('[role="option"]')!;
  option.focus();
  press(option, "Enter");
  option.click();
  await flush();
  expect(unsent(mona)).toBe(0);
  // (Les avertissements de React sur `act` viennent de l'échange, fait hors de React.)
  expect(mona.errors.filter((error) => !error.includes("not wrapped in act"))).toEqual([]);
});

test("escape_in_a_read_only_session_leaves_the_focus_in_the_editor", async () => {
  // Une session en lecture seule monte son éditeur sans `tabindex` : il ne prend pas le focus de
  // lui-même, et le focus tombait sur `body` (R470).
  const m = await mountEditor({ blocks: PAGE, pageId: "page-5" });
  fill(await openBookmarkWindow(m), "https://mementum.dev/tapé");
  const reader = await renewSession(m, { editable: false, pageId: "page-5" });
  expect(blockWindow("Adresse du signet")!.contains(document.activeElement)).toBe(true);
  press(document.activeElement!, "Escape");
  await flush();
  expect(blockWindow("Adresse du signet")).toBeNull();
  expect(document.activeElement).toBe(reader.editor.prosemirrorView.dom);
});
