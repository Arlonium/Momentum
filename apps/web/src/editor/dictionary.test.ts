// Tout le dictionnaire que reçoit BlockNote (`EDITOR_OPTIONS.dictionary`), relu (R766) : on tutoie,
// rien n'est resté en anglais, « Télécharger » ne dit jamais un envoi, un seul groupe « Médias » dans
// le menu « / », et « … » plutôt que trois points.
import { en } from "@blocknote/core/locales";
import { locales as multiColumnLocales } from "@blocknote/xl-multi-column";
import { expect, test } from "vitest";
import { EDITOR_OPTIONS } from "./Editor";

/** Chaque chaîne d'un dictionnaire, sous son chemin (`file_panel.upload.title`), listes comprises. */
function leaves(value: unknown, path = ""): [path: string, text: string][] {
  if (typeof value === "string") return [[path, value]];
  if (Array.isArray(value)) return value.flatMap((item, i) => leaves(item, `${path}[${i}]`));
  if (typeof value === "object" && value !== null) {
    return Object.entries(value).flatMap(([key, item]) => leaves(item, path === "" ? key : `${path}.${key}`));
  }
  return [];
}

const given = leaves(EDITOR_OPTIONS.dictionary);
const english = new Map(leaves({ ...en, multi_column: multiColumnLocales.en }));

test("the editor is given a dictionary that says every text BlockNote shows", () => {
  // Toutes les clés du dictionnaire anglais, rien de moins.
  const paths = new Set(given.map(([path]) => path.replace(/\[\d+\]$/, "")));
  const missing = [...english.keys()].filter((path) => !paths.has(path.replace(/\[\d+\]$/, "")));
  expect(missing).toEqual([]);
  expect(given.length).toBeGreaterThan(300);
});

test("the dictionary says tu, never vous", () => {
  // « vous », « votre », « veuillez », et un impératif en « -ez » (« Entrez », « Cliquez »).
  const formal = /\b(vous|votre|vos|veuillez)\b|\b\p{L}+ez\b/iu;
  const allowed = new Set(["chez", "assez"]);
  const said = given.filter(([, text]) => {
    const words = text.match(/\p{L}+/gu) ?? [];
    return formal.test(text) && words.some((word) => !allowed.has(word.toLowerCase()) && formal.test(word));
  });
  expect(said).toEqual([]);
});

test("no text is left in English, but names and shortcuts", () => {
  // Les mots que le français partage avec l'anglais, et les raccourcis (« Mod+B », « Tab »).
  const shared = new Set(["Image", "Audio", "Emoji", "OK", "Auto", "Orange", "Code", "Versions", "Ctrl", ""]);
  const shortcut = /^(Mod\+)?(Maj\+|Shift\+)?(\w|Tab)$/;
  const left = given.filter(
    ([path, text]) =>
      !/\.aliases\[\d+\]$/.test(path) &&
      path !== "locale" &&
      english.get(path) === text &&
      !shared.has(text) &&
      !shortcut.test(text),
  );
  expect(left).toEqual([]);
  // Les touches se disent comme sur un clavier français.
  expect(given.filter(([, text]) => /\bShift\b/.test(text))).toEqual([]);
});

test("télécharger is said only for a download, envoyer for an upload", () => {
  const download = given.filter(([, text]) => /télécharg/i.test(text)).map(([path]) => path);
  expect(download.every((path) => path.startsWith("formatting_toolbar.file_download."))).toBe(true);
  expect(download).toHaveLength(4);
  const upload = given.filter(([path]) => path.startsWith("file_panel.upload."));
  expect(upload.map(([, text]) => text)).toEqual([
    "Envoyer",
    "Envoyer une image",
    "Envoyer une vidéo",
    "Envoyer un fichier audio",
    "Envoyer un fichier",
    "Erreur : l'envoi a échoué",
  ]);
});

test("the slash menu has one group for its media", () => {
  const groups = given.filter(([path]) => /^slash_menu\.(image|video|audio|file)\.group$/.test(path));
  expect(groups.map(([, text]) => text)).toEqual(["Médias", "Médias", "Médias", "Médias"]);
  expect(given.filter(([path, text]) => path.endsWith(".group") && text === "Média")).toEqual([]);
});

test("an ellipsis is one character", () => {
  expect(given.filter(([, text]) => text.includes("..."))).toEqual([]);
});
