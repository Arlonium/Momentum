// Le fenêtrage d'un arbre de la barre latérale (Ruling R43). Un arbre a la hauteur de toutes
// ses lignes (`rows.length × ROW_HEIGHT`), mais seules celles de la partie visible de la zone
// de défilement, plus `OVERSCAN` de chaque côté, sont dans le document, chacune placée à son
// rang. Une navigation ne recalcule donc pas les styles de toutes les pages.
//
// La barre latérale porte plusieurs arbres (un par section) dans la même zone de défilement :
// chacun a sa fenêtre, calculée d'après sa propre place dans la zone. Elle est recalculée au
// défilement, quand la zone change de taille, et quand quelque chose bouge au-dessus de
// l'arbre : une section qui se replie ou qui gagne une page, un message qui apparaît.
import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import { flushSync } from "react-dom";
import { OVERSCAN, ROW_HEIGHT } from "./rows";

/** Les lignes rendues : de `start` (comprise) à `end` (exclue). */
export type Range = { start: number; end: number };

/**
 * Une zone de défilement : l'élément qui défile, et ce qui reste collé à son bord haut et à
 * son bord bas, par-dessus ce qui défile (la tête et le pied de la barre latérale).
 */
export type Viewport = {
  scroller: HTMLElement;
  top: HTMLElement | null;
  bottom: HTMLElement | null;
};

/**
 * Les lignes à rendre d'un arbre de `count` lignes. `top` : le haut de la partie visible,
 * mesuré depuis le haut de l'arbre (négatif : l'arbre commence plus bas) ; `height` : la
 * hauteur de la partie visible.
 */
export function windowRange(top: number, height: number, count: number): Range {
  const clamp = (n: number) => Math.min(Math.max(n, 0), count);
  return {
    start: clamp(Math.floor(top / ROW_HEIGHT) - OVERSCAN),
    end: clamp(Math.ceil((top + height) / ROW_HEIGHT) + OVERSCAN),
  };
}

/**
 * De combien faire défiler pour montrer une ligne. `top` : le haut de la ligne, mesuré depuis
 * le haut de la partie visible, haute de `viewHeight`. Visible en entier, on n'y touche pas ;
 * coupée par un bord, on défile du strict nécessaire ; hors de vue, elle arrive au milieu.
 */
export function revealDelta(top: number, viewHeight: number): number {
  const bottom = top + ROW_HEIGHT;
  if (top >= 0 && bottom <= viewHeight) return 0;
  if (bottom > 0 && top < viewHeight) return top < 0 ? top : bottom - viewHeight;
  return top - (viewHeight - ROW_HEIGHT) / 2;
}

/** Haut de la partie visible de `scroller`, dans la fenêtre du navigateur. */
function viewTop(scroller: HTMLElement): number {
  return scroller.getBoundingClientRect().top + scroller.clientTop;
}

/**
 * Appelle `onChange` quand la place de `tree` dans `scroller` a pu changer sans défilement :
 * la zone change de taille, ou un élément placé avant l'arbre (un voisin qui le précède, ou
 * qui précède un de ses ancêtres) change de taille, apparaît ou disparaît. Rend la fonction
 * qui arrête l'écoute.
 */
function watchLayoutAbove(scroller: HTMLElement, tree: HTMLElement, onChange: () => void): () => void {
  const resized = new ResizeObserver(onChange);
  const ancestors: HTMLElement[] = [];
  for (let el = tree.parentElement; el; el = el === scroller ? null : el.parentElement) {
    ancestors.push(el);
  }
  const observe = () => {
    resized.disconnect();
    resized.observe(scroller);
    for (let el: Element = tree; el !== scroller && el.parentElement; el = el.parentElement) {
      for (let before = el.previousElementSibling; before; before = before.previousElementSibling) {
        resized.observe(before);
      }
    }
  };
  // Un voisin ajouté ou retiré : ceux qui précèdent l'arbre sont repris.
  const mutated = new MutationObserver(() => {
    observe();
    onChange();
  });
  for (const ancestor of ancestors) mutated.observe(ancestor, { childList: true });
  observe();
  return () => {
    resized.disconnect();
    mutated.disconnect();
  };
}

/**
 * La fenêtre de `tree`, un arbre de `count` lignes, dans `viewport`. `range` : les lignes à
 * mettre dans le document. `reveal(index)` fait défiler la zone pour montrer la ligne de ce
 * rang, hors de ce qui est collé à ses bords, et dit si elle a pu le faire (`false` : la zone
 * ou l'arbre ne sont pas encore dans le document).
 */
export function useTreeWindow(
  viewport: Viewport | null,
  tree: HTMLElement | null,
  count: number,
): { range: Range; reveal: (index: number) => boolean } {
  const [range, setRange] = useState<Range>({ start: 0, end: 0 });
  const scroller = viewport?.scroller ?? null;

  const measure = useCallback(() => {
    if (!scroller || !tree) return;
    const top = viewTop(scroller) - tree.getBoundingClientRect().top;
    const next = windowRange(top, scroller.clientHeight, count);
    setRange((current) =>
      current.start === next.start && current.end === next.end ? current : next,
    );
  }, [scroller, tree, count]);

  // Avant la peinture : à l'arrivée de l'arbre, et chaque fois que son nombre de lignes change.
  useLayoutEffect(() => measure(), [measure]);

  useEffect(() => {
    if (!scroller || !tree) return;
    scroller.addEventListener("scroll", measure, { passive: true });
    // Ce qui bouge au-dessus de l'arbre le déplace d'un coup, parfois de toute sa hauteur :
    // ses lignes sont rendues avant la peinture, sinon l'arbre resterait vide une image.
    const unwatch = watchLayoutAbove(scroller, tree, () => flushSync(measure));
    return () => {
      scroller.removeEventListener("scroll", measure);
      unwatch();
    };
  }, [scroller, tree, measure]);

  const reveal = useCallback(
    (index: number) => {
      if (!viewport || !tree) return false;
      const { scroller, top, bottom } = viewport;
      const hiddenTop = top?.offsetHeight ?? 0;
      const hiddenBottom = bottom?.offsetHeight ?? 0;
      // Haut de la ligne, mesuré depuis le haut de la partie visible que rien ne recouvre.
      const rowTop =
        tree.getBoundingClientRect().top + index * ROW_HEIGHT - viewTop(scroller) - hiddenTop;
      const delta = revealDelta(rowTop, scroller.clientHeight - hiddenTop - hiddenBottom);
      if (delta !== 0) {
        scroller.scrollTop += delta;
        measure();
      }
      return true;
    },
    [viewport, tree, measure],
  );

  return { range, reveal };
}
