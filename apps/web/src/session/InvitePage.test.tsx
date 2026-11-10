// La page d'un lien d'invitation : ce qu'elle dit d'une adresse qui ne porte pas de jeton (la
// page rechargée, un lien tronqué), et d'un jeton que le serveur ne reconnaît plus.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ApiError, lookupInvitation } from "../api";
import "../test/hooks";
import { InvitePage } from "./InvitePage";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  lookupInvitation: vi.fn(),
}));

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(lookupInvitation).mockReset();
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const render = (token: string | null) =>
  act(async () =>
    root.render(
      <InvitePage
        token={token}
        session={null}
        onSignup={() => {}}
        onLogin={() => {}}
        onAccepted={() => {}}
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
