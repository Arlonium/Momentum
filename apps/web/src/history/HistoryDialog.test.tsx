// La fenêtre « Historique » d'une page (R135) : la liste des versions (« Version actuelle », puis
// chaque version, la plus récente d'abord, avec ses auteurs), l'aperçu de celle qu'on choisit, et
// « Restaurer cette version » pour qui peut modifier la page, après une confirmation qui prévient
// quand d'autres ont la page ouverte. L'aperçu lui-même est essayé à part (`VersionPreview`).
import type { Level, Version } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as Y from "yjs";
import { ApiError, getVersions, restoreVersion } from "../api";
import type { PageEvent, RosterUser, SyncStatus } from "../editor/collab";
import { author } from "../test/comments";
import "../test/hooks";
import { page } from "../test/pages";
import { HistoryDialog } from "./HistoryDialog";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getVersions: vi.fn(),
  restoreVersion: vi.fn(),
}));

// L'aperçu (BlockNote) est essayé dans son propre fichier : ici, il dit seulement ce qu'il montre.
vi.mock("./VersionPreview", () => ({
  VersionPreview: ({ versionId, current }: { versionId: string | null; current: unknown }) => (
    <div
      data-testid="preview"
      data-version={versionId ?? "current"}
      data-has-current={String(current !== null)}
    />
  ),
}));

const MINUTE = 60_000;
const NOW = Date.UTC(2026, 10, 12, 14, 0);

const v2: Version = {
  id: "v2",
  createdAt: NOW - 5 * MINUTE,
  reason: "before_restore",
  authors: [author("alice", "Alice"), author("bruno", "Bruno")],
};
const v1: Version = {
  id: "v1",
  createdAt: NOW - 3 * 60 * MINUTE,
  reason: "periodic",
  authors: [author("alice", "Alice")],
};

/** Une session de synchro réduite à ce que la fenêtre lit. */
function fakeSession(roster: RosterUser[] = []) {
  const listeners = new Set<() => void>();
  const events = new Set<(event: PageEvent) => void>();
  const session = {
    synced: true,
    status: "connected" as SyncStatus,
    roster,
    provider: { doc: new Y.Doc() },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    onPageEvent(listener: (event: PageEvent) => void) {
      events.add(listener);
      return () => events.delete(listener);
    },
    frame(event: PageEvent = "versions") {
      for (const listener of [...events]) listener(event);
    },
    setRoster(next: RosterUser[]) {
      session.roster = next;
      for (const listener of [...listeners]) listener();
    },
  };
  return session;
}

type Session = ReturnType<typeof fakeSession>;

let host: HTMLDivElement;
let root: Root;
const onClose = vi.fn();
const onRefused = vi.fn();

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  onClose.mockClear();
  onRefused.mockClear();
  vi.mocked(getVersions).mockReset();
  vi.mocked(restoreVersion).mockReset();
  // jsdom n'ouvre pas les fenêtres `dialog`.
  HTMLDialogElement.prototype.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

type Options = {
  level?: Level;
  trashed?: boolean;
  readOnly?: boolean;
  session?: Session | null;
  versions?: Version[];
};

/** La fenêtre ouverte, la liste lue. */
async function opened({
  level = "full",
  trashed = false,
  readOnly = false,
  session = fakeSession(),
  versions = [v2, v1],
}: Options = {}) {
  vi.mocked(getVersions).mockResolvedValue(versions);
  const node = page("p", { level, trashedAt: trashed ? NOW - MINUTE : null });
  await act(async () =>
    root.render(
      <HistoryDialog
        node={node}
        session={session}
        readOnly={readOnly}
        selfId="alice"
        onRefused={onRefused}
        onClose={onClose}
      />,
    ),
  );
  // La liste est lue au tour suivant (`setTimeout`) : on attend qu'elle soit là.
  await until(() => list().getAttribute("aria-busy") === "false", "la liste");
  return { session };
}

/** Attend que `check` soit vrai, en laissant React et les promesses suivre (borne large). */
async function until(check: () => boolean, what: string): Promise<void> {
  // `Date` est figée par le test : la borne se mesure autrement.
  const end = performance.now() + 5_000;
  while (!check()) {
    if (performance.now() > end) throw new Error(`toujours pas : ${what}`);
    await act(async () => {
      await new Promise((done) => setTimeout(done, 10));
    });
  }
}

const dialog = () => host.querySelector<HTMLDialogElement>("dialog.mm-history")!;
const list = () => host.querySelector<HTMLElement>('ul[aria-label="Versions de la page"]')!;
const items = () => [...list().querySelectorAll<HTMLButtonElement>(":scope > li > button")];
const preview = () => host.querySelector<HTMLElement>('[data-testid="preview"]');
const confirmation = () => host.querySelector<HTMLDialogElement>('dialog[role="alertdialog"]');
const buttonNamed = (within: Element, name: string) =>
  [...within.querySelectorAll("button")].find((b) => b.textContent === name) ?? null;
const restoreButton = () => buttonNamed(dialog(), "Restaurer cette version");
const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });
const key = (element: Element, init: KeyboardEventInit) =>
  act(async () => {
    element.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
  });

test("versions_are_listed_most_recent_first_with_their_authors", async () => {
  await opened();
  expect(dialog().getAttribute("open")).not.toBeNull();
  expect(host.querySelector("h2")?.textContent).toBe("Historique");
  // « Version actuelle », puis les versions dans l'ordre du serveur : la plus récente d'abord.
  const [current, recent, older] = items();
  expect(items()).toHaveLength(3);
  expect(current.textContent).toContain("Version actuelle");
  expect(recent.querySelector("time")?.getAttribute("dateTime")).toBe(new Date(v2.createdAt).toISOString());
  expect(recent.textContent).toContain("il y a 5 minutes");
  expect(recent.textContent).toContain("avant une restauration");
  expect([...recent.querySelectorAll(".mm-history-author")].map((n) => n.textContent)).toEqual([
    "Alice",
    "Bruno",
  ]);
  // Une avatar par auteur, sans nom accessible à lui : le nom est écrit à côté.
  expect(recent.querySelectorAll(".mm-history-avatar")).toHaveLength(2);
  expect(older.textContent).toContain("il y a 3 heures");
  expect(older.textContent).not.toContain("avant une restauration");
  expect([...older.querySelectorAll(".mm-history-author")].map((n) => n.textContent)).toEqual([
    "Alice",
  ]);
  // À l'ouverture, la version actuelle est choisie : aucun état de version n'est demandé.
  expect(current.getAttribute("aria-current")).toBe("true");
  expect(preview()?.dataset.version).toBe("current");
  expect(preview()?.dataset.hasCurrent).toBe("true");

  // Choisir une version la montre.
  await click(older);
  expect(older.getAttribute("aria-current")).toBe("true");
  expect(current.getAttribute("aria-current")).toBeNull();
  expect(preview()?.dataset.version).toBe("v1");
});

test("a_page_without_versions_says_so", async () => {
  await opened({ versions: [] });
  expect(items()).toHaveLength(1);
  expect(dialog().textContent).toContain("Aucune version pour l'instant");
});

test("restore_is_absent_below_edit_level", async () => {
  for (const level of ["read", "comment"] as const) {
    await opened({ level });
    await click(items()[1]);
    expect(preview()?.dataset.version).toBe("v2");
    expect(restoreButton(), level).toBeNull();
    await act(async () => root.unmount());
    root = createRoot(host);
  }
  for (const level of ["edit", "full"] as const) {
    await opened({ level });
    // Pas pour la version actuelle : elle est déjà le contenu de la page.
    expect(restoreButton(), level).toBeNull();
    await click(items()[1]);
    expect(restoreButton(), level).not.toBeNull();
    await act(async () => root.unmount());
    root = createRoot(host);
  }
  // Ni à la corbeille, ni dans un onglet d'un autre schéma que le serveur (R35).
  for (const options of [{ trashed: true }, { readOnly: true }]) {
    await opened(options);
    await click(items()[1]);
    expect(preview()?.dataset.version).toBe("v2");
    expect(restoreButton(), JSON.stringify(options)).toBeNull();
    await act(async () => root.unmount());
    root = createRoot(host);
  }
});

const REPLACED = "Le contenu actuel sera remplacé. Il restera dans l'historique.";
const OTHERS =
  "D'autres personnes ont cette page ouverte : ce qu'elles tapent pendant la restauration peut être perdu.";

test("restore_asks_first_and_restores", async () => {
  // Seule la personne connectée a la page ouverte : pas de mise en garde de plus.
  await opened({ session: fakeSession([{ id: "alice", name: "Alice", canWrite: true }]) });
  await click(items()[2]);
  await click(restoreButton()!);
  const asked = confirmation()!;
  expect(asked.querySelector("h2")?.textContent).toBe("Restaurer cette version ?");
  expect(asked.querySelector(".mm-dialog-lead")?.textContent).toBe(REPLACED);
  expect(restoreVersion).not.toHaveBeenCalled();
  // Le focus va sur « Annuler » : Entrée par réflexe ne restaure rien.
  expect(document.activeElement).toBe(buttonNamed(asked, "Annuler"));

  vi.mocked(restoreVersion).mockResolvedValue(undefined);
  await click(buttonNamed(asked, "Restaurer")!);
  expect(restoreVersion).toHaveBeenCalledWith("p", "v1");
  // Fait : la fenêtre se ferme, et le contenu arrive par la synchro.
  expect(onClose).toHaveBeenCalled();
  expect(onRefused).not.toHaveBeenCalled();
});

test("restore_warns_when_others_have_the_page_open", async () => {
  const session = fakeSession([{ id: "alice", name: "Alice", canWrite: true }]);
  await opened({ session });
  await click(items()[1]);
  await click(restoreButton()!);
  expect(confirmation()!.querySelector(".mm-dialog-lead")?.textContent).toBe(REPLACED);
  await click(buttonNamed(confirmation()!, "Annuler")!);
  expect(confirmation()).toBeNull();
  // Bruno ouvre la page : la liste des présents, hors soi, n'est plus vide.
  await act(async () =>
    session.setRoster([
      { id: "alice", name: "Alice", canWrite: true },
      { id: "bruno", name: "Bruno", canWrite: false },
    ]),
  );
  await click(restoreButton()!);
  const paragraphs = [...confirmation()!.querySelectorAll(".mm-dialog-lead p")].map((p) => p.textContent);
  expect(paragraphs).toEqual([REPLACED, OTHERS]);
  // Et elle suit la liste pendant que la confirmation est ouverte.
  await act(async () => session.setRoster([{ id: "alice", name: "Alice", canWrite: true }]));
  expect(confirmation()!.querySelector(".mm-dialog-lead")?.textContent).toBe(REPLACED);
});

test("a_refused_restore_is_shown_as_the_server_says_it", async () => {
  await opened();
  await click(items()[1]);
  await click(restoreButton()!);
  vi.mocked(restoreVersion).mockRejectedValue(
    new ApiError(409, "trashed", "Cette page est à la corbeille : restaure-la d'abord."),
  );
  await click(buttonNamed(confirmation()!, "Restaurer")!);
  expect(confirmation()).toBeNull();
  expect(dialog().querySelector('[role="alert"]')?.textContent).toBe(
    "Cette page est à la corbeille : restaure-la d'abord.",
  );
  // Un droit ou la page ont changé : la page est relue ; la fenêtre reste ouverte.
  expect(onRefused).toHaveBeenCalledTimes(1);
  expect(onClose).not.toHaveBeenCalled();
});

test("a_purged_version_closes_its_preview_with_a_sentence", async () => {
  const { session } = await opened();
  await click(items()[2]);
  expect(preview()?.dataset.version).toBe("v1");
  // Une version de plus : la plus ancienne sort des cent gardées.
  vi.mocked(getVersions).mockResolvedValue([{ ...v2, id: "v3" }, v2]);
  await act(async () => session!.frame("versions"));
  await act(async () => {
    await new Promise((done) => setTimeout(done, 400));
  });
  expect(items()).toHaveLength(3);
  expect(preview()).toBeNull();
  expect(restoreButton()).toBeNull();
  expect(dialog().textContent).toContain("Cette version n'est plus dans l'historique");
  expect(items().some((item) => item.getAttribute("aria-current") === "true")).toBe(false);
});

test("a_confirmation_without_object_closes_and_focus_stays_in_the_window", async () => {
  const { session } = await opened();
  await click(items()[2]);
  await click(restoreButton()!);
  expect(confirmation()).not.toBeNull();
  // La version sort de l'historique pendant qu'on hésite : rien à restaurer.
  vi.mocked(getVersions).mockResolvedValue([{ ...v2, id: "v3" }, v2]);
  await act(async () => session!.frame("versions"));
  await until(() => confirmation() === null, "la confirmation fermée");
  expect(restoreVersion).not.toHaveBeenCalled();
  expect(dialog().contains(document.activeElement)).toBe(true);

  // Le droit de restaurer part (niveau baissé ailleurs) : pareil.
  await click(items()[1]);
  await click(restoreButton()!);
  expect(confirmation()).not.toBeNull();
  await act(async () =>
    root.render(
      <HistoryDialog
        node={page("p", { level: "read" })}
        session={session}
        readOnly={false}
        selfId="alice"
        onRefused={onRefused}
        onClose={onClose}
      />,
    ),
  );
  expect(confirmation()).toBeNull();
  expect(restoreButton()).toBeNull();
  expect(dialog().contains(document.activeElement)).toBe(true);
});

test("focus_enters_the_window_stays_in_it_and_escape_closes_it", async () => {
  await opened();
  // Le focus entre dans la fenêtre, sur la version choisie.
  expect(document.activeElement).toBe(items()[0]);
  // Les flèches parcourent la liste, sans rien choisir.
  await key(items()[0], { key: "ArrowDown" });
  expect(document.activeElement).toBe(items()[1]);
  await key(items()[1], { key: "End" });
  expect(document.activeElement).toBe(items()[2]);
  await key(items()[2], { key: "ArrowDown" });
  expect(document.activeElement).toBe(items()[0]);
  await key(items()[0], { key: "ArrowUp" });
  expect(document.activeElement).toBe(items()[2]);
  await key(items()[2], { key: "Home" });
  expect(document.activeElement).toBe(items()[0]);
  expect(preview()?.dataset.version).toBe("current");

  // Tab ne sort pas de la fenêtre : du dernier élément, il revient au premier, et l'inverse.
  const focusable = [...dialog().querySelectorAll<HTMLElement>("button")];
  focusable[focusable.length - 1].focus();
  await key(focusable[focusable.length - 1], { key: "Tab" });
  expect(document.activeElement).toBe(focusable[0]);
  await key(focusable[0], { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(focusable[focusable.length - 1]);

  // Échap (l'événement `cancel` de la fenêtre) la ferme.
  await act(async () => {
    dialog().dispatchEvent(new Event("cancel", { cancelable: true }));
  });
  expect(onClose).toHaveBeenCalledTimes(1);
  // « Fermer » aussi.
  await click(dialog().querySelector('button[aria-label="Fermer l\'historique"]')!);
  expect(onClose).toHaveBeenCalledTimes(2);
});

test("escape_in_the_confirmation_leaves_the_window_open", async () => {
  await opened();
  await click(items()[1]);
  await click(restoreButton()!);
  await act(async () => {
    confirmation()!.dispatchEvent(new Event("cancel", { cancelable: true }));
  });
  expect(confirmation()).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
});

test("names_from_elsewhere_are_isolated_and_an_absurd_date_breaks_nothing", async () => {
  const strange: Version = {
    id: "v9",
    createdAt: 9e15,
    reason: "closing",
    authors: [author("eve", "‮evE"), author("zoe", "Zoé")],
  };
  await opened({ versions: [strange] });
  const item = items()[1];
  expect(item.textContent).toContain("date inconnue");
  expect(item.querySelector("time")?.getAttribute("dateTime")).toBeNull();
  for (const name of item.querySelectorAll(".mm-history-author")) {
    expect(name.getAttribute("dir")).toBe("auto");
    expect(name.classList.contains("mm-isolate")).toBe(true);
  }
  // Dans un attribut, chaque nom est entouré de U+2068 et U+2069.
  expect(item.querySelector(".mm-history-authors")?.getAttribute("title")).toBe(
    "⁨‮evE⁩, ⁨Zoé⁩",
  );
});
