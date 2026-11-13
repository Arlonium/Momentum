// La page d'un lien d'invitation : ce qu'elle dit d'une adresse qui ne porte pas de jeton (la
// page rechargée, un lien tronqué), d'un jeton que le serveur ne reconnaît plus, et d'une
// invitation qui ne donne rien au compte qui l'accepte (R257, M7).
import type { InvitationInfo, Me } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { acceptInvitation, ApiError, lookupInvitation } from "../api";
import "../test/hooks";
import { space } from "../test/pages";
import { InvitePage } from "./InvitePage";
import type { Session } from "./session";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  acceptInvitation: vi.fn(),
  lookupInvitation: vi.fn(),
}));

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(lookupInvitation).mockReset();
  vi.mocked(acceptInvitation).mockReset();
  history.replaceState(null, "", "/invite");
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const onAccepted = vi.fn();

const render = (token: string | null, session: Session | null = null) =>
  act(async () =>
    root.render(
      <InvitePage
        token={token}
        session={session}
        onSignup={() => {}}
        onLogin={() => {}}
        onAccepted={onAccepted}
      />,
    ),
  );

test("address_without_a_token_asks_to_open_the_link_again", async () => {
  // Le jeton est effacé de l'adresse dès l'ouverture du lien : la page rechargée ne l'a plus.
  await render(null);
  expect(host.querySelector("h1")?.textContent).toBe("Ce lien d'invitation est incomplet");
  expect(host.textContent).toContain("Rouvre le lien que tu as reçu.");
  // Rien n'est demandé au serveur, et l'invitation, qui vaut peut-être encore, n'est pas dite
  // périmée.
  expect(lookupInvitation).not.toHaveBeenCalled();
  expect(host.textContent).not.toContain("n'est plus valable");
  expect(host.textContent).not.toContain("Demande un nouveau lien");
});

test("token_the_server_no_longer_knows_says_the_invitation_is_over", async () => {
  vi.mocked(lookupInvitation).mockRejectedValue(
    new ApiError(404, "invalid_invitation", "invitation non valable"),
  );
  await render("jeton-de-test-manifestement-faux");
  expect(lookupInvitation).toHaveBeenCalledExactlyOnceWith("jeton-de-test-manifestement-faux");
  expect(host.querySelector("h1")?.textContent).toBe("Cette invitation n'est plus valable");
  expect(host.textContent).toContain("Demande un nouveau lien à la personne qui t'a invité.");
});

const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("pro")],
};

for (const [name, info, accepted, link] of [
  [
    "page",
    { spaceName: "Pro", pageTitle: "Plan", invitedBy: "Bruno", email: "alice@exemple.fr" },
    { spaceId: "pro", pageId: "plan", changed: false },
    { text: "Ouvrir la page « Plan »", href: "/p/plan", foreign: "Plan" },
  ],
  [
    "espace",
    { spaceName: "Pro", pageTitle: null, invitedBy: "Bruno", email: "alice@exemple.fr" },
    { spaceId: "pro", pageId: null, changed: false },
    { text: "Ouvrir l'espace « Pro »", href: "/s/pro", foreign: "Pro" },
  ],
] as const) {
  test(`an_invitation_that_gives_nothing_says_so_and_stays_valid (${name})`, async () => {
    vi.mocked(lookupInvitation).mockResolvedValue(info satisfies InvitationInfo);
    vi.mocked(acceptInvitation).mockResolvedValue(accepted);
    onAccepted.mockClear();
    const refresh = vi.fn(async () => {});
    await render("jeton-de-test-manifestement-faux", { me, refresh });
    const accept = [...host.querySelectorAll("button")].find((b) => b.textContent === "Accepter")!;
    await act(async () => accept.click());
    expect(acceptInvitation).toHaveBeenCalledExactlyOnceWith("jeton-de-test-manifestement-faux");
    // Le serveur n'a rien écrit : l'écran le dit, et le lien vaut encore.
    const status = host.querySelector('[role="status"]');
    expect(status?.textContent).toBe(
      "Tu as déjà cet accès. Le lien reste valable pour quelqu'un d'autre.",
    );
    // Aucune navigation d'office, et le jeton n'est pas oublié par la porte de l'application.
    expect(location.pathname).toBe("/invite");
    expect(onAccepted).not.toHaveBeenCalled();
    // « Accepter » est parti ; le focus va au message, pas sur `body`.
    expect([...host.querySelectorAll("button")].map((b) => b.textContent)).toEqual([]);
    expect(document.activeElement).toBe(status);
    // Un lien vers ce que l'invitation donne, son nom isolé.
    const anchor = [...host.querySelectorAll("a")].find((a) => a.textContent === link.text);
    expect(anchor?.getAttribute("href")).toBe(link.href);
    const isolated = anchor?.querySelector('.mm-isolate[dir="auto"]');
    expect(isolated?.textContent).toBe(link.foreign);
    await act(async () => anchor!.click());
    expect(location.pathname).toBe(link.href);
  });
}

test("an_invitation_that_gives_something_leads_where_it_gives", async () => {
  // Témoin : `changed` vrai, l'invitation a servi ; on arrive sur la page, sans message.
  vi.mocked(lookupInvitation).mockResolvedValue({
    spaceName: "Pro",
    pageTitle: "Plan",
    invitedBy: "Bruno",
    email: "alice@exemple.fr",
  });
  vi.mocked(acceptInvitation).mockResolvedValue({ spaceId: "pro", pageId: "plan", changed: true });
  onAccepted.mockClear();
  await render("jeton-de-test-manifestement-faux", { me, refresh: async () => {} });
  const accept = [...host.querySelectorAll("button")].find((b) => b.textContent === "Accepter")!;
  await act(async () => accept.click());
  expect(location.pathname).toBe("/p/plan");
  expect(onAccepted).toHaveBeenCalledTimes(1);
  expect(host.textContent).not.toContain("Tu as déjà cet accès");
});
