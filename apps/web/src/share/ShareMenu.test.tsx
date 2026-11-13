// Le menu Partager, pour l'accès général d'une page ouverte aux membres par sa propre ligne :
// la fermer aux membres (« Seulement les invités ») la retire de la section « Espace » de tous
// les autres, et demande donc une confirmation (Ruling R127). Baisser le niveau des membres,
// ou leur ouvrir une page, n'en demande pas. Et une invitation en attente nomme sa page (R257,
// M11).
import type { AccessEntry, Me, PageAccess, PendingInvitation } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getAccess, setGeneralAccess } from "../api";
import { SessionContext } from "../session/session";
import "../test/hooks";
import { page, space } from "../test/pages";
import { ShareMenu } from "./ShareMenu";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getAccess: vi.fn(),
  setGeneralAccess: vi.fn(),
}));

const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("space")],
};
const own: AccessEntry = {
  userId: "alice",
  name: "Alice",
  email: "alice@exemple.fr",
  level: "full",
  inherited: false,
};
const access = (overrides: Partial<PageAccess> = {}): PageAccess => ({
  general: "full",
  inheritedGeneral: null,
  entries: [],
  invitations: [],
  ...overrides,
});

let host: HTMLDivElement;
let root: Root;
const onChanged = vi.fn();
const onAccessLost = vi.fn();

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  for (const mock of [onChanged, onAccessLost]) mock.mockClear();
  for (const mock of [getAccess, setGeneralAccess]) vi.mocked(mock).mockReset();
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

/** Le menu ouvert sur une page dont le serveur rend les accès `first`. */
async function opened(first: PageAccess = access()) {
  vi.mocked(getAccess).mockResolvedValue(first);
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me, refresh: async () => {} }}>
        <ShareMenu node={page("x")} onChanged={onChanged} onAccessLost={onAccessLost} />
      </SessionContext.Provider>,
    ),
  );
  await click(host.querySelector("button")!);
  expect(host.querySelector('[role="dialog"]')?.getAttribute("aria-busy")).toBe("false");
}

const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });

/** Un choix de l'accès général, par son nom. */
function radio(name: string): HTMLInputElement {
  const found = [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')].find(
    (input) => document.getElementById(input.getAttribute("aria-labelledby")!)?.textContent === name,
  );
  if (!found) throw new Error(`pas de choix « ${name} »`);
  return found;
}
const invited = () => radio("Seulement les invités");
const members = () => radio("Tous les membres de l'espace");
const confirmation = () => host.querySelector<HTMLDialogElement>('dialog[role="alertdialog"]');
/** Le texte de la fenêtre de confirmation, sous son titre. */
const lead = (dialog: Element) => dialog.querySelector(".mm-dialog-lead")?.textContent ?? "";
const button = (within: Element, name: string) => {
  const found = [...within.querySelectorAll("button")].find((b) => b.textContent === name);
  if (!found) throw new Error(`pas de bouton « ${name} »`);
  return found;
};

test("closing_a_page_to_members_asks_first", async () => {
  await opened();
  expect(members().checked).toBe(true);
  await click(invited());
  // Une fenêtre de la page, comme pour « Retirer » : rien n'est encore demandé au serveur, et
  // le choix reste où il était.
  const dialog = confirmation()!;
  expect(dialog.querySelector("h2")?.textContent).toBe("Fermer cette page aux membres\u00a0?");
  // Personne n'est nommé sur cette page : le texte ne renvoie à personne.
  expect(lead(dialog)).toBe(
    "Les autres membres de l'espace ne la verront plus, ni ses sous-pages. Tu y gardes l'accès complet.",
  );
  expect([...dialog.querySelectorAll("button")].map((b) => b.textContent)).toEqual([
    "Annuler",
    "Fermer aux membres",
  ]);
  // Rouge, comme les autres confirmations ; et le focus sur « Annuler ».
  expect(button(dialog, "Fermer aux membres").className).toContain("mm-button-danger");
  expect(document.activeElement).toBe(button(dialog, "Annuler"));
  expect(setGeneralAccess).not.toHaveBeenCalled();
  expect(members().checked).toBe(true);
});

test("confirmation_mentions_the_people_named_only_when_there_are_others", async () => {
  const bruno: AccessEntry = { ...own, userId: "bruno", name: "Bruno", email: "bruno@exemple.fr" };
  const alone = "Tu y gardes l'accès complet.";
  const withOthers = "Tu y gardes l'accès complet, avec les personnes nommées ci-dessus.";
  // Seule la personne connectée est nommée : « les personnes nommées ci-dessus » ne renverrait
  // qu'à elle.
  await opened(access({ entries: [own] }));
  await click(invited());
  expect(lead(confirmation()!).endsWith(` ${alone}`)).toBe(true);
  await click(button(confirmation()!, "Annuler"));
  await act(async () => root.unmount());

  // Quelqu'un d'autre est nommé, avec elle ou sans elle : il garde son accès, et le texte le
  // dit.
  for (const entries of [[own, bruno], [bruno]]) {
    root = createRoot(host);
    await opened(access({ entries }));
    await click(invited());
    expect(lead(confirmation()!).endsWith(` ${withOthers}`)).toBe(true);
    await click(button(confirmation()!, "Annuler"));
    await act(async () => root.unmount());
  }
  root = createRoot(host);
});

test("cancelling_leaves_the_page_open_to_members", async () => {
  await opened();
  await click(invited());
  await click(button(confirmation()!, "Annuler"));
  expect(confirmation()).toBeNull();
  expect(setGeneralAccess).not.toHaveBeenCalled();
  expect(members().checked).toBe(true);
  expect(invited().checked).toBe(false);
  // Le menu est toujours ouvert.
  expect(host.querySelector('[role="dialog"]')).not.toBeNull();
  expect(onChanged).not.toHaveBeenCalled();
});

test("confirming_closes_the_page_to_members", async () => {
  await opened();
  // Le serveur garde l'accès complet à qui ferme la page (Ruling R122), et rend les accès.
  vi.mocked(setGeneralAccess).mockResolvedValue(access({ general: null, entries: [own] }));
  await click(invited());
  await click(button(confirmation()!, "Fermer aux membres"));
  expect(setGeneralAccess).toHaveBeenCalledExactlyOnceWith("x", null);
  expect(confirmation()).toBeNull();
  expect(invited().checked).toBe(true);
  expect(host.textContent).toContain("alice@exemple.fr");
  // La barre latérale est à relire : la page a changé de section.
  expect(onChanged).toHaveBeenCalledTimes(1);
  expect(onAccessLost).not.toHaveBeenCalled();
});

test("lowering_the_members_level_asks_nothing", async () => {
  await opened();
  vi.mocked(setGeneralAccess).mockResolvedValue(access({ general: "read" }));
  const level = host.querySelector<HTMLSelectElement>(
    'select[aria-label="Niveau des membres de l\'espace"]',
  )!;
  await act(async () => {
    level.value = "read";
    level.dispatchEvent(new Event("change", { bubbles: true }));
  });
  // Les membres voient toujours la page : pas de confirmation.
  expect(confirmation()).toBeNull();
  expect(setGeneralAccess).toHaveBeenCalledExactlyOnceWith("x", "read");
});

test("opening_a_page_to_members_asks_nothing", async () => {
  await opened(access({ general: null, entries: [own] }));
  vi.mocked(setGeneralAccess).mockResolvedValue(access({ general: "read", entries: [own] }));
  expect(invited().checked).toBe(true);
  await click(members());
  expect(confirmation()).toBeNull();
  expect(setGeneralAccess).toHaveBeenCalledExactlyOnceWith("x", "read");
  expect(members().checked).toBe(true);
});

/** Une invitation de la page `x` qui attend, en modification. */
const pendingOn = (id: string, pageTitle: string): PendingInvitation => ({
  id,
  email: `${id}@exemple.fr`,
  role: null,
  level: "edit",
  pageId: "x",
  pageTitle,
  expiresAt: 1_800_000_000_000,
});

test("a_pending_page_invitation_names_its_page", async () => {
  await opened(access({ invitations: [pendingOn("i1", "Plan"), pendingOn("i2", "")] }));
  const details = [
    ...host.querySelectorAll('[aria-label="Invitations en attente"] .mm-person-detail'),
  ];
  // Le niveau, puis la page, son titre isolé ; « Sans titre » pour une page qui n'en a pas.
  expect(details.map((detail) => detail.textContent!.split(" · ")[0])).toEqual([
    "Peut modifier sur « Plan »",
    "Peut modifier sur « Sans titre »",
  ]);
  expect(details[0].querySelector('.mm-isolate[dir="auto"]')?.textContent).toBe("Plan");
  expect(details[0].textContent).toContain(" · expire le ");
});
