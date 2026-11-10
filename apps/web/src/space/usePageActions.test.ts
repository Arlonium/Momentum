// Les actions sur les pages : ce qu'elles envoient au serveur, ce qu'elles changent à
// l'écran une fois qu'il a répondu, et ce qu'elles disent quand il refuse.
import type { PageNode, Tree } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { ApiError, createPage, getTree, restorePage, trashPage, updatePage } from "../api";
import { useOpenPage } from "../page/useOpenPage";
import { renderHook, settle } from "../test/hooks";
import { page, unauthenticated } from "../test/pages";
import { usePageActions } from "./usePageActions";
import { useSpaceTree, useTreeReader } from "./useSpaceTree";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getTree: vi.fn(),
  getPage: vi.fn(() => new Promise(() => {})),
  createPage: vi.fn(),
  trashPage: vi.fn(),
  restorePage: vi.fn(),
  updatePage: vi.fn(),
}));

/** Ce que le serveur rend de l'arbre : chaque lecture le relit. */
let server: PageNode[] = [];
const caughtUp = vi.fn();

beforeEach(() => {
  history.replaceState(null, "", "/s/space");
  server = [];
  caughtUp.mockClear();
  for (const mock of [createPage, trashPage, restorePage, updatePage]) vi.mocked(mock).mockReset();
  vi.mocked(getTree).mockReset();
  vi.mocked(getTree).mockImplementation(
    async (spaceId): Promise<Tree> => ({ spaceId, nodes: [...server] }),
  );
});

type Props = { activeId?: string | null; pathname?: string; stale?: boolean };

function hook({ activeId = null, pathname = "/s/space", stale = false }: Props) {
  const tree = useSpaceTree();
  const inTree = tree.loaded?.nodes.find((n) => n.id === activeId);
  const open = useOpenPage(activeId, inTree, { retries: 0, onSessionLost: () => {} });
  useTreeReader(tree, "space", {
    retries: 0,
    onSessionLost: () => {},
    onGone: () => {},
    onError: () => {},
  });
  const actions = usePageActions({
    tree,
    page: open,
    spaceId: "space",
    activeId,
    pathname,
    isStale: () => stale,
    caughtUp,
  });
  return { actions, open, nodes: tree.loaded?.nodes ?? null };
}

/** Le hook, une fois l'arbre `nodes` lu. */
async function withTree(nodes: PageNode[], props: Props = {}) {
  server = nodes;
  const rendered = await renderHook(hook, props);
  await settle();
  expect(rendered.current.nodes).toEqual(nodes);
  return rendered;
}

const ids = (nodes: PageNode[] | null) => nodes?.map((n) => n.id);
const refusal = (code: ApiError["code"], message: string, status = 409) =>
  new ApiError(status, code, message);

test("created_page_joins_the_tree_and_opens", async () => {
  const app = await withTree([page("a")]);
  const created = page("n", { section: "private" });
  vi.mocked(createPage).mockResolvedValue(created);

  await settle(() => app.current.actions.create({ section: "private" }));
  // À la racine : l'espace et la section ; elle s'ouvre, son titre prendra le focus.
  expect(createPage).toHaveBeenCalledWith({ spaceId: "space", section: "private" });
  expect(ids(app.current.nodes)).toEqual(["a", "n"]);
  expect(app.current.actions.created).toBe("n");
  expect(location.pathname).toBe("/p/n");
  await settle(() => app.current.actions.titleFocused());
  expect(app.current.actions.created).toBeNull();

  // Une sous-page : sa page parente suffit.
  vi.mocked(createPage).mockResolvedValue(page("n1", { parentId: "n" }));
  await settle(() => app.current.actions.create({ parentId: "n" }));
  expect(createPage).toHaveBeenLastCalledWith({ parentId: "n" });
  expect(ids(app.current.nodes)).toEqual(["a", "n", "n1"]);
});

test("nothing_is_created_under_a_stale_schema", async () => {
  const app = await withTree([page("a")], { stale: true });
  await settle(() => app.current.actions.create({ section: "space" }));
  expect(createPage).not.toHaveBeenCalled();
});

test("refused_creation_says_why", async () => {
  const app = await withTree([page("a")]);
  vi.mocked(createPage).mockRejectedValue(refusal("forbidden", "un invité ne crée pas ici", 403));
  await settle(() => app.current.actions.create({ section: "space" }));
  expect(app.current.actions.notice).toBe(
    "La page n'a pas pu être créée. Un invité ne crée pas ici",
  );
  expect(ids(app.current.nodes)).toEqual(["a"]);
  expect(location.pathname).toBe("/s/space");
  // Le message part à la navigation suivante.
  await app.rerender({ pathname: "/p/a" });
  expect(app.current.actions.notice).toBeNull();
});

test("saved_title_updates_the_tree", async () => {
  const app = await withTree([page("a"), page("b")]);
  const renamed = page("a", { title: "Nouveau titre" });
  await settle(() => app.current.actions.saved(renamed));
  expect(app.current.nodes).toEqual([renamed, page("b")]);
});

test("trashed_page_leaves_the_tree_with_its_subpages", async () => {
  const child = page("a1", { parentId: "a" });
  const app = await withTree([page("a"), child, page("b")], { activeId: "a1" });
  expect(app.current.open.node).toEqual(child);
  vi.mocked(trashPage).mockResolvedValue(page("a", { trashedAt: 42 }));

  await settle(() => app.current.actions.trash("a"));
  expect(trashPage).toHaveBeenCalledWith("a");
  expect(ids(app.current.nodes)).toEqual(["b"]);
  // La page ouverte est partie avec sa page parente : elle reste à l'écran, à la corbeille.
  expect(app.current.open.node).toEqual({ ...child, trashedAt: 42 });
  expect(app.current.actions.notice).toBeNull();
});

test("refused_trash_says_why_and_a_lost_session_says_nothing", async () => {
  const app = await withTree([page("a")]);
  vi.mocked(trashPage).mockRejectedValue(refusal("forbidden", "niveau insuffisant", 403));
  await settle(() => app.current.actions.trash("a"));
  expect(app.current.actions.notice).toBe(
    "La page n'a pas pu être mise à la corbeille. Niveau insuffisant",
  );
  expect(ids(app.current.nodes)).toEqual(["a"]);

  // Session perdue : la connexion est demandée ailleurs, pas de message de plus.
  await app.rerender({ pathname: "/s/space/trash" });
  vi.mocked(trashPage).mockRejectedValue(unauthenticated());
  await settle(() => app.current.actions.trash("a"));
  expect(app.current.actions.notice).toBeNull();
});

test("move_among_the_same_siblings_does_not_name_the_parent", async () => {
  const app = await withTree([page("a"), page("b", { position: "a1" })]);
  const moved = page("b", { position: "Zz" });
  vi.mocked(updatePage).mockResolvedValue(moved);
  const reads = vi.mocked(getTree).mock.calls.length;

  await settle(() => app.current.actions.move("b", { parentId: null, afterId: null }));
  expect(updatePage).toHaveBeenCalledWith("b", { afterId: null });
  expect(app.current.nodes).toEqual([page("a"), moved]);
  // Ni son niveau ni sa section n'ont changé : l'arbre n'est pas relu.
  expect(getTree).toHaveBeenCalledTimes(reads);
});

test("move_under_another_page_reloads_the_tree", async () => {
  const app = await withTree([page("a"), page("b", { position: "a1" })]);
  const moved = page("b", { parentId: "a" });
  vi.mocked(updatePage).mockResolvedValue(moved);
  server = [page("a"), moved];
  const reads = vi.mocked(getTree).mock.calls.length;

  await settle(() => app.current.actions.move("b", { parentId: "a", afterId: null }));
  expect(updatePage).toHaveBeenCalledWith("b", { parentId: "a", afterId: null });
  expect(vi.mocked(getTree).mock.calls.length).toBeGreaterThan(reads);
  expect(app.current.nodes).toEqual([page("a"), moved]);
  // Une page inconnue de l'arbre : rien n'est envoyé.
  await settle(() => app.current.actions.move("absente", { parentId: "a", afterId: null }));
  expect(updatePage).toHaveBeenCalledTimes(1);
});

test("refused_move_says_why_and_reloads_the_tree", async () => {
  const app = await withTree([page("a"), page("b", { position: "a1" })]);
  // Entre-temps, « a » est partie à la corbeille ailleurs.
  server = [page("b", { position: "a1" })];
  vi.mocked(updatePage).mockRejectedValue(
    refusal("trashed", "la page est à la corbeille : la restaurer d'abord"),
  );
  await settle(() => app.current.actions.move("b", { parentId: "a", afterId: null }));
  expect(app.current.actions.notice).toBe(
    "La page n'a pas pu être déplacée. La page est à la corbeille : la restaurer d'abord",
  );
  // La barre latérale telle que le serveur la voit.
  expect(ids(app.current.nodes)).toEqual(["b"]);

  // Deux refus parlent des champs de l'API : ils sont dits autrement.
  for (const code of ["invalid_after", "invalid_parent"] as const) {
    vi.mocked(updatePage).mockRejectedValue(refusal(code, "afterId ne désigne pas…", 422));
    await settle(() => app.current.actions.move("b", { parentId: null, afterId: null }));
    expect(app.current.actions.notice).toBe(
      "La page n'a pas pu être déplacée. Ces pages ne sont pas rangées ensemble dans l'espace, ou l'endroit visé a changé entre-temps.",
    );
  }
});

test("restored_page_comes_back_and_the_tree_is_reloaded", async () => {
  const app = await withTree([page("b")], { activeId: "a" });
  const restored = page("a");
  vi.mocked(restorePage).mockResolvedValue(restored);
  server = [restored, page("b")];
  await settle(() => app.current.actions.restore("a"));
  expect(restorePage).toHaveBeenCalledWith("a");
  expect(caughtUp).toHaveBeenCalledWith("a");
  expect(ids(app.current.nodes)).toEqual(["a", "b"]);
  expect(app.current.open.node).toEqual(restored);
});

test("refused_restore_rejects_with_the_server_error", async () => {
  const app = await withTree([page("b")], { activeId: "a" });
  const error = refusal("parent_trashed", "une page parente est à la corbeille");
  vi.mocked(restorePage).mockRejectedValue(error);
  await expect(app.current.actions.restore("a")).rejects.toBe(error);
});
