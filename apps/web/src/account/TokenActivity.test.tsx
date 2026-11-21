// L'activité d'un jeton d'API (lot 5, Task C2), dépliée sous sa ligne dans l'écran « Jetons d'API » :
// lue seulement quand on la demande, page par page ; les appels refusés, le débit et les lignes
// purgées y sont dits ; une page que le compte ne voit plus n'y a pas de titre.
import type { Me, TokenCall, TokenCalls, TokenInfo } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { listTokens, tokenCalls } from "../api";
import { SessionContext } from "../session/session";
import { deferred, until } from "../test/hooks";
import { space } from "../test/pages";
import AccountTokens from "./AccountTokens";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  listTokens: vi.fn(),
  tokenCalls: vi.fn(),
}));

const pro = space("pro", { name: "Pro" });
const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [pro],
};

const token = (id: string, overrides: Partial<TokenInfo> = {}): TokenInfo => ({
  id,
  name: `Jeton ${id.toUpperCase()}`,
  hint: "00ab",
  level: "edit",
  scope: "all",
  space: null,
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

const call = (id: number, overrides: Partial<TokenCall> = {}): TokenCall => ({
  id,
  at: Date.UTC(2026, 10, 21, 14, 2, 5) + id * 1000,
  surface: "rest",
  tool: "GET /api/v1/pages/{id}",
  spaceId: "pro",
  pageId: null,
  pageTitle: null,
  outcome: "ok",
  code: null,
  revisionBefore: null,
  revisionAfter: null,
  blocksTouched: null,
  ...overrides,
});

const answer = (calls: TokenCall[], overrides: Partial<TokenCalls> = {}): TokenCalls => ({
  calls,
  more: false,
  busyCount: 0,
  purged: 0,
  ...overrides,
});

/** Les rangs `from` à `to`, en descendant (le plus récent d'abord). */
const ranks = (from: number, to: number) => Array.from({ length: from - to + 1 }, (_, i) => from - i);

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(listTokens).mockReset();
  vi.mocked(tokenCalls).mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
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

const lineOf = (id: string) => host.querySelector<HTMLElement>(`li[data-token-id="${id}"]`)!;
const toggleOf = (id: string) => lineOf(id).querySelector<HTMLButtonElement>("button[aria-expanded]")!;
const activityOf = (id: string) => lineOf(id).querySelector<HTMLElement>(".mm-token-activity");
const shownCalls = (id: string) =>
  [...(activityOf(id)?.querySelectorAll<HTMLElement>("li[data-call-id]") ?? [])].map((li) =>
    Number(li.dataset.callId),
  );
const buttonNamed = (within: Element, name: string) =>
  [...within.querySelectorAll("button")].find((b) => b.textContent?.trim() === name) ?? null;
/** Le texte, ses espaces insécables (fines ou non) lus comme des espaces. */
const plain = (element: Element | null) => (element?.textContent ?? "").replace(/[\u00a0\u202f]/g, " ");
const click = (element: Element, detail = 1) =>
  act(async () => {
    element.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, detail }));
  });

test("the_activity_is_read_on_demand_and_paged", async () => {
  await rendered([token("a"), token("b")]);
  // La liste des jetons ne lit l'activité d'aucun.
  await new Promise((done) => setTimeout(done, 20));
  expect(tokenCalls).not.toHaveBeenCalled();
  expect(toggleOf("a").getAttribute("aria-expanded")).toBe("false");
  expect(toggleOf("a").textContent).toBe("Activité");

  // Dépliée, l'activité de « A » lit sa première page, et elle seule.
  vi.mocked(tokenCalls).mockResolvedValueOnce(answer(ranks(150, 101).map((id) => call(id)), { more: true }));
  await click(toggleOf("a"));
  expect(toggleOf("a").getAttribute("aria-expanded")).toBe("true");
  expect(toggleOf("a").getAttribute("aria-controls")).toBe(activityOf("a")!.id);
  await until(host, () => shownCalls("a").length === 50, "la première page");
  expect(tokenCalls).toHaveBeenCalledTimes(1);
  expect(vi.mocked(tokenCalls).mock.calls[0]).toEqual(["a", { before: undefined, outcome: undefined }]);
  expect(shownCalls("a")).toEqual(ranks(150, 101));
  expect(activityOf("b")).toBeNull();

  // La suite : la page avant la dernière ligne lue, ajoutée dessous ; le second clic d'un double clic,
  // arrivé une fois la suite lue, n'en demande pas une autre.
  vi.mocked(tokenCalls).mockResolvedValueOnce(answer(ranks(100, 96).map((id) => call(id)), { more: true }));
  const more = buttonNamed(activityOf("a")!, "Afficher plus d'appels")!;
  more.focus();
  await click(more);
  await until(host, () => shownCalls("a").length === 55, "la suite");
  expect(vi.mocked(tokenCalls).mock.calls[1]).toEqual(["a", { before: 101, outcome: undefined }]);
  await click(more, 2);
  await new Promise((done) => setTimeout(done, 20));
  expect(tokenCalls).toHaveBeenCalledTimes(2);
  expect(shownCalls("a")).toEqual(ranks(150, 96));

  // Grisé sans être retiré pendant la demande : il garde le focus, et un autre clic n'y fait rien.
  const later = deferred<TokenCalls>();
  vi.mocked(tokenCalls).mockReturnValueOnce(later.promise);
  await click(more);
  await until(host, () => vi.mocked(tokenCalls).mock.calls.length === 3, "la suite demandée");
  expect(vi.mocked(tokenCalls).mock.calls[2]).toEqual(["a", { before: 96, outcome: undefined }]);
  expect(more.getAttribute("aria-disabled")).toBe("true");
  expect(document.activeElement).toBe(more);
  await click(more);
  await act(async () => later.resolve(answer(ranks(95, 91).map((id) => call(id)))));
  await until(host, () => shownCalls("a").length === 60, "la fin");
  expect(tokenCalls).toHaveBeenCalledTimes(3);
  expect(shownCalls("a")).toEqual(ranks(150, 91));
  // Plus rien à lire : une phrase prend la place du bouton, et le focus qu'il avait ne tombe pas sur
  // `body`.
  expect(buttonNamed(activityOf("a")!, "Afficher plus d'appels")).toBeNull();
  expect(activityOf("a")!.textContent).toContain("Tous les appels gardés sont affichés.");
  expect(document.activeElement).not.toBe(document.body);
  expect(activityOf("a")!.contains(document.activeElement)).toBe(true);

  // Une autre issue repart de zéro, sous ce filtre.
  vi.mocked(tokenCalls).mockResolvedValueOnce(answer([call(7, { outcome: "refused", code: "forbidden" })]));
  const filter = activityOf("a")!.querySelector("select")!;
  await act(async () => {
    filter.value = "refused";
    filter.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await until(host, () => shownCalls("a").length === 1, "les refus");
  expect(vi.mocked(tokenCalls).mock.calls[3]).toEqual(["a", { before: undefined, outcome: "refused" }]);

  // Repliée, elle part ; dépliée de nouveau, elle est relue.
  await click(toggleOf("a"));
  expect(activityOf("a")).toBeNull();
  expect(document.activeElement).toBe(toggleOf("a"));
  vi.mocked(tokenCalls).mockResolvedValueOnce(answer([call(160)]));
  await click(toggleOf("a"));
  await until(host, () => shownCalls("a").length === 1, "l'activité relue");
  expect(tokenCalls).toHaveBeenCalledTimes(5);
  expect(vi.mocked(tokenCalls).mock.calls[4]).toEqual(["a", { before: undefined, outcome: undefined }]);
});

test("refused_and_busy_calls_and_purged_lines_are_told", async () => {
  // Un jeton mort garde son activité : sa ligne l'offre encore.
  await rendered([token("a", { live: false, revokedAt: 1_800_000_100_000 })]);
  vi.mocked(tokenCalls).mockResolvedValueOnce(
    answer(
      [
        call(3, { outcome: "refused", code: "forbidden", tool: "update_page", surface: "mcp" }),
        call(2, { outcome: "error", code: "internal" }),
        call(1, { tool: "POST /api/v1/pages", blocksTouched: 3 }),
      ],
      { busyCount: 12, purged: 1234 },
    ),
  );
  await click(toggleOf("a"));
  await until(host, () => shownCalls("a").length === 3, "l'activité");
  const activity = activityOf("a")!;
  const text = plain(activity);
  expect(text).toContain("1 234 appels plus anciens ne sont plus gardés.");
  expect(text).toContain("12 appels refusés par le débit");
  const line = (id: number) => activity.querySelector<HTMLElement>(`li[data-call-id="${id}"]`)!;
  expect(line(3).querySelector(".mm-token-call-outcome")!.textContent).toBe("Refusé");
  expect(line(3).querySelector(".mm-token-call-code")!.textContent).toBe("forbidden");
  expect(line(3).querySelector(".mm-token-call-tool")!.textContent).toBe("update_page");
  expect(line(2).querySelector(".mm-token-call-outcome")!.textContent).toBe("En erreur");
  expect(line(2).querySelector(".mm-token-call-code")!.textContent).toBe("internal");
  expect(line(1).querySelector(".mm-token-call-outcome")!.textContent).toBe("Réussi");
  expect(line(1).querySelector(".mm-token-call-code")).toBeNull();
  expect(plain(line(1))).toContain("3 blocs touchés");
  // La date : le jour, l'heure à la seconde, et sa forme lisible par une machine.
  expect(line(1).querySelector("time")!.getAttribute("dateTime")).toBe(new Date(call(1).at).toISOString());

  // Au singulier ; et rien n'est dit de ce qui n'a pas eu lieu.
  await click(toggleOf("a"));
  vi.mocked(tokenCalls).mockResolvedValueOnce(answer([call(1)], { busyCount: 1, purged: 1 }));
  await click(toggleOf("a"));
  await until(host, () => shownCalls("a").length === 1, "l'activité relue");
  expect(plain(activityOf("a"))).toContain("1 appel plus ancien n'est plus gardé.");
  expect(plain(activityOf("a"))).toContain("1 appel refusé par le débit");
  await click(toggleOf("a"));
  vi.mocked(tokenCalls).mockResolvedValueOnce(answer([call(1)]));
  await click(toggleOf("a"));
  await until(host, () => shownCalls("a").length === 1, "l'activité relue");
  expect(plain(activityOf("a"))).not.toContain("plus gardé");
  expect(plain(activityOf("a"))).not.toContain("débit");
});

test("a_page_one_no_longer_sees_has_no_title", async () => {
  await rendered([token("a")]);
  vi.mocked(tokenCalls).mockResolvedValueOnce(
    answer([
      call(3, { pageId: "page-qu-on-ne-voit-plus", pageTitle: null }),
      call(2, { pageId: "projet", pageTitle: "Projet" }),
      call(1, { pageId: null, pageTitle: null, tool: "GET /api/v1/spaces" }),
    ]),
  );
  await click(toggleOf("a"));
  await until(host, () => shownCalls("a").length === 3, "l'activité");
  const activity = activityOf("a")!;
  const line = (id: number) => activity.querySelector<HTMLElement>(`li[data-call-id="${id}"]`)!;
  expect(plain(line(3))).toContain("une page que tu ne vois plus");
  // Ni son titre, ni son identifiant : rien de la page.
  expect(activity.innerHTML).not.toContain("page-qu-on-ne-voit-plus");
  // Une page vue : son titre, venu d'ailleurs, isolé.
  const title = line(2).querySelector<HTMLElement>(".mm-token-call-page .mm-isolate")!;
  expect(title.textContent).toBe("Projet");
  expect(title.getAttribute("dir")).toBe("auto");
  expect(plain(line(2))).not.toContain("ne vois plus");
  // Un appel qui ne nomme aucune page n'en dit rien.
  expect(line(1).querySelector(".mm-token-call-page")).toBeNull();
});
