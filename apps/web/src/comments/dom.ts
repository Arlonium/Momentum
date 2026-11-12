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

/**
 * Le contenu propre d'un bloc : sous son conteneur, ou, pour un bloc repliable, dans le cadre du
 * repli (`.bn-toggle-frame > .bn-toggle-slot`, où sont aussi ses enfants).
 */
const OWN_CONTENT =
  ":scope > .bn-block > .bn-block-content, " +
  ":scope > .bn-block > .bn-toggle-frame > .bn-toggle-slot > .bn-block-content";

/** Le texte propre d'un bloc (sans celui de ses blocs enfants). */
export function blockText(element: Element): string {
  return element.querySelector(OWN_CONTENT)?.textContent?.trim() ?? "";
}

/**
 * Un texte venu d'ailleurs (un nom, un emoji, le texte d'un bloc) posé dans un attribut (une
 * infobulle, un nom accessible) : entouré de U+2068 et U+2069, un caractère de format qu'il porte
 * (U+202E…) ne déborde pas sur ce qui le suit.
 */
export function isolate(text: string): string {
  return `\u2068${text}\u2069`;
}

/**
 * L'élément est-il affiché ? Les enfants d'un bloc repliable fermé sont sous `display: none` :
 * leur boîte ne dit rien de leur place. Sans `checkVisibility` (jsdom), tenu pour affiché.
 */
export function shown(element: Element): boolean {
  return typeof element.checkVisibility !== "function" || element.checkVisibility();
}

/** Les premiers `max` caractères de `text`, suivis de « … » s'il en avait plus. */
export function shorten(text: string, max: number): string {
  const chars = Array.from(text);
  return chars.length > max ? `${chars.slice(0, max).join("")}…` : text;
}
