// Fixtures de fidélité (Task 8 du lot 1), en développement seulement : `/p/<id>?fixture=<nom>`
// charge `fixtures/<nom>.input.json` dans une page neuve, et l'e2e `fidelity.spec.ts` compare
// ce que BlockNote en fait, et ce que le serveur en relit, aux fixtures de `spec/mmd/fixtures/`.
// Hors développement, ni le paramètre ni les fichiers n'existent (`import.meta.env.DEV` vaut
// `false` à la construction, et la branche disparaît avec ses imports).
import type { MementumEditor } from "./debug";

type Blocks = Parameters<MementumEditor["replaceBlocks"]>[1];

const INPUTS: Record<string, () => Promise<Blocks>> = import.meta.env.DEV
  ? import.meta.glob<Blocks>("./fixtures/*.input.json", { import: "default" })
  : {};

/** Le nom de la fixture demandée par l'adresse, en développement seulement. */
export function requestedFixture(): string | null {
  if (!import.meta.env.DEV) return null;
  return new URLSearchParams(location.search).get("fixture");
}

/** Vrai pour une page neuve : le seul paragraphe vide que le serveur y écrit. */
function isNewPage(editor: MementumEditor): boolean {
  const [only, ...rest] = editor.document;
  return (
    rest.length === 0 &&
    only.type === "paragraph" &&
    Array.isArray(only.content) &&
    only.content.length === 0 &&
    only.children.length === 0
  );
}

/** Les blocs de la fixture `name`, ou `null` si elle n'existe pas. */
export async function fixtureInput(name: string): Promise<Blocks | null> {
  const load = INPUTS[`./fixtures/${name}.input.json`];
  if (!load) {
    console.error(`Fixture inconnue : ${name}`);
    return null;
  }
  return load();
}

/**
 * Remplace le contenu d'une page neuve par `blocks`. Refuse (`false`) une page qui a déjà
 * du contenu, pour ne jamais l'effacer.
 */
export function replaceNewPage(editor: MementumEditor, blocks: Blocks): boolean {
  if (!isNewPage(editor)) {
    console.error("Fixture refusée : la page n'est pas neuve.");
    return false;
  }
  editor.replaceBlocks(editor.document, blocks);
  return true;
}
