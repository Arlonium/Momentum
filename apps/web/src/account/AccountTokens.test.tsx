// L'écran « Jetons d'API » du compte (lot 5, Task C1) : le jeton montré une fois, dans la fenêtre qui
// le crée, et parti avec elle ; Échap n'y détruit rien sans demander ; rien de lui dans le stockage
// du navigateur ni dans la console ; une réponse perdue dit le doute et fait relire la liste ;
// révoquer demande d'abord, et rend le focus ; rien ne bouge sous le pointeur quand la liste est relue.
import type { Me, TokenInfo } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, createToken, listTokens, revokeToken } from "../api";
import { SessionContext } from "../session/session";
import { navigate, tokensPath, usePathname } from "../router";
import { renderHook, until } from "../test/hooks";
import { space } from "../test/pages";
import AccountTokens from "./AccountTokens";
import { announceTokensChanged } from "./tokensChanged";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  listTokens: vi.fn(),
  createToken: vi.fn(),
  revokeToken: vi.fn(),
}));

/** Un jeton de la forme du serveur, fabriqué ici : jamais un vrai. */
const TOKEN = "mmt_TEST0000000000000000000000000000000000000ab";
const pro = space("pro", { name: "Pro" });
const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [pro],
};

const token = (id: string, overrides: Partial<TokenInfo> = {}): TokenInfo => ({
  id,
  name: `Jeton ${id.toUpperCase()}`,
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
const revoked = (t: TokenInfo): TokenInfo => ({ ...t, live: false, revokedAt: 1_800_000_100_000 });

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(listTokens).mockReset();
  vi.mocked(createToken).mockReset();
  vi.mocked(revokeToken).mockReset();
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function (this: HTMLDialogElement) {
    this.removeAttribute("open");
    this.dispatchEvent(new Event("close"));
  };
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn(async () => {}) },
  });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  localStorage.clear();
  sessionStorage.clear();
});

async function rendered(tokens: TokenInfo[]) {
  vi.mocked(listTokens).mockResolvedValue(tokens);
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me, refresh: async () => {} }}>
        <AccountTokens space={pro} />
      </SessionContext.Provider>,
    ),
  );
  await until(host, () => host.querySelector('[aria-busy="false"]') !== null, "la liste lue");
}

const dialog = () => host.querySelector<HTMLDialogElement>("dialog.mm-token-dialog");
const confirmation = () => host.querySelector<HTMLDialogElement>('dialog[role="alertdialog"]');
const buttonNamed = (within: Element, name: string) =>
  [...within.querySelectorAll("button")].find((b) => b.textContent?.trim() === name) ?? null;
const lines = () => [...host.querySelectorAll<HTMLElement>("li[data-token-id]")];
const order = () => lines().map((line) => line.dataset.tokenId);
const lineOf = (id: string) => host.querySelector<HTMLElement>(`li[data-token-id="${id}"]`)!;
const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });
async function type(input: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const field = (label: string) =>
  [...dialog()!.querySelectorAll<HTMLInputElement>("input")].find(
    (input) => input.labels?.[0]?.textContent === label || input.getAttribute("aria-label") === label,
  )!;

/** Ouvre « Créer un jeton », remplit, envoie. */
async function create() {
  await click(buttonNamed(host, "Créer un jeton")!);
  await type(field("Nom du jeton"), "Claude Code");
  await type(field("Mot de passe actuel"), "mot-de-passe");
  await click(buttonNamed(dialog()!, "Créer le jeton")!);
}

/** Le champ qui montre le jeton. */
const TOKEN_FIELD = `[aria-label="Jeton d'API"]`;

/** Tout ce que la page montre ou garde dans ses champs, sauf le champ `except`. */
const everything = (except: Element | null = null) => {
  const body = document.body.cloneNode(true) as HTMLElement;
  if (except !== null) body.querySelector(TOKEN_FIELD)?.remove();
  const values = [...document.querySelectorAll("input")].filter((input) => input !== except).map((i) => i.value);
  return `${body.innerHTML} ${values.join(" ")}`;
};

test("the_token_is_shown_once_and_gone_when_the_window_closes", async () => {
  await rendered([]);
  const created = token("t1", { name: "Claude Code" });
  vi.mocked(createToken).mockResolvedValue({ token: TOKEN, info: created });
  vi.mocked(listTokens).mockResolvedValue([created]);
  await create();
  // Dans la fenêtre, une fois ; la liste relue derrière elle n'en montre que la fin.
  await until(host, () => lines().length === 1, "la liste relue");
  expect(field("Jeton d'API").value).toBe(TOKEN);
  // Le premier geste a le focus : copier la ligne qui attend le jeton (R1462).
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Copier la ligne 1");
  expect(dialog()!.querySelectorAll(TOKEN_FIELD)).toHaveLength(1);
  expect(everything(field("Jeton d'API"))).not.toContain(TOKEN);
  expect(lineOf("t1").textContent).toContain("…00ab");
  expect(lineOf("t1").textContent).not.toContain(TOKEN);

  // Fermée, la fenêtre emporte le jeton ; rouverte, elle repart d'un formulaire vide.
  await click(buttonNamed(dialog()!, "Fermer")!);
  expect(dialog()).toBeNull();
  expect(everything()).not.toContain(TOKEN);
  await click(buttonNamed(host, "Créer un jeton")!);
  expect(everything()).not.toContain(TOKEN);
  expect(field("Nom du jeton").value).toBe("");
  expect(field("Mot de passe actuel").value).toBe("");
  expect(dialog()!.querySelector('[aria-label="Jeton d\'API"]')).toBeNull();
});

test("escape_on_a_shown_token_asks_before_closing", async () => {
  await rendered([]);
  vi.mocked(createToken).mockResolvedValue({ token: TOKEN, info: token("t1") });
  // Avant la création, Échap ferme la fenêtre comme toute autre, sans rien détruire : rouverte, elle
  // retrouve ce qui a été choisi, jamais le mot de passe.
  await click(buttonNamed(host, "Créer un jeton")!);
  await type(field("Nom du jeton"), "Claude Code");
  await click(dialog()!.querySelector('input[type="radio"][value="edit"]')!);
  await type(field("Mot de passe actuel"), "mot-de-passe");
  await act(async () => {
    dialog()!.dispatchEvent(new Event("cancel", { cancelable: true }));
  });
  expect(dialog()).toBeNull();
  await click(buttonNamed(host, "Créer un jeton")!);
  expect(field("Nom du jeton").value).toBe("Claude Code");
  expect(dialog()!.querySelector<HTMLInputElement>('input[type="radio"][value="edit"]')!.checked).toBe(true);
  expect(field("Mot de passe actuel").value).toBe("");
  await click(buttonNamed(dialog()!, "Annuler")!);
  expect(dialog()).toBeNull();

  await create();
  expect(field("Jeton d'API").value).toBe(TOKEN);
  // Échap demande : la fenêtre et le jeton restent.
  const escape = new Event("cancel", { cancelable: true });
  await act(async () => {
    dialog()!.dispatchEvent(escape);
  });
  expect(escape.defaultPrevented).toBe(true);
  expect(confirmation()).not.toBeNull();
  expect(field("Jeton d'API").value).toBe(TOKEN);
  // Échap dans la confirmation la ferme, elle seule.
  await act(async () => {
    confirmation()!.dispatchEvent(new Event("cancel", { cancelable: true }));
  });
  expect(confirmation()).toBeNull();
  expect(field("Jeton d'API").value).toBe(TOKEN);

  // Chrome ferme quand même la fenêtre au second Échap sans geste entre les deux : elle se rouvre,
  // et demande.
  await act(async () => {
    dialog()!.removeAttribute("open");
    dialog()!.dispatchEvent(new Event("close"));
  });
  expect(dialog()!.hasAttribute("open")).toBe(true);
  expect(confirmation()).not.toBeNull();
  await click(buttonNamed(confirmation()!, "Rester")!);
  expect(confirmation()).toBeNull();
  expect(field("Jeton d'API").value).toBe(TOKEN);

  // Confirmé, il part.
  await act(async () => {
    dialog()!.dispatchEvent(new Event("cancel", { cancelable: true }));
  });
  await click(buttonNamed(confirmation()!, "Fermer")!);
  expect(dialog()).toBeNull();
  expect(everything()).not.toContain(TOKEN);
});

test("the_token_is_never_in_storage_nor_in_the_console", async () => {
  const logged: unknown[] = [];
  for (const level of ["log", "info", "warn", "error", "debug"] as const) {
    vi.spyOn(console, level).mockImplementation((...args: unknown[]) => void logged.push(...args));
  }
  const stored: unknown[] = [];
  vi.spyOn(Storage.prototype, "setItem").mockImplementation((key: string, value: string) =>
    void stored.push(key, value),
  );
  await rendered([]);
  vi.mocked(createToken).mockResolvedValue({ token: TOKEN, info: token("t1") });
  vi.mocked(listTokens).mockResolvedValue([token("t1")]);
  await create();
  // Copié : il va au presse-papiers, et nulle part ailleurs.
  await click(buttonNamed(dialog()!, "Copier le jeton")!);
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith(TOKEN);
  for (const label of ["bash ou zsh", "PowerShell", "cmd"]) {
    const shell = [...dialog()!.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find(
      (input) => input.closest("label")?.textContent?.trim() === label,
    )!;
    await click(shell);
  }
  await click(buttonNamed(dialog()!, "Fermer")!);

  const seen = (values: unknown[]) => values.map((value) => (typeof value === "string" ? value : String(value)));
  expect(seen(logged).filter((text) => text.includes(TOKEN))).toEqual([]);
  expect(seen(stored).filter((text) => text.includes(TOKEN))).toEqual([]);
  expect(JSON.stringify({ ...localStorage })).not.toContain(TOKEN);
  expect(JSON.stringify({ ...sessionStorage })).not.toContain(TOKEN);
  expect(location.href).not.toContain(TOKEN);
  expect(JSON.stringify(history.state ?? null)).not.toContain(TOKEN);
});

test("a_lost_answer_says_the_token_may_exist", async () => {
  await rendered([]);
  expect(listTokens).toHaveBeenCalledTimes(1);
  // Un refus du serveur (témoin) : rien n'a été créé, la liste n'est pas relue.
  vi.mocked(createToken).mockRejectedValueOnce(
    new ApiError(401, "invalid_credentials", "mot de passe actuel incorrect"),
  );
  await create();
  expect(listTokens).toHaveBeenCalledTimes(1);

  // La réponse perdue : le jeton a peut-être été créé, sans que personne en ait le texte.
  vi.mocked(listTokens).mockResolvedValue([token("t1")]);
  vi.mocked(createToken).mockRejectedValueOnce(
    new ApiError(0, null, "La réponse du serveur n'est pas arrivée : ce que tu as demandé a peut-être été fait."),
  );
  await click(buttonNamed(dialog()!, "Créer le jeton")!);
  const alert = dialog()!.querySelector('[role="alert"]')?.textContent ?? "";
  expect(alert).toContain("le jeton a peut-être été créé");
  expect(alert).toContain("révoque-le");
  // La liste est relue : elle dit s'il existe.
  await until(host, () => lines().length === 1, "la liste relue");
  expect(listTokens).toHaveBeenCalledTimes(2);
  // La fenêtre reste sur son formulaire : aucun jeton à montrer.
  expect(dialog()!.querySelector('[aria-label="Jeton d\'API"]')).toBeNull();
});

test("revoking_asks_first_and_gives_the_focus_back", async () => {
  const a = token("a");
  await rendered([a, token("b")]);
  const revoke = () => buttonNamed(lineOf("a"), "Révoquer")!;

  // Il demande d'abord ; annulé, rien n'est envoyé, et le focus revient au bouton.
  revoke().focus();
  await click(revoke());
  expect(confirmation()).not.toBeNull();
  // Le nom du jeton, venu d'ailleurs, isolé (U+2068 et U+2069), dans le titre et dans le nom du bouton.
  const isolated = `${String.fromCharCode(0x2068)}Jeton A${String.fromCharCode(0x2069)}`;
  expect(confirmation()!.querySelector("h2")!.textContent).toContain(isolated);
  expect(revoke().getAttribute("aria-label")).toContain(isolated);
  await click(buttonNamed(confirmation()!, "Annuler")!);
  expect(confirmation()).toBeNull();
  expect(revokeToken).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(revoke());

  // Confirmé : le jeton est révoqué, la liste relue le dit à sa place, et le focus, que le bouton
  // parti ne peut plus tenir, va à sa ligne, jamais sur `body`.
  vi.mocked(revokeToken).mockResolvedValue(undefined);
  vi.mocked(listTokens).mockResolvedValue([token("b"), revoked(a)]);
  revoke().focus();
  await click(revoke());
  await click(buttonNamed(confirmation()!, "Révoquer")!);
  await until(host, () => confirmation() === null, "la confirmation fermée");
  expect(revokeToken).toHaveBeenCalledWith("a");
  expect(lineOf("a").textContent).toContain("Révoqué");
  expect(buttonNamed(lineOf("a"), "Révoquer")).toBeNull();
  expect(document.activeElement).not.toBe(document.body);
  expect(lineOf("a").contains(document.activeElement)).toBe(true);
});

test("nothing_moves_under_the_pointer_when_the_list_is_read_again", async () => {
  const a = token("a");
  const b = token("b");
  await rendered([a, b]);
  expect(order()).toEqual(["a", "b"]);

  // Le pointeur sur la liste : une relecture qui révoque « A » (les vivants d'abord) et apporte « C »
  // laisse chaque ligne à sa place ; « A » dit sa révocation là où elle est, « C » vient à la fin.
  await act(async () => {
    lineOf("a").dispatchEvent(new MouseEvent("pointerover", { bubbles: true }));
  });
  vi.mocked(listTokens).mockResolvedValue([token("c"), b, revoked(a)]);
  await act(async () => announceTokensChanged());
  await until(host, () => lines().length === 3, "la liste relue");
  expect(order()).toEqual(["a", "b", "c"]);
  expect(lineOf("a").textContent).toContain("Révoqué");

  // Le pointeur part : l'ordre du serveur revient.
  await act(async () => {
    document.body.dispatchEvent(new MouseEvent("pointerover", { bubbles: true }));
  });
  expect(order()).toEqual(["c", "b", "a"]);

  // Le focus dans la liste gèle aussi, pointeur parti.
  await act(async () => buttonNamed(lineOf("b"), "Révoquer")!.focus());
  vi.mocked(listTokens).mockResolvedValue([b, token("c"), revoked(a)]);
  await act(async () => announceTokensChanged());
  await until(host, () => vi.mocked(listTokens).mock.calls.length === 3, "la liste relue");
  await act(async () => {});
  expect(order()).toEqual(["c", "b", "a"]);
});

test("the_screen_takes_the_focus_when_it_arrives_on_body", async () => {
  // Arrivé depuis le menu du compte au clavier, le lien suivi part avec le menu : le titre de l'écran
  // prend le focus, jamais `body` (R1621).
  (document.activeElement as HTMLElement | null)?.blur();
  await rendered([]);
  expect(document.activeElement?.tagName).toBe("H1");
  expect(document.activeElement?.textContent).toBe("Jetons d'API");
});

test("leaving_with_a_shown_token_asks_first", async () => {
  // Le routeur suit l'adresse, comme dans l'application (`App` lit `usePathname`).
  history.replaceState(null, "", "/s/pro");
  await renderHook(usePathname, undefined);
  await act(async () => navigate(tokensPath()));
  await rendered([]);
  vi.mocked(createToken).mockResolvedValue({ token: TOKEN, info: token("t1") });
  // Sans jeton montré, rien n'est retenu.
  const before = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(before);
  expect(before.defaultPrevented).toBe(false);

  await create();
  expect(field("Jeton d'API").value).toBe(TOKEN);
  // Recharger ou fermer l'onglet : le navigateur demande.
  const unload = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(unload);
  expect(unload.defaultPrevented).toBe(true);
  // Précédent : l'adresse revient, et la fenêtre demande ; « Rester » garde le jeton.
  await act(async () => {
    const popped = new Promise((done) => window.addEventListener("popstate", done, { once: true }));
    history.back();
    await popped;
  });
  expect(location.pathname).toBe("/account/tokens");
  expect(confirmation()).not.toBeNull();
  expect(document.activeElement).not.toBe(document.body);
  await click(buttonNamed(confirmation()!, "Rester")!);
  expect(field("Jeton d'API").value).toBe(TOKEN);

  // Fermée, la fenêtre ne retient plus rien.
  await click(buttonNamed(dialog()!, "Fermer")!);
  const after = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(after);
  expect(after.defaultPrevented).toBe(false);
});
