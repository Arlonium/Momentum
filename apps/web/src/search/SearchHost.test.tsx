// Ce qui ouvre la fenêtre de recherche (lot 4, Task C1) : `useSearchWindow`, que rend `App`. Le
// code de la fenêtre n'est demandé qu'à la demande ; ce qui est tapé est gardé d'une ouverture à
// l'autre (Échap ne détruit pas un texte tapé), par espace ; sans espace, rien ne s'ouvre.
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import "../test/hooks";
import { page, space } from "../test/pages";
import { useSearchWindow, type SearchWindow } from "./SearchHost";

type FakeProps = {
  space: SpaceInfo;
  nodes: PageNode[] | null;
  query: string;
  onQuery: (query: string) => void;
  onClose: () => void;
};

/** Les props de la fenêtre au dernier rendu ; `null` : elle n'est pas rendue. */
let shown: FakeProps | null = null;

const loadSearch = vi.hoisted(() => vi.fn());
vi.mock("./loadSearch", () => ({ loadSearch }));

/** La fenêtre, réduite à ce que son hôte lui donne. */
function FakeDialog(props: FakeProps) {
  shown = props;
  return <dialog data-fake="" />;
}

let host: HTMLDivElement;
let root: Root;
let current: SearchWindow;
const NODES = [page("a")];

function Probe({ of }: { of: SpaceInfo | null }) {
  current = useSearchWindow(of, NODES);
  // Rendue à côté du reste, comme dans `App`.
  shown = null;
  return <>{current.window}</>;
}

beforeEach(() => {
  shown = null;
  loadSearch.mockReset();
  loadSearch.mockImplementation(async () => ({ SearchDialog: FakeDialog }));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
});

const show = (of: SpaceInfo | null = space("s1")) => act(async () => root.render(<Probe of={of} />));

/** Ctrl+K, tapé dans la page. */
async function shortcut(): Promise<KeyboardEvent> {
  const event = new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true });
  await act(async () => {
    document.body.dispatchEvent(event);
  });
  return event;
}

test("nothing_of_the_window_is_loaded_before_it_is_asked_for", async () => {
  await show();
  // Rendre l'application ne demande rien de la fenêtre.
  expect(loadSearch).not.toHaveBeenCalled();
  expect(shown).toBeNull();
  expect(host.querySelector("dialog")).toBeNull();
  // Le raccourci la demande, et l'ouvre sur l'espace affiché.
  await shortcut();
  expect(loadSearch).toHaveBeenCalledTimes(1);
  expect(shown).toMatchObject({ query: "", nodes: NODES });
  expect(shown!.space.id).toBe("s1");
  expect(host.querySelector("dialog[data-fake]")).not.toBeNull();
});

test("the_sidebar_entry_opens_it_and_may_ask_for_its_code_ahead", async () => {
  await show();
  // Survolée, ou au focus : le code est demandé, rien n'est ouvert.
  await act(async () => current.preload());
  expect(shown).toBeNull();
  await act(async () => current.open());
  expect(shown).not.toBeNull();
  await act(async () => shown!.onClose());
  expect(shown).toBeNull();
  expect(host.querySelector("dialog")).toBeNull();
});

test("what_was_typed_is_kept_when_the_window_closes_and_is_of_its_space", async () => {
  await show();
  await act(async () => current.open());
  await act(async () => shown!.onQuery("un texte tapé"));
  expect(shown!.query).toBe("un texte tapé");
  // Fermée (Échap), puis rouverte : le texte est rendu.
  await act(async () => shown!.onClose());
  await shortcut();
  expect(shown!.query).toBe("un texte tapé");
  // Dans un autre espace, le champ est vide ; de retour, le texte du premier n'y est plus.
  await act(async () => shown!.onClose());
  await show(space("s2"));
  await act(async () => current.open());
  expect(shown!.space.id).toBe("s2");
  expect(shown!.query).toBe("");
  await act(async () => shown!.onClose());
  await show(space("s1"));
  await act(async () => current.open());
  expect(shown!.query).toBe("");
});

test("without_a_space_nothing_opens_and_the_key_is_left_to_the_browser", async () => {
  await show(null);
  expect((await shortcut()).defaultPrevented).toBe(false);
  await act(async () => current.open());
  expect(shown).toBeNull();
  expect(loadSearch).not.toHaveBeenCalled();
  // L'espace parti pendant que la fenêtre est ouverte : elle part, et ne revient pas seule.
  await show(space("s1"));
  expect((await shortcut()).defaultPrevented).toBe(true);
  expect(shown).not.toBeNull();
  await show(null);
  expect(shown).toBeNull();
  await show(space("s1"));
  expect(shown).toBeNull();
});
