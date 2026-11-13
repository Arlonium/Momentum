// Les plafonds des commentaires (S4) : le serveur refuse par `409` au-delà de 2 000 fils par page
// (`thread_limit`), 500 messages par fil (`message_limit`) et 32 emojis distincts par message
// (`reaction_limit`). Le client montre le message du serveur tel quel, comme tout refus, et le
// texte refusé reste : dans son champ, et dans son brouillon, qui survit au panneau. Rien n'est
// renvoyé de soi-même. Ici, c'est le test qui joue le serveur.
import type { ErrorCode, Thread } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError } from "../api";
import { message, thread } from "../test/comments";
import "../test/hooks";
import { CommentsPanel, type PanelTarget } from "./CommentsPanel";
import { createDrafts, DRAFT, type Drafts } from "./drafts";
import type { Comments } from "./useComments";

let host: HTMLDivElement;
let root: Root;
const onTargetDone = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onTargetDone.mockClear();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

/**
 * Un refus d'un plafond, tel que `api.ts` le lève. Les trois codes viennent de S4 et ne sont pas
 * encore dans les types générés (`packages/protocol`) : la conversion tombera avec eux. Le client
 * ne lit pas le code, seulement le message.
 */
const limit = (code: string, said: string) => new ApiError(409, code as ErrorCode, said);

const THREADS = limit(
  "thread_limit",
  "cette page a déjà 2 000 fils : résous ou supprime des fils pour en ouvrir un autre",
);
const MESSAGES = limit(
  "message_limit",
  "ce fil a déjà 500 messages : ouvre un autre fil pour continuer",
);
const REACTIONS_FULL = limit(
  "reaction_limit",
  "ce message a déjà 32 réactions différentes : ajoute-toi à l'une d'elles",
);

function comments(threads: Thread[], overrides: Partial<Comments>): Comments {
  return {
    threads,
    state: "ready",
    error: null,
    open: vi.fn(async () => thread("neuf")),
    reply: vi.fn(async () => message("réponse")),
    edit: vi.fn(async () => message("modifié")),
    remove: vi.fn(async () => {}),
    resolve: vi.fn(async () => thread("résolu")),
    react: vi.fn(async () => message("réaction")),
    reload: vi.fn(),
    ...overrides,
  };
}

async function show(given: Comments, drafts: Drafts, target: PanelTarget | null = null) {
  await act(async () =>
    root.render(
      <CommentsPanel
        id="panneau"
        comments={given}
        order={[]}
        level="comment"
        trashed={false}
        readOnly={false}
        selfId="alice"
        target={target}
        drafts={drafts}
        excerpt={() => "Le texte du bloc"}
        onShowBlock={() => {}}
        onTargetDone={onTargetDone}
        onClose={() => {}}
      />,
    ),
  );
}

const field = (name: string) =>
  host.querySelector<HTMLTextAreaElement>(`textarea[aria-label="${name}"]`);
const button = (scope: ParentNode, name: string) =>
  [...scope.querySelectorAll("button")].find(
    (b) => (b.getAttribute("aria-label") ?? b.textContent ?? "").trim() === name,
  )!;
const alerts = () => [...host.querySelectorAll('[role="alert"]')].map((a) => a.textContent);
const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });

async function type(target: HTMLTextAreaElement, text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  await act(async () => {
    target.focus();
    setter.call(target, text);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

test("a_page_at_its_thread_limit_shows_the_refusal_and_keeps_the_draft", async () => {
  const drafts = createDrafts();
  const given = comments([], { open: vi.fn(async () => Promise.reject(THREADS)) });
  await show(given, drafts);
  await type(field("Commenter la page")!, "Un fil de trop");
  await click(button(host, "Commenter"));
  expect(given.open).toHaveBeenCalledExactlyOnceWith(null, "Un fil de trop");
  expect(alerts()).toEqual([THREADS.message]);
  expect(field("Commenter la page")!.value).toBe("Un fil de trop");
  expect(drafts.get(DRAFT.page)).toBe("Un fil de trop");
  // Fermé puis rouvert, le panneau le retrouve ; rien n'est reparti.
  await act(async () => root.render(null));
  await show(given, drafts);
  expect(field("Commenter la page")!.value).toBe("Un fil de trop");
  expect(given.open).toHaveBeenCalledTimes(1);
});

test("a_block_comment_at_the_thread_limit_keeps_its_zone_and_text", async () => {
  const drafts = createDrafts();
  const given = comments([], { open: vi.fn(async () => Promise.reject(THREADS)) });
  await show(given, drafts, { kind: "block", blockId: "b1", seq: 1 });
  await type(field("Commenter le bloc")!, "Sur ce bloc");
  await click(button(host, "Commenter le bloc"));
  expect(given.open).toHaveBeenCalledExactlyOnceWith("b1", "Sur ce bloc");
  expect(alerts()).toEqual([THREADS.message]);
  // La zone reste, avec son texte : le commentaire n'est pas parti.
  expect(onTargetDone).not.toHaveBeenCalled();
  expect(field("Commenter le bloc")!.value).toBe("Sur ce bloc");
  expect(drafts.get(DRAFT.block("b1"))).toBe("Sur ce bloc");
});

test("a_thread_at_its_message_limit_shows_the_refusal_and_keeps_the_reply", async () => {
  const drafts = createDrafts();
  const given = comments([thread("t")], { reply: vi.fn(async () => Promise.reject(MESSAGES)) });
  await show(given, drafts);
  await click(button(host, "Répondre"));
  await type(field("Répondre au fil")!, "Une réponse de trop");
  await click(button(host, "Envoyer la réponse"));
  expect(given.reply).toHaveBeenCalledExactlyOnceWith("t", "Une réponse de trop");
  expect(alerts()).toEqual([MESSAGES.message]);
  expect(field("Répondre au fil")!.value).toBe("Une réponse de trop");
  expect(drafts.get(DRAFT.reply("t"))).toBe("Une réponse de trop");
});

test("a_message_at_its_reaction_limit_shows_the_refusal", async () => {
  const given = comments([thread("t")], { react: vi.fn(async () => Promise.reject(REACTIONS_FULL)) });
  await show(given, createDrafts());
  await click(button(host, "Ajouter une réaction"));
  await click(button(host, "Réagir avec 🎉"));
  expect(given.react).toHaveBeenCalledExactlyOnceWith("t", "t-1", "🎉", true);
  expect(alerts()).toEqual([REACTIONS_FULL.message]);
});
