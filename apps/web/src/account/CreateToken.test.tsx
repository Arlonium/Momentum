// La fenêtre « Créer un jeton » (lot 5, Task C1) : deux niveaux seulement (R1218), ce qu'un agent
// fait et ne fait pas, une portée de pages prise dans l'arbre affiché et nulle part ailleurs, et les
// refus du serveur tels qu'il les dit.
import type { SpaceInfo, TokenInfo } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, createToken, getTree } from "../api";
import { publishPages } from "../space/pageDirectory";
import "../test/hooks";
import { page, space } from "../test/pages";
import { CreateToken } from "./CreateToken";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  createToken: vi.fn(),
  getTree: vi.fn(),
}));

const TOKEN = "mmt_TEST0000000000000000000000000000000000000ab";
const pro = space("pro", { name: "Pro" });
const perso = space("perso", { name: "Perso", role: "member" });

const info = (overrides: Partial<TokenInfo> = {}): TokenInfo => ({
  id: "t1",
  name: "Claude Code",
  hint: "00ab",
  level: "read",
  scope: "space",
  space: { id: "pro", name: "Pro" },
  pages: [],
  canTrash: false,
  createdAt: 1_800_000_000_000,
  expiresAt: null,
  lastUsedAt: null,
  revokedAt: null,
  live: true,
  busyCount: 0,
  callsPurged: 0,
  ...overrides,
});

let host: HTMLDivElement;
let root: Root;
const onChanged = vi.fn();
const onClose = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onChanged.mockClear();
  onClose.mockClear();
  vi.mocked(createToken).mockReset();
  vi.mocked(getTree).mockReset();
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  publishPages(null, null);
});

async function opened(current: SpaceInfo | null = pro) {
  await act(async () =>
    root.render(
      <CreateToken
        spaces={[pro, perso]}
        current={current}
        email="alice@exemple.fr"
        draft={null}
        onDraft={() => {}}
        onChanged={onChanged}
        onClose={onClose}
      />,
    ),
  );
}

const dialog = () => host.querySelector<HTMLDialogElement>("dialog.mm-token-dialog")!;
const radios = (name: string) =>
  [...dialog().querySelectorAll<HTMLInputElement>(`input[type="radio"][name$="${name}"]`)];
/** Le libellé d'un choix, sans sa précision. */
const labelOf = (input: HTMLInputElement) => {
  const label = input.closest("label");
  return (label?.querySelector(".mm-token-choice-label") ?? label)?.textContent?.trim() ?? "";
};
const field = (label: string) =>
  [...dialog().querySelectorAll<HTMLInputElement>("input")].find(
    (input) => input.labels?.[0]?.textContent === label || input.getAttribute("aria-label") === label,
  )!;
const buttonNamed = (name: string) =>
  [...dialog().querySelectorAll("button")].find((b) => b.textContent?.trim() === name) ?? null;
const alerts = () => [...dialog().querySelectorAll('[role="alert"]')].map((a) => a.textContent);

/** Tape `value` dans un champ contrôlé par React. */
async function type(input: HTMLInputElement, value: string) {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });

async function fillAndSubmit() {
  await type(field("Nom du jeton"), "Claude Code");
  await type(field("Mot de passe actuel"), "mot-de-passe");
  await click(buttonNamed("Créer le jeton")!);
}

test("two_levels_are_offered", async () => {
  await opened();
  const levels = radios("level");
  // « Lecture » et « Modification », et rien d'autre : pas d'« Accès complet » par jeton (R1218).
  expect(levels.map(labelOf)).toEqual(["Lecture", "Modification"]);
  expect(levels.map((input) => input.value)).toEqual(["read", "edit"]);
  // Le moindre privilège d'abord.
  expect(levels[0].checked).toBe(true);
  vi.mocked(createToken).mockResolvedValue({ token: TOKEN, info: info({ level: "edit" }) });
  await click(levels[1]);
  await fillAndSubmit();
  expect(vi.mocked(createToken).mock.calls[0][0]).toMatchObject({ level: "edit" });
});

test("the_screen_says_an_agent_creates_at_the_root_and_never_moves_a_page", async () => {
  await opened();
  const text = dialog().textContent ?? "";
  expect(text).toContain("à la racine");
  expect(text).toContain("dans «\u00a0Privé\u00a0» par défaut");
  expect(text).toContain("dans «\u00a0Espace\u00a0» s'il le demande");
  expect(text).toContain("ne déplace jamais une page sous une autre");
  expect(text).toContain("ne supprime rien pour de bon");
});

test("the_scope_picker_only_offers_pages_of_the_tree", async () => {
  // L'arbre affiché : celui de « Pro », sans la page à la corbeille.
  publishPages("pro", [
    page("a", { spaceId: "pro", title: "Agenda" }),
    page("b", { spaceId: "pro", title: "Budget" }),
    page("c", { spaceId: "pro", title: "Archives", trashedAt: 1 }),
  ]);
  await opened();
  const pages = radios("scope").find((input) => input.value === "pages")!;
  expect(labelOf(pages)).toContain("Pro");
  await click(pages);
  const offered = () =>
    [...dialog().querySelectorAll<HTMLInputElement>('[aria-label="Pages proposées"] input[type="checkbox"]')].map(
      (box) => box.closest("label")?.textContent?.trim(),
    );
  // Les pages de l'arbre, jamais une page à la corbeille, jamais une lecture du serveur.
  expect(offered()).toEqual(["Agenda", "Budget"]);
  await type(field("Chercher une page"), "arch");
  expect(offered()).toEqual([]);
  await type(field("Chercher une page"), "bud");
  expect(offered()).toEqual(["Budget"]);
  expect(getTree).not.toHaveBeenCalled();

  // Rien de choisi : la fenêtre le dit, sans rien envoyer.
  await fillAndSubmit();
  expect(createToken).not.toHaveBeenCalled();
  expect(alerts()).toEqual(["Choisis au moins une page."]);

  await click(dialog().querySelector('[aria-label="Pages proposées"] input[type="checkbox"]')!);
  vi.mocked(createToken).mockResolvedValue({ token: TOKEN, info: info({ scope: "pages" }) });
  await click(buttonNamed("Créer le jeton")!);
  expect(vi.mocked(createToken).mock.calls[0][0]).toMatchObject({
    scope: "pages",
    spaceId: "pro",
    pageIds: ["b"],
  });
});

test("the_scope_picker_offers_nothing_from_another_space", async () => {
  // L'arbre publié est celui d'un autre espace que celui de la portée : rien n'est proposé.
  publishPages("autre", [page("x", { spaceId: "autre", title: "Ailleurs" })]);
  await opened();
  await click(radios("scope").find((input) => input.value === "pages")!);
  expect(dialog().querySelectorAll('[aria-label="Pages proposées"] input[type="checkbox"]')).toHaveLength(0);
});

test("a_refusal_is_said_as_the_server_says_it", async () => {
  await opened();
  // Un mot de passe faux n'est pas une session perdue : son message, tel quel, et le champ à reprendre.
  vi.mocked(createToken).mockRejectedValueOnce(
    new ApiError(401, "invalid_credentials", "mot de passe actuel incorrect"),
  );
  await fillAndSubmit();
  expect(alerts()).toEqual(["mot de passe actuel incorrect"]);
  expect(document.activeElement).toBe(field("Mot de passe actuel"));
  // Un refus ne fait pas relire la liste : rien n'a été créé.
  expect(onChanged).not.toHaveBeenCalled();

  vi.mocked(createToken).mockRejectedValueOnce(
    new ApiError(422, "invalid_name", "le nom d'un jeton fait de 1 à 60 caractères"),
  );
  await click(buttonNamed("Créer le jeton")!);
  expect(alerts()).toEqual(["le nom d'un jeton fait de 1 à 60 caractères"]);
  expect(document.activeElement).toBe(field("Nom du jeton"));

  vi.mocked(createToken).mockRejectedValueOnce(
    new ApiError(409, "token_limit", "vingt jetons vivants au plus : révoque-en un"),
  );
  await click(buttonNamed("Créer le jeton")!);
  expect(alerts()).toEqual(["vingt jetons vivants au plus : révoque-en un"]);
  // Le formulaire reste, rempli : rien n'est à retaper.
  expect(field("Nom du jeton").value).toBe("Claude Code");
});
