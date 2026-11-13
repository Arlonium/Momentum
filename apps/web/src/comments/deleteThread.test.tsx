// « Supprimer le fil » (R352, R360 à R364) : pour qui a l'accès complet à la page et écrit d'ici,
// tout le fil part, avec les messages de tous et leurs réactions (`DELETE …/comments/{threadId}`),
// après une confirmation qui dit combien de messages partent. Ce que ces tests tiennent :
// - le bouton n'est montré qu'avec l'accès complet, hors corbeille et depuis un onglet du schéma
//   du serveur, sur un fil ouvert, résolu ou d'un bloc supprimé ;
// - Échap et « Annuler » ne suppriment rien, laissent le panneau ouvert et rendent le focus au
//   bouton ; un double clic ne supprime rien de plus ;
// - la suppression confirmée appelle la route une fois, relit la liste, et le focus va au fil
//   voisin, ou au titre du panneau, jamais à `body` ; un brouillon du fil rejoint « Textes non
//   envoyés » ;
// - un refus `403` ou `409` s'affiche tel quel et fait relire la page ; un fil déjà parti (`404`)
//   n'est pas une erreur.
// Le hook des fils est le vrai (`useComments`), l'API est simulée : le test tient le serveur.
import type { Level, Thread } from "@mementum/protocol";
import { act, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, deleteThread, getComments } from "../api";
import type { PageEvent } from "../editor/collab";
import { author, message, thread, useMemoryComments } from "../test/comments";
import { deferred, until } from "../test/hooks";
import { CommentsPanel } from "./CommentsPanel";
import { useComments, type CommentsSession } from "./useComments";
import { useCommentsPanel, type CommentsPanelWiring } from "./useCommentsPanel";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getComments: vi.fn(),
  deleteThread: vi.fn(),
}));

/** Le bouton de chaque fil. */
const DELETE = "Supprimer le fil";

/** Les fils tels que le serveur les garde : `getComments` les rend, `deleteThread` en retire. */
let server: Thread[] = [];
let host: HTMLDivElement;
let root: Root;
const onRefused = vi.fn();
const onClose = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onRefused.mockClear();
  onClose.mockClear();
  server = [];
  vi.mocked(getComments).mockReset();
  vi.mocked(getComments).mockImplementation(async () => structuredClone(server));
  vi.mocked(deleteThread).mockReset();
  vi.mocked(deleteThread).mockImplementation(async (_page: string, threadId: string) => {
    server = server.filter((t) => t.id !== threadId);
  });
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const buttons = (scope: ParentNode, name: string) =>
  [...scope.querySelectorAll("button")].filter(
    (b) => (b.getAttribute("aria-label") ?? b.textContent ?? "").trim() === name,
  );
const button = (scope: ParentNode, name: string) => {
  const found = buttons(scope, name);
  if (found.length !== 1) throw new Error(`${found.length} boutons « ${name} »`);
  return found[0];
};
const threadOf = (id: string) => host.querySelector<HTMLElement>(`[data-thread-id="${id}"]`);
const field = (scope: ParentNode, name: string) =>
  scope.querySelector<HTMLTextAreaElement>(`textarea[aria-label="${name}"]`)!;
const dialog = () => host.querySelector<HTMLDialogElement>("dialog");
const panel = () => host.querySelector<HTMLElement>("aside.mm-comments");
const alerts = () => [...host.querySelectorAll('[role="alert"]')].map((a) => a.textContent);
const active = () => document.activeElement as HTMLElement;

/** Un clic de souris : le bouton prend le focus, puis le clic. */
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

/** Échap dans une fenêtre modale : la touche part de l'élément qui a le focus, puis `cancel`. */
async function escapeFromDialog() {
  await act(async () => {
    active().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    dialog()!.dispatchEvent(new Event("cancel", { cancelable: true }));
  });
}

/** Le focus est dans le panneau, sur un élément encore là : jamais `body`. */
function focusIsInThePanel() {
  expect(active()).not.toBe(document.body);
  expect(active().isConnected).toBe(true);
  expect(panel()!.contains(active())).toBe(true);
}

// Le bouton : qui le voit, et sur quels fils.

function Static({
  initial,
  level = "full",
  trashed = false,
  readOnly = false,
  order = [],
  absent,
}: {
  initial: Thread[];
  level?: Level;
  trashed?: boolean;
  readOnly?: boolean;
  order?: string[];
  absent?: string[];
}) {
  const comments = useMemoryComments(initial);
  return (
    <CommentsPanel
      id="panneau"
      comments={comments}
      order={order}
      absent={absent}
      level={level}
      trashed={trashed}
      readOnly={readOnly}
      selfId="alice"
      target={null}
      excerpt={() => null}
      onShowBlock={() => {}}
      onTargetDone={() => {}}
      onClose={onClose}
    />
  );
}

test("delete_thread_is_offered_only_with_full_access", async () => {
  // Le fil est d'Alice, qui l'a ouvert : elle y a tous les autres boutons de son niveau.
  const initial = [thread("t")];
  const shown: Record<string, number> = {};
  for (const level of ["read", "comment", "edit", "full"] as const) {
    await act(async () => root.render(<Static key={level} initial={initial} level={level} />));
    shown[level] = buttons(host, DELETE).length;
  }
  expect(shown).toEqual({ read: 0, comment: 0, edit: 0, full: 1 });
});

test("delete_thread_is_not_offered_in_the_trash_nor_from_a_tab_of_another_schema", async () => {
  const initial = [thread("t")];
  await act(async () => root.render(<Static key="corbeille" initial={initial} trashed />));
  expect(buttons(host, DELETE)).toHaveLength(0);
  await act(async () => root.render(<Static key="schéma" initial={initial} readOnly />));
  expect(buttons(host, DELETE)).toHaveLength(0);
  // Témoin : le même fil, la même personne, hors corbeille et du schéma du serveur.
  await act(async () => root.render(<Static key="témoin" initial={initial} />));
  expect(buttons(host, DELETE)).toHaveLength(1);
});

test("delete_thread_is_offered_on_open_resolved_and_deleted_block_threads", async () => {
  const initial = [
    thread("ouvert", { blockId: "là" }),
    thread("orphelin", { blockId: "parti" }),
    thread("résolu", { resolvedAt: 5, resolvedBy: author("bruno") }),
  ];
  await act(async () =>
    root.render(<Static initial={initial} order={["là"]} absent={["parti"]} />),
  );
  await click(button(host, "Fils résolus (1)"));
  const lists = ["Fils ouverts", "Fils d'un bloc supprimé", "Fils résolus"].map((name) =>
    host.querySelector(`ul[aria-label="${name}"]`),
  );
  expect(lists.map((list) => (list ? buttons(list, DELETE).length : null))).toEqual([1, 1, 1]);
});

test("the_confirmation_says_how_many_messages_go_and_that_it_cannot_be_undone", async () => {
  const initial = [
    thread("trois", {
      comments: [
        message("m1"),
        message("m2", { author: author("bruno") }),
        message("m3", { author: author("carla") }),
      ],
    }),
    thread("un"),
  ];
  await act(async () => root.render(<Static initial={initial} />));
  await click(button(threadOf("trois")!, DELETE));
  expect(dialog()?.getAttribute("role")).toBe("alertdialog");
  expect(dialog()!.textContent).toContain("Supprimer ce fil ?");
  expect(dialog()!.textContent).toContain("ses 3 messages");
  expect(dialog()!.textContent).toContain("Cela ne se défait pas.");
  await click(button(dialog()!, "Annuler"));
  await click(button(threadOf("un")!, DELETE));
  expect(dialog()!.textContent).toContain("son message");
  expect(dialog()!.textContent).not.toContain("messages");
});

test("cancelling_after_the_thread_changed_list_gives_the_focus_to_its_new_button", async () => {
  // La fenêtre est ouverte quand le bloc du fil est supprimé dans l'éditeur : le fil passe sous
  // « Bloc supprimé », et son élément est recréé, bouton compris.
  const initial = [thread("t", { blockId: "b" })];
  await act(async () => root.render(<Static initial={initial} order={["b"]} absent={[]} />));
  await click(button(threadOf("t")!, DELETE));
  await act(async () => root.render(<Static initial={initial} order={[]} absent={["b"]} />));
  const moved = host.querySelector('ul[aria-label="Fils d\'un bloc supprimé"] [data-thread-id="t"]');
  expect(moved).not.toBeNull();
  await click(button(dialog()!, "Annuler"));
  expect(dialog()).toBeNull();
  expect(active()).toBe(button(threadOf("t")!, DELETE));
});

// La suppression, avec le vrai hook des fils.

/** Une session de synchro réduite à ce que lit `useComments` : synchronisée, et ses trames. */
function fakeSession() {
  const events = new Set<(event: PageEvent) => void>();
  return {
    synced: true,
    status: "connected",
    subscribe: () => () => {},
    onPageEvent(listener: (event: PageEvent) => void) {
      events.add(listener);
      return () => events.delete(listener);
    },
    /** Le serveur envoie la trame `comments` : une écriture d'ailleurs. */
    frame() {
      for (const listener of [...events]) listener("comments");
    },
  } satisfies CommentsSession & { frame(): void };
}

let wiring: CommentsPanelWiring;

type Rights = { level?: Level; trashed?: boolean; readOnly?: boolean };

function Live({
  session,
  level = "full",
  trashed = false,
  readOnly = false,
}: { session: CommentsSession } & Rights) {
  const comments = useComments("p", session, { onRefused });
  const frame = useRef<HTMLElement>(null);
  wiring = useCommentsPanel(comments, frame);
  return (
    wiring.panel && (
      <CommentsPanel
        {...wiring.panel}
        comments={comments}
        level={level}
        trashed={trashed}
        readOnly={readOnly}
        selfId="alice"
      />
    )
  );
}

/** Le panneau ouvert sur les fils `threads` du serveur, une fois lus. */
async function live(threads: Thread[]) {
  server = threads;
  const session = fakeSession();
  await act(async () => root.render(<Live session={session} />));
  await act(async () => wiring.button.onClick());
  await until(host, () => threads.every((t) => threadOf(t.id) !== null), "les fils lus");
  return session;
}

/** Ouvre la confirmation sur le fil `id`, puis la confirme. */
async function confirmDeletion(id: string) {
  await click(button(threadOf(id)!, DELETE));
  await click(button(dialog()!, "Supprimer"));
}

test("cancelling_the_confirmation_deletes_nothing_and_gives_the_focus_back", async () => {
  await live([thread("a"), thread("b")]);
  const asked = button(threadOf("b")!, DELETE);
  await click(asked);
  expect(dialog()).not.toBeNull();
  expect(active().textContent).toBe("Annuler");
  // « Annuler ».
  await click(button(dialog()!, "Annuler"));
  expect(dialog()).toBeNull();
  expect(vi.mocked(deleteThread)).not.toHaveBeenCalled();
  expect(panel()).not.toBeNull();
  expect(active()).toBe(button(threadOf("b")!, DELETE));
  // Échap : seulement la fenêtre, pas le panneau derrière elle.
  await click(button(threadOf("b")!, DELETE));
  await escapeFromDialog();
  expect(dialog()).toBeNull();
  expect(vi.mocked(deleteThread)).not.toHaveBeenCalled();
  expect(panel()).not.toBeNull();
  expect(wiring.button.expanded).toBe(true);
  expect(active()).toBe(button(threadOf("b")!, DELETE));
  expect(threadOf("b")).not.toBeNull();
});

test("a_confirmed_deletion_calls_the_route_once_rereads_and_focuses_a_neighbor", async () => {
  await live([thread("a"), thread("b"), thread("c")]);
  const reads = vi.mocked(getComments).mock.calls.length;
  await confirmDeletion("b");
  expect(vi.mocked(deleteThread)).toHaveBeenCalledExactlyOnceWith("p", "b");
  expect(threadOf("b")).toBeNull();
  expect(dialog()).toBeNull();
  // Le fil qui le suivait.
  focusIsInThePanel();
  expect(active()).toBe(threadOf("c"));
  // Et la liste est relue, comme après toute écriture d'ici.
  await until(host, () => vi.mocked(getComments).mock.calls.length > reads, "la relecture");
  // Le dernier de la liste : le fil d'avant.
  await confirmDeletion("c");
  focusIsInThePanel();
  expect(active()).toBe(threadOf("a"));
  // Le seul qui reste : le titre du panneau.
  await confirmDeletion("a");
  focusIsInThePanel();
  expect(active().textContent).toBe("Commentaires");
  expect(vi.mocked(deleteThread).mock.calls.map(([, id]) => id)).toEqual(["b", "c", "a"]);
  expect(host.textContent).toContain("Aucun fil ouvert.");
});

test("a_double_click_deletes_nothing_more", async () => {
  await live([thread("a"), thread("b")]);
  const answer = deferred<void>();
  vi.mocked(deleteThread).mockImplementation(() => answer.promise);
  // Un double clic sur le bouton du fil : une seule fenêtre, rien d'envoyé.
  const asked = button(threadOf("b")!, DELETE);
  await act(async () => {
    asked.focus();
    asked.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
    asked.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 2 }));
  });
  expect(host.querySelectorAll("dialog")).toHaveLength(1);
  expect(vi.mocked(deleteThread)).not.toHaveBeenCalled();
  // Un double clic sur « Supprimer », coup sur coup, avant tout rendu : un seul envoi.
  const confirm = button(dialog()!, "Supprimer");
  await act(async () => {
    confirm.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 1 }));
    confirm.dispatchEvent(new MouseEvent("click", { bubbles: true, detail: 2 }));
  });
  expect(vi.mocked(deleteThread)).toHaveBeenCalledTimes(1);
  server = server.filter((t) => t.id !== "b");
  await act(async () => answer.resolve());
  expect(threadOf("b")).toBeNull();
  expect(vi.mocked(deleteThread)).toHaveBeenCalledTimes(1);
  focusIsInThePanel();
});

test("drafts_of_the_deleted_thread_go_to_unsent_texts", async () => {
  await live([thread("a"), thread("b", { comments: [message("b-1", { body: "à corriger" })] })]);
  const b = threadOf("b")!;
  await click(button(b, "Répondre"));
  await type(field(b, "Répondre au fil"), "ma réponse");
  await click(button(b, "Modifier ce message"));
  await type(field(b, "Modifier le message"), "corrigé");
  await confirmDeletion("b");
  expect(threadOf("b")).toBeNull();
  const unsent = host.querySelector('section[aria-label="Textes non envoyés"]');
  expect([...(unsent?.querySelectorAll("li") ?? [])].map((li) => li.textContent)).toEqual([
    "Ce fil a été supprimé. Ton texte :ma réponseÉcarter",
    "Ce message a été supprimé. Ton texte :corrigéÉcarter",
  ]);
  focusIsInThePanel();
});

test("a_refusal_is_shown_as_said_and_the_page_is_reread", async () => {
  const refusals = [
    new ApiError(
      403,
      "forbidden",
      "niveau insuffisant sur cette page : il faut « Accès complet », tu as « Peut modifier »",
    ),
    new ApiError(
      409,
      "trashed",
      "la page est à la corbeille : elle se lit, mais ne s'écrit plus ; la restaurer d'abord",
    ),
  ];
  for (const refusal of refusals) {
    await act(async () => root.render(null));
    onRefused.mockClear();
    vi.mocked(deleteThread).mockRejectedValue(refusal);
    await live([thread("a"), thread("b")]);
    const reads = vi.mocked(getComments).mock.calls.length;
    await confirmDeletion("b");
    expect(vi.mocked(deleteThread)).toHaveBeenCalledWith("p", "b");
    expect(dialog()).toBeNull();
    // Dans le fil, tel que le serveur le dit.
    expect(threadOf("b")).not.toBeNull();
    const said = [...threadOf("b")!.querySelectorAll('[role="alert"]')].map((a) => a.textContent);
    expect(said).toEqual([refusal.message]);
    expect(onRefused).toHaveBeenCalledExactlyOnceWith(refusal);
    await until(host, () => vi.mocked(getComments).mock.calls.length > reads, "la relecture");
    focusIsInThePanel();
    expect(active()).toBe(button(threadOf("b")!, DELETE));
  }
});

test("a_thread_already_gone_is_not_an_error", async () => {
  await live([thread("a"), thread("b")]);
  const reads = vi.mocked(getComments).mock.calls.length;
  // Supprimé ailleurs entre-temps : la route répond `404`.
  server = server.filter((t) => t.id !== "b");
  vi.mocked(deleteThread).mockRejectedValue(
    new ApiError(404, "not_found", "fil de commentaires introuvable sur cette page"),
  );
  await confirmDeletion("b");
  expect(threadOf("b")).toBeNull();
  expect(alerts()).toEqual([]);
  expect(onRefused).not.toHaveBeenCalled();
  await until(host, () => vi.mocked(getComments).mock.calls.length > reads, "la relecture");
  focusIsInThePanel();
  expect(active()).toBe(threadOf("a"));
});

test("the_confirmation_closes_when_the_thread_is_deleted_elsewhere", async () => {
  const session = await live([thread("a"), thread("b")]);
  await click(button(threadOf("b")!, DELETE));
  expect(dialog()).not.toBeNull();
  // Quelqu'un d'autre le supprime : la trame `comments` fait relire.
  server = server.filter((t) => t.id !== "b");
  await act(async () => session.frame());
  await until(host, () => threadOf("b") === null, "le fil relu sans b");
  expect(dialog()).toBeNull();
  expect(vi.mocked(deleteThread)).not.toHaveBeenCalled();
  focusIsInThePanel();
});

test("the_confirmation_closes_when_the_right_goes", async () => {
  // La fenêtre est ouverte quand la page relue (un refus ailleurs, la vérification avant une
  // connexion de synchro) dit que le droit est parti : elle se ferme, et rien ne part, même par
  // son bouton s'il était resté.
  const cases: Record<string, Rights> = {
    "niveau baissé": { level: "edit" },
    corbeille: { trashed: true },
    "autre schéma": { readOnly: true },
  };
  const seen: Record<string, string> = {};
  for (const [name, rights] of Object.entries(cases)) {
    await act(async () => root.render(null));
    const session = await live([thread("a"), thread("b")]);
    await click(button(threadOf("b")!, DELETE));
    expect(dialog()).not.toBeNull();
    await act(async () => root.render(<Live session={session} {...rights} />));
    const open = dialog();
    const focus = active() === threadOf("b") ? "le fil" : active().tagName;
    seen[name] = `fenêtre ${open === null ? "fermée" : "ouverte"}, focus : ${focus}`;
    const left = open === null ? [] : buttons(open, "Supprimer");
    for (const confirm of left) await click(confirm);
  }
  expect(seen).toEqual({
    "niveau baissé": "fenêtre fermée, focus : le fil",
    corbeille: "fenêtre fermée, focus : le fil",
    "autre schéma": "fenêtre fermée, focus : le fil",
  });
  expect(vi.mocked(deleteThread)).not.toHaveBeenCalled();
});
