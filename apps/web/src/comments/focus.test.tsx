// Le focus reste dans le panneau des commentaires : après chaque action (réponse envoyée ou
// annulée, message modifié ou non, réaction choisie, fil résolu, message supprimé), l'élément qui
// l'avait peut avoir quitté la page ; le focus va alors à un élément qui est encore là et qui a du
// sens, jamais à `body`, et Échap ferme toujours le panneau.
import type { Thread } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { author, message, thread, useMemoryComments } from "../test/comments";
import "../test/hooks";
import { CommentsPanel } from "./CommentsPanel";
import { createDrafts, DRAFT, type Drafts } from "./drafts";

let host: HTMLDivElement;
let root: Root;
const onClose = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onClose.mockClear();
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

function Panel({ initial }: { initial: Thread[] }) {
  const comments = useMemoryComments(initial);
  return (
    <CommentsPanel
      id="panneau"
      comments={comments}
      order={[]}
      level="full"
      trashed={false}
      readOnly={false}
      selfId="alice"
      target={null}
      excerpt={() => null}
      onShowBlock={() => {}}
      onTargetDone={() => {}}
      onClose={onClose}
    />
  );
}

async function show(initial: Thread[]) {
  await act(async () => root.render(<Panel initial={initial} />));
}

const button = (scope: ParentNode, name: string) =>
  [...scope.querySelectorAll("button")].find(
    (b) => (b.getAttribute("aria-label") ?? b.textContent ?? "").trim() === name,
  )!;

const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).focus();
    (element as HTMLElement).click();
  });

async function type(field: HTMLTextAreaElement, text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(field, text);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

const press = (element: Element, key: string, init: KeyboardEventInit = {}) =>
  act(async () => {
    element.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
  });

/** Le focus est dans le panneau, sur un élément encore là ; Échap, envoyé sur lui, ferme. */
async function focusStaysAndEscapeCloses(expected?: (active: HTMLElement) => boolean) {
  const active = document.activeElement as HTMLElement;
  expect(active).not.toBe(document.body);
  expect(active.isConnected).toBe(true);
  expect(host.querySelector("#panneau")!.contains(active)).toBe(true);
  if (expected) expect(expected(active)).toBe(true);
  await press(active, "Escape");
  expect(onClose).toHaveBeenCalledTimes(1);
}

const two = () => [
  thread("t", {
    comments: [message("m1", { body: "premier" }), message("m2", { body: "second" })],
  }),
];

test("after_a_reply_is_sent_the_focus_is_on_reply", async () => {
  await show(two());
  await click(button(host, "Répondre"));
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Répondre au fil"]')!;
  expect(document.activeElement).toBe(field);
  await type(field, "Ma réponse");
  await press(field, "Enter", { ctrlKey: true });
  expect(host.textContent).toContain("Ma réponse");
  await focusStaysAndEscapeCloses((active) => active.textContent === "Répondre");
});

test("after_a_reply_is_cancelled_the_focus_is_on_reply", async () => {
  await show(two());
  await click(button(host, "Répondre"));
  await click(button(host, "Annuler"));
  await focusStaysAndEscapeCloses((active) => active.textContent === "Répondre");
});

test("after_an_edit_is_saved_the_focus_is_on_its_edit_button", async () => {
  await show(two());
  const second = [...host.querySelectorAll(".mm-comment")][1];
  await click(button(second, "Modifier ce message"));
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Modifier le message"]')!;
  await type(field, "second, corrigé");
  await click(button(host, "Enregistrer le message"));
  expect(host.textContent).toContain("second, corrigé");
  await focusStaysAndEscapeCloses(
    (active) =>
      active.getAttribute("aria-label") === "Modifier ce message" &&
      active.closest(".mm-comment")!.textContent!.includes("second, corrigé"),
  );
});

test("after_an_edit_is_cancelled_the_focus_is_on_its_edit_button", async () => {
  await show(two());
  const first = [...host.querySelectorAll(".mm-comment")][0];
  await click(button(first, "Modifier ce message"));
  await click(button(host, "Annuler"));
  await focusStaysAndEscapeCloses(
    (active) =>
      active.getAttribute("aria-label") === "Modifier ce message" &&
      active.closest(".mm-comment")!.textContent!.includes("premier"),
  );
});

test("after_a_reaction_is_chosen_the_focus_is_on_add_a_reaction", async () => {
  await show(two());
  const first = [...host.querySelectorAll(".mm-comment")][0];
  await click(button(first, "Ajouter une réaction"));
  await click(button(first, "Réagir avec 🎉"));
  expect(first.querySelector(".mm-reaction-emoji")?.textContent).toBe("🎉");
  await focusStaysAndEscapeCloses(
    (active) => active.getAttribute("aria-label") === "Ajouter une réaction",
  );
});

test("after_ones_own_reaction_is_removed_the_focus_is_on_add_a_reaction", async () => {
  await show([
    thread("t", { comments: [message("m", { reactions: [{ emoji: "👀", users: [author("alice")] }] })] }),
  ]);
  const chip = host.querySelector<HTMLButtonElement>('ul[aria-label="Réactions"] button[aria-pressed="true"]')!;
  await click(chip);
  expect(host.querySelector(".mm-reaction-emoji")).toBeNull();
  await focusStaysAndEscapeCloses(
    (active) => active.getAttribute("aria-label") === "Ajouter une réaction",
  );
});

test("after_a_thread_is_resolved_the_focus_is_on_the_panel_title", async () => {
  await show([thread("a"), thread("b")]);
  const first = host.querySelector('[data-thread-id="a"]')!;
  await click(button(first, "Résoudre"));
  expect(host.querySelector('[data-thread-id="a"]')).toBeNull();
  await focusStaysAndEscapeCloses((active) => active.textContent === "Commentaires");
});

test("after_a_thread_is_reopened_the_focus_is_on_the_thread", async () => {
  await show([thread("a", { resolvedAt: 5, resolvedBy: author("alice") })]);
  await click(button(host, "Fils résolus (1)"));
  await click(button(host, "Rouvrir"));
  await focusStaysAndEscapeCloses((active) => active.getAttribute("data-thread-id") === "a");
});

test("after_a_message_is_deleted_the_focus_is_on_its_thread", async () => {
  await show(two());
  const second = [...host.querySelectorAll(".mm-comment")][1];
  await click(button(second, "Supprimer ce message"));
  await click(button(host.querySelector("dialog")!, "Supprimer"));
  expect(host.textContent).not.toContain("second");
  await focusStaysAndEscapeCloses((active) => active.getAttribute("data-thread-id") === "t");
});

test("after_the_last_message_is_deleted_the_focus_is_on_the_panel_title", async () => {
  await show([thread("seul"), thread("autre")]);
  const only = host.querySelector('[data-thread-id="seul"]')!;
  await click(button(only, "Supprimer ce message"));
  await click(button(host.querySelector("dialog")!, "Supprimer"));
  expect(host.querySelector('[data-thread-id="seul"]')).toBeNull();
  await focusStaysAndEscapeCloses((active) => active.textContent === "Commentaires");
});

test("a_thread_that_changes_list_under_the_focus_keeps_it", async () => {
  // Le bloc d'un fil revient dans la page (une annulation) : le fil passe de « Bloc supprimé »
  // aux fils ouverts, et son élément est recréé sous le focus.
  const threads = [thread("t", { blockId: "revenu" })];
  function Moving({ order }: { order: string[] }) {
    const comments = useMemoryComments(threads);
    return (
      <CommentsPanel
        id="panneau"
        comments={comments}
        order={order}
        absent={order.includes("revenu") ? [] : ["revenu"]}
        level="full"
        trashed={false}
        readOnly={false}
        selfId="alice"
        target={null}
        excerpt={() => null}
        onShowBlock={() => {}}
        onTargetDone={() => {}}
        onClose={onClose}
      />
    );
  }
  await act(async () => root.render(<Moving order={[]} />));
  expect(host.querySelector('ul[aria-label="Fils d\'un bloc supprimé"]')).not.toBeNull();
  await click(button(host, "Répondre"));
  await click(button(host, "Annuler"));
  expect(document.activeElement?.textContent).toBe("Répondre");
  await act(async () => root.render(<Moving order={["revenu"]} />));
  expect(host.querySelector('ul[aria-label="Fils ouverts"] [data-thread-id="t"]')).not.toBeNull();
  await focusStaysAndEscapeCloses((active) => active.getAttribute("data-thread-id") === "t");
});

// Un champ ne prend le focus que sur un geste (« Répondre », « Modifier », « Commenter »…) : un
// champ rouvert parce qu'il a un brouillon s'affiche avec son texte, sans prendre le focus, et
// aucun rendu venu du réseau, de l'éditeur ou d'un changement de liste ne déplace le focus hors
// de l'endroit où la personne tape.

function Listed({
  drafts,
  order,
  absent,
  initial,
}: {
  drafts: Drafts;
  order: string[];
  absent: string[];
  initial: Thread[];
}) {
  const comments = useMemoryComments(initial);
  return (
    <CommentsPanel
      id="panneau"
      comments={comments}
      order={order}
      absent={absent}
      level="full"
      trashed={false}
      readOnly={false}
      selfId="alice"
      target={null}
      drafts={drafts}
      composing={null}
      excerpt={() => null}
      onShowBlock={() => {}}
      onTargetDone={() => {}}
      onClose={onClose}
    />
  );
}

/** Un fil sur le bloc `b`, avec une réponse en brouillon. */
function withReplyDraft() {
  const drafts = createDrafts();
  drafts.set(DRAFT.reply("t"), "réponse en cours");
  const initial = [thread("t", { blockId: "b" })];
  return { drafts, initial };
}

test("a_reply_draft_reopened_by_a_list_change_leaves_the_focus_in_the_editor", async () => {
  // La sonde du re-relecteur, telle quelle.
  const { drafts, initial } = withReplyDraft();
  const editor = document.createElement("div");
  editor.contentEditable = "true";
  editor.tabIndex = 0;
  document.body.append(editor);
  await act(async () =>
    root.render(<Listed drafts={drafts} order={["b"]} absent={[]} initial={initial} />),
  );
  await act(async () => editor.focus());
  // Le bloc commenté est supprimé dans l'éditeur : le fil passe sous « Bloc supprimé ».
  await act(async () =>
    root.render(<Listed drafts={drafts} order={[]} absent={["b"]} initial={initial} />),
  );
  expect(document.activeElement).toBe(editor);
  // Le champ rouvert montre son texte.
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Répondre au fil"]');
  expect(field?.value).toBe("réponse en cours");
  editor.remove();
});

test("a_reply_draft_reopened_by_a_list_change_leaves_the_focus_in_the_page_comment", async () => {
  const { drafts, initial } = withReplyDraft();
  await act(async () =>
    root.render(<Listed drafts={drafts} order={["b"]} absent={[]} initial={initial} />),
  );
  const page = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Commenter la page"]')!;
  await act(async () => page.focus());
  await act(async () =>
    root.render(<Listed drafts={drafts} order={[]} absent={["b"]} initial={initial} />),
  );
  expect(document.activeElement).toBe(page);
});

test("showing_resolved_threads_with_a_draft_leaves_the_focus_on_the_toggle", async () => {
  const drafts = createDrafts();
  drafts.set(DRAFT.reply("clos"), "réponse au fil résolu");
  const initial = [thread("clos", { resolvedAt: 5, resolvedBy: author("alice") })];
  await act(async () =>
    root.render(<Listed drafts={drafts} order={[]} absent={[]} initial={initial} />),
  );
  const toggle = button(host, "Fils résolus (1)");
  await click(toggle);
  expect(
    host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Répondre au fil"]')?.value,
  ).toBe("réponse au fil résolu");
  expect(document.activeElement).toBe(toggle);
});

test("a_click_on_reply_still_gives_the_field_the_focus", async () => {
  const drafts = createDrafts();
  await act(async () =>
    root.render(<Listed drafts={drafts} order={[]} absent={[]} initial={[thread("t")]} />),
  );
  await click(button(host, "Répondre"));
  expect(document.activeElement).toBe(
    host.querySelector('textarea[aria-label="Répondre au fil"]'),
  );
  // Et « Modifier » aussi.
  await click(button(host, "Modifier ce message"));
  expect(document.activeElement).toBe(
    host.querySelector('textarea[aria-label="Modifier le message"]'),
  );
});

test("typing_in_a_reply_that_changes_list_keeps_the_focus_in_it", async () => {
  // La personne tape sa réponse quand le fil change de liste (un autre supprime le bloc) : le
  // champ est recréé, et le focus le retrouve.
  const drafts = createDrafts();
  const initial = [thread("t", { blockId: "b" })];
  await act(async () =>
    root.render(<Listed drafts={drafts} order={["b"]} absent={[]} initial={initial} />),
  );
  await click(button(host, "Répondre"));
  await type(
    host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Répondre au fil"]')!,
    "je tape",
  );
  await act(async () =>
    root.render(<Listed drafts={drafts} order={[]} absent={["b"]} initial={initial} />),
  );
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Répondre au fil"]')!;
  expect(field.value).toBe("je tape");
  expect(document.activeElement).toBe(field);
});
