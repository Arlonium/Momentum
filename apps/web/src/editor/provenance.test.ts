// La marque de provenance d'un bloc copié (R746 à R752, R776, R777, R780) : le HMAC, le secret du
// navigateur et sa vie (créé pour le compte entré dans l'onglet, effacé à la déconnexion et à l'entrée
// d'un autre compte, neuf à chaque session ouverte par un formulaire), le jeton de la dernière copie et
// celui de la dernière prise au glisser.
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { forgetOtherRecents, forgetRecents } from "../space/recents";
import {
  clipboardSecret,
  copier,
  forgetClipboardSecret,
  forgetOtherClipboardSecrets,
  hmac,
  markedPage,
  originMark,
  renewClipboardSecret,
  taking,
  vouches,
} from "./provenance";

const PAGE = "0192f0c1-0000-7000-8000-000000000002";
const FILE = `/api/v1/pages/${PAGE}/files/0192f0c1-0000-7000-8000-000000000101`;
const KEY = "mementum.clipboard.";

beforeEach(() => {
  localStorage.clear();
  // Le compte `u1` entre dans l'onglet.
  forgetOtherRecents("u1");
  forgetOtherClipboardSecrets("u1");
});

afterEach(() => {
  vi.restoreAllMocks();
  forgetRecents("u1");
  forgetClipboardSecret("u1");
  forgetClipboardSecret("u10");
  localStorage.clear();
});

const bytes = (hexa: string) => Uint8Array.from(hexa.match(/../g) ?? [], (pair) => parseInt(pair, 16));
const text = (value: string) => new TextEncoder().encode(value);
/** Ce que le navigateur garde pour `userId`, tel quel. */
const stored = (userId: string) =>
  JSON.parse(localStorage.getItem(`${KEY}${userId}`) ?? "null") as Record<string, string | null> | null;

test("hmac_is_hmac_sha256_of_rfc_4231", () => {
  // Cas 1, 2, 3 et 6 (une clé plus longue qu'un bloc) de la RFC 4231.
  expect(hmac(bytes("0b".repeat(20)), text("Hi There"))).toBe(
    "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
  );
  expect(hmac(text("Jefe"), text("what do ya want for nothing?"))).toBe(
    "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
  );
  expect(hmac(bytes("aa".repeat(20)), bytes("dd".repeat(50)))).toBe(
    "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
  );
  expect(hmac(bytes("aa".repeat(131)), text("Test Using Larger Than Block-Size Key - Hash Key First"))).toBe(
    "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
  );
});

test("the_secret_is_random_kept_for_the_account_and_shared_by_the_tabs", () => {
  const secret = clipboardSecret("u1")!;
  expect(secret).toMatch(/^[0-9a-f]{64}$/);
  // Gardé dans le navigateur, sous le compte : un autre onglet lit le même.
  expect(stored("u1")).toEqual({ secret, copy: null, drag: null });
  expect(clipboardSecret("u1")).toBe(secret);
  // Rien n'a encore été copié ni pris : personne ne pose rien.
  expect(copier("u1", "copy")).toBeNull();
  expect(copier("u1", "drag")).toBeNull();
  expect(copier(null, "copy")).toBeNull();
  expect(taking(null, "copy")).toBeNull();
  // Une valeur qui n'en est pas une (abîmée, ou d'une autre forme) est remplacée.
  for (const broken of ["abc", secret, "{}", JSON.stringify({ secret: "court" }), "null"]) {
    localStorage.setItem(`${KEY}u1`, broken);
    const next = clipboardSecret("u1")!;
    expect(next, broken).toMatch(/^[0-9a-f]{64}$/);
    expect(next, broken).not.toBe(secret);
  }
});

test("only_the_last_copy_and_the_last_drag_of_the_browser_are_vouched_for", () => {
  // R776 : le presse-papiers ne tient qu'une copie. Chaque copie de blocs tire un jeton, gardé à la place
  // de celui d'avant ; une marque d'une copie plus ancienne n'est plus admise.
  const first = taking("u1", "copy")!;
  expect(first.token).toMatch(/^[0-9a-f]{64}$/);
  expect(first.token).not.toBe(first.secret);
  const mark = originMark(first, PAGE, [FILE]);
  // Ni le secret ni le jeton ne sont dans la marque.
  expect(mark).not.toContain(first.secret);
  expect(mark).not.toContain(first.token);
  // Collée autant de fois qu'on veut, dans cet onglet ou un autre (le stockage est celui du navigateur).
  expect(copier("u1", "copy")).toEqual(first);
  expect(vouches(mark, copier("u1", "copy")!, [FILE])).toBe(true);
  expect(vouches(mark, copier("u1", "copy")!, [FILE])).toBe(true);
  // Une prise au glisser a son jeton à elle : elle ne périme pas la copie, et la copie ne vaut pas au dépôt.
  expect(copier("u1", "drag")).toBeNull();
  const dragged = taking("u1", "drag")!;
  expect(dragged.token).not.toBe(first.token);
  expect(dragged.secret).toBe(first.secret);
  expect(vouches(mark, copier("u1", "copy")!, [FILE])).toBe(true);
  expect(vouches(mark, copier("u1", "drag")!, [FILE])).toBe(false);
  expect(vouches(originMark(dragged, PAGE, [FILE]), copier("u1", "drag")!, [FILE])).toBe(true);
  // Une autre copie : la première n'est plus reconnue, la prise l'est toujours.
  const second = taking("u1", "copy")!;
  expect(second.token).not.toBe(first.token);
  expect(second.secret).toBe(first.secret);
  expect(vouches(mark, copier("u1", "copy")!, [FILE])).toBe(false);
  expect(vouches(originMark(second, PAGE, [FILE]), copier("u1", "copy")!, [FILE])).toBe(true);
  expect(copier("u1", "drag")).toEqual(dragged);
  expect(stored("u1")).toEqual({ secret: first.secret, copy: second.token, drag: dragged.token });
});

test("a_tab_the_account_has_not_entered_or_has_left_creates_and_writes_nothing", () => {
  // Un autre compte que celui de l'onglet : rien n'est créé, ni gardé.
  expect(clipboardSecret("u2")).toBeNull();
  expect(taking("u2", "copy")).toBeNull();
  expect(copier("u2", "copy")).toBeNull();
  expect(localStorage.getItem(`${KEY}u2`)).toBeNull();
  // Après la déconnexion (ici ou dans un autre onglet), un onglet resté ouvert n'en recrée pas.
  taking("u1", "copy");
  forgetRecents("u1");
  forgetClipboardSecret("u1");
  expect(clipboardSecret("u1")).toBeNull();
  expect(taking("u1", "copy")).toBeNull();
  expect(localStorage.getItem(`${KEY}u1`)).toBeNull();
  // Ni n'écrit dans ce qu'un autre onglet garde encore pour ce compte : il le lit seulement.
  const kept = JSON.stringify({ secret: "ab".repeat(32), copy: "cd".repeat(32), drag: null });
  localStorage.setItem(`${KEY}u1`, kept);
  expect(taking("u1", "copy")).toBeNull();
  expect(localStorage.getItem(`${KEY}u1`)).toBe(kept);
  expect(copier("u1", "copy")).toEqual({ account: "u1", secret: "ab".repeat(32), token: "cd".repeat(32) });
});

test("what_was_copied_before_a_logout_or_a_login_by_a_form_is_no_longer_vouched_for", () => {
  const before = taking("u1", "copy")!;
  const mark = originMark(before, PAGE, [FILE]);
  expect(vouches(mark, copier("u1", "copy")!, [FILE])).toBe(true);
  // Déconnexion, puis le même compte revient : un secret neuf, et plus aucune copie reconnue.
  forgetRecents("u1");
  forgetClipboardSecret("u1");
  expect(localStorage.getItem(`${KEY}u1`)).toBeNull();
  forgetOtherRecents("u1");
  forgetOtherClipboardSecrets("u1");
  expect(copier("u1", "copy")).toBeNull();
  const after = taking("u1", "copy")!;
  expect(after.secret).not.toBe(before.secret);
  expect(vouches(mark, after, [FILE])).toBe(false);
  // R777 : la session rouverte par un formulaire, sans déconnexion entre-temps (une session perdue).
  renewClipboardSecret("u1");
  expect(localStorage.getItem(`${KEY}u1`)).toBeNull();
  const renewed = taking("u1", "copy")!;
  expect(renewed.secret).not.toBe(after.secret);
  expect(vouches(originMark(after, PAGE, [FILE]), renewed, [FILE])).toBe(false);
  // Un autre onglet du compte lit ce que celui-ci vient de garder : ils s'entendent.
  expect(copier("u1", "copy")).toEqual(renewed);
  expect(vouches(originMark(renewed, PAGE, [FILE]), copier("u1", "copy")!, [FILE])).toBe(true);
  // Le simple retour d'un compte déjà entré (un chargement) ne tire rien de neuf.
  forgetOtherRecents("u1");
  forgetOtherClipboardSecrets("u1");
  expect(copier("u1", "copy")).toEqual(renewed);
});

test("the_entry_of_an_account_takes_the_secrets_of_the_others_out_of_the_browser", () => {
  const own = clipboardSecret("u1")!;
  localStorage.setItem(`${KEY}u10`, "cd".repeat(32));
  localStorage.setItem(`${KEY}u2`, "ef".repeat(32));
  localStorage.setItem("mementum.autre", "reste");
  forgetOtherClipboardSecrets("u1");
  // Le compte `u1` garde le sien, pas celui du compte `u10` ; rien d'autre n'est touché.
  expect(Object.keys(localStorage).filter((key) => key.startsWith(KEY))).toEqual([`${KEY}u1`]);
  expect(clipboardSecret("u1")).toBe(own);
  expect(localStorage.getItem("mementum.autre")).toBe("reste");
  // La déconnexion n'efface que le sien.
  localStorage.setItem(`${KEY}u10`, "cd".repeat(32));
  forgetClipboardSecret("u1");
  expect(Object.keys(localStorage).filter((key) => key.startsWith(KEY))).toEqual([`${KEY}u10`]);
});

test("a_refused_storage_keeps_the_secret_and_the_tokens_in_the_tab_only", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("refusé");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("refusé");
  });
  const secret = clipboardSecret("u1")!;
  expect(secret).toMatch(/^[0-9a-f]{64}$/);
  expect(clipboardSecret("u1")).toBe(secret);
  const taken = taking("u1", "copy")!;
  expect(taken.secret).toBe(secret);
  expect(copier("u1", "copy")).toEqual(taken);
  expect(taking("u1", "copy")!.token).not.toBe(taken.token);
  // Pas pour un compte qui n'est pas celui de l'onglet ; et tout part à la déconnexion.
  expect(clipboardSecret("u2")).toBeNull();
  expect(taking("u2", "copy")).toBeNull();
  forgetClipboardSecret("u1");
  expect(copier("u1", "copy")).toBeNull();
  expect(clipboardSecret("u1")).not.toBe(secret);
});

test("a_mark_vouches_for_its_account_its_page_its_gesture_and_exactly_its_files", () => {
  const by = { account: "u1", secret: "ab".repeat(32), token: "ef".repeat(32) };
  const other = FILE.replace(/101$/, "102");
  const mark = originMark(by, PAGE, [FILE, other]);
  expect(JSON.parse(mark)).toEqual({ v: 2, page: PAGE, mac: expect.stringMatching(/^[0-9a-f]{64}$/) });
  expect(markedPage(mark)).toBe(PAGE);
  // L'ordre, les doublons et la casse ne comptent pas.
  expect(vouches(mark, by, [other, FILE, FILE.toUpperCase()])).toBe(true);
  /** La marque, un chiffre de son `mac` changé. */
  const altered = (at: number) => {
    const parsed = JSON.parse(mark) as { v: number; page: string; mac: string };
    const digit = parsed.mac[at] === "0" ? "1" : "0";
    return JSON.stringify({ ...parsed, mac: parsed.mac.slice(0, at) + digit + parsed.mac.slice(at + 1) });
  };
  const refused: [string, boolean][] = [
    ["un fichier de moins", vouches(mark, by, [FILE])],
    ["un fichier de plus", vouches(mark, by, [FILE, other, FILE.replace(/101$/, "103")])],
    ["aucun fichier", vouches(mark, by, [])],
    ["un autre secret", vouches(mark, { ...by, secret: "cd".repeat(32) }, [FILE, other])],
    ["un autre compte", vouches(mark, { ...by, account: "u2" }, [FILE, other])],
    ["un autre geste", vouches(mark, { ...by, token: "aa".repeat(32) }, [FILE, other])],
    ["une autre page", vouches(mark.replace(PAGE, PAGE.replace(/2$/, "3")), by, [FILE, other])],
    // R780 : la comparaison est entière. Un seul chiffre changé : le premier, un du milieu, le dernier.
    ["le premier chiffre changé", vouches(altered(0), by, [FILE, other])],
    ["un chiffre du milieu changé", vouches(altered(31), by, [FILE, other])],
    ["le dernier chiffre changé", vouches(altered(63), by, [FILE, other])],
    ["l'ancienne version", vouches(mark.replace('"v":2', '"v":1'), by, [FILE, other])],
    ["rien", vouches("", by, [FILE, other])],
    ["pas du JSON", vouches("{", by, [FILE, other])],
    ["un tableau", vouches("[]", by, [FILE, other])],
    ["null", vouches("null", by, [FILE, other])],
  ];
  expect(refused.filter(([, vouched]) => vouched)).toEqual([]);
  // Ce qui n'est pas une marque ne nomme pas de page.
  const none = [
    "",
    "{",
    "null",
    "7",
    JSON.stringify({ v: 1, page: PAGE, mac: "0".repeat(64) }),
    JSON.stringify({ v: 3, page: PAGE, mac: "0".repeat(64) }),
    JSON.stringify({ v: 2, page: "x", mac: "0".repeat(64) }),
    JSON.stringify({ v: 2, page: PAGE, mac: "0" }),
  ];
  for (const raw of none) expect(markedPage(raw), raw).toBeNull();
});
