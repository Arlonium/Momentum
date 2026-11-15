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
