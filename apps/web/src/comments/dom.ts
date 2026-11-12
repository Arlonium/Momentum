// Ce que les commentaires lisent dans l'éditeur affiché : les éléments des blocs de BlockNote,
// par leur identifiant (`data-id`), et leur texte. Lu à la demande (un clic, une mesure des
// pastilles), jamais écrit : rien n'entre dans le document.

/** L'élément extérieur d'un bloc de BlockNote, qui porte son identifiant (`data-id`). */
export const BLOCK = '[data-node-type="blockOuter"]';

/** Une valeur d'attribut entre guillemets dans un sélecteur CSS, quoi qu'elle contienne. */
export function cssString(value: string): string {
  const escaped = value
    .replace(/["\\]/g, "\\$&")
    .replace(/[\u0000-\u001f\u007f]/g, (c) => `\\${c.charCodeAt(0).toString(16)} `);
  return `"${escaped}"`;
}

/** L'élément du bloc `blockId` sous `root`, s'il y est. */
export function blockElement(root: ParentNode, blockId: string): HTMLElement | null {
  return root.querySelector<HTMLElement>(`${BLOCK}[data-id=${cssString(blockId)}]`);
}

/**
 * Le bloc où est la sélection du document (son point de départ), s'il est sous `root` ; sinon
 * `null`. Pour Ctrl+Alt+M dans un éditeur en lecture seule, qui n'a pas de curseur à lui : une
 * sélection à la souris, ou le curseur de la navigation au curseur du navigateur (F7). Le bloc
 * le plus profond : un bloc imbriqué plutôt que son parent, un tableau pour une de ses cellules.
 */
export function blockOfSelection(root: Element, selection: Selection | null): string | null {
  if (selection === null || selection.rangeCount === 0) return null;
  const node = selection.anchorNode;
  if (node === null) return null;
  const element = node instanceof Element ? node : node.parentElement;
  const block = element?.closest(`${BLOCK}[data-id]`);
  if (!block || !root.contains(block)) return null;
  return block.getAttribute("data-id");
}

/** Le texte propre d'un bloc (sans celui de ses blocs enfants). */
export function blockText(element: Element): string {
  return (
    element.querySelector(":scope > .bn-block > .bn-block-content")?.textContent?.trim() ?? ""
  );
}

/** Les premiers `max` caractères de `text`, suivis de « … » s'il en avait plus. */
export function shorten(text: string, max: number): string {
  const chars = Array.from(text);
  return chars.length > max ? `${chars.slice(0, max).join("")}…` : text;
}
