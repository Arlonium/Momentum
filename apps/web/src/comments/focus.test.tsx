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
