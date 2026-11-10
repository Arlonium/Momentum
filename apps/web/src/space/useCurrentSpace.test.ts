// L'espace affiché, d'après l'adresse, la page ouverte et le dernier espace gardé.
import type { SpaceInfo } from "@mementum/protocol";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { parseRoute } from "../router";
import { LAST_SPACE_KEY } from "../spaces";
import { deferred, renderHook, settle } from "../test/hooks";
import { space } from "../test/pages";
import { resolveSpace, useCurrentSpace } from "./useCurrentSpace";

const perso = space("perso");
const pro = space("pro", { role: "member" });

type Props = { path: string; pageSpaceId?: string | null; spaces?: SpaceInfo[] };

/** Le hook, à l'adresse `path` (posée dans le navigateur : il la remplace quand il redirige). */
function at(refresh: () => Promise<void> = async () => {}) {
  return ({ path, pageSpaceId = null, spaces = [perso, pro] }: Props) =>
    useCurrentSpace({ route: parseRoute(path), pageSpaceId, spaces, refresh });
}

async function render(props: Props, refresh?: () => Promise<void>) {
  history.replaceState(null, "", props.path);
  return renderHook(at(refresh), props);
}

beforeEach(() => {
  localStorage.clear();
  history.replaceState(null, "", "/");
});
afterEach(() => {
  vi.restoreAllMocks();
});

test("named_space_wins_then_the_last_shown_then_the_first", () => {
  expect(resolveSpace([perso, pro], "pro", "perso")).toBe(pro);
  // Aucun espace nommé, ou un espace où le compte n'a pas de rôle : le dernier affiché.
  expect(resolveSpace([perso, pro], null, "pro")).toBe(pro);
  expect(resolveSpace([perso, pro], "ailleurs", "pro")).toBe(pro);
  // Sinon le premier du compte ; sans espace, rien.
  expect(resolveSpace([perso, pro], null, null)).toBe(perso);
  expect(resolveSpace([perso, pro], "ailleurs", "quitté")).toBe(perso);
  expect(resolveSpace([], "pro", "pro")).toBeNull();
});

test("home_leads_to_the_last_space_else_the_first", async () => {
  // Jamais venu : le premier espace, à la place de `/`.
  const first = await render({ path: "/" });
  expect(first.current.space).toBe(perso);
  expect(location.pathname).toBe("/s/perso");
  expect(localStorage.getItem(LAST_SPACE_KEY)).toBe("perso");
  await first.unmount();

  // Un espace gardé : c'est lui.
  localStorage.setItem(LAST_SPACE_KEY, "pro");
  const last = await render({ path: "/" });
  expect(last.current.space).toBe(pro);
  expect(location.pathname).toBe("/s/pro");
  await last.unmount();

  // Un espace gardé où le compte n'a plus de rôle : le premier, sans rien dire.
  localStorage.setItem(LAST_SPACE_KEY, "quitté");
  const gone = await render({ path: "/" });
  expect(gone.current.space).toBe(perso);
  expect(location.pathname).toBe("/s/perso");
});

test("account_without_a_space_stays_home", async () => {
  const hook = await render({ path: "/", spaces: [] });
  expect(hook.current.space).toBeNull();
  expect(hook.current.atSpace).toBeNull();
  expect(location.pathname).toBe("/");
});

test("space_address_shows_that_space_and_keeps_it", async () => {
  const hook = await render({ path: "/s/pro" });
  expect(hook.current.space).toBe(pro);
  expect(hook.current.atSpace).toBe(pro);
  expect(localStorage.getItem(LAST_SPACE_KEY)).toBe("pro");
  // Sa corbeille aussi ; ses réglages affichent l'espace, sans être « à » lui (ils ont leur
  // propre écran).
  await hook.rerender({ path: "/s/pro/trash" });
  expect(hook.current.atSpace).toBe(pro);
  await hook.rerender({ path: "/s/perso/settings" });
  expect(hook.current.space).toBe(perso);
  expect(hook.current.atSpace).toBeNull();
});

test("page_address_shows_the_space_of_the_page", async () => {
  localStorage.setItem(LAST_SPACE_KEY, "perso");
  // Le serveur n'a pas encore dit où est la page : le dernier espace affiché reste.
  const hook = await render({ path: "/p/x" });
  expect(hook.current.space).toBe(perso);
  expect(hook.current.atSpace).toBeNull();
  // La page est dans « pro » : c'est lui, et il est gardé.
  await hook.rerender({ path: "/p/x", pageSpaceId: "pro" });
  expect(hook.current.space).toBe(pro);
  expect(localStorage.getItem(LAST_SPACE_KEY)).toBe("pro");
  // Une autre page, pas encore située : « pro » reste.
  await hook.rerender({ path: "/p/y" });
  expect(hook.current.space).toBe(pro);
  expect(location.pathname).toBe("/p/x");
});

test("space_just_joined_elsewhere_is_found_after_one_refresh", async () => {
  const joined = space("asso", { role: "member" });
  // La relecture du compte : elle rend la liste à jour, puis se termine.
  const read = deferred<void>();
  const refresh = vi.fn(() => read.promise);
  const hook = await render({ path: "/s/asso" }, refresh);
  // Pas dans la liste du compte : elle est relue, une fois.
  expect(refresh).toHaveBeenCalledTimes(1);
  expect(hook.current.atSpace).toBeNull();
  await hook.rerender({ path: "/s/asso", spaces: [perso, pro, joined] });
  await settle(() => read.resolve());
  expect(hook.current.space).toBe(joined);
  expect(hook.current.atSpace).toBe(joined);
  expect(location.pathname).toBe("/s/asso");
  expect(refresh).toHaveBeenCalledTimes(1);
});

test("space_without_a_role_is_missing_after_one_refresh", async () => {
  // La relecture du compte est en cours : on ne sait pas encore.
  const read = deferred<void>();
  const refresh = vi.fn(() => read.promise);
  const hook = await render({ path: "/s/ailleurs" }, refresh);
  expect(refresh).toHaveBeenCalledTimes(1);
  expect(hook.current.missing).toBe(false);
  expect(hook.current.atSpace).toBeNull();
  // La liste relue n'a toujours pas cet espace : il est introuvable. L'adresse reste (pas de
  // renvoi silencieux ailleurs), et la barre latérale garde un espace du compte.
  await settle(() => read.resolve());
  expect(hook.current.missing).toBe(true);
  expect(hook.current.atSpace).toBeNull();
  expect(hook.current.space).toBe(perso);
  expect(location.pathname).toBe("/s/ailleurs");
  expect(refresh).toHaveBeenCalledTimes(1);
  // Cet espace n'est pas gardé comme le dernier ouvert.
  expect(localStorage.getItem(LAST_SPACE_KEY)).toBe("perso");

  // Pareil pour sa corbeille et ses réglages ; une relecture qui échoue ne change rien.
  for (const path of ["/s/autre/trash", "/s/autre/settings"]) {
    const failing = vi.fn(async () => {
      throw new Error("réseau");
    });
    const other = await render({ path }, failing);
    await settle();
    expect(failing).toHaveBeenCalledTimes(1);
    expect(other.current.missing).toBe(true);
    expect(location.pathname).toBe(path);
    await other.unmount();
  }
});

test("known_space_is_never_missing", async () => {
  const hook = await render({ path: "/s/pro/settings" });
  expect(hook.current.missing).toBe(false);
  // Ni l'accueil, ni une page (dont l'espace se lit ailleurs).
  await hook.rerender({ path: "/p/x", pageSpaceId: "ailleurs" });
  await settle();
  expect(hook.current.missing).toBe(false);
});
