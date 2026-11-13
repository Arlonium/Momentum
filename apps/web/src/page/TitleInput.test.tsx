// Le titre n'envoie que ce que la personne a tapé. Tant qu'elle n'a rien tapé, le champ suit
// le titre distant (un renommage fait ailleurs, que la barre latérale en direct apporte), même
// quand il a le focus ; dès qu'elle a tapé, sa saisie gagne, et rien de distant ne la remplace
// sous ses doigts. Un titre distant qui arrive pendant le focus ne fait sauter ni le curseur
// ni la sélection.
import type { Me, PageNode } from "@mementum/protocol";
import { act, useEffect, useLayoutEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, updatePage } from "../api";
import { SessionContext } from "../session/session";
import { deferred } from "../test/hooks";
import { page, space, unauthenticated, unreachable } from "../test/pages";
import { TitleInput } from "./TitleInput";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  updatePage: vi.fn(),
}));

const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("space")],
};

let host: HTMLDivElement;
let root: Root;
const onSaved = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onSaved.mockClear();
  vi.mocked(updatePage).mockReset();
  vi.mocked(updatePage).mockImplementation(async (id, { title }) =>
    page(id, { title: title?.trim() || "Sans titre" }),
  );
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

/** Rend le titre de `node`, sous le compte `account` (un autre objet : le compte relu). */
async function render(node: PageNode, account: Me = me, readOnly = false) {
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me: account, refresh: async () => {} }}>
        <TitleInput
          node={node}
          autoFocus={false}
          onAutoFocused={() => {}}
          readOnly={readOnly}
          onSaved={onSaved}
        />
      </SessionContext.Provider>,
    ),
  );
}

const field = () => host.querySelector<HTMLTextAreaElement>('textarea[aria-label="Titre de la page"]')!;

const focus = () => act(async () => field().focus());
const blur = () => act(async () => field().blur());

async function type(text: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  await act(async () => {
    field().focus();
    setter.call(field(), text);
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
}

const enter = () =>
  act(async () => {
    field().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });

test("remote_rename_while_focused_without_typing_is_kept", async () => {
  await render(page("p", { title: "Ancien" }));
  await focus();
  // Bob renomme la page ; la barre latérale en direct apporte son titre.
  await render(page("p", { title: "Titre de Bob" }));
  // Alice quitte le champ sans avoir rien tapé : rien ne part, le titre de Bob reste.
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
  expect(field().value).toBe("Titre de Bob");
});

test("the_field_follows_a_remote_title_while_focused_until_something_is_typed", async () => {
  await render(page("p", { title: "Ancien" }));
  await focus();
  await render(page("p", { title: "Titre de Bob" }));
  // Le champ le montre aussitôt, focus ou non : il n'y a rien de tapé à protéger.
  expect(document.activeElement).toBe(field());
  expect(field().value).toBe("Titre de Bob");
  await type("Titre de Bob, revu");
  await render(page("p", { title: "Titre de Carla" }));
  expect(field().value).toBe("Titre de Bob, revu");
});

test("enter_without_typing_sends_nothing", async () => {
  await render(page("p", { title: "Ancien" }));
  await focus();
  await render(page("p", { title: "Titre de Bob" }));
  await enter();
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
});

test("what_was_typed_wins_over_a_remote_rename_and_is_sent", async () => {
  await render(page("p", { title: "Ancien" }));
  await type("Titre d'Alice");
  // Un renommage distant arrive pendant qu'elle tape : sa saisie reste sous ses doigts.
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Titre d'Alice");
  await blur();
  expect(updatePage).toHaveBeenCalledTimes(1);
  expect(updatePage).toHaveBeenCalledWith("p", { title: "Titre d'Alice" });
  expect(onSaved).toHaveBeenCalledWith(page("p", { title: "Titre d'Alice" }));
});

test("once_saved_the_field_follows_remote_renames_again", async () => {
  await render(page("p", { title: "Ancien" }));
  await type("  Titre d'Alice ");
  await enter();
  expect(updatePage).toHaveBeenCalledWith("p", { title: "  Titre d'Alice " });
  // Le titre normalisé par le serveur est affiché, et la saisie est enregistrée.
  expect(field().value).toBe("Titre d'Alice");
  await render(page("p", { title: "Titre d'Alice" }));
  // Le champ a toujours le focus ; un renommage suivant fait ailleurs le remplace.
  expect(document.activeElement).toBe(field());
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Titre de Bob");
  await blur();
  expect(updatePage).toHaveBeenCalledTimes(1);
});

test("typing_back_the_saved_title_sends_nothing_and_follows_again", async () => {
  await render(page("p", { title: "Ancien" }));
  await type("Ancienne");
  await type("Ancien");
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
  await focus();
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Titre de Bob");
});

test("text_typed_during_the_send_stays_typed", async () => {
  const answer = deferred<PageNode>();
  vi.mocked(updatePage).mockReturnValueOnce(answer.promise);
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await type("Premier et second");
  await act(async () => answer.resolve(page("p", { title: "Premier" })));
  await render(page("p", { title: "Premier" }));
  // Ce qu'elle a tapé pendant l'envoi reste, et n'est pas remplacé par un titre distant.
  expect(field().value).toBe("Premier et second");
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Premier et second");
  await blur();
  expect(updatePage).toHaveBeenLastCalledWith("p", { title: "Premier et second" });
});

test("a_title_refused_for_a_lost_session_stays_typed_and_leaves_once_the_account_is_back", async () => {
  vi.mocked(updatePage).mockRejectedValueOnce(unauthenticated());
  await render(page("p", { title: "Ancien" }));
  await type("Tapé sans session");
  await enter();
  expect(updatePage).toHaveBeenCalledTimes(1);
  // Un titre distant arrive pendant l'attente : la saisie reste.
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Tapé sans session");
  // Le compte revient (un autre objet `me`) : le titre repart.
  await render(page("p", { title: "Titre de Bob" }), { ...me });
  expect(updatePage).toHaveBeenCalledTimes(2);
  expect(updatePage).toHaveBeenLastCalledWith("p", { title: "Tapé sans session" });
  expect(field().value).toBe("Tapé sans session");
});

test("a_remote_title_moves_neither_the_caret_nor_the_selection", async () => {
  await render(page("p", { title: "Bonjour le monde" }));
  await focus();
  await act(async () => field().setSelectionRange(3, 7, "backward"));
  await render(page("p", { title: "Bonjour tout le monde" }));
  expect(field().value).toBe("Bonjour tout le monde");
  expect([field().selectionStart, field().selectionEnd, field().selectionDirection]).toEqual([
    3,
    7,
    "backward",
  ]);
  // Un titre plus court que la sélection la ramène à sa fin, sans la poser ailleurs.
  await act(async () => field().setSelectionRange(10, 18));
  await render(page("p", { title: "Bon" }));
  expect([field().selectionStart, field().selectionEnd]).toEqual([3, 3]);
});

test("a_read_only_field_follows_the_remote_title_even_after_typing", async () => {
  await render(page("p", { title: "Ancien" }));
  await type("Titre d'Alice");
  // Son niveau baisse (ou la page part à la corbeille) avant qu'elle ait quitté le champ : ce
  // qu'elle a tapé ne partira plus. Ce test disait que le texte restait affiché dans le champ ;
  // depuis R257 (M9), le champ dit le titre de la page, et le texte attend dans un message.
  await render(page("p", { title: "Ancien" }), me, true);
  expect(field().value).toBe("Ancien");
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
  // Un renommage fait ailleurs le remplace alors, plutôt qu'un titre qui n'est pas celui de
  // la page ; et le champ revenu en écriture le suit encore.
  await render(page("p", { title: "Titre de Bob" }), me, true);
  expect(field().value).toBe("Titre de Bob");
  await render(page("p", { title: "Titre de Bob" }));
  await focus();
  await render(page("p", { title: "Titre de Carla" }));
  expect(field().value).toBe("Titre de Carla");
});

test("a_remote_title_replaces_the_field_out_of_focus", async () => {
  await render(page("p", { title: "Ancien" }));
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Titre de Bob");
});

test("typing_undone_before_a_remote_rename_sends_nothing", async () => {
  // La sonde du relecteur : une lettre tapée par mégarde, puis effacée ; le champ redit le titre
  // enregistré. Bob renomme la page : le champ suit, et rien ne part au blur.
  await render(page("p", { title: "Ancien" }));
  await type("Ancienx");
  await type("Ancien");
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Titre de Bob");
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
});

test("the_same_title_typed_again_then_a_remote_rename_sends_nothing", async () => {
  await render(page("p", { title: "Ancien" }));
  // Tout sélectionné, et le même titre retapé.
  await type("A");
  await type("Ancien");
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Titre de Bob");
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
});

test("a_title_typed_back_during_a_send_is_kept_and_sent", async () => {
  // Elle envoie « Premier », puis, pendant l'envoi, revient à « Ancien » : c'est ce qu'elle a
  // tapé en dernier, et cela part à son tour.
  const answer = deferred<PageNode>();
  vi.mocked(updatePage).mockReturnValueOnce(answer.promise);
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await type("Ancien");
  await act(async () => answer.resolve(page("p", { title: "Premier" })));
  await render(page("p", { title: "Premier" }));
  expect(field().value).toBe("Ancien");
  await blur();
  expect(updatePage).toHaveBeenLastCalledWith("p", { title: "Ancien" });
});

/** Quitte le champ dans l'effet qui suit celui du titre, avant le rendu que ce dernier demande. */
function BlurAfter({ title }: { title: string }) {
  useEffect(() => {
    if (title !== "Ancien") field().blur();
  }, [title]);
  return null;
}

test("a_blur_between_a_remote_title_and_its_render_sends_nothing", async () => {
  // Le titre distant est posé (l'effet du titre), le champ n'est pas encore rendu de nouveau :
  // un blur à cet instant voit encore l'ancien texte. Rien n'a été tapé : rien ne part.
  const both = (node: PageNode) =>
    act(async () =>
      root.render(
        <SessionContext.Provider value={{ me, refresh: async () => {} }}>
          <TitleInput node={node} autoFocus={false} onAutoFocused={() => {}} readOnly={false} onSaved={onSaved} />
          <BlurAfter title={node.title} />
        </SessionContext.Provider>,
      ),
    );
  await both(page("p", { title: "Ancien" }));
  await focus();
  await both(page("p", { title: "Titre de Bob" }));
  expect(updatePage).not.toHaveBeenCalled();
  expect(field().value).toBe("Titre de Bob");
});

for (const [name, failure] of [
  ["network", unreachable()],
  ["422", new ApiError(422, "title_too_long", "un titre a 500 caractères au plus")],
] as const) {
  test(`a_refused_title_typed_back_during_its_send_stays_and_leaves_again (${name})`, async () => {
    // La sonde du relecteur : pendant l'envoi de « Premier », une lettre tapée puis effacée ramène
    // le champ au texte qui part. L'envoi échoue : ce texte n'est pas le titre de la page, c'est
    // une saisie, gardée contre un renommage fait ailleurs et renvoyée au blur.
    const answer = deferred<PageNode>();
    vi.mocked(updatePage).mockReturnValueOnce(answer.promise);
    await render(page("p", { title: "Ancien" }));
    await type("Premier");
    await enter();
    await type("Premierx");
    await type("Premier");
    await act(async () => answer.reject(failure));
    await render(page("p", { title: "Titre de Bob" }));
    const error = host.querySelector('[role="alert"]') !== null;
    await blur();
    expect(
      `erreur=${error} champ_apres_renommage=${field().value} renvois_au_blur=${
        vi.mocked(updatePage).mock.calls.length - 1
      }`,
    ).toBe("erreur=true champ_apres_renommage=Premier renvois_au_blur=1");
    expect(updatePage).toHaveBeenLastCalledWith("p", { title: "Premier" });
  });
}

test("a_title_typed_back_during_a_second_send_is_sent", async () => {
  // « Premier » part, puis « Second » ; le premier aboutit pendant que le second est en route ;
  // elle revient alors à « Premier », qu'elle a tapé en dernier : c'est lui qui part au blur.
  const first = deferred<PageNode>();
  const second = deferred<PageNode>();
  vi.mocked(updatePage).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await type("Second");
  await enter();
  await act(async () => first.resolve(page("p", { title: "Premier" })));
  await render(page("p", { title: "Premier" }));
  await type("Premier");
  await act(async () => second.resolve(page("p", { title: "Second" })));
  await render(page("p", { title: "Second" }));
  expect(field().value).toBe("Premier");
  await blur();
  expect(`dernier_envoi=${JSON.stringify(vi.mocked(updatePage).mock.lastCall)}`).toBe(
    'dernier_envoi=["p",{"title":"Premier"}]',
  );
});

test("a_title_typed_while_a_send_waits_for_the_account_is_kept_and_sent", async () => {
  // L'envoi de « Premier » attend le retour du compte (`401`) ; elle tape « Second » entre-temps.
  vi.mocked(updatePage).mockRejectedValueOnce(unauthenticated());
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await type("Second");
  await render(page("p", { title: "Ancien" }), { ...me });
  expect(updatePage).toHaveBeenLastCalledWith("p", { title: "Premier" });
  await render(page("p", { title: "Premier" }));
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Second");
  await blur();
  expect(updatePage).toHaveBeenLastCalledWith("p", { title: "Second" });
});

test("a_send_refused_once_the_field_is_read_only_lets_the_remote_title_in", async () => {
  // L'envoi est en route quand la page passe en lecture seule ; il est refusé. Rien de ce qui est
  // dans le champ ne peut plus partir : il suit le titre distant (R210).
  const answer = deferred<PageNode>();
  vi.mocked(updatePage).mockReturnValueOnce(answer.promise);
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await render(page("p", { title: "Ancien" }), me, true);
  await act(async () =>
    answer.reject(new ApiError(403, "forbidden", "niveau insuffisant sur cette page")),
  );
  await render(page("p", { title: "Titre de Bob" }), me, true);
  expect(field().value).toBe("Titre de Bob");
  await blur();
  expect(updatePage).toHaveBeenCalledTimes(1);
});

// Un titre tapé, pas encore enregistré, quand la page passe en lecture seule (R257, M9) : le
// champ revient au titre du serveur, et le texte tapé est gardé dans un message. Il est proposé
// quand la page redevient modifiable, sans rentrer seul dans le champ (R210 tient).

const UNSAVED = "Ce titre n'a pas été enregistré : ";
const unsavedMessage = () => host.querySelector('[role="status"]');
const buttonNamed = (name: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === name) ?? null;
const press = (name: string) =>
  act(async () => {
    buttonNamed(name)!.click();
  });

/** Le titre « Ancien », puis « Titre d'Alice » tapé, puis la page en lecture seule. */
async function typedThenReadOnly() {
  await render(page("p", { title: "Ancien" }));
  await type("Titre d'Alice");
  await render(page("p", { title: "Ancien" }), me, true);
}

test("unsaved_title_is_not_shown_as_the_title_once_read_only", async () => {
  await typedThenReadOnly();
  // Le champ dit le titre de la page ; le texte tapé est dans un message, isolé.
  expect(field().value).toBe("Ancien");
  const message = unsavedMessage();
  expect(message?.textContent).toBe(`${UNSAVED}Titre d'Alice`);
  expect(message?.querySelector('.mm-isolate[dir="auto"]')?.textContent).toBe("Titre d'Alice");
  // Rien à reprendre tant que la page est en lecture seule, et rien ne part.
  expect(buttonNamed("Reprendre ce titre")).toBeNull();
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
});

test("unsaved_title_is_offered_again_when_the_page_can_be_edited_again", async () => {
  await typedThenReadOnly();
  await blur();
  await render(page("p", { title: "Ancien" }));
  // Le message reste, et propose le texte ; le champ garde le titre du serveur, et rien ne part.
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Titre d'Alice`);
  expect(buttonNamed("Reprendre ce titre")).not.toBeNull();
  expect(field().value).toBe("Ancien");
  await focus();
  await blur();
  expect(updatePage).not.toHaveBeenCalled();
  // Repris : le texte est dans le champ comme une saisie, le focus aussi, et le message part.
  await press("Reprendre ce titre");
  expect(field().value).toBe("Titre d'Alice");
  expect(document.activeElement).toBe(field());
  expect(unsavedMessage()).toBeNull();
  expect(updatePage).not.toHaveBeenCalled();
  await blur();
  expect(updatePage).toHaveBeenCalledExactlyOnceWith("p", { title: "Titre d'Alice" });

  // Une seconde fois, écarté : le message part, le champ garde le titre, et le focus va au
  // champ plutôt que sur `body`.
  await render(page("p", { title: "Titre d'Alice" }));
  await type("Autre titre");
  await render(page("p", { title: "Titre d'Alice" }), me, true);
  await blur();
  await render(page("p", { title: "Titre d'Alice" }));
  await press("Écarter");
  expect(unsavedMessage()).toBeNull();
  expect(field().value).toBe("Titre d'Alice");
  expect(document.activeElement).toBe(field());
  await blur();
  expect(updatePage).toHaveBeenCalledTimes(1);
});

test("a_remote_rename_while_read_only_keeps_the_offered_text", async () => {
  await typedThenReadOnly();
  await render(page("p", { title: "Titre de Bob" }), me, true);
  // Le champ suit le titre distant ; le texte gardé reste proposé.
  expect(field().value).toBe("Titre de Bob");
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Titre d'Alice`);
  await render(page("p", { title: "Titre de Bob" }));
  expect(field().value).toBe("Titre de Bob");
  expect(buttonNamed("Reprendre ce titre")).not.toBeNull();
  expect(updatePage).not.toHaveBeenCalled();
});

test("a_saved_title_shows_no_message", async () => {
  // Témoin : un titre tapé et enregistré, puis la page en lecture seule : aucun message.
  await render(page("p", { title: "Ancien" }));
  await type("Titre d'Alice");
  await enter();
  await render(page("p", { title: "Titre d'Alice" }), me, true);
  expect(field().value).toBe("Titre d'Alice");
  expect(unsavedMessage()).toBeNull();
  // Ni pour un champ où rien n'a été tapé.
  await render(page("p", { title: "Titre de Bob" }), me, false);
  await render(page("p", { title: "Titre de Bob" }), me, true);
  expect(unsavedMessage()).toBeNull();
});

test("a_send_that_lands_after_the_page_turned_read_only_takes_the_message_away", async () => {
  // L'envoi est en route quand la page passe en lecture seule ; il aboutit (le serveur l'a
  // reçu avant la baisse) : le texte est le titre de la page, plus rien à proposer.
  const answer = deferred<PageNode>();
  vi.mocked(updatePage).mockReturnValueOnce(answer.promise);
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await render(page("p", { title: "Ancien" }), me, true);
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Premier`);
  await act(async () => answer.resolve(page("p", { title: "Premier" })));
  expect(unsavedMessage()).toBeNull();
  await render(page("p", { title: "Premier" }), me, true);
  expect(field().value).toBe("Premier");
  expect(unsavedMessage()).toBeNull();
});

test("a_remote_rename_that_gives_the_kept_text_takes_the_message_away", async () => {
  await typedThenReadOnly();
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Titre d'Alice`);
  // Quelqu'un d'autre donne à la page le titre gardé : il n'y a plus rien à proposer (R332).
  await render(page("p", { title: "Titre d'Alice" }), me, true);
  expect(field().value).toBe("Titre d'Alice");
  expect(unsavedMessage()).toBeNull();
  expect(updatePage).not.toHaveBeenCalled();
});

test("a_text_that_is_no_longer_flagged_as_typed_goes_to_the_message_too", async () => {
  // Le cas de R218 : pendant l'envoi de « Premier », une lettre tapée puis effacée ramène le
  // champ au texte qui part, et le drapeau « saisi » retombe. La page passe en lecture seule : ce
  // texte n'est pas encore le titre de la page, il va au message, drapeau levé ou non (R332).
  const answer = deferred<PageNode>();
  vi.mocked(updatePage).mockReturnValueOnce(answer.promise);
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await type("Premierx");
  await type("Premier");
  await render(page("p", { title: "Ancien" }), me, true);
  expect(field().value).toBe("Ancien");
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Premier`);
  // L'envoi est refusé : le texte n'est pas devenu le titre de la page, le message reste.
  await act(async () =>
    answer.reject(new ApiError(403, "forbidden", "niveau insuffisant sur cette page")),
  );
  expect(field().value).toBe("Ancien");
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Premier`);
  expect(updatePage).toHaveBeenCalledTimes(1);
});

test("a_title_waiting_for_the_account_stays_in_the_message_once_the_page_is_read_only", async () => {
  // Le titre attend le retour du compte (`401`), et la page passe en lecture seule entre-temps
  // (niveau, corbeille, schéma) : il est dans le message, dit non enregistré.
  vi.mocked(updatePage).mockRejectedValueOnce(unauthenticated());
  await render(page("p", { title: "Ancien" }));
  await type("Tapé sans session");
  await enter();
  expect(updatePage).toHaveBeenCalledTimes(1);
  await render(page("p", { title: "Ancien" }), me, true);
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Tapé sans session`);
  // Le compte revient (un autre objet `me`) : ce titre ne repart pas, le message disait vrai.
  const back = { ...me };
  await render(page("p", { title: "Ancien" }), back, true);
  expect(updatePage).toHaveBeenCalledTimes(1);
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Tapé sans session`);
  expect(field().value).toBe("Ancien");
  // La page redevient modifiable : le texte est proposé, et ne part que repris.
  await render(page("p", { title: "Ancien" }), back);
  await focus();
  await blur();
  expect(updatePage).toHaveBeenCalledTimes(1);
  await press("Reprendre ce titre");
  await blur();
  expect(updatePage).toHaveBeenCalledTimes(2);
  expect(updatePage).toHaveBeenLastCalledWith("p", { title: "Tapé sans session" });
});

// Le focus ne tombe jamais sur `body` : un bouton du message qui part sous le focus (la page
// repasse en lecture seule, le message n'a plus d'objet) le rend au champ du titre.

test("focus_goes_to_the_field_when_the_resume_button_leaves_under_it", async () => {
  await typedThenReadOnly();
  await blur();
  await render(page("p", { title: "Ancien" }));
  await act(async () => buttonNamed("Reprendre ce titre")!.focus());
  expect(document.activeElement).toBe(buttonNamed("Reprendre ce titre"));
  // La page repasse en lecture seule : le bouton part, le message reste.
  await render(page("p", { title: "Ancien" }), me, true);
  expect(buttonNamed("Reprendre ce titre")).toBeNull();
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Titre d'Alice`);
  expect(document.activeElement).toBe(field());
});

/** « Premier » part, la page passe en lecture seule pendant l'envoi : le message le garde. */
async function sentThenReadOnly() {
  const answer = deferred<PageNode>();
  vi.mocked(updatePage).mockReturnValueOnce(answer.promise);
  await render(page("p", { title: "Ancien" }));
  await type("Premier");
  await enter();
  await render(page("p", { title: "Ancien" }), me, true);
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Premier`);
  return answer;
}

test("focus_goes_to_the_field_when_the_message_leaves_under_it", async () => {
  const answer = await sentThenReadOnly();
  await act(async () => buttonNamed("Écarter")!.focus());
  expect(document.activeElement).toBe(buttonNamed("Écarter"));
  // L'envoi en route aboutit : le message part, avec le bouton qui avait le focus.
  await act(async () => answer.resolve(page("p", { title: "Premier" })));
  expect(unsavedMessage()).toBeNull();
  expect(document.activeElement).toBe(field());
});

test("a_render_that_keeps_the_focused_button_does_not_make_the_net_forget_it", async () => {
  // Le filet ne tient pour parti qu'un élément retiré du document (`isConnected`). Un rendu
  // passe pendant que le bouton a le focus, et le laisse là (un renommage fait ailleurs) : le
  // filet ne l'oublie pas, et le focus va encore au champ quand le message part ensuite.
  const answer = await sentThenReadOnly();
  await act(async () => buttonNamed("Écarter")!.focus());
  await render(page("p", { title: "Autre" }), me, true);
  expect(unsavedMessage()?.textContent).toBe(`${UNSAVED}Premier`);
  expect(document.activeElement).toBe(buttonNamed("Écarter"));
  // L'envoi en route aboutit : le message part, avec le bouton qui avait le focus.
  await act(async () => answer.resolve(page("p", { title: "Premier" })));
  expect(unsavedMessage()).toBeNull();
  expect(document.activeElement).toBe(field());
});

/** Donne le focus à `target` dans le rendu même où la page passe en lecture seule. */
function FocusOnReadOnly({ readOnly, target }: { readOnly: boolean; target: HTMLElement }) {
  useLayoutEffect(() => {
    if (readOnly) target.focus();
  }, [readOnly, target]);
  return null;
}

test("focus_taken_out_of_the_message_is_not_brought_back_when_it_leaves", async () => {
  // Témoin : le focus a quitté le message par un geste, avant qu'il parte. Il n'est pas repris.
  const elsewhere = document.createElement("button");
  document.body.append(elsewhere);
  try {
    // Vers un autre élément de la page.
    let answer = await sentThenReadOnly();
    await act(async () => buttonNamed("Écarter")!.focus());
    await act(async () => elsewhere.focus());
    await act(async () => answer.resolve(page("p", { title: "Premier" })));
    expect(unsavedMessage()).toBeNull();
    expect(document.activeElement).toBe(elsewhere);

    // Vers rien (un clic dans le vide) : le focus est sur `body` par ce geste, et y reste.
    await act(async () => root.unmount());
    root = createRoot(host);
    answer = await sentThenReadOnly();
    await act(async () => buttonNamed("Écarter")!.focus());
    await act(async () => buttonNamed("Écarter")!.blur());
    expect(document.activeElement).toBe(document.body);
    await act(async () => answer.resolve(page("p", { title: "Premier" })));
    expect(unsavedMessage()).toBeNull();
    expect(document.activeElement).toBe(document.body);

    // Le bouton part, et le même rendu pose le focus ailleurs (un effet d'un autre composant) :
    // il n'est plus perdu, et n'est pas repris.
    await act(async () => root.unmount());
    root = createRoot(host);
    const withFocusTaken = (readOnly: boolean) =>
      act(async () =>
        root.render(
          <SessionContext.Provider value={{ me, refresh: async () => {} }}>
            <FocusOnReadOnly readOnly={readOnly} target={elsewhere} />
            <TitleInput
              node={page("p", { title: "Ancien" })}
              autoFocus={false}
              onAutoFocused={() => {}}
              readOnly={readOnly}
              onSaved={onSaved}
            />
          </SessionContext.Provider>,
        ),
      );
    await withFocusTaken(false);
    await type("Titre d'Alice");
    await withFocusTaken(true);
    await withFocusTaken(false);
    await act(async () => buttonNamed("Reprendre ce titre")!.focus());
    await withFocusTaken(true);
    expect(buttonNamed("Reprendre ce titre")).toBeNull();
    expect(document.activeElement).toBe(elsewhere);
  } finally {
    elsewhere.remove();
  }
});
