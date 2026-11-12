// Un brouillon de commentaire dont la personne a perdu le droit d'envoi (niveau tombé sous
// « Peut commenter », page mise à la corbeille, onglet d'un autre schéma que le serveur) ne
// disparaît pas avec son champ : il est montré dans l'encart « Textes non envoyés », avec la
// raison (le message du serveur s'il a refusé ce texte, sinon « Tu ne peux plus écrire ici. »)
// et « Écarter », le texte sélectionnable. Si le droit revient, le brouillon retrouve son champ,
// sans rien envoyer de lui-même et sans prendre le focus : c'est la personne qui envoie.
import type { Level, Thread } from "@mementum/protocol";
import { act, useMemo, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError } from "../api";
import { message, thread, useMemoryComments } from "../test/comments";
import "../test/hooks";
import { CommentsPanel } from "./CommentsPanel";
import { createDrafts, DRAFT, type Drafts } from "./drafts";
import { useCommentsPanel, type CommentsPanelWiring } from "./useCommentsPanel";

let host: HTMLDivElement;
let root: Root;
const onClose = vi.fn();
const opened = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onClose.mockClear();
  opened.mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const LEVEL_REFUSAL = new ApiError(
  403,
  "forbidden",
  "niveau insuffisant sur cette page : il faut « Peut commenter », tu as « Peut lire »",
);

type PanelProps = {
  initial: Thread[];
  drafts: Drafts;
  level?: Level;
  trashed?: boolean;
  readOnly?: boolean;
  /** Ce que rend le serveur à l'ouverture d'un fil ; absent : il l'accepte. */
  refuse?: ApiError;
};

function Panel({ initial, drafts, level = "comment", trashed = false, readOnly = false, refuse }: PanelProps) {
  const base = useMemoryComments(initial);
  const comments = useMemo(
    () => ({
      ...base,
      open: async (blockId: string | null, body: string) => {
        opened(blockId, body);
        if (refuse) throw refuse;
        return base.open(blockId, body);
      },
    }),
    [base, refuse],
  );
  return (
    <CommentsPanel
      id="panneau"
      comments={comments}
      order={[]}
      level={level}
      trashed={trashed}
      readOnly={readOnly}
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
const textareas = () => host.querySelectorAll("textarea").length;
const unsent = () => host.querySelector<HTMLElement>('section[aria-label="Textes non envoyés"]');
const lines = () => [...(unsent()?.querySelectorAll("li") ?? [])].map((li) => li.textContent);
/** Le texte est à l'écran, dans un élément qu'on peut sélectionner. */
const visible = (text: string) =>
  [...host.querySelectorAll("p, textarea")].some((element) =>
    element instanceof HTMLTextAreaElement ? element.value === text : element.textContent === text,
  );

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

test("a_draft_stays_visible_when_the_level_drops_to_read", async () => {
  // La sonde du relecteur : le serveur refuse l'envoi (le niveau a baissé ailleurs), la page
  // relue dit « Peut lire », et les champs du panneau disparaissent.
  const drafts = createDrafts();
  const props = { initial: [thread("t")], drafts, refuse: LEVEL_REFUSAL };
  await act(async () => root.render(<Panel {...props} />));
  await type(field("Commenter la page")!, "mon brouillon");
  await click(button(host, "Commenter"));
  expect(opened).toHaveBeenCalledWith(null, "mon brouillon");
  await act(async () => root.render(<Panel {...props} level="read" />));
  expect(`visible: ${visible("mon brouillon")} | textareas: ${textareas()}`).toBe(
    "visible: true | textareas: 0",
  );
  // Dans l'encart, avec le message du serveur, et « Écarter ».
  expect(lines()).toEqual([
    "niveau insuffisant sur cette page : il faut « Peut commenter », tu as « Peut lire »" +
      "mon brouillon" +
      "Écarter",
  ]);
  expect(unsent()!.querySelector(".mm-comments-orphan-text")?.textContent).toBe("mon brouillon");
});

test("a_draft_stays_visible_when_the_page_is_trashed_elsewhere", async () => {
  // Rien n'a été envoyé : une réponse en cours d'écriture, et la page part à la corbeille.
  const drafts = createDrafts();
  const props = { initial: [thread("t")], drafts };
  await act(async () => root.render(<Panel {...props} />));
  await click(button(host, "Répondre"));
  await type(field("Répondre au fil")!, "ma réponse");
  await act(async () => root.render(<Panel {...props} trashed />));
  expect(`visible: ${visible("ma réponse")} | textareas: ${textareas()}`).toBe(
    "visible: true | textareas: 0",
  );
  expect(lines()).toEqual(["Tu ne peux plus écrire ici.ma réponseÉcarter"]);
  // Le focus n'est pas tombé sur `body` avec le champ.
  expect(document.activeElement).not.toBe(document.body);
  expect(host.contains(document.activeElement)).toBe(true);
});

test("every_kind_of_draft_is_kept_from_a_tab_of_another_schema", async () => {
  const drafts = createDrafts();
  const initial = [thread("t", { comments: [message("m", { body: "à corriger" })] })];
  await act(async () => root.render(<Panel initial={initial} drafts={drafts} />));
  await type(field("Commenter la page")!, "sur la page");
  await click(button(host, "Répondre"));
  await type(field("Répondre au fil")!, "en réponse");
  await click(button(host, "Modifier ce message"));
  await type(field("Modifier le message")!, "corrigé");
  await act(async () => root.render(<Panel initial={initial} drafts={drafts} readOnly />));
  expect(textareas()).toBe(0);
  expect(lines()).toEqual([
    "Tu ne peux plus écrire ici.sur la pageÉcarter",
    "Tu ne peux plus écrire ici.en réponseÉcarter",
    "Tu ne peux plus écrire ici.corrigéÉcarter",
  ]);
  // Le message qu'on modifiait se lit tel qu'il est sur le serveur.
  expect(host.querySelector(".mm-comment-body")?.textContent).toBe("à corriger");
});

test("a_draft_finds_its_field_again_when_the_right_comes_back_and_is_not_sent", async () => {
  const drafts = createDrafts();
  const props = { initial: [thread("t")], drafts };
  await act(async () => root.render(<Panel {...props} refuse={LEVEL_REFUSAL} />));
  await type(field("Commenter la page")!, "mon brouillon");
  await click(button(host, "Commenter"));
  await click(button(host, "Répondre"));
  await type(field("Répondre au fil")!, "ma réponse");
  await act(async () => root.render(<Panel {...props} level="read" />));
  expect(lines()).toHaveLength(2);
  // Le focus est ailleurs (dans l'éditeur) quand le droit revient.
  const outside = document.createElement("div");
  outside.contentEditable = "true";
  outside.tabIndex = 0;
  document.body.append(outside);
  await act(async () => outside.focus());
  await act(async () => root.render(<Panel {...props} level="comment" />));
  expect(unsent()).toBeNull();
  expect(field("Commenter la page")?.value).toBe("mon brouillon");
  expect(field("Répondre au fil")?.value).toBe("ma réponse");
  // Rien ne part tout seul, et aucun champ ne prend le focus.
  expect(opened).toHaveBeenCalledTimes(1);
  expect(document.activeElement).toBe(outside);
  // C'est la personne qui envoie.
  await click(button(host, "Commenter"));
  expect(opened).toHaveBeenCalledTimes(2);
  expect(opened).toHaveBeenLastCalledWith(null, "mon brouillon");
  outside.remove();
});

test("a_refusal_arriving_after_the_field_is_gone_becomes_the_reason", async () => {
  // L'envoi est en route quand la page relue (une reconnexion) baisse le niveau ; le refus
  // arrive ensuite.
  let refuse!: (error: ApiError) => void;
  const drafts = createDrafts();
  function Late({ level }: { level: Level }) {
    const base = useMemoryComments([]);
    const comments = useMemo(
      () => ({
        ...base,
        open: () => new Promise<Thread>((_, reject) => (refuse = reject)),
      }),
      [base],
    );
    return (
      <CommentsPanel
        id="panneau"
        comments={comments}
        order={[]}
        level={level}
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
  await act(async () => root.render(<Late level="comment" />));
  await type(field("Commenter la page")!, "en route");
  await click(button(host, "Commenter"));
  await act(async () => root.render(<Late level="read" />));
  expect(lines()).toEqual(["Tu ne peux plus écrire ici.en routeÉcarter"]);
  await act(async () => refuse(LEVEL_REFUSAL));
  expect(lines()).toEqual([
    "niveau insuffisant sur cette page : il faut « Peut commenter », tu as « Peut lire »" +
      "en route" +
      "Écarter",
  ]);
  expect(drafts.get(DRAFT.page)).toBe("en route");
});

test("a_dismissed_unsent_draft_is_gone_for_good", async () => {
  const drafts = createDrafts();
  const props = { initial: [], drafts };
  await act(async () => root.render(<Panel {...props} />));
  await type(field("Commenter la page")!, "à écarter");
  await act(async () => root.render(<Panel {...props} trashed />));
  await click(button(unsent()!, "Écarter ce texte"));
  expect(unsent()).toBeNull();
  expect(drafts.has(DRAFT.page)).toBe(false);
  // Le focus ne tombe pas sur `body` avec le bouton.
  expect(document.activeElement).not.toBe(document.body);
  await act(async () => root.render(<Panel {...props} />));
  expect(field("Commenter la page")?.value).toBe("");
});

// Le câblage réel du panneau (`useCommentsPanel`), pour le commentaire neuf d'un bloc.
let wiring: CommentsPanelWiring;

function Wired({ level }: { level: Level }) {
  const comments = useMemoryComments([]);
  const frame = useRef<HTMLElement>(null);
  wiring = useCommentsPanel(comments, frame);
  return (
    wiring.panel && (
      <CommentsPanel
        {...wiring.panel}
        comments={comments}
        level={level}
        trashed={false}
        readOnly={false}
        selfId="alice"
      />
    )
  );
}

test("a_block_comment_being_written_is_kept_and_comes_back_with_the_right", async () => {
  await act(async () => root.render(<Wired level="comment" />));
  await act(async () => wiring.commentBlock("b1"));
  await type(field("Commenter le bloc")!, "sur le bloc");
  await act(async () => root.render(<Wired level="read" />));
  expect(lines()).toEqual(["Tu ne peux plus écrire ici.sur le blocÉcarter"]);
  await act(async () => root.render(<Wired level="comment" />));
  expect(field("Commenter le bloc")?.value).toBe("sur le bloc");
  // Écarté sans le droit, il ne revient pas avec lui, ni sa zone vide.
  await act(async () => root.render(<Wired level="read" />));
  await click(button(unsent()!, "Écarter ce texte"));
  await act(async () => root.render(<Wired level="comment" />));
  expect(field("Commenter le bloc")).toBeNull();
});

test("typing_again_clears_the_refusal_of_a_draft", () => {
  const drafts = createDrafts();
  drafts.set(DRAFT.page, "texte");
  drafts.refuse(DRAFT.page, "refusé");
  expect(drafts.refusal(DRAFT.page)).toBe("refusé");
  drafts.set(DRAFT.page, "texte revu");
  expect(drafts.refusal(DRAFT.page)).toBeNull();
  drafts.refuse(DRAFT.page, "refusé");
  expect(drafts.startSending(DRAFT.page)).toBe(true);
  expect(drafts.refusal(DRAFT.page)).toBeNull();
  drafts.finishSending(DRAFT.page, "texte revu", false);
  drafts.refuse(DRAFT.page, "refusé");
  drafts.drop(DRAFT.page);
  expect(drafts.refusal(DRAFT.page)).toBeNull();
  expect(drafts.list()).toEqual([]);
});
