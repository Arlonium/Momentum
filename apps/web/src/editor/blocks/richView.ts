// Ce qu'ont en commun les vues des blocs riches dessinées sans React (le callout, le signet, le lien
// de page ; R626).
//
// Pourquoi sans React : la vue React d'un bloc (`createReactBlockSpec`) passe par tiptap, qui monte
// un portail React par bloc (avec ses écouteurs d'événements), recopie à chaque bloc monté la liste
// de tous les portails, et, pour un bloc qui a du texte, lit la sélection du document avant d'y
// mettre ce texte, ce qui fait refaire à Chromium la mise en page de toute la page, une fois par
// callout. À l'ouverture d'une page de 1 000 callouts : 840 ms, contre 130 ms pour 1 000 paragraphes ;
// d'une page de 1 000 blocs dont 100 riches, 60 ms de plus que sans eux, et le budget de 300 ms
// dépassé sur une machine lente (`budgets.spec.ts`, R346, R580). Une vue sans React est faite
// d'éléments créés une fois, comme celles des blocs de BlockNote.
//
// Ce qui ne change pas, et que les tests des blocs riches prouvent toujours : la fenêtre d'un bloc
// est tenue par son id hors de la vue (`blockWindows.ts`), toute prop se lit par `textProp`, rien
// du document ne va dans un attribut `style`, le bouton qui règle un bloc n'est là que pour qui
// modifie (suivi par `watchEditable`, sans que la vue soit recréée), et son `aria-expanded` suit
// la fenêtre (`watchBlockWindows`). Une vue ne s'abonne à rien quand BlockNote la dessine hors de
// l'éditeur (copie, export : `renderType` « dom »), et se désabonne quand ProseMirror la détruit.
// Une prop changée ailleurs met la vue à jour en place (`update`) : l'élément qui a le focus reste.
import { applyDataAttributes, type PropSchema } from "@blocknote/core";

/** Un élément HTML de la classe `className`. */
export function element<K extends keyof HTMLElementTagNameMap>(tag: K, className: string): HTMLElementTagNameMap[K] {
  const created = document.createElement(tag);
  created.className = className;
  return created;
}

/** Un trait d'icône : son élément SVG et ses attributs. */
export type IconShape = readonly [tag: "path" | "circle" | "rect", attributes: Readonly<Record<string, string>>];

const SVG = "http://www.w3.org/2000/svg";

/**
 * Une icône de l'interface, comme celles de `icons.tsx` (même trait, mêmes attributs) ; ses traits
 * sont ceux de l'icône React du même nom (`the_icons_drawn_without_react_are_those_of_the_interface`).
 */
export function iconElement(size: number, shapes: readonly IconShape[]): SVGSVGElement {
  const svg = document.createElementNS(SVG, "svg");
  const attributes: Record<string, string> = {
    "aria-hidden": "true",
    width: String(size),
    height: String(size),
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "1.8",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
  };
  for (const [name, value] of Object.entries(attributes)) svg.setAttribute(name, value);
  for (const [tag, shape] of shapes) {
    const child = document.createElementNS(SVG, tag);
    for (const [name, value] of Object.entries(shape)) child.setAttribute(name, value);
    svg.append(child);
  }
  return svg;
}

/** `InfoIcon` (l'icône par défaut d'un callout). */
export const INFO_ICON: readonly IconShape[] = [
  ["circle", { cx: "12", cy: "12", r: "9" }],
  ["path", { d: "M12 11v5" }],
  ["path", { d: "M12 8h.01" }],
];

/** `BookmarkIcon`. */
export const BOOKMARK_ICON: readonly IconShape[] = [["path", { d: "M6 3h12v18l-6-4-6 4z" }]];

/** `PageIcon`. */
export const PAGE_ICON: readonly IconShape[] = [
  ["path", { d: "M6 3h8l4 4v14H6z" }],
  ["path", { d: "M14 3v4h4" }],
];

/**
 * Les événements qu'une vue garde pour elle (`stopEvent` de ProseMirror), comme tiptap pour la vue
 * React d'un bloc qu'on ne glisse pas (`NodeView.stopEvent` de `@tiptap/core` 3.33) : hors du texte
 * de la vue, tout ce qui vise un bouton ou un champ (sauf le glisser-déposer), et tout le reste sauf
 * ce que ProseMirror doit voir (le bouton de la souris enfoncé, qui sélectionne le bloc, le dépôt, le
 * survol d'un glisser, la copie, le collage, la coupe). Sinon ProseMirror prenait un clic sur le
 * bouton pour un clic dans le document, et relâcher la souris sur un lien le suivait (`handleClick`).
 * `dom` : l'élément de la vue que rend `render` (BlockNote le pose dans l'élément du bloc).
 */
export function stopEventsOutside(dom: HTMLElement, contentDOM: HTMLElement | null = null) {
  return (event: Event): boolean => {
    const root = dom.parentElement ?? dom;
    const target = event.target;
    if (!(target instanceof Element) || !root.contains(target) || contentDOM?.contains(target)) return false;
    const drag = event.type.startsWith("drag");
    const drop = event.type === "drop";
    const input =
      ["INPUT", "BUTTON", "SELECT", "TEXTAREA"].includes(target.tagName) ||
      (target instanceof HTMLElement && target.isContentEditable);
    if (input && !drag && !drop) return true;
    // ProseMirror glisse un bloc qu'on peut sélectionner, même s'il ne se glisse pas.
    if (drag && target === root) event.preventDefault();
    return !(
      event.type === "dragover" ||
      event.type === "dragenter" ||
      drop ||
      event.type === "copy" ||
      event.type === "paste" ||
      event.type === "cut" ||
      event.type === "mousedown"
    );
  };
}

/**
 * Les changements de l'arbre que ProseMirror ignore dans une vue qui a du texte (`ignoreMutation`) :
 * tout ce qui est hors du texte (le bouton remplacé, son `aria-expanded`), comme tiptap ; dans le
 * texte, ProseMirror relit ce qui a changé.
 */
export function outsideContent(contentDOM: HTMLElement) {
  return (mutation: { type: string; target: Node }): boolean => {
    if (mutation.type === "selection") return false;
    if (mutation.target === contentDOM && mutation.type === "attributes") return true;
    return !contentDOM.contains(mutation.target);
  };
}

/**
 * Remet les attributs `data-*` de l'élément du bloc (posés par BlockNote quand il a créé la vue,
 * d'après les props) à jour après une prop changée en place (`update`).
 */
export function followProps(dom: HTMLElement, props: Record<string, unknown>, propSchema: PropSchema): void {
  const wrapper = dom.parentElement;
  if (wrapper?.classList.contains("bn-block-content")) applyDataAttributes(wrapper, props as never, propSchema);
}
