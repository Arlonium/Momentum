// Le titre n'envoie que ce que la personne a tapé. Tant qu'elle n'a rien tapé, le champ suit
// le titre distant (un renommage fait ailleurs, que la barre latérale en direct apporte), même
// quand il a le focus ; dès qu'elle a tapé, sa saisie gagne, et rien de distant ne la remplace
// sous ses doigts. Un titre distant qui arrive pendant le focus ne fait sauter ni le curseur
// ni la sélection.
import type { Me, PageNode } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { updatePage } from "../api";
import { SessionContext } from "../session/session";
import { deferred } from "../test/hooks";
import { page, space, unauthenticated } from "../test/pages";
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
  // qu'elle a tapé reste affiché, mais ne partira plus.
  await render(page("p", { title: "Ancien" }), me, true);
  expect(field().value).toBe("Titre d'Alice");
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
