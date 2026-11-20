// Entre Ctrl+K et l'arrivée du code de la fenêtre (son premier chargement), rien de ce qui est tapé
// ne va à la page : une coquille, qui n'est pas chargée à la demande, ouvre tout de suite une fenêtre
// modale et son champ. Quand le code arrive, la fenêtre entière prend sa place, et le champ garde son
// texte, le focus et la place du curseur. Seul dans son fichier : le module chargé à la demande n'est
// résolu qu'une fois par processus.
import type { Me, SearchResults } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SessionContext } from "../session/session";
import { deferred } from "../test/hooks";
import { page, space } from "../test/pages";
import { useSearchWindow, type SearchWindow } from "./SearchHost";

/** Le code de la fenêtre, que le test laisse arriver quand il veut. */
const code = deferred<typeof import("./SearchDialog")>();
const asked = vi.hoisted(() => ({ times: 0 }));
vi.mock("./loadSearch", () => ({
  loadSearch: () => {
    asked.times += 1;
    return code.promise;
  },
}));
// Aucune recherche ne répond pendant ce test.
vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  searchSpace: () => new Promise<SearchResults>(() => {}),
}));

let host: HTMLDivElement;
let root: Root;
let current: SearchWindow;
const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("s1")],
};
const NODES = [page("a")];

function Probe() {
  current = useSearchWindow(space("s1"), NODES);
  return <SessionContext.Provider value={{ me, refresh: async () => {} }}>{current.window}</SessionContext.Provider>;
}

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
  document.body.replaceChildren();
});

const field = () => host.querySelector<HTMLInputElement>("dialog[open] input")!;

/** Une frappe, telle que le navigateur la mène : dans ce qui a le focus. */
async function typeAtFocus(text: string) {
  const target = document.activeElement;
  if (!(target instanceof HTMLInputElement)) throw new Error("le focus n'est pas dans un champ");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(target, target.value + text);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

test("what_is_typed_before_the_code_arrives_goes_to_the_field_and_stays_there", async () => {
  // Le texte d'une page, où le curseur est au milieu d'un mot.
  const editor = document.createElement("div");
  editor.className = "bn-editor";
  editor.setAttribute("contenteditable", "true");
  editor.tabIndex = 0;
  const text = document.createTextNode("le chat dort");
  editor.append(text);
  document.body.append(editor);
  editor.focus();
  document.getSelection()!.setBaseAndExtent(text, 5, text, 5);
  await act(async () => root.render(<Probe />));

  // Ctrl+K : dans le même geste, avant tout code chargé, une fenêtre modale et son champ, au focus.
  const key = new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true });
  await act(async () => {
    editor.dispatchEvent(key);
  });
  expect(key.defaultPrevented).toBe(true);
  expect(asked.times).toBe(1);
  const shell = host.querySelector<HTMLDialogElement>("dialog")!;
  expect(shell.open).toBe(true);
  expect(shell.getAttribute("aria-label")).toBe("Rechercher");
  expect(document.activeElement).toBe(field());
  expect(field().getAttribute("aria-label")).toBe("Rechercher dans l'espace");
  expect(field().maxLength).toBe(200);
  // La coquille ne porte que le champ : ni liste, ni rien du reste de la fenêtre.
  expect(shell.querySelector('[role="listbox"]')).toBeNull();

  // Ce qui est tapé va au champ, pas à la page.
  await typeAtFocus("chaton");
  expect(field().value).toBe("chaton");
  expect(text.data).toBe("le chat dort");
  // Le curseur au milieu de ce qui est tapé.
  field().setSelectionRange(3, 3);
  const before = field();

  // Le code arrive : la fenêtre entière, avec le même texte, le focus et le curseur où ils étaient.
  await act(async () => {
    code.resolve(await vi.importActual<typeof import("./SearchDialog")>("./SearchDialog"));
  });
  const whole = host.querySelector<HTMLDialogElement>("dialog")!;
  expect(host.querySelectorAll("dialog")).toHaveLength(1);
  expect(whole.open).toBe(true);
  expect(whole.querySelector('[role="listbox"]')).not.toBeNull();
  expect(field()).not.toBe(before);
  expect(field().value).toBe("chaton");
  expect(document.activeElement).toBe(field());
  expect([field().selectionStart, field().selectionEnd]).toEqual([3, 3]);
  await typeAtFocus("s");
  expect(field().value).toBe("chatons");

  // Fermée : le focus et la sélection reviennent au texte, tels que l'hôte les a relevés à Ctrl+K.
  document.getSelection()!.removeAllRanges();
  await act(async () => {
    whole.dispatchEvent(new Event("cancel", { cancelable: true }));
  });
  expect(host.querySelector("dialog")).toBeNull();
  expect(document.activeElement).toBe(editor);
  const selection = document.getSelection()!;
  expect([selection.anchorNode, selection.anchorOffset]).toEqual([text, 5]);
});
