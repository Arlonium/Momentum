// Le panneau des commentaires d'une page : ce qu'il range où (blocs dans l'ordre de la page, puis
// la page, puis « Bloc supprimé », puis les fils résolus sur demande), ce qu'il laisse faire à qui
// (tableau des niveaux de R134), et comment il montre ce que le serveur dit : un refus tel quel,
// un texte hostile comme du texte.
import type { Level, Thread } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError } from "../api";
import { author, message, thread } from "../test/comments";
import "../test/hooks";
import { CommentsPanel, type PanelTarget } from "./CommentsPanel";
import { REACTIONS } from "./ThreadView";
import type { Comments } from "./useComments";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

/** Ce que rend `useComments`, ses écritures simulées. */
function fakeComments(threads: Thread[] | null, overrides: Partial<Comments> = {}): Comments {
  return {
    threads,
    state: threads === null ? "loading" : "ready",
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

type Options = {
  level?: Level;
  trashed?: boolean;
  readOnly?: boolean;
  order?: string[] | null;
  target?: PanelTarget | null;
  selfId?: string;
};

const onClose = vi.fn();
const onShowBlock = vi.fn();
const onTargetDone = vi.fn();

beforeEach(() => {
  onClose.mockClear();
  onShowBlock.mockClear();
  onTargetDone.mockClear();
});

async function show(comments: Comments, options: Options = {}) {
  await act(async () =>
    root.render(
      <CommentsPanel
        id="panneau"
        comments={comments}
        order={options.order === undefined ? [] : options.order}
        level={options.level ?? "full"}
        trashed={options.trashed ?? false}
        readOnly={options.readOnly ?? false}
        selfId={options.selfId ?? "alice"}
        target={options.target ?? null}
        excerpt={() => "Le texte du bloc"}
        onShowBlock={onShowBlock}
        onTargetDone={onTargetDone}
        onClose={onClose}
      />,
    ),
  );
}

const list = (name: string) => host.querySelector<HTMLUListElement>(`ul[aria-label="${name}"]`);
const bodies = (scope: ParentNode | null) =>
  [...(scope?.querySelectorAll(".mm-comment-body") ?? [])].map((p) => p.textContent);
const buttons = (scope: ParentNode, name: string) =>
  [...scope.querySelectorAll("button")].filter(
    (b) => (b.getAttribute("aria-label") ?? b.textContent ?? "").trim() === name,
  );
const textareas = () => [...host.querySelectorAll("textarea")];

/** Tape `text` dans un champ, comme le ferait un clavier (React lit l'événement `input`). */
async function type(field: HTMLTextAreaElement, text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(field, text);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });

test("threads_follow_the_blocks_of_the_page_then_the_page", async () => {
  const threads = [
    thread("page", { comments: [message("m-page", { body: "sur la page" })] }),
    thread("b2", { blockId: "deux", comments: [message("m-b2", { body: "bloc deux" })] }),
    thread("b1", { blockId: "un", comments: [message("m-b1", { body: "bloc un" })] }),
  ];
  await show(fakeComments(threads), { order: ["un", "deux"] });
  expect(bodies(list("Fils ouverts"))).toEqual(["bloc un", "bloc deux", "sur la page"]);
});

test("orphan_thread_is_listed_apart", async () => {
  const threads = [
    thread("vivant", { blockId: "un", comments: [message("m1", { body: "bloc présent" })] }),
    thread("orphelin", { blockId: "parti", comments: [message("m2", { body: "bloc parti" })] }),
  ];
  await show(fakeComments(threads), { order: ["un"] });
  expect(bodies(list("Fils ouverts"))).toEqual(["bloc présent"]);
  const section = list("Fils d'un bloc supprimé")!.closest("section")!;
  expect(section.querySelector("h3")?.textContent).toBe("Bloc supprimé");
  expect(bodies(section)).toEqual(["bloc parti"]);
  // Toujours lisible, on y répond et on le résout ; pas de bloc où aller.
  expect(buttons(section, "Répondre")).toHaveLength(1);
  expect(buttons(section, "Résoudre")).toHaveLength(1);
  expect(buttons(section, "Aller au bloc")).toHaveLength(0);
});

test("before_the_document_is_known_no_thread_is_said_deleted", async () => {
  const threads = [thread("t", { blockId: "un", comments: [message("m", { body: "bloc" })] })];
  await show(fakeComments(threads), { order: null });
  expect(bodies(list("Fils ouverts"))).toEqual(["bloc"]);
  expect(list("Fils d'un bloc supprimé")).toBeNull();
});

test("resolved_threads_are_hidden_until_asked", async () => {
  const threads = [
    thread("ouvert", { comments: [message("m1", { body: "encore ouvert" })] }),
    thread("clos", {
      resolvedAt: 2_000,
      resolvedBy: author("bruno"),
      comments: [message("m2", { body: "déjà résolu" })],
    }),
  ];
  await show(fakeComments(threads));
  expect(host.textContent).not.toContain("déjà résolu");
  const toggle = buttons(host, "Fils résolus (1)")[0];
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  await click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(bodies(list("Fils résolus"))).toEqual(["déjà résolu"]);
  // Qui l'a ouvert le rouvre.
  expect(buttons(list("Fils résolus")!, "Rouvrir")).toHaveLength(1);
  await click(toggle);
  expect(host.textContent).not.toContain("déjà résolu");
});

test("composer_is_absent_below_comment_level", async () => {
  const threads = [
    thread("t", {
      openedBy: author("bruno"),
      comments: [
        message("m", {
          author: author("bruno"),
          body: "à lire",
          reactions: [{ emoji: "👍", users: [author("bruno")] }],
        }),
      ],
    }),
  ];
  // « Peut lire » : les fils se lisent, rien ne s'écrit.
  await show(fakeComments(threads), { level: "read" });
  expect(bodies(host)).toEqual(["à lire"]);
  expect(textareas()).toHaveLength(0);
  expect(buttons(host, "Répondre")).toHaveLength(0);
  expect(buttons(host, "Résoudre")).toHaveLength(0);
  expect(buttons(host, "Ajouter une réaction")).toHaveLength(0);
  // La réaction se voit, sans être un bouton.
  const reactions = list("Réactions")!;
  expect(reactions.textContent).toContain("👍");
  expect(reactions.querySelector("button")).toBeNull();

  // « Peut commenter » : on écrit, on répond, on réagit ; le fil de Bruno ne se résout pas.
  await show(fakeComments(threads), { level: "comment" });
  expect(textareas()).toHaveLength(1);
  expect(buttons(host, "Répondre")).toHaveLength(1);
  expect(buttons(host, "Ajouter une réaction")).toHaveLength(1);
  expect(buttons(host, "Résoudre")).toHaveLength(0);
});

test("in_the_trash_everything_reads_and_nothing_writes", async () => {
  const threads = [thread("t", { comments: [message("m", { body: "à la corbeille" })] })];
  await show(fakeComments(threads), { level: "full", trashed: true });
  expect(bodies(host)).toEqual(["à la corbeille"]);
  expect(textareas()).toHaveLength(0);
  for (const name of ["Répondre", "Résoudre", "Modifier ce message", "Supprimer ce message"]) {
    expect(buttons(host, name)).toHaveLength(0);
  }
  // Un onglet d'un autre schéma (R35) n'écrit pas non plus.
  await show(fakeComments(threads), { level: "full", readOnly: true });
  expect(textareas()).toHaveLength(0);
  expect(buttons(host, "Répondre")).toHaveLength(0);
});

test("each_button_follows_the_table_of_levels", async () => {
  const mine = message("à-moi", { author: author("alice"), body: "le mien" });
  const theirs = message("à-bruno", { author: author("bruno"), body: "le sien" });
  const opened = thread("t", { openedBy: author("bruno"), comments: [theirs, mine] });
  const messageOf = (body: string) =>
    [...host.querySelectorAll<HTMLElement>(".mm-comment")].find((li) =>
      li.textContent?.includes(body),
    )!;

  // « Peut commenter » : son message se modifie et se supprime, pas celui des autres ; le fil
  // de Bruno ne se résout pas.
  await show(fakeComments([opened]), { level: "comment" });
  expect(buttons(messageOf("le mien"), "Modifier ce message")).toHaveLength(1);
  expect(buttons(messageOf("le mien"), "Supprimer ce message")).toHaveLength(1);
  expect(buttons(messageOf("le sien"), "Modifier ce message")).toHaveLength(0);
  expect(buttons(messageOf("le sien"), "Supprimer ce message")).toHaveLength(0);
  expect(buttons(host, "Résoudre")).toHaveLength(0);

  // Le fil ouvert par Alice se résout avec « Peut commenter », même sans son premier message.
  const hers = thread("t", { openedBy: author("alice"), comments: [theirs] });
  await show(fakeComments([hers]), { level: "comment" });
  expect(buttons(host, "Résoudre")).toHaveLength(1);

  // « Peut modifier » résout tout fil ; « Accès complet » supprime tout message, et ne
  // modifie toujours que le sien.
  await show(fakeComments([opened]), { level: "edit" });
  expect(buttons(host, "Résoudre")).toHaveLength(1);
  expect(buttons(messageOf("le sien"), "Supprimer ce message")).toHaveLength(0);
  await show(fakeComments([opened]), { level: "full" });
  expect(buttons(messageOf("le sien"), "Supprimer ce message")).toHaveLength(1);
  expect(buttons(messageOf("le sien"), "Modifier ce message")).toHaveLength(0);
});

test("a_refusal_is_shown_as_the_server_says_it", async () => {
  const said = "il faut « Peut commenter » sur cette page pour le faire";
  const comments = fakeComments([thread("t")], {
    reply: vi.fn(async () => {
      throw new ApiError(403, "forbidden", said);
    }),
    resolve: vi.fn(async () => {
      throw new ApiError(409, "trashed", "cette page est à la corbeille");
    }),
  });
  await show(comments, { level: "comment" });
  await click(buttons(host, "Répondre")[0]);
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Répondre au fil"]')!;
  await type(field, "Ma réponse");
  await click(buttons(host, "Envoyer la réponse")[0]);
  expect(vi.mocked(comments.reply)).toHaveBeenCalledWith("t", "Ma réponse");
  const alerts = () => [...host.querySelectorAll('[role="alert"]')].map((a) => a.textContent);
  expect(alerts()).toEqual([said]);
  // Le texte tapé reste, pour un nouvel essai.
  expect(field.value).toBe("Ma réponse");

  // Une action du fil refusée : son message aussi, tel quel.
  await click(buttons(host, "Résoudre")[0]);
  expect(alerts()).toContain("cette page est à la corbeille");
});

test("a_page_comment_is_sent_and_the_field_emptied", async () => {
  const comments = fakeComments([]);
  await show(comments, { level: "comment" });
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Commenter la page"]')!;
  const send = buttons(host, "Commenter")[0];
  // Rien à envoyer tant que le champ est vide ou blanc.
  await type(field, "   ");
  await click(send);
  expect(vi.mocked(comments.open)).not.toHaveBeenCalled();
  await type(field, "Première ligne\nseconde");
  await click(send);
  expect(vi.mocked(comments.open)).toHaveBeenCalledWith(null, "Première ligne\nseconde");
  expect(field.value).toBe("");
});

test("ctrl_enter_sends_the_comment", async () => {
  const comments = fakeComments([]);
  await show(comments, { level: "comment" });
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Commenter la page"]')!;
  await type(field, "au clavier");
  await act(async () => {
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true }));
  });
  expect(vi.mocked(comments.open)).toHaveBeenCalledWith(null, "au clavier");
});

test("a_block_comment_is_written_from_its_target", async () => {
  const comments = fakeComments([]);
  await show(comments, { level: "comment", target: { kind: "block", blockId: "un", seq: 1 } });
  const field = host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Commenter le bloc"]')!;
  expect(document.activeElement).toBe(field);
  expect(host.textContent).toContain("Le texte du bloc");
  await type(field, "sur ce bloc");
  await click(buttons(host, "Commenter le bloc")[0]);
  expect(vi.mocked(comments.open)).toHaveBeenCalledWith("un", "sur ce bloc");
  expect(onTargetDone).toHaveBeenCalled();
});

test("text_from_others_is_shown_as_isolated_text", async () => {
  const hostile = "‮evil <b>gras</b>\n⁦ligne⁩";
  const name = "‮trom <img src=x onerror=alert(1)>";
  const emoji = "‮👍x";
  const threads = [
    thread("t", {
      openedBy: author("bruno", name),
      comments: [
        message("m", {
          author: author("bruno", name),
          body: hostile,
          reactions: [{ emoji, users: [author("bruno", name)] }],
        }),
      ],
    }),
  ];
  await show(fakeComments(threads), { level: "comment" });
  expect(host.querySelector("b")).toBeNull();
  expect(host.querySelector("img")).toBeNull();
  const body = host.querySelector<HTMLElement>(".mm-comment-body")!;
  expect(body.textContent).toBe(hostile);
  const who = host.querySelector<HTMLElement>(".mm-comment-author")!;
  expect(who.textContent).toBe(name);
  // Chaque texte venu d'ailleurs est isolé : il ne déborde pas sur ce qui l'entoure.
  for (const element of [body, who]) {
    expect(element.getAttribute("dir")).toBe("auto");
    expect(element.classList.contains("mm-isolate")).toBe(true);
  }
  const reaction = list("Réactions")!.querySelector<HTMLElement>(".mm-reaction-emoji")!;
  expect(reaction.textContent).toBe(emoji);
  expect(reaction.getAttribute("dir")).toBe("auto");
  expect(reaction.classList.contains("mm-isolate")).toBe(true);
  // Rien de ces textes n'entre dans un attribut `style`.
  for (const element of host.querySelectorAll<HTMLElement>("[style]")) {
    expect(element.getAttribute("style")).not.toContain("evil");
    expect(element.getAttribute("style")).not.toContain("trom");
  }
});

test("the_six_reactions_are_exact_and_toggle", async () => {
  expect(REACTIONS).toEqual(["👍", "❤️", "😄", "🎉", "👀", "✅"]);
  // ❤️ porte son sélecteur de variante : pour le serveur, c'est un autre emoji que ❤.
  expect([...REACTIONS[1]].map((c) => c.codePointAt(0))).toEqual([0x2764, 0xfe0f]);
  const comments = fakeComments([
    thread("t", {
      comments: [message("m", { reactions: [{ emoji: "🎉", users: [author("alice")] }] })],
    }),
  ]);
  await show(comments, { level: "comment" });
  // La sienne se retire d'un clic.
  const mine = list("Réactions")!.querySelector<HTMLButtonElement>("button[aria-pressed]")!;
  expect(mine.getAttribute("aria-pressed")).toBe("true");
  await click(mine);
  expect(vi.mocked(comments.react)).toHaveBeenLastCalledWith("t", "m", "🎉", false);
  // Le choix des six, au clavier comme à la souris.
  await click(buttons(host, "Ajouter une réaction")[0]);
  const picker = host.querySelector<HTMLElement>('[role="group"][aria-label="Choisir une réaction"]')!;
  expect([...picker.querySelectorAll("button")].map((b) => b.textContent)).toEqual([...REACTIONS]);
  await click(buttons(picker, "Réagir avec 👍")[0]);
  expect(vi.mocked(comments.react)).toHaveBeenLastCalledWith("t", "m", "👍", true);
});

test("escape_closes_the_panel_but_not_from_a_dialog", async () => {
  await show(fakeComments([thread("t")]), { level: "full" });
  const panel = host.querySelector<HTMLElement>("#panneau")!;
  await act(async () => {
    panel.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  expect(onClose).toHaveBeenCalledTimes(1);
  // La confirmation d'une suppression a son propre Échap.
  await click(buttons(host, "Supprimer ce message")[0]);
  const dialog = host.querySelector("dialog")!;
  await act(async () => {
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a_block_thread_leads_to_its_block", async () => {
  await show(fakeComments([thread("t", { blockId: "un" })]), { order: ["un"] });
  await click(buttons(host, "Aller au bloc")[0]);
  expect(onShowBlock).toHaveBeenCalledWith("un");
  // Un clic dans le fil, hors de ses boutons, y mène aussi.
  onShowBlock.mockClear();
  await click(host.querySelector(".mm-comment-body")!);
  expect(onShowBlock).toHaveBeenCalledWith("un");
});

test("a_target_thread_takes_the_focus", async () => {
  await show(fakeComments([thread("a"), thread("b", { blockId: "un" })]), {
    order: ["un"],
    target: { kind: "thread", threadId: "b", seq: 1 },
  });
  expect(document.activeElement?.getAttribute("data-thread-id")).toBe("b");
});

test("the_lists_are_lists_of_items", async () => {
  await show(fakeComments([thread("a"), thread("b")]));
  const items = [...list("Fils ouverts")!.children];
  expect(items).toHaveLength(2);
  for (const item of items) expect(item.tagName).toBe("LI");
  for (const messages of host.querySelectorAll('ul[aria-label="Messages"]')) {
    for (const item of messages.children) expect(item.tagName).toBe("LI");
  }
});

test("names_and_emojis_in_attributes_are_isolated", async () => {
  // Un caractère de retournement dans un nom ne retourne pas les noms qui le suivent.
  const evil = author("bruno", "‮onurb");
  const threads = [
    thread("t", {
      openedBy: evil,
      comments: [
        message("m", {
          reactions: [{ emoji: "‮👍", users: [evil, author("carla", "Carla")] }],
        }),
      ],
    }),
  ];
  await show(fakeComments(threads), { level: "comment" });
  const article = host.querySelector("article")!;
  expect(article.getAttribute("aria-label")).toBe("Fil ouvert par ⁨‮onurb⁩");
  const chip = list("Réactions")!.querySelector<HTMLButtonElement>("button[aria-pressed]")!;
  expect(chip.getAttribute("title")).toBe("⁨‮onurb⁩, ⁨Carla⁩");
  expect(chip.getAttribute("aria-label")).toBe("⁨‮👍⁩ 2");
});

test("an_absurd_date_shows_a_neutral_text", async () => {
  const threads = [
    thread("t", {
      resolvedAt: Number.NaN,
      resolvedBy: author("bruno"),
      comments: [message("m", { createdAt: 1e20, editedAt: -1e20, body: "daté de nulle part" })],
    }),
  ];
  await show(fakeComments(threads));
  await click(buttons(host, "Fils résolus (1)")[0]);
  expect(host.textContent).toContain("daté de nulle part");
  const time = host.querySelector<HTMLTimeElement>(".mm-comment-time")!;
  expect(time.textContent).toBe("date inconnue");
  expect(time.hasAttribute("datetime")).toBe(false);
  expect(host.textContent).toContain("Résolu par BRUNO, date inconnue");
});
