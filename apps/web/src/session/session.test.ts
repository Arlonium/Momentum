// Session du client : couleur de présence d'un compte, et ce qui, dans une réponse `401`,
// veut dire « plus de session ».
import { afterEach, expect, test, vi } from "vitest";

/** Des identifiants de comptes, de la forme de ceux du serveur (UUID v7). */
const IDS = Array.from(
  { length: 24 },
  (_, i) => `0193a1b2-c3d4-7e5f-8a9b-${(0x1c2d3e4f5a60 + i * 7919).toString(16).padStart(12, "0")}`,
);

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

test("color_is_stable_per_user", async () => {
  const { colorOf, PRESENCE_COLORS } = await import("./session");
  const first = IDS.map(colorOf);
  // Même compte, même couleur, d'un appel à l'autre…
  expect(IDS.map(colorOf)).toEqual(first);
  for (const color of first) expect(PRESENCE_COLORS).toContain(color);
  // … et d'un chargement de l'application à l'autre : rien n'est tiré au hasard.
  vi.resetModules();
  const reloaded = await import("./session");
  expect(IDS.map(reloaded.colorOf)).toEqual(first);
  // Les comptes ne prennent pas tous la même couleur.
  expect(new Set(first).size).toBeGreaterThan(1);
});

/** `fetch` répond une fois `status` et ce corps JSON. */
function respond(status: number, body: unknown): void {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(JSON.stringify(body), { status })),
  );
}

test("only_unauthenticated_means_session_lost", async () => {
  const api = await import("../api");
  const lost = vi.fn();
  const stop = api.onSessionLost(lost);

  // Un mot de passe faux répond aussi `401`, mais la session, elle, est toujours là.
  respond(401, { error: "invalid_credentials", message: "adresse e-mail ou mot de passe invalide" });
  const refused = await api.logIn({ email: "a@exemple.fr", password: "faux" }).catch((e) => e);
  expect(refused).toBeInstanceOf(api.ApiError);
  expect(refused.message).toBe("adresse e-mail ou mot de passe invalide");
  expect(lost).not.toHaveBeenCalled();

  respond(401, { error: "unauthenticated", message: "session absente ou échue : se connecter" });
  const gone = await api.getMe().catch((e) => e);
  expect(gone).toBeInstanceOf(api.ApiError);
  expect(gone.code).toBe("unauthenticated");
  expect(lost).toHaveBeenCalledTimes(1);

  // Un écouteur retiré n'est plus appelé.
  stop();
  await api.getMe().catch(() => {});
  expect(lost).toHaveBeenCalledTimes(1);
});
