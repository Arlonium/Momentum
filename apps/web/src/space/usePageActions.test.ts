// Les actions sur les pages : ce qu'elles envoient au serveur, ce qu'elles changent à
// l'écran une fois qu'il a répondu, et ce qu'elles disent quand il refuse.
import type { PageNode, Tree } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import {
  ApiError,
  createPage,
  getPage,
  getTree,
  restorePage,
  trashPage,
  updatePage,
} from "../api";
import { useOpenPage } from "../page/useOpenPage";
import { deferred, renderHook, settle } from "../test/hooks";
import { notFound, page, unauthenticated } from "../test/pages";
import { usePageActions } from "./usePageActions";
import { useSpaceTree, useTreeReader } from "./useSpaceTree";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getTree: vi.fn(),
  getPage: vi.fn(),
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
  // Par défaut, le serveur ne répond pas encore à qui demande une page.
  vi.mocked(getPage).mockReset();
  vi.mocked(getPage).mockImplementation(() => new Promise(() => {}));
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
  useTreeReader(tree, "space", { retries: 0, onSessionLost: () => {}, onGone: () => {} });
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

/** Le nombre de lectures de l'arbre faites jusqu'ici. */
const treeReads = () => vi.mocked(getTree).mock.calls.length;

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

test("refused_creation_reloads_the_tree", async () => {
  const app = await withTree([page("a"), page("b")]);
  // Entre-temps, « a » a été supprimée ailleurs : y créer une sous-page est refusé.
  server = [page("b")];
  vi.mocked(createPage).mockRejectedValue(
    refusal("invalid_parent", "parentId ne désigne pas une page de cet espace", 422),
  );
  await settle(() => app.current.actions.create({ parentId: "a" }));
  // Le refus est dit sans le vocabulaire de l'API, et la barre latérale revient à ce que le
  // serveur voit.
  expect(app.current.actions.notice).toBe(
    "La page n'a pas pu être créée. Sa page parente n'est plus là, ou tu ne la vois plus.",
  );
  expect(ids(app.current.nodes)).toEqual(["b"]);
});

test("lost_session_on_creation_says_nothing", async () => {
  const app = await withTree([page("a")]);
  // La connexion est demandée par-dessus : pas de message de plus.
  vi.mocked(createPage).mockRejectedValue(unauthenticated());
  await settle(() => app.current.actions.create({ section: "space" }));
  expect(app.current.actions.notice).toBeNull();
});

test("created_page_already_in_the_tree_is_not_added_twice", async () => {
  const app = await withTree([page("a")]);
  const created = page("n");
  vi.mocked(createPage).mockResolvedValue(created);
  await settle(() => app.current.actions.create({ section: "space" }));
  // La même page rendue une seconde fois (une relecture l'avait déjà apportée) : une ligne.
  await settle(() => app.current.actions.create({ section: "space" }));
  expect(ids(app.current.nodes)).toEqual(["a", "n"]);
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

test("refused_trash_reloads_the_tree", async () => {
  const app = await withTree([page("a"), page("b")]);
  // Entre-temps, « a » a été supprimée ailleurs.
  server = [page("b")];
  vi.mocked(trashPage).mockRejectedValue(notFound());
  await settle(() => app.current.actions.trash("a"));
  expect(app.current.actions.notice).toBe(
    "La page n'a pas pu être mise à la corbeille. Page introuvable",
  );
  // Sa ligne s'en va : un second essai ne redonnerait que la même erreur.
  expect(ids(app.current.nodes)).toEqual(["b"]);
});

test("trash_and_restore_tell_the_trash_view", async () => {
  const app = await withTree([page("a"), page("b")], { activeId: "x" });
  const before = app.current.actions.trashChanges;
  vi.mocked(trashPage).mockResolvedValue(page("a", { trashedAt: 1 }));
  await settle(() => app.current.actions.trash("a"));
  const afterTrash = app.current.actions.trashChanges;
  expect(afterTrash).not.toBe(before);
  // Refusée aussi : ce que la corbeille montre a pu changer ailleurs.
  vi.mocked(trashPage).mockRejectedValue(notFound());
  await settle(() => app.current.actions.trash("b"));
  const afterRefusal = app.current.actions.trashChanges;
  expect(afterRefusal).not.toBe(afterTrash);
  vi.mocked(restorePage).mockResolvedValue(page("x"));
  await settle(() => app.current.actions.restore("x"));
  expect(app.current.actions.trashChanges).not.toBe(afterRefusal);
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

test("refused_restore_rejects_and_reads_the_page_and_the_tree_again", async () => {
  const app = await withTree([page("b")], { activeId: "a" });
  expect(getPage).toHaveBeenCalledTimes(1);
  const reads = treeReads();
  const error = refusal("parent_trashed", "une page parente est à la corbeille");
  vi.mocked(restorePage).mockRejectedValue(error);
  let thrown: unknown;
  await settle(() => app.current.actions.restore("a").catch((e: unknown) => (thrown = e)));
  // Le bandeau dira le refus du serveur.
  expect(thrown).toBe(error);
  // La page ouverte est redemandée (elle a pu être supprimée ou restaurée ailleurs), et la
  // barre latérale relue.
  expect(getPage).toHaveBeenCalledTimes(2);
  expect(treeReads()).toBeGreaterThan(reads);
});

test("change_from_the_trash_view_reloads_the_tree", async () => {
  const app = await withTree([page("b")]);
  const reads = treeReads();
  server = [page("a"), page("b")];
  await settle(() => app.current.actions.trashChanged());
  expect(treeReads()).toBeGreaterThan(reads);
  expect(ids(app.current.nodes)).toEqual(["a", "b"]);
});

test("page_moved_meanwhile_says_so_and_reloads_the_tree", async () => {
  const app = await withTree([page("a"), page("b", { position: "a1" })]);
  const reads = treeReads();
  // La page a changé de place ailleurs à chaque tentative du serveur (R91) : rien n'est écrit.
  server = [page("b", { position: "a0" }), page("a", { position: "a1" })];
  vi.mocked(updatePage).mockRejectedValue(
    refusal("page_moved", "la page a changé de place entre-temps : réessayer"),
  );
  await settle(() => app.current.actions.move("b", { parentId: null, afterId: null }));
  // Le message du serveur, en pied de barre, et l'arbre tel qu'il le voit.
  expect(app.current.actions.notice).toBe(
    "La page n'a pas pu être déplacée. La page a changé de place entre-temps : réessayer",
  );
  expect(treeReads()).toBeGreaterThan(reads);
  expect(app.current.nodes).toEqual(server);
});

test("page_reread_with_another_level_updates_its_row_and_reloads_the_tree", async () => {
  const child = page("a1", { parentId: "a" });
  const app = await withTree([page("a"), child, page("b")], { activeId: "a" });
  const reads = treeReads();
  // Avant une connexion de synchro, le serveur dit que le niveau a baissé. La page ouverte et
  // sa ligne le savent tout de suite, sans attendre la relecture de l'arbre ; ses sous-pages
  // suivront avec elle.
  const lowered = page("a", { level: "read" });
  const reread = deferred<Tree>();
  vi.mocked(getTree).mockImplementation(() => reread.promise);
  await settle(() => app.current.actions.reread(lowered));
  expect(app.current.open.node).toEqual(lowered);
  expect(app.current.nodes).toEqual([lowered, child, page("b")]);
  expect(treeReads()).toBeGreaterThan(reads);
  // L'arbre relu arrive : les sous-pages ont suivi.
  const after = [lowered, { ...child, level: "read" as const }, page("b")];
  await settle(() => reread.resolve({ spaceId: "space", nodes: after }));
  expect(app.current.nodes).toEqual(after);
});

test("page_reread_as_it_was_changes_nothing", async () => {
  const a = page("a");
  const app = await withTree([a, page("b")], { activeId: "a" });
  const reads = treeReads();
  const before = app.current.nodes;
  // À chaque connexion de synchro, le plus souvent : rien de neuf.
  await settle(() => app.current.actions.reread({ ...a }));
  expect(app.current.nodes).toBe(before);
  expect(treeReads()).toBe(reads);
  // Renommée ailleurs : sa ligne suit, sans relire tout l'arbre.
  const renamed = page("a", { title: "Renommée ailleurs" });
  await settle(() => app.current.actions.reread(renamed));
  expect(app.current.nodes).toEqual([renamed, page("b")]);
  expect(app.current.open.node).toEqual(renamed);
  expect(treeReads()).toBe(reads);
});

test("page_reread_at_the_trash_stays_on_screen_and_leaves_the_tree", async () => {
  const child = page("a1", { parentId: "a" });
  const app = await withTree([page("a"), child, page("b")], { activeId: "a" });
  // Jetée ailleurs pendant qu'elle est ouverte : elle reste à l'écran, à la corbeille, et sa
  // ligne part avec celles de ses sous-pages.
  const trashed = page("a", { trashedAt: 42 });
  server = [page("b")];
  await settle(() => app.current.actions.reread(trashed));
  expect(app.current.open.node).toEqual(trashed);
  expect(ids(app.current.nodes)).toEqual(["b"]);

  // Restaurée ailleurs : elle revient, et l'arbre est relu.
  const restored = page("a");
  server = [restored, child, page("b")];
  await settle(() => app.current.actions.reread(restored));
  expect(app.current.open.node).toEqual(restored);
  expect(ids(app.current.nodes)).toEqual(["a", "a1", "b"]);
});

test("page_reread_after_it_was_left_is_ignored", async () => {
  const app = await withTree([page("a"), page("b")], { activeId: "b" });
  const before = app.current.nodes;
  const reads = treeReads();
  // La réponse arrive après qu'on a ouvert une autre page : elle ne touche à rien.
  await settle(() => app.current.actions.reread(page("a", { trashedAt: 42 })));
  expect(app.current.nodes).toBe(before);
  expect(app.current.open.node).toEqual(page("b"));
  expect(treeReads()).toBe(reads);
});

test("lost_page_says_so_and_leaves_the_tree_with_its_subpages", async () => {
  const app = await withTree([page("a"), page("a1", { parentId: "a" }), page("b")], {
    activeId: "a",
  });
  const reads = treeReads();
  // Le serveur dit que la page ouverte n'est plus là pour ce compte (supprimée, ou plus
  // partagée avec lui).
  server = [page("b")];
  vi.mocked(getPage).mockRejectedValue(notFound());
  await settle(() => app.current.actions.lost("a"));
  expect(app.current.open.node).toBeUndefined();
  expect(app.current.open.looked).toEqual({ id: "a", state: "gone" });
  expect(ids(app.current.nodes)).toEqual(["b"]);
  expect(treeReads()).toBeGreaterThan(reads);
  expect(app.current.actions.notice).toBeNull();
});

test("change_from_the_share_menu_reloads_the_tree", async () => {
  const mine = page("a", { section: "private" });
  const app = await withTree([mine], { activeId: "a" });
  const reads = treeReads();
  // Ouverte aux membres par le menu Partager : elle change de section, ce que seul le serveur
  // calcule.
  server = [page("a", { section: "space" })];
  await settle(() => app.current.actions.shared());
  expect(treeReads()).toBeGreaterThan(reads);
  expect(app.current.nodes).toEqual(server);
});

test("refusal_from_the_share_menu_is_said_in_the_sidebar", async () => {
  const app = await withTree([page("a")], { activeId: "a" });
  // Le menu Partager s'est fermé (la personne n'a plus l'accès complet) : le refus du serveur
  // qui l'a révélé se dit en pied de barre latérale, jusqu'à la navigation suivante.
  await settle(() => app.current.actions.shareRefused("accès complet requis sur cette page"));
  expect(app.current.actions.notice).toBe(
    "Le partage n'a pas pu être modifié. Accès complet requis sur cette page",
  );
  await app.rerender({ activeId: "a", pathname: "/s/space/trash" });
  expect(app.current.actions.notice).toBeNull();
});
