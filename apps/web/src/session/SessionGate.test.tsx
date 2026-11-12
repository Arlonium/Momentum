// La porte de l'application, quand le compte de la session n'est plus celui qu'elle a chargé
// (Ruling R115) : rouvert sous un autre compte par la fenêtre « Ta session a pris fin », ou
// changé dans un autre onglet du même navigateur et appris à la relecture du compte. Dans les
// deux cas l'application est démontée, puis l'onglet rechargé ; le même compte, lui, continue.
// Et le compte relu au retour sur l'onglet, avec l'arbre (Ruling R126).
import type { Me, Tree } from "@mementum/protocol";
import { act, useCallback, useEffect, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getMe, getTree, logIn, onSessionLost } from "../api";
import { REFRESH_INTERVAL_MS, useSpaceTree, useTreeReader } from "../space/useSpaceTree";
import { deferred } from "../test/hooks";
import { space, unreachable } from "../test/pages";
import { useSession } from "./session";
import { SessionGate } from "./SessionGate";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getMe: vi.fn(),
  getTree: vi.fn(),
  logIn: vi.fn(),
  onSessionLost: vi.fn(),
}));

const account = (id: string, name: string): Me => ({
  user: { id, email: `${id}@exemple.fr`, name, instanceAdmin: false },
  spaces: [space("perso")],
});

let host: HTMLDivElement;
let root: Root;
/** Ce qui arrive à l'application, dans l'ordre : montée, démontée, onglet rechargé. */
let events: string[];
/** Les comptes pour lesquels l'application a été rendue. */
let shownFor: Set<string>;
/** `refresh` de la session, tel que l'application le reçoit. */
let refresh: () => Promise<void>;
/** Ce que `api.ts` appelle quand un appel répond `401 unauthenticated`. */
let sessionLost: () => void;
const reload = vi.fn(() => {
  events.push("rechargé");
});

/** L'application, réduite à ce qui compte ici : le compte qu'elle montre. */
function App() {
  const session = useSession();
  refresh = session.refresh;
  shownFor.add(session.me.user.name);
  useEffect(() => {
    events.push("montée");
    return () => {
      events.push("démontée");
    };
  }, []);
  return <p>{session.me.user.name}</p>;
}

/**
 * L'application sur un espace, câblée comme `App.tsx` : au retour sur l'onglet, le compte est
 * relu avec l'arbre, et un échec de cette relecture ne dit rien.
 */
function AppOnASpace() {
  const { refresh } = useSession();
  const reread = useCallback(() => void refresh().catch(() => {}), [refresh]);
  useTreeReader(useSpaceTree(), "perso", {
    retries: 0,
    onSessionLost: () => {},
    onGone: () => {},
    onReturn: reread,
  });
  return <App />;
}

beforeEach(() => {
  history.replaceState(null, "", "/");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  events = [];
  shownFor = new Set();
  reload.mockClear();
  for (const mock of [getMe, logIn]) vi.mocked(mock).mockReset();
  vi.mocked(getTree).mockReset();
  vi.mocked(getTree).mockImplementation(async (spaceId): Promise<Tree> => ({ spaceId, lsn: 0, nodes: [] }));
  vi.mocked(onSessionLost).mockReset();
  vi.mocked(onSessionLost).mockImplementation((listener) => {
    sessionLost = listener;
    return () => {};
  });
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

/** L'application ouverte sous le compte d'Alice. */
async function opened(app: ReactNode = <App />) {
  vi.mocked(getMe).mockResolvedValueOnce(account("alice", "Alice"));
  await act(async () => root.render(<SessionGate reload={reload}>{app}</SessionGate>));
  expect(host.textContent).toBe("Alice");
  expect(events).toEqual(["montée"]);
}

/** Se connecte par la fenêtre « Ta session a pris fin » ; le serveur répond `me`. */
async function logInOver(me: Me) {
  await act(async () => sessionLost());
  const dialog = host.querySelector("dialog")!;
  expect(dialog.textContent).toContain("Ta session a pris fin");
  vi.mocked(logIn).mockResolvedValueOnce(me);
  await act(async () => {
    dialog.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
}

test("account_read_again_and_still_the_same_goes_on", async () => {
  await opened();
  vi.mocked(getMe).mockResolvedValueOnce(account("alice", "Alice Martin"));
  await act(() => refresh());
  // Le même compte, renommé : l'application continue, avec son nom à jour.
  expect(host.textContent).toBe("Alice Martin");
  expect(events).toEqual(["montée"]);
  expect(reload).not.toHaveBeenCalled();
});

test("account_read_again_and_no_longer_the_same_reloads_the_tab", async () => {
  await opened();
  // La session a été rouverte sous un autre compte dans un autre onglet : le serveur répond
  // désormais pour lui.
  vi.mocked(getMe).mockResolvedValueOnce(account("bruno", "Bruno"));
  await act(() => refresh());
  // L'application est démontée avant le rechargement, et n'a jamais été rendue pour Bruno :
  // rien de ce qu'Alice avait ouvert ne sert sous son compte.
  expect(events).toEqual(["montée", "démontée", "rechargé"]);
  expect([...shownFor]).toEqual(["Alice"]);
  expect(host.textContent).toBe("");
  expect(reload).toHaveBeenCalledTimes(1);
});

test("same_account_logging_in_over_the_application_goes_on", async () => {
  await opened();
  await logInOver(account("alice", "Alice"));
  expect(host.querySelector("dialog")).toBeNull();
  expect(host.textContent).toBe("Alice");
  expect(events).toEqual(["montée"]);
  expect(reload).not.toHaveBeenCalled();
});

test("another_account_logging_in_over_the_application_reloads_the_tab", async () => {
  await opened();
  await logInOver(account("bruno", "Bruno"));
  expect(events).toEqual(["montée", "démontée", "rechargé"]);
  expect([...shownFor]).toEqual(["Alice"]);
  expect(host.textContent).toBe("");
  expect(reload).toHaveBeenCalledTimes(1);
});

/** La fenêtre « Ta session a pris fin », si elle est ouverte. */
const loginWindow = () => host.querySelector("dialog");

test("account_read_again_under_the_login_window_closes_it", async () => {
  await opened();
  // La session est perdue : la connexion est demandée par-dessus l'application.
  await act(async () => sessionLost());
  expect(loginWindow()?.textContent).toContain("Ta session a pris fin");
  // Elle est rouverte ailleurs sous le même compte (un autre onglet), puis le compte est relu
  // (retour sur l'onglet) : le serveur répond, la session est donc là.
  vi.mocked(getMe).mockResolvedValueOnce(account("alice", "Alice"));
  await act(() => refresh());
  // La fenêtre se ferme, sans mot de passe retapé, et l'application continue.
  expect(loginWindow()).toBeNull();
  expect(host.textContent).toBe("Alice");
  expect(events).toEqual(["montée"]);
  expect(logIn).not.toHaveBeenCalled();
  expect(reload).not.toHaveBeenCalled();
});

test("account_that_cannot_be_read_again_leaves_the_login_window", async () => {
  await opened();
  await act(async () => sessionLost());
  // Le serveur ne répond pas : rien ne prouve la session, la fenêtre reste.
  vi.mocked(getMe).mockRejectedValueOnce(unreachable());
  await act(async () => {
    await refresh().catch(() => {});
  });
  expect(loginWindow()?.textContent).toContain("Ta session a pris fin");
  expect(events).toEqual(["montée"]);
});

test("account_read_before_the_session_was_lost_leaves_the_login_window", async () => {
  await opened();
  // Une relecture du compte est en route quand la session se perd : sa réponse, partie avant,
  // ne prouve rien de la session d'après.
  const answer = deferred<Me>();
  vi.mocked(getMe).mockReturnValueOnce(answer.promise);
  let read!: Promise<void>;
  await act(async () => {
    read = refresh();
  });
  await act(async () => sessionLost());
  await act(async () => {
    answer.resolve(account("alice", "Alice Martin"));
    await read;
  });
  expect(loginWindow()?.textContent).toContain("Ta session a pris fin");
});

/** L'application sur un espace, à l'instant `OPENED_AT` ; rend de quoi revenir sur l'onglet. */
const OPENED_AT = 1_000_000;
async function openedOnASpace() {
  // Seule l'horloge est fausse : les promesses suivent leur cours.
  vi.useFakeTimers({ toFake: ["Date"], now: OPENED_AT });
  await opened(<AppOnASpace />);
  expect(getMe).toHaveBeenCalledTimes(1);
  /** L'onglet reprend le focus, `after` millisecondes après l'ouverture. */
  return async (after: number) => {
    vi.setSystemTime(OPENED_AT + after);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
  };
}

test("tab_coming_back_reads_the_account_again_at_most_every_five_seconds", async () => {
  const comeBack = await openedOnASpace();
  // Trop tôt après la dernière lecture de l'arbre : ni l'arbre ni le compte.
  await comeBack(REFRESH_INTERVAL_MS - 1);
  expect(getMe).toHaveBeenCalledTimes(1);
  // Cinq secondes après : le compte est relu avec l'arbre, et ce que d'autres y ont changé
  // (son nom, son rôle dans l'espace) arrive à l'application, qui continue.
  vi.mocked(getMe).mockResolvedValueOnce({
    ...account("alice", "Alice Martin"),
    spaces: [space("perso", { role: "admin" })],
  });
  await comeBack(REFRESH_INTERVAL_MS);
  expect(getMe).toHaveBeenCalledTimes(2);
  expect(getTree).toHaveBeenCalledTimes(2);
  expect(host.textContent).toBe("Alice Martin");
  expect(events).toEqual(["montée"]);
  // Au plus une fois toutes les cinq secondes.
  await comeBack(REFRESH_INTERVAL_MS + 1);
  expect(getMe).toHaveBeenCalledTimes(2);
  expect(reload).not.toHaveBeenCalled();
});

test("tab_coming_back_under_another_account_reloads_the_tab", async () => {
  const comeBack = await openedOnASpace();
  // Pendant que l'onglet était à l'arrière-plan, la session a été rouverte sous un autre
  // compte dans un autre onglet.
  vi.mocked(getMe).mockResolvedValueOnce(account("bruno", "Bruno"));
  await comeBack(REFRESH_INTERVAL_MS);
  expect(events).toEqual(["montée", "démontée", "rechargé"]);
  expect([...shownFor]).toEqual(["Alice"]);
  expect(host.textContent).toBe("");
  expect(reload).toHaveBeenCalledTimes(1);
});

test("tab_coming_back_to_an_unreachable_server_changes_nothing", async () => {
  const comeBack = await openedOnASpace();
  vi.mocked(getMe).mockRejectedValueOnce(unreachable());
  await comeBack(REFRESH_INTERVAL_MS);
  // La relecture du compte a échoué : rien n'est dit, rien n'est rechargé, l'application
  // continue avec le compte qu'elle avait.
  expect(getMe).toHaveBeenCalledTimes(2);
  expect(host.textContent).toBe("Alice");
  expect(events).toEqual(["montée"]);
  expect(reload).not.toHaveBeenCalled();
});
