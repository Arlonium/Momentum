// Les réglages d'un espace, pour sa propriétaire : une invitation de page en attente nomme sa
// page (R257, M11) ; une invitation dans l'espace dit son rôle, comme avant.
import type { Me, Member, PendingInvitation } from "@mementum/protocol";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getMembers, getSpaceInvitations } from "../api";
import { SessionContext } from "../session/session";
import "../test/hooks";
import { space } from "../test/pages";
import { SpaceSettings } from "./SpaceSettings";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  getMembers: vi.fn(),
  getSpaceInvitations: vi.fn(),
}));

const me: Me = {
  user: { id: "alice", email: "alice@exemple.fr", name: "Alice", instanceAdmin: false },
  spaces: [space("pro")],
};
const alice: Member = { userId: "alice", name: "Alice", email: "alice@exemple.fr", role: "owner" };

const invitation = (id: string, overrides: Partial<PendingInvitation>): PendingInvitation => ({
  id,
  email: `${id}@exemple.fr`,
  role: null,
  level: null,
  pageId: null,
  pageTitle: null,
  expiresAt: 1_800_000_000_000,
  ...overrides,
});

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(getMembers).mockReset();
  vi.mocked(getSpaceInvitations).mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

test("a_pending_page_invitation_names_its_page", async () => {
  vi.mocked(getMembers).mockResolvedValue([alice]);
  vi.mocked(getSpaceInvitations).mockResolvedValue([
    invitation("espace", { role: "member" }),
    invitation("plan", { level: "edit", pageId: "p1", pageTitle: "Plan" }),
    invitation("vide", { level: "read", pageId: "p2", pageTitle: "" }),
  ]);
  await act(async () =>
    root.render(
      <SessionContext.Provider value={{ me, refresh: async () => {} }}>
        <SpaceSettings spaceId="pro" />
      </SessionContext.Provider>,
    ),
  );
  const details = [
    ...host.querySelectorAll('[aria-label="Invitations en attente"] .mm-person-detail'),
  ];
  // L'échéance d'abord, sur toutes les lignes : la ligne ne passe pas à la ligne, et ce qu'un
  // titre long fait couper est alors le titre, jamais elle. Puis le rôle d'une invitation dans
  // l'espace ; le niveau et la page, son titre isolé, pour une invitation de page ; « Sans
  // titre » pour une page qui n'en a pas.
  const expires = expect.stringMatching(/^expire le \S/);
  expect(details.map((detail) => detail.textContent!.split(" · "))).toEqual([
    [expires, "Membre"],
    [expires, "Peut modifier sur « Plan »"],
    [expires, "Peut lire sur « Sans titre »"],
  ]);
  expect(details[1].querySelector('.mm-isolate[dir="auto"]')?.textContent).toBe("Plan");
  expect(details[0].querySelector(".mm-isolate")).toBeNull();
});
