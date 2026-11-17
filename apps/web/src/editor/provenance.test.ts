// La marque de provenance d'un bloc copié (R746 à R752) : le HMAC, le secret du navigateur et sa vie
// (créé pour le compte entré dans l'onglet, effacé à la déconnexion et à l'entrée d'un autre compte).
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
  expect(localStorage.getItem(`${KEY}u1`)).toBe(secret);
  expect(clipboardSecret("u1")).toBe(secret);
  expect(copier("u1")).toEqual({ account: "u1", secret });
  expect(copier(null)).toBeNull();
  // Une valeur qui n'en est pas un est remplacée.
  localStorage.setItem(`${KEY}u1`, "abc");
  const next = clipboardSecret("u1")!;
  expect(next).toMatch(/^[0-9a-f]{64}$/);
  expect(next).not.toBe(secret);
});

test("a_tab_the_account_has_not_entered_or_has_left_creates_no_secret", () => {
  // Un autre compte que celui de l'onglet : rien n'est créé, ni gardé.
  expect(clipboardSecret("u2")).toBeNull();
  expect(copier("u2")).toBeNull();
  expect(localStorage.getItem(`${KEY}u2`)).toBeNull();
  // Après la déconnexion (ici ou dans un autre onglet), un onglet resté ouvert n'en recrée pas.
  clipboardSecret("u1");
  forgetRecents("u1");
  forgetClipboardSecret("u1");
  expect(clipboardSecret("u1")).toBeNull();
  expect(localStorage.getItem(`${KEY}u1`)).toBeNull();
});

test("what_was_copied_before_a_logout_is_no_longer_vouched_for", () => {
  const before = copier("u1")!;
  const mark = originMark(before, PAGE, [FILE]);
  expect(vouches(mark, copier("u1")!, [FILE])).toBe(true);
  // Déconnexion, puis le même compte revient : un secret neuf.
  forgetRecents("u1");
  forgetClipboardSecret("u1");
  expect(localStorage.getItem(`${KEY}u1`)).toBeNull();
  forgetOtherRecents("u1");
  forgetOtherClipboardSecrets("u1");
  const after = copier("u1")!;
  expect(after.secret).not.toBe(before.secret);
  expect(vouches(mark, after, [FILE])).toBe(false);
});

test("the_entry_of_an_account_takes_the_secrets_of_the_others_out_of_the_browser", () => {
  const own = clipboardSecret("u1")!;
  localStorage.setItem(`${KEY}u10`, "cd".repeat(32));
  localStorage.setItem(`${KEY}u2`, "ef".repeat(32));
  localStorage.setItem("mementum.autre", "reste");
  forgetOtherClipboardSecrets("u1");
  // Le compte `u1` garde le sien, pas celui du compte `u10` ; rien d'autre n'est touché.
  expect(Object.keys(localStorage).filter((key) => key.startsWith(KEY))).toEqual([`${KEY}u1`]);
  expect(localStorage.getItem(`${KEY}u1`)).toBe(own);
  expect(localStorage.getItem("mementum.autre")).toBe("reste");
  // La déconnexion n'efface que le sien.
  localStorage.setItem(`${KEY}u10`, "cd".repeat(32));
  forgetClipboardSecret("u1");
  expect(Object.keys(localStorage).filter((key) => key.startsWith(KEY))).toEqual([`${KEY}u10`]);
});

test("a_refused_storage_keeps_the_secret_in_the_tab_only", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("refusé");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("refusé");
  });
  const secret = clipboardSecret("u1")!;
  expect(secret).toMatch(/^[0-9a-f]{64}$/);
  expect(clipboardSecret("u1")).toBe(secret);
  // Pas pour un compte qui n'est pas celui de l'onglet ; et il part à la déconnexion.
  expect(clipboardSecret("u2")).toBeNull();
  forgetClipboardSecret("u1");
  expect(clipboardSecret("u1")).not.toBe(secret);
});

test("a_mark_vouches_for_its_account_its_page_and_exactly_its_files", () => {
  const by = { account: "u1", secret: "ab".repeat(32) };
  const other = FILE.replace(/101$/, "102");
  const mark = originMark(by, PAGE, [FILE, other]);
  expect(markedPage(mark)).toBe(PAGE);
  expect(mark).not.toContain(by.secret);
  // L'ordre, les doublons et la casse ne comptent pas.
  expect(vouches(mark, by, [other, FILE, FILE.toUpperCase()])).toBe(true);
  const refused: [string, boolean][] = [
    ["un fichier de moins", vouches(mark, by, [FILE])],
    ["un fichier de plus", vouches(mark, by, [FILE, other, FILE.replace(/101$/, "103")])],
    ["aucun fichier", vouches(mark, by, [])],
    ["un autre secret", vouches(mark, { ...by, secret: "cd".repeat(32) }, [FILE, other])],
    ["un autre compte", vouches(mark, { ...by, account: "u2" }, [FILE, other])],
    ["une autre page", vouches(mark.replace(PAGE, PAGE.replace(/2$/, "3")), by, [FILE, other])],
    ["rien", vouches("", by, [FILE, other])],
    ["pas du JSON", vouches("{", by, [FILE, other])],
    ["un tableau", vouches("[]", by, [FILE, other])],
    ["null", vouches("null", by, [FILE, other])],
  ];
  expect(refused.filter(([, vouched]) => vouched)).toEqual([]);
  // Ce qui n'est pas une marque ne nomme pas de page.
  for (const raw of ["", "{", "null", "7", JSON.stringify({ v: 2, page: PAGE, mac: "0".repeat(64) }), JSON.stringify({ v: 1, page: "x", mac: "0".repeat(64) }), JSON.stringify({ v: 1, page: PAGE, mac: "0" })]) {
    expect(markedPage(raw), raw).toBeNull();
  }
});
