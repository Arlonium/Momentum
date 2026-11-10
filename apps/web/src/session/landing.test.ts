// Où arrive un compte créé par une invitation sur une page : sur cette page, que le serveur ne
// nomme pas dans la réponse de l'inscription.
import type { Me, Tree } from "@mementum/protocol";
import { beforeEach, expect, test, vi } from "vitest";
import { getTree } from "../api";
import { page, space, unreachable } from "../test/pages";
import { invitedPage } from "./landing";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getTree: vi.fn(),
}));

const me = (...spaces: string[]): Me => ({
  user: { id: "u", email: "bruno@exemple.fr", name: "Bruno", instanceAdmin: false },
  spaces: spaces.map((id) => space(id, { role: "guest" })),
});

const tree = (...nodes: Tree["nodes"]): Tree => ({ spaceId: "pro", nodes });

beforeEach(() => {
  vi.mocked(getTree).mockReset();
});

test("new_guest_lands_on_the_only_page_it_sees", async () => {
  // Le compte neuf n'a que l'espace où on l'invite, et n'y voit que la page de l'invitation
  // et ses sous-pages : c'est la seule à la racine de ce qu'il voit.
  vi.mocked(getTree).mockResolvedValue(
    tree(
      page("plan", { section: "shared", level: "edit" }),
      page("annexe", { parentId: "plan", section: "shared", level: "edit" }),
    ),
  );
  expect(await invitedPage(me("pro"))).toBe("plan");
  expect(getTree).toHaveBeenCalledExactlyOnceWith("pro");
});

test("nothing_certain_lands_at_home", async () => {
  // Deux pages à la racine (une autre lui a été partagée entre-temps), ou aucune : on ne
  // devine pas ; l'accueil de l'espace les montre.
  vi.mocked(getTree).mockResolvedValue(tree(page("a"), page("b")));
  expect(await invitedPage(me("pro"))).toBeNull();
  vi.mocked(getTree).mockResolvedValue(tree());
  expect(await invitedPage(me("pro"))).toBeNull();
  // L'arbre ne se lit pas : l'accueil, sans faire échouer l'inscription, qui est faite.
  vi.mocked(getTree).mockRejectedValue(unreachable());
  expect(await invitedPage(me("pro"))).toBeNull();
  // Un compte sans espace, ou qui en a plusieurs : pas de devinette, ni de lecture.
  vi.mocked(getTree).mockClear();
  expect(await invitedPage(me())).toBeNull();
  expect(await invitedPage(me("pro", "perso"))).toBeNull();
  expect(getTree).not.toHaveBeenCalled();
});
