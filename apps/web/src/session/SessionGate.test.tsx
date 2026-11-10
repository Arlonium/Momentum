// La porte de l'application, quand le compte de la session n'est plus celui qu'elle a chargé
// (Ruling R115) : rouvert sous un autre compte par la fenêtre « Ta session a pris fin », ou
// changé dans un autre onglet du même navigateur et appris à la relecture du compte. Dans les
// deux cas l'application est démontée, puis l'onglet rechargé ; le même compte, lui, continue.
import type { Me } from "@mementum/protocol";
import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getMe, logIn, onSessionLost } from "../api";
import "../test/hooks";
import { space } from "../test/pages";
import { useSession } from "./session";
import { SessionGate } from "./SessionGate";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getMe: vi.fn(),
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

beforeEach(() => {
  history.replaceState(null, "", "/");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  events = [];
  shownFor = new Set();
  reload.mockClear();
  for (const mock of [getMe, logIn]) vi.mocked(mock).mockReset();
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
});

/** L'application ouverte sous le compte d'Alice. */
async function opened() {
  vi.mocked(getMe).mockResolvedValueOnce(account("alice", "Alice"));
  await act(async () =>
    root.render(
      <SessionGate reload={reload}>
        <App />
      </SessionGate>,
    ),
  );
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
