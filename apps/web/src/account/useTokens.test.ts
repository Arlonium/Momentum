// La liste des jetons d'API (lot 5, Task C1) : lue à l'ouverture, relue quand un changement de mot de
// passe les a révoqués, et, quand une lecture a trouvé la session perdue, au retour du compte (R212).
import type { TokenInfo } from "@mementum/protocol";
import { expect, test, vi } from "vitest";
import { listTokens } from "../api";
import { deferred, renderHook, settle } from "../test/hooks";
import { unauthenticated } from "../test/pages";
import { announceTokensChanged } from "./tokensChanged";
import { useTokens } from "./useTokens";

vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  listTokens: vi.fn(),
}));

const token = { id: "t1", name: "Agent" } as TokenInfo;

/** Laisse partir les lectures demandées (elles partent au tour suivant). */
const tick = () => settle(() => new Promise((done) => setTimeout(done, 5)));

test("a_list_that_finds_the_session_lost_is_read_again_when_the_account_comes_back", async () => {
  const login = deferred<void>();
  vi.mocked(listTokens).mockReset().mockRejectedValueOnce(unauthenticated()).mockResolvedValue([token]);
  const hook = await renderHook(() => useTokens(() => login.promise), undefined);
  await tick();
  expect(listTokens).toHaveBeenCalledTimes(1);
  // Rien ne part pendant l'attente du compte, même redemandé : tout répondrait `401`.
  await settle(() => announceTokensChanged());
  await tick();
  expect(listTokens).toHaveBeenCalledTimes(1);
  expect(hook.current.tokens).toBeNull();
  expect(hook.current.error).toBeNull();
  await settle(() => login.resolve());
  await tick();
  expect(listTokens).toHaveBeenCalledTimes(2);
  expect(hook.current.tokens).toEqual([token]);
});

test("a_password_change_makes_the_list_read_again", async () => {
  vi.mocked(listTokens).mockReset().mockResolvedValue([]);
  const hook = await renderHook(() => useTokens(), undefined);
  await tick();
  expect(hook.current.tokens).toEqual([]);
  vi.mocked(listTokens).mockResolvedValue([token]);
  await settle(() => announceTokensChanged());
  await tick();
  expect(hook.current.tokens).toEqual([token]);
  // Une relecture attendue rend la main une fois la liste lue.
  vi.mocked(listTokens).mockResolvedValue([]);
  await settle(() => hook.current.reread());
  expect(hook.current.tokens).toEqual([]);
});
