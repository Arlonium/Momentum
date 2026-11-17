// La barre de mise en forme de l'éditeur de la page. BlockNote traite tout bloc qui a une prop `url`
// en chaîne comme un bloc fichier : sur un signet, elle offrait « Remplacer le fichier » (son
// panneau écrit l'adresse sans la vérifier), « Supprimer le fichier » et « Télécharger le fichier »
// (vu à l'écran le 2026-11-15). Ces boutons ne restent qu'aux blocs fichier. Et le callout entre
// dans le choix du type de bloc (« Transformer en »).
import { NodeSelection, TextSelection } from "prosemirror-state";
import { expect, test } from "vitest";
import { containerOf } from "../test/editors";
import { flush, mountEditor, type Mounted } from "../test/richEditor";

/** Les noms des boutons de la barre de mise en forme affichée. */
function toolbarButtons(): string[] {
  const bar = document.querySelector(".bn-formatting-toolbar");
  if (bar === null) return [];
  return [...bar.querySelectorAll("button")].map((b) => b.getAttribute("aria-label") ?? b.textContent ?? "");
}

async function selectNode(m: Mounted, id: string) {
  m.editor.focus();
  m.editor.transact((tr) => tr.setSelection(NodeSelection.create(tr.doc, containerOf(tr.doc, id).pos + 1)));
  await flush();
}

async function selectText(m: Mounted, id: string, length: number) {
  m.editor.focus();
  m.editor.transact((tr) => {
    const start = containerOf(tr.doc, id).pos + 2;
    tr.setSelection(TextSelection.create(tr.doc, start, start + length));
  });
  await flush();
}

const PAGE = [
  { id: "x", type: "paragraph" as const, content: "Un paragraphe" },
  { id: "s", type: "bookmark" as const, props: { url: "https://mementum.dev/" } },
  { id: "i", type: "image" as const, props: { url: "https://exemple.fr/a.png", name: "a.png" } },
  { id: "c", type: "callout" as const, content: "Attention" },
];

/** Les boutons que BlockNote ne donne qu'aux blocs fichier. */
const FILE_BUTTON = /fichier|image|légende|Toggle preview/i;

test("bookmark_is_not_a_file_block_for_the_toolbar", async () => {
  const m = await mountEditor({ blocks: PAGE });
  // Témoin : une image sélectionnée a les boutons de fichier.
  await selectNode(m, "i");
  const image = toolbarButtons();
  expect(image).toEqual(expect.arrayContaining(["Remplacer l'image", "Télécharger l'image", "Modifier la légende"]));
  // Un signet sélectionné : aucun.
  await selectNode(m, "s");
  expect(m.editor.getTextCursorPosition().block.id, "le signet est sélectionné").toBe("s");
  expect(toolbarButtons().filter((name) => FILE_BUTTON.test(name))).toEqual([]);
  // Il n'en reste aucun : la barre n'est pas montrée du tout (pas de cadre vide).
  expect(document.querySelector(".bn-formatting-toolbar")).toBeNull();
});

/** Ce que dit le choix du type de bloc de la barre affichée (`null` : il n'y est pas). */
function blockTypeChoice(): string | null {
  return document.querySelector(".bn-formatting-toolbar .mantine-Button-label")?.textContent ?? null;
}

test("callout_is_offered_in_the_block_type_choice", async () => {
  const m = await mountEditor({ blocks: PAGE });
  // Du texte choisi dans un callout : le choix du type de bloc le nomme (BlockNote ne le montre que
  // pour un type qu'il propose).
  await selectText(m, "c", 4);
  expect(blockTypeChoice(), "le choix du type de bloc, dans un callout").toBe("Callout");
  // Témoin : dans un paragraphe, il dit « Paragraphe ».
  await selectText(m, "x", 4);
  expect(blockTypeChoice()).toBe("Paragraphe");
});

// « Télécharger » de la barre (Task C8, tour de correction 1, I2) : celui de BlockNote ouvrait une
// adresse web par `window.open(adresse)`, avec son ouvreur : le site ouvert pouvait emmener l'onglet
// de Mementum ailleurs. Le nôtre fait ce que fait le lien du bloc (`blocks/FileBlock.tsx`).

const FILE_PATH = "/api/v1/pages/0192f0c1-0000-7000-8000-000000000001/files/0192f0c1-0000-7000-8000-0000000000f1";

/** Clique « Télécharger » de la barre sur le bloc `f`, et rend ce que le navigateur aurait fait. */
async function downloadFromToolbar(url: string) {
  const m = await mountEditor({
    blocks: [
      { id: "x", type: "paragraph" as const, content: "Un paragraphe" },
      { id: "f", type: "file" as const, props: { url, name: "rapport final.pdf" } },
    ],
  });
  await selectNode(m, "f");
  const button = [...document.querySelectorAll<HTMLElement>(".bn-formatting-toolbar button")].find((b) =>
    (b.getAttribute("aria-label") ?? "").startsWith("Télécharger"),
  );
  const opened: unknown[][] = [];
  const followed: { href: string | null; download: string | null; target: string | null }[] = [];
  const open = window.open;
  const click = HTMLAnchorElement.prototype.click;
  window.open = ((...args: unknown[]) => {
    opened.push(args);
    return null;
  }) as typeof window.open;
  HTMLAnchorElement.prototype.click = function (this: HTMLAnchorElement) {
    followed.push({
      href: this.getAttribute("href"),
      download: this.getAttribute("download"),
      target: this.getAttribute("target"),
    });
  };
  try {
    button?.click();
    await flush();
  } finally {
    window.open = open;
    HTMLAnchorElement.prototype.click = click;
  }
  return { button, opened, followed, others: toolbarButtons() };
}

test("download_in_the_toolbar_opens_a_web_address_without_its_opener", async () => {
  const { button, opened, followed } = await downloadFromToolbar(" https://AILLEURS.example/rapport.pdf");
  expect(button, "le bouton de la barre").toBeDefined();
  // L'adresse que rend `shownUrl`, dans un autre onglet, sans ouvreur ni référent.
  expect(opened).toEqual([["https://ailleurs.example/rapport.pdf", "_blank", "noopener,noreferrer"]]);
  expect(followed).toEqual([]);
});

test("download_in_the_toolbar_downloads_a_file_of_the_instance_without_opening_anything", async () => {
  const { button, opened, followed } = await downloadFromToolbar(FILE_PATH);
  expect(button, "le bouton de la barre").toBeDefined();
  expect(opened).toEqual([]);
  expect(followed).toEqual([{ href: FILE_PATH, download: "rapport final.pdf", target: null }]);
  // Le lien d'un instant n'est pas resté dans la page.
  expect(document.querySelector(`a[href="${FILE_PATH}"]:not(.mm-file-download)`)).toBeNull();
});

test("download_in_the_toolbar_is_not_offered_for_an_address_that_is_not_permitted", async () => {
  for (const url of ["javascript:alert(1)", "data:text/html,x", "/api/v1/auth/logout", `${FILE_PATH}?x`, ""]) {
    const { button, opened, followed, others } = await downloadFromToolbar(url);
    expect(button, url).toBeUndefined();
    expect([opened, followed], url).toEqual([[], []]);
    // Témoin : la barre est là, avec ses autres boutons de fichier.
    expect(others, url).toEqual(expect.arrayContaining(["Remplacer le fichier"]));
  }
});
