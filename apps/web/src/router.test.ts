// Les adresses de l'application : un espace, sa corbeille, ses réglages, une page, et le bloc visé
// d'une page (`/p/<id>#b=<bloc>`, lot 4).
import { act } from "react";
import { beforeEach, expect, test, vi } from "vitest";
import { renderHook } from "./test/hooks";
import {
  blockPath,
  navigate,
  pagePath,
  parseBlockTarget,
  parseRoute,
  spacePath,
  spaceSettingsPath,
  trashPath,
  useBlockTarget,
  useLinkInterception,
  usePathname,
} from "./router";

beforeEach(() => {
  history.replaceState(null, "", "/");
});

test("space_routes_are_parsed", () => {
  expect(parseRoute("/")).toEqual({ name: "home" });
  expect(parseRoute("/s/abc")).toEqual({ name: "space", spaceId: "abc" });
  expect(parseRoute("/s/abc/")).toEqual({ name: "space", spaceId: "abc" });
  expect(parseRoute("/s/abc/trash")).toEqual({ name: "trash", spaceId: "abc" });
  expect(parseRoute("/s/abc/settings")).toEqual({ name: "spaceSettings", spaceId: "abc" });
  expect(parseRoute("/p/xyz")).toEqual({ name: "page", id: "xyz" });
  // Rien d'autre sous un espace, et pas d'espace sans id.
  expect(parseRoute("/s/abc/autre")).toEqual({ name: "notFound" });
  expect(parseRoute("/s/")).toEqual({ name: "notFound" });
  expect(parseRoute("/s/%E0%A4%A")).toEqual({ name: "notFound" });
});

test("paths_roundtrip_through_the_parser", () => {
  const id = "espace à/part";
  expect(parseRoute(spacePath(id))).toEqual({ name: "space", spaceId: id });
  expect(parseRoute(trashPath(id))).toEqual({ name: "trash", spaceId: id });
  expect(parseRoute(spaceSettingsPath(id))).toEqual({ name: "spaceSettings", spaceId: id });
  expect(parseRoute(pagePath(id))).toEqual({ name: "page", id });
});

/** Des identifiants de bloc comme un client peut en écrire : un texte quelconque, pas un UUID. */
const BLOCK_IDS = [
  "0193b2c4-5d6e-7f80-9a1b-2c3d4e5f6a7b",
  'a"b',
  "a#b=c",
  "a/b?c&d=e",
  "100 % sûr",
  "é" + String.fromCodePoint(0x1f600),
  "</a><img onerror=x>",
  "a\\b]c[",
  " bords ",
];

test("a_block_fragment_is_parsed_and_kept_out_of_the_page_id", () => {
  for (const blockId of BLOCK_IDS) {
    const url = new URL(blockPath("la page", blockId), location.origin);
    // Le bloc est dans le fragment, échappé : ni dans le chemin, ni dans la requête.
    expect(parseRoute(url.pathname)).toEqual({ name: "page", id: "la page" });
    expect(url.search).toBe("");
    expect(url.hash.startsWith("#b=")).toBe(true);
    expect(url.hash.slice(3)).toMatch(/^[A-Za-z0-9\-_.!~*'()%]+$/);
    expect(parseBlockTarget(url.hash)).toBe(blockId);
  }
  // Sans bloc : l'adresse de la page, telle quelle.
  expect(blockPath("x", null)).toBe(pagePath("x"));
});

test("a_fragment_that_is_not_a_block_is_ignored", async () => {
  for (const hash of ["", "#", "#b", "#b=", "#B=abc", "#x=abc", "#abc", "#b=%E0%A4%A", "#/b=abc"]) {
    expect(parseBlockTarget(hash), hash).toBeNull();
  }
  // Dans l'adresse d'une page, un tel fragment ne vise rien ; hors d'une page, un bloc non plus.
  history.replaceState(null, "", "/p/x#titre");
  const onPage = await renderHook(useBlockTarget, "x");
  expect(onPage.current).toBeNull();
  await act(async () => navigate("/s/x#b=abc"));
  expect(onPage.current).toBeNull();
  // Témoin : la même page, avec un bloc.
  await act(async () => navigate("/p/x#b=abc"));
  expect(onPage.current).toEqual({ pageId: "x", blockId: "abc" });
  // Le bloc d'une page n'est pas celui d'une autre.
  const elsewhere = await renderHook(useBlockTarget, "y");
  expect(elsewhere.current).toBeNull();
});

/** Un clic simple sur un lien posé dans la page ; rend l'événement, que rien ne suit dans jsdom. */
function clickLink(href: string): MouseEvent {
  const anchor = document.createElement("a");
  anchor.href = href;
  document.body.append(anchor);
  const click = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
  let prevented = false;
  // Après l'écouteur du document : ce qu'il a décidé est relevé, puis jsdom ne navigue pas.
  const last = (event: Event) => {
    prevented = event.defaultPrevented;
    event.preventDefault();
  };
  window.addEventListener("click", last, { once: true });
  anchor.dispatchEvent(click);
  anchor.remove();
  Object.defineProperty(click, "intercepted", { value: prevented });
  return click;
}
const intercepted = (click: MouseEvent) => (click as MouseEvent & { intercepted: boolean }).intercepted;

test("an_intercepted_link_keeps_its_fragment", async () => {
  await renderHook(useLinkInterception, undefined);
  const target = await renderHook(useBlockTarget, "x");
  const before = history.length;
  let click!: MouseEvent;
  await act(async () => {
    click = clickLink(blockPath("x", "a b"));
  });
  expect(intercepted(click)).toBe(true);
  expect(location.pathname + location.hash).toBe("/p/x#b=a%20b");
  expect(history.length).toBe(before + 1);
  expect(target.current).toEqual({ pageId: "x", blockId: "a b" });
});

test("an_invitation_link_is_still_left_to_the_browser", async () => {
  await renderHook(useLinkInterception, undefined);
  const click = clickLink("/invite#b=jeton");
  expect(intercepted(click)).toBe(false);
  expect(location.pathname + location.hash).toBe("/");
  // Témoin : un lien de page est pris.
  expect(intercepted(clickLink("/p/x"))).toBe(true);
});

test("navigating_to_another_block_of_the_open_page_is_followed", async () => {
  history.replaceState(null, "", blockPath("x", "un"));
  let pathRenders = 0;
  const path = await renderHook(() => {
    pathRenders += 1;
    return usePathname();
  }, undefined);
  const target = await renderHook(useBlockTarget, "x");
  const first = target.current;
  expect(first).toEqual({ pageId: "x", blockId: "un" });
  const before = history.length;
  const rendersBefore = pathRenders;
  await act(async () => navigate(blockPath("x", "deux")));
  expect(location.pathname + location.hash).toBe("/p/x#b=deux");
  expect(history.length).toBe(before + 1);
  expect(target.current).toEqual({ pageId: "x", blockId: "deux" });
  // Ce qui ne lit que le chemin n'est pas rendu de nouveau : la page ne sera pas remontée.
  expect(path.current).toBe("/p/x");
  expect(pathRenders).toBe(rendersBefore);
  // Aller à la page sans bloc retire le bloc de l'adresse.
  await act(async () => navigate(pagePath("x")));
  expect(location.pathname + location.hash).toBe("/p/x");
  expect(target.current).toBeNull();
});

test("navigating_to_the_same_block_again_adds_nothing_to_the_history", async () => {
  history.replaceState(null, "", blockPath("x", "un"));
  const target = await renderHook(useBlockTarget, "x");
  const first = target.current;
  const before = history.length;
  // Rien n'a bougé : le même objet, d'un rendu à l'autre.
  await target.rerender("x");
  expect(target.current).toBe(first);
  await act(async () => navigate(blockPath("x", "un")));
  expect(history.length).toBe(before);
  expect(location.pathname + location.hash).toBe("/p/x#b=un");
  // Le bloc est visé de nouveau (la personne a pu faire défiler la page) : une visée neuve.
  expect(target.current).toEqual(first);
  expect(target.current).not.toBe(first);
  // Sans bloc dans l'adresse, la même adresse ne fait rien du tout.
  await act(async () => navigate(pagePath("y")));
  let pathRenders = 0;
  await renderHook(() => {
    pathRenders += 1;
    return usePathname();
  }, undefined);
  const rendersBefore = pathRenders;
  const length = history.length;
  await act(async () => navigate(pagePath("y")));
  expect(history.length).toBe(length);
  expect(pathRenders).toBe(rendersBefore);
});

test("back_returns_to_the_block_before", async () => {
  history.replaceState(null, "", blockPath("x", "un"));
  const target = await renderHook(useBlockTarget, "x");
  await act(async () => navigate(blockPath("x", "deux")));
  expect(target.current?.blockId).toBe("deux");
  await act(async () => {
    const back = new Promise((done) => window.addEventListener("popstate", done, { once: true }));
    history.back();
    await back;
  });
  expect(location.hash).toBe("#b=un");
  expect(target.current).toEqual({ pageId: "x", blockId: "un" });
});

test("a_fragment_too_long_is_ignored_and_the_address_is_decoded_once", async () => {
  // Un identifiant de bloc fait cent octets au plus (trois cents caractères échappés) : au-delà
  // d'un Kio, le fragment n'en est pas un.
  expect(parseBlockTarget(`#b=${"a".repeat(1_021)}`)).toBe("a".repeat(1_021));
  expect(parseBlockTarget(`#b=${"a".repeat(1_022)}`)).toBeNull();
  expect(parseBlockTarget(`#b=${"%C3%A9".repeat(400)}`)).toBeNull();
  history.replaceState(null, "", `/p/x#b=${"a".repeat(5_000)}`);
  const long = await renderHook(useBlockTarget, "x");
  expect(long.current).toBeNull();
  // L'adresse n'est décodée qu'une fois, pas à chaque lecture du magasin (chaque rendu le relit).
  await act(async () => navigate(blockPath("la page", "un bloc")));
  const target = await renderHook(useBlockTarget, "la page");
  expect(target.current).toEqual({ pageId: "la page", blockId: "un bloc" });
  const decode = vi.spyOn(globalThis, "decodeURIComponent");
  try {
    for (let i = 0; i < 3; i++) await target.rerender("la page");
    expect(target.current).toEqual({ pageId: "la page", blockId: "un bloc" });
    expect(decode).not.toHaveBeenCalled();
    // Témoin : une autre adresse est décodée.
    await act(async () => navigate(blockPath("la page", "un autre")));
    expect(target.current?.blockId).toBe("un autre");
    expect(decode).toHaveBeenCalled();
  } finally {
    decode.mockRestore();
  }
});

test("a_fragment_changed_by_hand_is_followed", async () => {
  history.replaceState(null, "", blockPath("x", "un"));
  const target = await renderHook(useBlockTarget, "x");
  await act(async () => {
    const changed = new Promise((done) => window.addEventListener("hashchange", done, { once: true }));
    location.hash = "#b=deux";
    await changed;
  });
  expect(target.current).toEqual({ pageId: "x", blockId: "deux" });
});
