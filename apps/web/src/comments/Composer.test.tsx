// La zone de rédaction pendant l'envoi : le champ est en lecture seule (pas désactivé : il garde
// le focus) et le bouton inactif ; un second Ctrl+Entrée n'envoie rien de plus. À la réussite, le
// texte envoyé est retiré ; à l'échec, tout est rendu tel quel, avec le refus du serveur.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError } from "../api";
import { deferred } from "../test/hooks";
import { Composer } from "./Composer";
import { createDrafts, DRAFT } from "./drafts";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function type(field: HTMLTextAreaElement, text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(field, text);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

const ctrlEnter = (field: HTMLTextAreaElement) =>
  field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true }));

test("sending_locks_the_field_and_a_second_ctrl_enter_sends_nothing", async () => {
  const answer = deferred<void>();
  const onSubmit = vi.fn(() => answer.promise);
  const drafts = createDrafts();
  await act(async () =>
    root.render(
      <Composer
        label="Commenter la page"
        submitLabel="Commenter"
        draft={{ store: drafts, key: DRAFT.page }}
        onSubmit={onSubmit}
      />,
    ),
  );
  const field = host.querySelector("textarea")!;
  const send = host.querySelector<HTMLButtonElement>('button[type="submit"]')!;
  await type(field, "une fois");
  // Deux Ctrl+Entrée coup sur coup, avant tout rendu.
  await act(async () => {
    ctrlEnter(field);
    ctrlEnter(field);
  });
  expect(onSubmit).toHaveBeenCalledTimes(1);
  expect(field.readOnly).toBe(true);
  expect(field.disabled).toBe(false);
  expect(send.getAttribute("aria-disabled")).toBe("true");
  await act(async () => answer.resolve());
  expect(field.value).toBe("");
  expect(field.readOnly).toBe(false);
  expect(drafts.has(DRAFT.page)).toBe(false);
});

test("a_refused_send_gives_everything_back", async () => {
  const answer = deferred<void>();
  const drafts = createDrafts();
  await act(async () =>
    root.render(
      <Composer
        label="Commenter la page"
        submitLabel="Commenter"
        draft={{ store: drafts, key: DRAFT.page }}
        onSubmit={() => answer.promise}
      />,
    ),
  );
  const field = host.querySelector("textarea")!;
  await type(field, "refusé");
  await act(async () => ctrlEnter(field));
  expect(field.readOnly).toBe(true);
  await act(async () => answer.reject(new ApiError(403, "forbidden", "il faut « Peut commenter »")));
  expect(field.value).toBe("refusé");
  expect(field.readOnly).toBe(false);
  expect(drafts.get(DRAFT.page)).toBe("refusé");
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("il faut « Peut commenter »");
});

// L'envoi en cours vit avec le brouillon, pas dans le composant : le fil change de liste pendant
// l'envoi, et le champ recréé depuis le brouillon est verrouillé lui aussi.

function reply(key: string, drafts: ReturnType<typeof createDrafts>, onSubmit: (body: string) => Promise<void>) {
  return (
    <Composer
      key={key}
      label="Répondre au fil"
      submitLabel="Envoyer"
      draft={{ store: drafts, key: DRAFT.reply("t") }}
      onSubmit={onSubmit}
    />
  );
}

test("two_ctrl_enter_around_a_remount_send_once", async () => {
  const answer = deferred<void>();
  const onSubmit = vi.fn(() => answer.promise);
  const drafts = createDrafts();
  await act(async () => root.render(reply("avant", drafts, onSubmit)));
  await type(host.querySelector("textarea")!, "une seule réponse");
  await act(async () => ctrlEnter(host.querySelector("textarea")!));
  // Le fil change de liste : le champ est recréé, depuis son brouillon.
  await act(async () => root.render(reply("après", drafts, onSubmit)));
  const field = host.querySelector("textarea")!;
  expect(field.value).toBe("une seule réponse");
  const probe = () => ({ lockedAfterRemount: field.readOnly, calls: onSubmit.mock.calls.length });
  await act(async () => ctrlEnter(field));
  expect(probe()).toEqual({ lockedAfterRemount: true, calls: 1 });
  // L'envoi aboutit : le texte envoyé ne reste pas dans le champ recréé, qui se déverrouille.
  await act(async () => answer.resolve());
  expect(field.value).toBe("");
  expect(field.readOnly).toBe(false);
  expect(drafts.has(DRAFT.reply("t"))).toBe(false);
});

test("a_send_refused_after_a_remount_unlocks_and_keeps_the_text", async () => {
  const answer = deferred<void>();
  const drafts = createDrafts();
  await act(async () => root.render(reply("avant", drafts, () => answer.promise)));
  await type(host.querySelector("textarea")!, "à garder");
  await act(async () => ctrlEnter(host.querySelector("textarea")!));
  await act(async () => root.render(reply("après", drafts, () => answer.promise)));
  const field = host.querySelector("textarea")!;
  expect(field.readOnly).toBe(true);
  await act(async () => answer.reject(new ApiError(409, "trashed", "cette page est à la corbeille")));
  expect(field.value).toBe("à garder");
  expect(field.readOnly).toBe(false);
  expect(drafts.get(DRAFT.reply("t"))).toBe("à garder");
});

test("a_draft_changed_during_the_send_is_not_removed", () => {
  // Défense : à la réussite, le brouillon n'est retiré que s'il vaut encore le texte envoyé.
  const drafts = createDrafts();
  drafts.set(DRAFT.page, "envoyé");
  drafts.startSending(DRAFT.page);
  expect(drafts.isSending(DRAFT.page)).toBe(true);
  drafts.set(DRAFT.page, "envoyé, puis autre chose");
  drafts.finishSending(DRAFT.page, "envoyé", true);
  expect(drafts.get(DRAFT.page)).toBe("envoyé, puis autre chose");
  expect(drafts.isSending(DRAFT.page)).toBe(false);
  drafts.startSending(DRAFT.page);
  drafts.finishSending(DRAFT.page, "envoyé, puis autre chose", true);
  expect(drafts.has(DRAFT.page)).toBe(false);
});
