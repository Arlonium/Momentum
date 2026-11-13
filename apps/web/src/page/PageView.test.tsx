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
import { SessionContext } from "../session/session";
import { until } from "../test/hooks";
import { page, space, unauthenticated } from "../test/pages";
import { PageView } from "./PageView";

/** Les sessions de synchro créées par la page, et ce que la fenêtre « Historique » a reçu. */
const probe = vi.hoisted(() => ({
  sessions: [] as {
    readOnly: boolean;
    check: () => Promise<boolean>;
    destroyed: boolean;
  }[],
  history: null as null | { afterLogin?: () => Promise<void>; onClose: () => void },
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
    destroyed = false;
    constructor(_pageId: string, mode: { readOnly: boolean; check: () => Promise<boolean> }) {
      this.readOnly = mode.readOnly;
      this.check = mode.check;
      probe.sessions.push(this);
    }
    subscribe() {
      return () => {};
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

vi.mock("../editor/LazyEditor", () => ({ LazyEditor: () => <div className="mm-editor" /> }));

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

/** Rend la page `node` sous le compte `me` (un autre objet : le compte revenu). */
async function show(node: PageNode, me: Me = account()) {
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me, refresh: async () => {} }}>
        <PageView
          node={node}
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

/** La page sous le vrai schéma du serveur de l'application : `readOnly` et `checkSchema` viennent de lui. */
function WithServerSchema({ node }: { node: PageNode }) {
  const schema = useServerSchema();
  return (
    <PageView
      node={node}
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
