// Un brouillon de commentaire n'est jamais perdu sans qu'on le veuille : Échap dans un champ où
// l'on a tapé du texte sort du champ sans fermer le panneau ; fermer puis rouvrir le panneau,
// cliquer une pastille ou relancer « Commenter » sur le même bloc retrouve le texte. Il part
// quand il est envoyé, ou annulé. Rien n'en va dans le stockage du navigateur.
import type { Thread } from "@mementum/protocol";
import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { thread, message, useMemoryComments } from "../test/comments";
import "../test/hooks";
import { CommentsPanel } from "./CommentsPanel";
import { createDrafts, type Drafts } from "./drafts";
import { useCommentsPanel, type CommentsPanelWiring } from "./useCommentsPanel";

let host: HTMLDivElement;
let root: Root;
const onClose = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onClose.mockClear();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

function Panel({ initial, drafts }: { initial: Thread[]; drafts: Drafts }) {
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
      drafts={drafts}
      composing={null}
      excerpt={() => null}
      onShowBlock={() => {}}
      onTargetDone={() => {}}
      onClose={onClose}
    />
  );
}

const field = (name: string) =>
  host.querySelector<HTMLTextAreaElement>(`textarea[aria-label="${name}"]`);

const button = (scope: ParentNode, name: string) =>
  [...scope.querySelectorAll("button")].find(
    (b) => (b.getAttribute("aria-label") ?? b.textContent ?? "").trim() === name,
  )!;

const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).focus();
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

const escape = (element: Element) =>
  act(async () => {
    element.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });

test("escape_in_a_field_with_text_leaves_the_field_and_keeps_the_text", async () => {
  await act(async () => root.render(<Panel initial={[]} drafts={createDrafts()} />));
  const page = field("Commenter la page")!;
  await type(page, "à ne pas perdre");
  await escape(page);
  expect(onClose).not.toHaveBeenCalled();
  expect(page.value).toBe("à ne pas perdre");
  expect(document.activeElement).toBe(host.querySelector("#panneau"));
  // Hors du champ, Échap ferme.
  await escape(document.activeElement!);
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("escape_in_an_empty_field_closes_the_panel", async () => {
  await act(async () => root.render(<Panel initial={[]} drafts={createDrafts()} />));
  const page = field("Commenter la page")!;
  page.focus();
  await escape(page);
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("drafts_survive_closing_and_reopening_the_panel", async () => {
  const drafts = createDrafts();
  const initial = [thread("t", { comments: [message("m", { body: "à modifier" })] })];
  await act(async () => root.render(<Panel initial={initial} drafts={drafts} />));
  await type(field("Commenter la page")!, "brouillon de page");
  await click(button(host, "Répondre"));
  await type(field("Répondre au fil")!, "brouillon de réponse");
  await click(button(host, "Modifier ce message"));
  await type(field("Modifier le message")!, "à modifier, en cours");
  // Le panneau se ferme (démonté), puis se rouvre.
  await act(async () => root.render(null));
  await act(async () => root.render(<Panel initial={initial} drafts={drafts} />));
  expect(field("Commenter la page")?.value).toBe("brouillon de page");
  expect(field("Répondre au fil")?.value).toBe("brouillon de réponse");
  expect(field("Modifier le message")?.value).toBe("à modifier, en cours");
});

test("a_draft_leaves_when_sent_or_cancelled", async () => {
  const drafts = createDrafts();
  const initial = [thread("t")];
  await act(async () => root.render(<Panel initial={initial} drafts={drafts} />));
  await type(field("Commenter la page")!, "envoyé");
  await click(button(host, "Commenter"));
  await click(button(host.querySelector('[data-thread-id="t"]')!, "Répondre"));
  await type(field("Répondre au fil")!, "abandonné");
  await click(button(host, "Annuler"));
  await act(async () => root.render(null));
  await act(async () => root.render(<Panel initial={initial} drafts={drafts} />));
  expect(field("Commenter la page")?.value).toBe("");
  expect(field("Répondre au fil")).toBeNull();
});

// Le câblage du panneau (`useCommentsPanel`) : là où vivent les brouillons, et le bloc qu'on
// commente.

let wiring: CommentsPanelWiring;

function Wired({ initial }: { initial: Thread[] }) {
  const comments = useMemoryComments(initial);
  const frame = useRef<HTMLElement>(null);
  wiring = useCommentsPanel(comments, frame);
  return (
    <>
      <button type="button" ref={wiring.button.buttonRef} onClick={wiring.button.onClick}>
        Commentaires
      </button>
      {wiring.panel && (
        <CommentsPanel
          {...wiring.panel}
          comments={comments}
          level="comment"
          trashed={false}
          readOnly={false}
          selfId="alice"
        />
      )}
    </>
  );
}

test("a_block_draft_survives_a_mark_a_second_comment_and_closing", async () => {
  await act(async () => root.render(<Wired initial={[thread("t", { blockId: "deux" })]} />));
  await act(async () => wiring.commentBlock("un"));
  await type(field("Commenter le bloc")!, "sur le bloc un");
  // Une pastille : le panneau va au fil, le commentaire du bloc reste.
  await act(async () => wiring.marks.onOpenThread("t"));
  expect(field("Commenter le bloc")?.value).toBe("sur le bloc un");
  // « Commenter » de nouveau sur le même bloc : le texte est là, et le champ a le focus.
  await act(async () => wiring.commentBlock("un"));
  expect(field("Commenter le bloc")?.value).toBe("sur le bloc un");
  expect(document.activeElement).toBe(field("Commenter le bloc"));
  // Fermé par le bouton, puis rouvert.
  await act(async () => wiring.button.onClick());
  expect(host.querySelector("#" + CSS.escape(wiring.button.panelId))).toBeNull();
  await act(async () => wiring.button.onClick());
  expect(field("Commenter le bloc")?.value).toBe("sur le bloc un");
});

test("another_block_keeps_the_first_blocks_draft", async () => {
  await act(async () => root.render(<Wired initial={[]} />));
  await act(async () => wiring.commentBlock("un"));
  await type(field("Commenter le bloc")!, "sur le bloc un");
  await act(async () => wiring.commentBlock("deux"));
  expect(field("Commenter le bloc")?.value).toBe("");
  await act(async () => wiring.commentBlock("un"));
  expect(field("Commenter le bloc")?.value).toBe("sur le bloc un");
});
