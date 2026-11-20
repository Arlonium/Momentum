// Ce que `PageView` branche, vu en le rendant lui-même : la session de synchro (`CollabSession`),
// l'éditeur et la fenêtre « Historique » sont simulés, l'API aussi. Chaque ligne de branchement de
// la relecture finale a son test : le schéma relu pour qui commente (C3), les fils relus au retour
// du compte (C2), l'attente du compte donnée à la fenêtre (C2), et le focus rendu au titre quand
// la fenêtre se ferme sans son bouton (C7).
import type { Me, PageNode } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getComments, getInfo, getPage } from "../api";
import { useServerSchema } from "../editor/useServerSchema";
import { CLIENT_SCHEMA_VERSION } from "../editor/version";
import { filesOf, uploader } from "../editor/files";
import { clearNotices, PAGE_LEFT, postNotice } from "../mentions/notice";
import { SessionContext } from "../session/session";
import { until } from "../test/hooks";
import { notFound, page, space, unauthenticated, unreachable } from "../test/pages";
import { blockPath, navigate, pagePath } from "../router";
import { PageView } from "./PageView";

/** Les sessions de synchro créées par la page, et ce que la fenêtre « Historique » a reçu. */
const probe = vi.hoisted(() => ({
  sessions: [] as {
    readOnly: boolean;
    check: () => Promise<boolean>;
    destroyed: boolean;
  }[],
  history: null as null | { afterLogin?: () => Promise<void>; onClose: () => void },
  /** Ce que l'éditeur a reçu pour mentionner, à chacun de ses rendus. */
  mentions: [] as unknown[],
  /** Ce que l'éditeur a reçu pour envoyer des fichiers, à chacun de ses rendus. */
  files: [] as unknown[],
  /** Le bloc visé que l'éditeur a reçu, à chacun de ses rendus, et de quoi dire qu'il l'a suivi. */
  arrivals: [] as { goTo: unknown; onReached?: (target: never) => void }[],
}));

vi.mock("../editor/collab", async () => {
  const Y = await import("yjs");
  class CollabSession {
    readonly readOnly: boolean;
    readonly check: () => Promise<boolean>;
    readonly doc = new Y.Doc();
    readonly provider = { doc: this.doc };
    synced = true;
    status = "connected";
    roster = [];
    blocked = false;
    destroyed = false;
    readonly listeners = new Set<() => void>();
    constructor(_pageId: string, mode: { readOnly: boolean; check: () => Promise<boolean> }) {
      this.readOnly = mode.readOnly;
      this.check = mode.check;
      probe.sessions.push(this);
    }
    subscribe(listener: () => void) {
      this.listeners.add(listener);
      return () => this.listeners.delete(listener);
    }
    /** Ce que le test change à la session est dit à qui l'écoute. */
    emit() {
      for (const listener of [...this.listeners]) listener();
    }
    onPageEvent() {
      return () => {};
    }
    onServerState() {
      return () => {};
    }
    destroy() {
      this.destroyed = true;
    }
  }
  return { CollabSession };
});

vi.mock("../editor/LazyEditor", () => ({
  LazyEditor: (props: {
    mentions?: unknown;
    files?: unknown;
    goTo?: unknown;
    onReached?: (target: never) => void;
  }) => {
    probe.mentions.push(props.mentions);
    probe.files.push(props.files);
    probe.arrivals.push({ goTo: props.goTo ?? null, onReached: props.onReached });
    return <div className="mm-editor" />;
  },
}));

vi.mock("../history/HistoryDialog", () => ({
  HistoryDialog: (props: { afterLogin?: () => Promise<void>; onClose: () => void }) => {
    probe.history = props;
    return (
      <div role="dialog" aria-label="Historique">
        <button type="button" onClick={props.onClose}>
          Fermer
        </button>
      </div>
    );
  },
}));

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getPage: vi.fn(),
  getComments: vi.fn(),
  getInfo: vi.fn(),
}));

const checkSchema = vi.fn(async () => true);

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  probe.sessions = [];
  probe.history = null;
  probe.mentions = [];
  probe.files = [];
  probe.arrivals = [];
  history.replaceState(null, "", "/");
  checkSchema.mockClear();
  vi.mocked(getPage).mockReset();
  vi.mocked(getComments).mockReset();
  vi.mocked(getComments).mockResolvedValue([]);
  vi.mocked(getInfo).mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  document.body.replaceChildren();
});

const account = (): Me => ({
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("space")],
});

/**
 * Rend la page `node` sous le compte `me` (un autre objet : le compte revenu), avec ses pages
 * parentes `ancestors`.
 */
async function show(
  node: PageNode,
  me: Me = account(),
  ancestors: PageNode[] = [],
  favorite?: { favorite: boolean; onFavorite: (id: string) => void },
  duplicate?: { onDuplicate: (id: string) => void; duplicating?: boolean },
) {
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me, refresh: async () => {} }}>
        <PageView
          node={node}
          ancestors={ancestors}
          favorite={favorite?.favorite}
          onFavorite={favorite?.onFavorite}
          onDuplicate={duplicate?.onDuplicate}
          duplicating={duplicate?.duplicating}
          readOnly={false}
          checkSchema={checkSchema}
          autoFocusTitle={false}
          onTitleFocused={() => {}}
          onSaved={() => {}}
          onRestore={async () => {}}
          onReread={() => {}}
          onLost={() => {}}
          onShared={() => {}}
          onShareRefused={() => {}}
        />
      </SessionContext.Provider>,
    ),
  );
}

const session = () => probe.sessions[probe.sessions.length - 1];
const click = (element: Element) =>
  act(async () => {
    (element as HTMLElement).click();
  });

test("page_view_rereads_the_schema_for_a_commenter", async () => {
  // « Peut commenter » : la session de synchro est en lecture seule, et le schéma est quand même
  // relu avant chaque connexion (R213).
  const node = page("x", { level: "comment" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  expect(session().readOnly).toBe(true);
  let verdict: boolean | undefined;
  await act(async () => {
    verdict = await session().check();
  });
  expect(verdict).toBe(true);
  expect(checkSchema).toHaveBeenCalledTimes(1);
});

test("page_view_reads_no_schema_for_a_reader", async () => {
  const node = page("x", { level: "read" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  await act(async () => {
    await session().check();
  });
  expect(checkSchema).not.toHaveBeenCalled();
});

test("page_view_rereads_the_comments_once_the_account_is_back", async () => {
  // La session est échue, la synchro reste ouverte : les fils répondent `401` ; le compte revient.
  vi.mocked(getComments).mockRejectedValueOnce(unauthenticated()).mockResolvedValue([]);
  const node = page("x", { level: "comment" });
  await show(node);
  await until(
    host,
    () => vi.mocked(getComments).mock.calls.length === 1,
    "la première lecture des fils",
  );
  await act(async () => {
    await new Promise((done) => setTimeout(done, 50));
  });
  expect(getComments).toHaveBeenCalledTimes(1);
  await show(node, account());
  await until(host, () => vi.mocked(getComments).mock.calls.length === 2, "les fils relus");
});

test("page_view_gives_the_account_wait_to_the_history", async () => {
  const node = page("x", { level: "edit" });
  await show(node);
  await click(host.querySelector('button[aria-label="Actions de la page"]')!);
  await click(document.querySelector('[role="menuitem"]')!);
  await until(host, () => probe.history !== null, "la fenêtre");
  // C'est l'attente du compte de la page : tenue quand le compte revient.
  let back = false;
  void probe.history!.afterLogin!().then(() => (back = true));
  await show(node, account());
  await until(host, () => back, "le compte revenu");
});

test("page_view_gives_the_focus_to_the_title_when_the_history_closes_without_its_button", async () => {
  await show(page("x", { level: "edit" }));
  await click(host.querySelector('button[aria-label="Actions de la page"]')!);
  await click(document.querySelector('[role="menuitem"]')!);
  await until(host, () => probe.history !== null, "la fenêtre");
  // Le niveau tombe pendant que la fenêtre est ouverte : le bouton « … » n'a plus d'entrée.
  await show(page("x", { level: "read" }));
  expect(host.querySelector('button[aria-label="Actions de la page"]')).toBeNull();
  await click(host.querySelector('[role="dialog"][aria-label="Historique"] button')!);
  expect(host.querySelector('[role="dialog"][aria-label="Historique"]')).toBeNull();
  expect(document.activeElement).toBe(host.querySelector(".mm-title-input"));
});

test("page_view_gives_the_focus_to_the_title_when_the_page_is_reopened", async () => {
  // Ouverte en lecture, puis le niveau monte : le bandeau propose de rouvrir la page.
  await show(page("x", { level: "read" }));
  await show(page("x", { level: "edit" }));
  const reopen = [...host.querySelectorAll("button")].find((b) => b.textContent === "Rouvrir la page");
  expect(reopen).toBeDefined();
  // Un clic ne donne pas le focus à un bouton dans jsdom : le test le lui donne, comme Chromium.
  reopen!.focus();
  await click(reopen!);
  // Le bouton part avec le bandeau : le focus va au titre, qui se modifie, pas sur `body`.
  expect(reopen!.isConnected).toBe(false);
  const title = host.querySelector<HTMLTextAreaElement>(".mm-title-input")!;
  expect(document.activeElement).toBe(title);
  expect(title.readOnly).toBe(false);
});

test("page_view_shows_the_breadcrumb_of_its_ancestors_and_follows_them", async () => {
  // Le fil d'Ariane de la barre du haut (R256) : les pages parentes que donne `App`, puis la page.
  const [a, b] = [page("a"), page("b", { parentId: "a" })];
  const node = page("c", { parentId: "b" });
  await show(node, account(), [a, b]);
  const trail = `nav[aria-label="Fil d'Ariane"]`;
  const crumbs = () =>
    [...host.querySelectorAll(`${trail} li`)].map(
      (item) => item.querySelector("a, [aria-current]")?.textContent,
    );
  expect(crumbs()).toEqual(["A", "B", "C"]);
  expect(host.querySelector(`${trail} a`)?.getAttribute("href")).toBe("/p/a");
  expect(host.querySelector(`${trail} [aria-current="page"]`)?.textContent).toBe("C");
  // La page déplacée ailleurs : le fil suit, sans recréer la session de synchro.
  const sessions = probe.sessions.length;
  await show({ ...node, parentId: "a" }, account(), [a]);
  expect(crumbs()).toEqual(["A", "C"]);
  expect(probe.sessions.length).toBe(sessions);
});

/** La page sous le vrai schéma du serveur de l'application : `readOnly` et `checkSchema` viennent de lui. */
function WithServerSchema({ node }: { node: PageNode }) {
  const schema = useServerSchema();
  return (
    <PageView
      node={node}
      ancestors={[]}
      readOnly={schema.stale}
      checkSchema={schema.check}
      autoFocusTitle={false}
      onTitleFocused={() => {}}
      onSaved={() => {}}
      onRestore={async () => {}}
      onReread={() => {}}
      onLost={() => {}}
      onShared={() => {}}
      onShareRefused={() => {}}
    />
  );
}

const info = (schemaVersion: number) =>
  ({ schemaVersion }) as unknown as Awaited<ReturnType<typeof getInfo>>;

test("page_view_hides_the_comment_fields_once_a_commenter_learns_a_new_schema", async () => {
  // Un commentateur, panneau ouvert ; le serveur est mis à jour, la synchro revient : la
  // vérification relit le schéma, et le panneau n'a plus de champ (R177). La session, qui ne fait
  // que recevoir, se connecte comme avant.
  const node = page("x", { level: "comment" });
  vi.mocked(getPage).mockResolvedValue(node);
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION));
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me: account(), refresh: async () => {} }}>
        <WithServerSchema node={node} />
      </SessionContext.Provider>,
    ),
  );
  const commentsButton = [...host.querySelectorAll("button")].find((b) =>
    b.textContent?.startsWith("Commentaires"),
  )!;
  await click(commentsButton);
  await until(
    host,
    () => host.querySelector('textarea[aria-label="Commenter la page"]') !== null,
    "le champ",
  );
  vi.mocked(getInfo).mockResolvedValue(info(CLIENT_SCHEMA_VERSION + 1));
  let verdict: boolean | undefined;
  await act(async () => {
    verdict = await session().check();
  });
  expect(verdict).toBe(true);
  expect(host.querySelector('textarea[aria-label="Commenter la page"]')).toBeNull();
  expect(host.querySelectorAll("textarea.mm-composer-field")).toHaveLength(0);
});

test("page_view_offers_the_favorite_of_its_page_in_its_menu", async () => {
  // Le favori de la page, que donne `App` (R254) : en tête du menu « … », à tout niveau, et il
  // nomme la page ouverte.
  const onFavorite = vi.fn();
  const node = page("x", { level: "read" });
  await show(node, account(), [], { favorite: false, onFavorite });
  await click(host.querySelector('button[aria-label="Actions de la page"]')!);
  const items = () => [...document.querySelectorAll('[role="menuitem"]')];
  expect(items().map((item) => item.textContent)).toEqual(["Ajouter aux favoris"]);
  await click(items()[0]);
  expect(onFavorite.mock.calls).toEqual([["x"]]);
  await show(node, account(), [], { favorite: true, onFavorite });
  await click(host.querySelector('button[aria-label="Actions de la page"]')!);
  expect(items().map((item) => item.textContent)).toEqual(["Retirer des favoris"]);
});

test("page_view_offers_the_duplicate_of_its_page_in_its_menu", async () => {
  // « Dupliquer », que donne `App` là où le serveur le permet (R252) : il nomme la page ouverte, et
  // reste grisé pendant une duplication.
  const onDuplicate = vi.fn();
  const node = page("x", { level: "read" });
  const favorite = { favorite: false, onFavorite: () => {} };
  await show(node, account(), [], favorite, { onDuplicate, duplicating: true });
  await click(host.querySelector('button[aria-label="Actions de la page"]')!);
  const item = () =>
    [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(
      (entry) => entry.textContent === "Dupliquer",
    )!;
  expect(item().getAttribute("aria-disabled")).toBe("true");
  await show(node, account(), [], favorite, { onDuplicate });
  await click(item());
  expect(onDuplicate.mock.calls).toEqual([["x"]]);
  // Sans `onDuplicate`, rien.
  await show(node, account(), [], favorite);
  await click(host.querySelector('button[aria-label="Actions de la page"]')!);
  expect([...document.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent)).toEqual([
    "Ajouter aux favoris",
  ]);
});

test("page_view_gives_the_editor_what_mentions_need", async () => {
  // Le rôle du compte dans l'espace de la page, et le droit d'y créer une sous-page (C4, R251).
  const mentioned = () => probe.mentions[probe.mentions.length - 1] as Record<string, unknown>;
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  const member: Me = { ...account(), spaces: [space("autre", { role: "guest" }), space("space", { role: "member" })] };
  await show(node, member);
  expect(mentioned()).toMatchObject({ spaceId: "space", role: "member", canCreate: true });
  // Un refus de création fait relire la page.
  const reads = vi.mocked(getPage).mock.calls.length;
  await act(async () => (mentioned().onRefused as () => void)());
  expect(vi.mocked(getPage).mock.calls.length).toBe(reads + 1);
  // Un rendu qui ne change rien de cela ne rend pas l'éditeur de nouveau.
  const renders = probe.mentions.length;
  await show({ ...node, title: "Autre titre" }, member);
  expect(probe.mentions.length).toBe(renders);
  // Invité de l'espace : c'est dit à l'éditeur, qui ne lui proposera que qui a la page ouverte.
  await show(node, { ...account(), spaces: [space("space", { role: "guest" })] });
  expect(mentioned()).toMatchObject({ role: "guest", canCreate: true });
  // Un espace que le compte n'a pas (ou plus) : comme un invité.
  await show(node, { ...account(), spaces: [] });
  expect(mentioned()).toMatchObject({ role: "guest" });
  // Qui ne peut pas modifier la page ne peut pas y créer de sous-page ; ni personne à la corbeille.
  await show(page("x", { level: "comment" }), member);
  expect(mentioned()).toMatchObject({ role: "member", canCreate: false });
  await show(page("x", { level: "full", trashedAt: 1 }), member);
  expect(mentioned()).toMatchObject({ canCreate: false });
});

test("page_view_shows_the_messages_of_a_page_creation_and_gives_the_focus_back", async () => {
  // Le message d'une création par `[[` est celui de la page, pas de l'éditeur (C4, R631) : il se dit
  // encore quand la session de la page a été recréée, et sa fermeture ne laisse pas le focus sur `body`.
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  expect(document.querySelector('[role="alert"]')).toBeNull();
  // L'éditeur reçoit de quoi attendre le compte après une session perdue.
  const mentioned = probe.mentions[probe.mentions.length - 1] as { afterLogin?: unknown };
  expect(typeof mentioned.afterLogin).toBe("function");
  try {
    await act(async () => void postNotice("La page « Compte rendu » n'a pas pu être créée. Non."));
    const close = document.querySelector<HTMLElement>('[role="alert"] button')!;
    expect(document.querySelector('[role="alert"] p')?.textContent).toContain("Compte rendu");
    close.focus();
    await act(async () => {
      close.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    });
    expect(document.querySelector('[role="alert"]')).toBeNull();
    // Sans éditeur affiché (il est simulé ici), le focus va au titre de la page.
    expect(document.activeElement).toBe(host.querySelector(".mm-title-input"));
  } finally {
    clearNotices();
  }
});

test("page_view_breaks_the_wait_for_the_account_of_a_page_creation_when_the_page_is_left", async () => {
  // Une création par `[[` qui attend le compte ne repart pas une fois la page quittée : son attente
  // est rompue, pour que son message cesse de promettre (C4, R632).
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  const me = account();
  await show(node, me);
  const mentioned = probe.mentions[probe.mentions.length - 1] as { afterLogin: () => Promise<void> };
  const outcomes: unknown[] = [];
  void mentioned.afterLogin().then(
    () => outcomes.push("revenu"),
    (why) => outcomes.push(why),
  );
  // Témoin : le compte revenu (un autre objet), l'attente est tenue.
  const kept: unknown[] = [];
  void mentioned.afterLogin().then(() => kept.push("revenu"));
  await show(node, { ...me });
  expect(kept).toEqual(["revenu"]);
  expect(outcomes).toEqual(["revenu"]);
  // La page quittée pendant une attente : rompue.
  const left: unknown[] = [];
  void mentioned.afterLogin().then(
    () => left.push("revenu"),
    (why) => left.push(why),
  );
  await act(async () => root.render(<div />));
  expect(left).toEqual([PAGE_LEFT]);
});

// Fichiers (Task C8) : ce que la page donne à l'éditeur pour envoyer, et ce qu'elle montre d'un envoi.

type FilesProp = {
  onRefused: (message: string) => void;
  reread: () => Promise<PageNode | null | undefined>;
};
const filesProp = () => probe.files[probe.files.length - 1] as FilesProp;
const alerts = () => [...host.querySelectorAll('.mm-file-refusals[role="alert"] p')].map((p) => p.textContent);

test("page_view_shows_a_refused_upload_until_it_is_closed", async () => {
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  expect(alerts()).toEqual([]);
  await act(async () => filesProp().onRefused("« photo.png » n'a pas été envoyé : la page est à la corbeille."));
  await act(async () => filesProp().onRefused("un second refus"));
  expect(alerts()).toEqual(["« photo.png » n'a pas été envoyé : la page est à la corbeille.", "un second refus"]);
  // Rendue par la page, hors de l'éditeur (elle est posée par la feuille de style au bas de la fenêtre).
  const box = host.querySelector(".mm-file-notices")!;
  expect(box.previousElementSibling?.matches("header.mm-topbar")).toBe(true);
  // Ce qu'on donne à l'éditeur ne change pas d'un rendu à l'autre (il ne serait pas rendu de nouveau).
  expect(probe.files.length).toBeGreaterThan(0);
  expect(probe.files.every((files) => files === probe.files[0])).toBe(true);
  // « Fermer » : le message part, et le focus, qui était sur le bouton, va au titre, pas à `body`.
  const close = box.querySelector<HTMLButtonElement>('button[aria-label="Fermer"]')!;
  close.focus();
  await click(close);
  expect(alerts()).toEqual([]);
  expect(host.querySelector(".mm-file-notices")).toBeNull();
  expect(document.activeElement).toBe(host.querySelector(".mm-title-input"));
});

test("escape_closes_the_refusals_of_uploads_only_from_inside_them", async () => {
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  await act(async () => filesProp().onRefused("un refus"));
  const press = (target: Element) =>
    act(async () => {
      target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
  // Tapé ailleurs (fermer un menu, un panneau) : le message reste.
  await press(host.querySelector(".mm-title-input")!);
  await press(document.body);
  expect(alerts()).toEqual(["un refus"]);
  const close = host.querySelector<HTMLButtonElement>('.mm-file-notices button[aria-label="Fermer"]')!;
  close.focus();
  await press(close);
  expect(alerts()).toEqual([]);
  expect(document.activeElement).toBe(host.querySelector(".mm-title-input"));
});

test("page_view_shows_the_uploads_in_course_and_cancels_them_when_the_page_is_left", async () => {
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  const signals: AbortSignal[] = [];
  const files = filesProp();
  const upload = uploader({
    pageId: "x",
    limits: async () => ({ maxFileBytes: 1024 }),
    blockType: () => "image",
    ...files,
    signal: new AbortController().signal,
    upload: (_page, _file, signal) =>
      new Promise((_done, fail) => {
        signals.push(signal!);
        signal!.addEventListener("abort", () => fail(new DOMException("annulé", "AbortError")));
      }),
  });
  const outcomes: string[] = [];
  const start = (name: string) =>
    void upload(new File(["abc"], name), "b").then(
      () => outcomes.push(`${name} posé`),
      () => outcomes.push(`${name} arrêté`),
    );
  await act(async () => {
    start("un.png");
    start("deux.png");
  });
  await until(host, () => host.querySelectorAll(".mm-file-uploads li").length === 2, "les deux envois");
  const rows = () =>
    [...host.querySelectorAll(".mm-file-uploads li")].map((li) => li.querySelector('[role="status"]')?.textContent);
  // Le plus récent en haut.
  expect(rows()).toEqual(["Envoi de deux.png…", "Envoi de un.png…"]);
  // « Annuler » le premier : lui seul s'arrête, rien n'est dit, et le focus ne tombe pas sur `body`.
  const cancel = [...host.querySelectorAll<HTMLButtonElement>(".mm-file-uploads button")].find(
    (button) => button.getAttribute("aria-label") === "Annuler l'envoi de \u2068un.png\u2069",
  )!;
  cancel.focus();
  await click(cancel);
  await until(host, () => rows().length === 1, "l'envoi annulé parti");
  expect(rows()).toEqual(["Envoi de deux.png…"]);
  expect(signals.map((s) => s.aborted)).toEqual([true, false]);
  expect(alerts()).toEqual([]);
  expect(document.activeElement).toBe(host.querySelector(".mm-title-input"));
  // La page est quittée : l'autre envoi est annulé, et plus rien ne se dit ni ne se garde.
  await act(async () => root.render(<div />));
  expect(signals.map((s) => s.aborted)).toEqual([true, true]);
  await until(host, () => outcomes.length === 2, "les deux envois arrêtés");
  expect(outcomes).toEqual(["un.png arrêté", "deux.png arrêté"]);
  expect(filesOf("x")).toEqual({ uploads: [], refusals: [] });
  files.onRefused("après la page");
  expect(filesOf("x")).toEqual({ uploads: [], refusals: [] });
});

test("the_refusals_of_uploads_of_a_page_are_not_shown_on_the_next", async () => {
  const first = page("x", { level: "edit" });
  const second = page("y", { level: "edit" });
  vi.mocked(getPage).mockImplementation(async (id) => (id === "x" ? first : second));
  await show(first);
  await act(async () => filesProp().onRefused("un refus de la page x"));
  expect(alerts()).toEqual(["un refus de la page x"]);
  await show(second);
  expect(alerts()).toEqual([]);
  // Témoin : la page suivante dit les siens.
  await act(async () => filesProp().onRefused("un refus de la page y"));
  expect(alerts()).toEqual(["un refus de la page y"]);
});

test("page_view_rereads_the_page_for_an_upload_and_says_what_it_has_become", async () => {
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  const lowered = page("x", { level: "read" });
  vi.mocked(getPage).mockResolvedValue(lowered);
  let read: PageNode | null | undefined;
  await act(async () => {
    read = await filesProp().reread();
  });
  expect(read).toEqual(lowered);
  // Le serveur injoignable : rien de su.
  vi.mocked(getPage).mockRejectedValue(unreachable());
  await act(async () => {
    read = await filesProp().reread();
  });
  expect(read).toBeUndefined();
  // La page n'est plus là pour ce compte.
  vi.mocked(getPage).mockRejectedValue(notFound());
  await act(async () => {
    read = await filesProp().reread();
  });
  expect(read).toBeNull();
});

test("nothing_moves_in_the_box_of_uploads_under_the_pointer_or_the_focus", async () => {
  // Tour de correction 1 (m1) : trois envois de front ; le premier finit pendant que le pointeur, puis
  // le focus, est sur « Annuler » du deuxième. Sa ligne reste, sans bouton : aucun autre « Annuler » ne
  // vient à la place de celui qu'on vise.
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  const ends: (() => void)[] = [];
  const upload = uploader({
    pageId: "x",
    limits: async () => ({ maxFileBytes: 1024 }),
    blockType: () => "image",
    ...filesProp(),
    signal: new AbortController().signal,
    upload: (_page, file) =>
      new Promise((done) => {
        ends.push(() => done({ id: "f", url: "/api/v1/pages/x/files/f", name: file.name, size: 3, mime: "image/png" }));
      }),
  });
  const start = (name: string) => void upload(new File(["abc"], name), "b").catch(() => {});
  const lines = () => [...host.querySelectorAll(".mm-file-uploads li")].map((li) => li.textContent);
  const box = () => host.querySelector<HTMLElement>(".mm-file-notices")!;
  const over = (target: Element, type: "mouseover" | "mouseout", related: Element | null) =>
    act(async () => {
      target.dispatchEvent(new MouseEvent(type, { bubbles: true, relatedTarget: related }));
    });
  for (const way of ["pointeur", "focus"] as const) {
    ends.length = 0;
    await act(async () => {
      start("un.png");
      start("deux.png");
      start("trois.png");
    });
    await until(host, () => ends.length === 3, "les trois envois");
    // Le plus récent en haut : la boîte, au bas de la fenêtre, grandit vers le haut.
    expect(lines(), way).toEqual(["Envoi de trois.png…Annuler", "Envoi de deux.png…Annuler", "Envoi de un.png…Annuler"]);
    const second = [...box().querySelectorAll("button")].find((b) => b.getAttribute("aria-label")?.includes("deux.png"))!;
    if (way === "pointeur") await over(second, "mouseover", document.body);
    else await act(async () => second.focus());
    // Le premier envoi finit.
    await act(async () => ends[0]());
    await until(host, () => lines().some((line) => line?.includes("n'est plus en cours")), "la ligne finie");
    expect(lines(), way).toEqual([
      "Envoi de trois.png…Annuler",
      "Envoi de deux.png…Annuler",
      "L'envoi de un.png n'est plus en cours.",
    ]);
    // Le bouton visé est le même élément, à la même place.
    expect(box().contains(second), way).toBe(true);
    // Un envoi de plus pendant ce temps se pose en haut, sans rien déplacer dessous.
    await act(async () => start("quatre.png"));
    await until(host, () => ends.length === 4, "le quatrième envoi");
    expect(lines().slice(1), way).toEqual([
      "Envoi de trois.png…Annuler",
      "Envoi de deux.png…Annuler",
      "L'envoi de un.png n'est plus en cours.",
    ]);
    // Le pointeur, ou le focus, sorti : la ligne finie part.
    if (way === "pointeur") await over(box(), "mouseout", document.body);
    else await act(async () => host.querySelector<HTMLElement>(".mm-title-input")!.focus());
    expect(lines(), way).toEqual(["Envoi de quatre.png…Annuler", "Envoi de trois.png…Annuler", "Envoi de deux.png…Annuler"]);
    // Tous finis, la boîte part ; elle ne se croit plus sous le pointeur à la série suivante.
    await act(async () => ends.slice(1).forEach((end) => end()));
    await until(host, () => lines().length === 0, "plus aucun envoi");
    // (Le faux serveur de ce test ne rend pas un fichier de la page : ses refus se ferment.)
    await click(box().querySelector('button[aria-label="Fermer"]')!);
    await until(host, () => host.querySelector(".mm-file-notices") === null, "la boîte partie");
  }
});

test("page_view_says_a_file_attached_from_another_page_as_a_status_that_leaves_by_itself", async () => {
  // Task C9 (R713) : une recopie réussie est dite, sans alerte et sans prendre le focus ; la boîte est
  // figée sous le pointeur, comme pour les envois.
  const { notesOf, sayNote } = await import("../editor/files");
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  const title = host.querySelector<HTMLElement>(".mm-title-input")!;
  title.focus();
  const files = probe.files[probe.files.length - 1] as { onAdopted: (message: string) => void };
  await act(async () => files.onAdopted("Un fichier d'une autre page a été rattaché à celle-ci : ceux qui lisent cette page le voient."));
  const notes = () => [...host.querySelectorAll('.mm-file-notes [role="status"]')].map((p) => p.textContent);
  expect(notes()).toEqual(["Un fichier d'une autre page a été rattaché à celle-ci : ceux qui lisent cette page le voient."]);
  expect(host.querySelector('.mm-file-notices [role="alert"]')).toBeNull();
  expect(document.activeElement).toBe(title);
  expect(notesOf("x")).toHaveLength(1);
  // Elle part seule ; sous le pointeur, elle reste jusqu'à ce qu'il sorte.
  await act(async () => sayNote("x", "une nouvelle courte", 30));
  const box = host.querySelector<HTMLElement>(".mm-file-notices")!;
  await act(async () => {
    box.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, relatedTarget: document.body }));
  });
  await until(host, () => notesOf("x").length === 1, "la nouvelle courte partie du magasin");
  expect(notes()).toHaveLength(2);
  await act(async () => {
    box.dispatchEvent(new MouseEvent("mouseout", { bubbles: true, relatedTarget: document.body }));
  });
  expect(notes()).toHaveLength(1);
  // Témoin : sans le pointeur, elle part de la page dès qu'elle part du magasin.
  await act(async () => sayNote("x", "une autre", 30));
  expect(notes()).toHaveLength(2);
  await until(host, () => notes().length === 1, "la nouvelle partie de la page");
});

test("a_blocked_session_says_that_nothing_is_saved_any_more", async () => {
  // R788 : le serveur a dit à la session que le compte est freiné sur cette page. Rien de ce
  // qui se tape ici ne s'enregistre plus : la page le dit en alerte, et ne dit pas que les
  // modifications partiront au retour du réseau.
  const node = page("x", { level: "full" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  expect(host.querySelector(".mm-banner-blocked")).toBeNull();
  await act(async () => {
    Object.assign(session(), { blocked: true, status: "disconnected" });
    (session() as unknown as { emit: () => void }).emit();
  });
  const banner = host.querySelector(".mm-banner-blocked");
  expect(banner?.getAttribute("role")).toBe("alert");
  expect(banner?.textContent).toContain("Tes dernières modifications sur cette page ne s'enregistrent plus.");
  expect(banner?.textContent).toContain("Copie ce que tu viens d'écrire, puis recharge la page.");
  expect(banner?.querySelector("button")?.textContent).toBe("Recharger");
  await new Promise((done) => setTimeout(done, 1_700));
  expect(host.querySelector(".mm-topbar-status")?.textContent).toBe("");
});

/** Le bloc visé que l'éditeur a reçu à son dernier rendu, et de quoi dire qu'il y est allé. */
const arrival = () => probe.arrivals[probe.arrivals.length - 1];
const reached = () =>
  act(async () => {
    const { goTo, onReached } = arrival();
    onReached!(goTo as never);
  });

test("the_block_of_the_address_is_given_to_the_editor_and_followed_once", async () => {
  // Un résultat de recherche ou un rétrolien : `/p/<page>#b=<bloc>` (lot 4, Task C2).
  history.replaceState(null, "", blockPath("x", 'un "bloc"'));
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  expect(arrival().goTo).toEqual({ pageId: "x", blockId: 'un "bloc"' });
  // Tant que l'éditeur n'y est pas allé (la page se synchronise), la visée reste, d'un rendu à
  // l'autre et d'une session à l'autre.
  const first = arrival().goTo;
  await show(page("x", { level: "edit", title: "Renommée" }));
  expect(arrival().goTo).toBe(first);
  // L'éditeur y est allé : la visée n'est plus donnée, ni à un rendu de plus…
  await reached();
  expect(arrival().goTo).toBeNull();
  await show(page("x", { level: "edit", title: "Renommée encore" }));
  expect(arrival().goTo).toBeNull();
  // … ni à une session recréée (la page jetée ailleurs : lecture seule, sur un document neuf).
  const sessions = probe.sessions.length;
  await show(page("x", { level: "edit", trashedAt: 1 }));
  expect(probe.sessions.length).toBe(sessions + 1);
  expect(arrival().goTo).toBeNull();
  // L'adresse n'a pas bougé.
  expect(location.pathname + location.hash).toBe(blockPath("x", 'un "bloc"'));
});

test("navigating_to_another_block_of_the_open_page_is_followed_without_remounting_the_page", async () => {
  history.replaceState(null, "", blockPath("x", "un"));
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  await show(node);
  await reached();
  const sessions = probe.sessions.length;
  const live = session();
  const title = host.querySelector(".mm-title-input");
  const frame = host.querySelector(".mm-editor-frame");
  // Un autre résultat de la même page : l'adresse change, la visée est donnée à l'éditeur.
  await act(async () => navigate(blockPath("x", "deux")));
  expect(location.hash).toBe("#b=deux");
  expect(arrival().goTo).toEqual({ pageId: "x", blockId: "deux" });
  // La page n'est pas remontée : la même session, les mêmes éléments (le brouillon d'un
  // commentaire, un titre en cours de frappe en partiraient).
  expect(probe.sessions.length).toBe(sessions);
  expect(live.destroyed).toBe(false);
  expect(host.querySelector(".mm-title-input")).toBe(title);
  expect(host.querySelector(".mm-editor-frame")).toBe(frame);
  await reached();
  expect(arrival().goTo).toBeNull();
  // Le même résultat choisi de nouveau : rien n'entre dans l'historique, le bloc est visé de nouveau.
  const length = history.length;
  await act(async () => navigate(blockPath("x", "deux")));
  expect(history.length).toBe(length);
  expect(arrival().goTo).toEqual({ pageId: "x", blockId: "deux" });
  expect(probe.sessions.length).toBe(sessions);
  // La page sans bloc : plus rien n'est visé.
  await reached();
  await act(async () => navigate(pagePath("x")));
  expect(arrival().goTo).toBeNull();
});

test("an_address_without_a_block_or_of_another_page_gives_the_editor_nothing_to_follow", async () => {
  const node = page("x", { level: "edit" });
  vi.mocked(getPage).mockResolvedValue(node);
  for (const address of ["/p/x", "/p/x#titre", "/p/x#b=", blockPath("y", "bloc")]) {
    history.replaceState(null, "", address);
    probe.arrivals = [];
    await show(node);
    expect(probe.arrivals.length, address).toBeGreaterThan(0);
    expect(probe.arrivals.every((given) => given.goTo === null), address).toBe(true);
    await act(async () => root.render(null));
  }
});
