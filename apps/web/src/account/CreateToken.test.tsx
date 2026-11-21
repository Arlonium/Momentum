// La fenêtre « Créer un jeton » (lot 5, Task C1) : deux niveaux seulement (R1218), ce qu'un agent
// fait et ne fait pas, une portée de pages prise dans l'arbre affiché et nulle part ailleurs, et les
// refus du serveur tels qu'il les dit.
import type { SpaceInfo, TokenInfo } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, createToken, getTree } from "../api";
import { publishPages } from "../space/pageDirectory";
import { deferred } from "../test/hooks";
import { page, space } from "../test/pages";
import { CreateToken, type TokenDraft } from "./CreateToken";

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

async function opened(current: SpaceInfo | null = pro, draft: TokenDraft | null = null) {
  await act(async () =>
    root.render(
      <CreateToken
        spaces={[pro, perso]}
        current={current}
        email="alice@exemple.fr"
        draft={draft}
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

test("the_scopes_go_from_the_narrowest_and_the_space_shown_is_the_default", async () => {
  // Le plan (LA-c, R1500 corrigé) : « des pages » avant « un espace », avant « tous mes espaces ». La
  // portée cochée d'abord reste l'espace affiché (le plan n'en dit pas d'autre, R1450).
  await opened();
  const scopes = radios("scope");
  expect(scopes.map((input) => input.value)).toEqual(["pages", "space", "all"]);
  expect(scopes.map(labelOf)).toEqual([
    "Des pages de « Pro », avec leurs sous-pages",
    "Un espace",
    "Tous mes espaces",
  ]);
  expect(scopes.find((input) => input.checked)?.value).toBe("space");
  expect(dialog().querySelector<HTMLSelectElement>('select[aria-label="Espace du jeton"]')!.value).toBe("pro");
  await act(async () => root.unmount());
  root = createRoot(host);

  // Sans espace affiché : « Des pages » est grisé, et la portée cochée est « Tous mes espaces ».
  await opened(null);
  expect(radios("scope").map((input) => [input.value, input.disabled, input.checked])).toEqual([
    ["pages", true, false],
    ["space", false, false],
    ["all", false, true],
  ]);
});

test("the_screen_warns_of_a_trapped_page", async () => {
  // LA-c : le texte d'une page lue par l'agent peut le pousser à agir avec les droits du jeton ; la
  // portée est la seule barrière. L'écran le dit avant les choix, et chaque groupe de choix le cite.
  await opened();
  const warning = dialog().querySelector<HTMLElement>(".mm-token-warning")!;
  expect(warning.textContent).toContain("Le texte d'une page peut tromper un agent qui la lit");
  expect(warning.textContent).toContain("avec les droits de ce jeton");
  expect(warning.textContent).toContain("la portée la plus étroite qui suffit");
  expect(warning.textContent).toContain("« Lecture » s'il n'a rien à écrire");
  // Avant le niveau et la portée, dans l'ordre du formulaire.
  const levels = dialog().querySelectorAll("fieldset")[0];
  expect(warning.compareDocumentPosition(levels) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  for (const fieldset of dialog().querySelectorAll("fieldset")) {
    expect(fieldset.getAttribute("aria-describedby")).toBe(warning.id);
  }
});

test("a_read_token_cannot_allow_the_trash", async () => {
  // I4 : sous « Lecture », la corbeille ne sert à rien (mettre à la corbeille et en sortir demandent
  // de modifier) : la case est grisée et décochée, et rien ne part.
  await opened();
  const trash = () =>
    [...dialog().querySelectorAll<HTMLInputElement>('input[type="checkbox"]')].find(
      (box) => document.getElementById(box.getAttribute("aria-labelledby") ?? "")?.textContent === "Permettre la corbeille",
    )!;
  const [read, edit] = radios("level");
  expect(read.checked).toBe(true);
  expect(trash()).toMatchObject({ disabled: true, checked: false });
  await click(edit);
  expect(trash().disabled).toBe(false);
  await click(trash());
  expect(trash().checked).toBe(true);
  // Revenue à « Lecture », elle se décoche ; reprise « Modification », elle reste décochée.
  await click(read);
  expect(trash()).toMatchObject({ disabled: true, checked: false });
  await click(edit);
  expect(trash()).toMatchObject({ disabled: false, checked: false });
  await click(read);
  vi.mocked(createToken).mockResolvedValue({ token: TOKEN, info: info() });
  await fillAndSubmit();
  expect(vi.mocked(createToken).mock.calls[0][0]).toMatchObject({ level: "read", canTrash: false });
  await act(async () => root.unmount());
  root = createRoot(host);

  // Un brouillon de « Lecture » qui gardait la corbeille (d'avant la règle) ne l'envoie pas.
  const draft: TokenDraft = {
    name: "Agent",
    level: "read",
    scope: "space",
    spaceId: "pro",
    chosen: [],
    pagesOf: null,
    canTrash: true,
    expiry: 90,
  };
  await opened(pro, draft);
  expect(trash()).toMatchObject({ disabled: true, checked: false });
  vi.mocked(createToken).mockClear();
  await type(field("Mot de passe actuel"), "mot-de-passe");
  await click(buttonNamed("Créer le jeton")!);
  expect(vi.mocked(createToken).mock.calls[0][0]).toMatchObject({ level: "read", canTrash: false });
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

test("escape_and_cancel_wait_for_the_creation_in_flight", async () => {
  // La réponse de la création doit trouver la fenêtre pour montrer le jeton : pendant qu'elle est en
  // route, Échap et « Annuler » n'y font rien (R1461).
  await opened();
  const answer = deferred<{ token: string; info: TokenInfo }>();
  vi.mocked(createToken).mockReturnValue(answer.promise);
  await fillAndSubmit();
  const escape = new Event("cancel", { cancelable: true });
  await act(async () => {
    dialog().dispatchEvent(escape);
  });
  expect(escape.defaultPrevented).toBe(true);
  await click(buttonNamed("Annuler")!);
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => answer.resolve({ token: TOKEN, info: info() }));
  expect(field("Jeton d'API").value).toBe(TOKEN);
  expect(onClose).not.toHaveBeenCalled();
});

test("the_level_says_its_hint_apart_from_its_name", async () => {
  await opened();
  // Le nom d'un niveau est son libellé ; sa précision le décrit, à part.
  for (const input of radios("level")) {
    const name = document.getElementById(input.getAttribute("aria-labelledby")!)?.textContent;
    const hint = document.getElementById(input.getAttribute("aria-describedby")!)?.textContent;
    expect(name).toBe(input.value === "read" ? "Lecture" : "Modification");
    expect(hint).toMatch(/^Il /);
  }
});

test("the_page_list_says_why_it_is_empty_and_enter_does_not_send", async () => {
  // L'arbre d'un autre espace : il n'est pas lu pour celui-ci.
  publishPages("autre", [page("x", { spaceId: "autre", title: "Ailleurs" })]);
  await opened();
  await click(radios("scope").find((input) => input.value === "pages")!);
  expect(dialog().textContent).toContain("ne sont pas encore lues");
  expect(dialog().textContent).not.toContain("Aucune page de l'arbre ne porte ce titre.");
  await act(async () => root.unmount());
  root = createRoot(host);

  // L'arbre de cet espace, sans page : il le dit ; une recherche sans réponse dit la sienne.
  publishPages("pro", [page("a", { spaceId: "pro", title: "Agenda" })]);
  await opened();
  await click(radios("scope").find((input) => input.value === "pages")!);
  await type(field("Chercher une page"), "zzz");
  expect(dialog().textContent).toContain("Aucune page de l'arbre ne porte ce titre.");
  // Entrée dans la recherche n'envoie pas le formulaire.
  const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  await act(async () => {
    field("Chercher une page").dispatchEvent(enter);
  });
  expect(enter.defaultPrevented).toBe(true);
});

test("sub_pages_come_under_their_page_set_in", async () => {
  // Recette du lot 5, B3 (R1553) : l'arbre publié est rangé par page parente, si bien qu'une
  // sous-page venait après les autres pages de la racine. Sans recherche, la liste suit l'arbre,
  // chaque sous-page sous sa page, en retrait de sa profondeur ; une recherche montre ses pages à
  // plat. Une page sous une page qui n'est pas dans l'arbre est à la racine de ce qu'on voit.
  publishPages("pro", [
    page("p", { spaceId: "pro", title: "Projet", position: "a0" }),
    page("a", { spaceId: "pro", title: "Autre", position: "a1" }),
    page("d", { spaceId: "pro", title: "Détails", parentId: "p", position: "a0" }),
    page("n", { spaceId: "pro", title: "Notes", parentId: "d", position: "a0" }),
    page("s", { spaceId: "pro", title: "Suite", parentId: "p", position: "a1" }),
    page("o", { spaceId: "pro", title: "Orpheline", parentId: "ailleurs", position: "a0" }),
  ]);
  await opened();
  await click(radios("scope").find((input) => input.value === "pages")!);
  const items = () => [...dialog().querySelectorAll<HTMLElement>('[aria-label="Pages proposées"] li')];
  const shown = () =>
    items().map((item) => `${item.style.getPropertyValue("--depth") || "0"} ${item.textContent?.trim()}`);
  expect(shown()).toEqual(["0 Projet", "1 Détails", "2 Notes", "1 Suite", "0 Autre", "0 Orpheline"]);
  // Cochée puis décochée, une sous-page reste à sa place.
  const box = (title: string) =>
    items()
      .find((item) => item.textContent?.trim() === title)!
      .querySelector<HTMLInputElement>("input")!;
  await click(box("Notes"));
  await click(box("Notes"));
  expect(shown()).toEqual(["0 Projet", "1 Détails", "2 Notes", "1 Suite", "0 Autre", "0 Orpheline"]);
  // Une recherche : ses pages, à plat.
  await type(field("Chercher une page"), "s");
  expect(shown().every((line) => line.startsWith("0 "))).toBe(true);
});

test("a_page_unchecked_keeps_its_place", async () => {
  publishPages("pro", [
    page("a", { spaceId: "pro", title: "Agenda" }),
    page("b", { spaceId: "pro", title: "Budget" }),
    page("c", { spaceId: "pro", title: "Comptes" }),
  ]);
  await opened();
  await click(radios("scope").find((input) => input.value === "pages")!);
  const boxes = (list: string) =>
    [...dialog().querySelectorAll<HTMLInputElement>(`[aria-label="${list}"] input[type="checkbox"]`)];
  const titles = (list: string) => boxes(list).map((box) => box.closest("label")?.textContent?.trim());
  await click(boxes("Pages proposées")[0]);
  await click(boxes("Pages proposées")[1]);
  // Une recherche qui ne les propose plus : elles passent dessous, cochées.
  await type(field("Chercher une page"), "comp");
  expect(titles("Autres pages choisies")).toEqual(["Agenda", "Budget"]);
  // Décochée, « Agenda » reste à sa place, et « Budget » ne monte pas sous le pointeur.
  await click(boxes("Autres pages choisies")[0]);
  expect(titles("Autres pages choisies")).toEqual(["Agenda", "Budget"]);
  expect(boxes("Autres pages choisies").map((box) => box.checked)).toEqual([false, true]);
});

test("an_outdated_draft_is_revised_at_opening", async () => {
  // Un espace quitté depuis, des pages choisies dont l'une n'est plus dans l'arbre.
  publishPages("pro", [page("a", { spaceId: "pro", title: "Agenda renommé" })]);
  const draft: TokenDraft = {
    name: "Agent",
    level: "edit",
    scope: "pages",
    spaceId: "quitte",
    chosen: [
      { id: "a", title: "Agenda" },
      { id: "z", title: "Partie" },
    ],
    pagesOf: "pro",
    canTrash: false,
    expiry: 30,
  };
  await opened(pro, draft);
  expect(field("Nom du jeton").value).toBe("Agent");
  const boxes = [...dialog().querySelectorAll<HTMLInputElement>('input[type="checkbox"]')].filter((b) => b.checked);
  expect(boxes.map((box) => box.closest("label")?.textContent?.trim())).toEqual(["Agenda renommé"]);
  await act(async () => root.unmount());
  root = createRoot(host);

  // Sans l'arbre de l'espace, une portée « pages » revient à la portée par défaut, et l'espace quitté
  // à l'espace affiché.
  publishPages(null, null);
  await opened(pro, draft);
  expect(radios("scope").find((input) => input.checked)?.value).toBe("space");
  expect(dialog().querySelector<HTMLSelectElement>('select[aria-label="Espace du jeton"]')!.value).toBe("pro");
});
