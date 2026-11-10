// Barre latérale : l'arbre des pages, « Nouvelle page » (à la racine) et « + » au survol
// d'une page (sous-page). Déplacer les pages à la souris viendra au lot 2.
//
// L'arbre est fenêtré (Ruling R43) : il a la hauteur de toutes ses lignes
// (`rows.length × ROW_HEIGHT`), mais seules celles de la partie visible de la barre
// latérale, plus `OVERSCAN` de chaque côté, sont dans le document, chacune placée à son
// rang. Une navigation ne recalcule donc plus les styles de toutes les pages, et ne rend
// de nouveau que les lignes dont `active` change (`TreeRow` est mémoïsée).
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { PlusIcon } from "../icons";
import { flattenRows, OVERSCAN, ROW_HEIGHT } from "./rows";
import { TreeRow } from "./TreeRow";

type Props = {
  /** `null` : l'arbre n'est pas encore chargé. */
  nodes: PageNode[] | null;
  activeId: string | null;
  /** Schéma du serveur différent (R35) : aucune création de page. */
  readOnly: boolean;
  /** Doit garder son identité d'un rendu à l'autre : les lignes sont mémoïsées. */
  onCreate: (parentId: string | null) => void;
  /** Erreur de la dernière action (création), en français. */
  notice: string | null;
};

/** Les lignes rendues : de `start` (comprise) à `end` (exclue). */
type Range = { start: number; end: number };

/** Haut de la partie visible de `scroller` (zone de défilement), dans la fenêtre. */
function viewTop(scroller: HTMLElement): number {
  return scroller.getBoundingClientRect().top + scroller.clientTop;
}

/**
 * Les lignes de `tree` (qui en compte `count`) à rendre pour la partie visible de
 * `scroller`, plus `OVERSCAN` de chaque côté.
 */
function visibleRange(scroller: HTMLElement, tree: HTMLElement, count: number): Range {
  // Haut de la partie visible, mesuré depuis le haut de l'arbre (négatif : au-dessus).
  const top = viewTop(scroller) - tree.getBoundingClientRect().top;
  const first = Math.floor(top / ROW_HEIGHT) - OVERSCAN;
  const last = Math.ceil((top + scroller.clientHeight) / ROW_HEIGHT) + OVERSCAN;
  const clamp = (n: number) => Math.min(Math.max(n, 0), count);
  return { start: clamp(first), end: clamp(last) };
}

export function PageTree({ nodes, activeId, readOnly, onCreate, notice }: Props) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const rows = useMemo(() => flattenRows(nodes ?? [], collapsed), [nodes, collapsed]);
  const count = rows.length;
  /** La barre latérale, qui défile. */
  const scrollerRef = useRef<HTMLElement>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  const [range, setRange] = useState<Range>({ start: 0, end: 0 });

  const measure = useCallback(() => {
    const scroller = scrollerRef.current;
    const tree = treeRef.current;
    if (!scroller || !tree) return;
    const next = visibleRange(scroller, tree, count);
    setRange((current) =>
      current.start === next.start && current.end === next.end ? current : next,
    );
  }, [count]);

  // Avant la peinture : à l'arrivée de l'arbre, et chaque fois que son nombre de lignes change.
  useLayoutEffect(() => measure(), [measure]);

  // Au défilement de la barre latérale, et quand sa hauteur change.
  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    scroller.addEventListener("scroll", measure, { passive: true });
    const resized = new ResizeObserver(measure);
    resized.observe(scroller);
    return () => {
      scroller.removeEventListener("scroll", measure);
      resized.disconnect();
    };
  }, [measure]);

  /** Page active que l'arbre a déjà montrée : il n'y revient pas tant qu'elle ne change pas. */
  const revealed = useRef<string | null>(null);

  // La page active est hors de la partie visible (lien direct vers une page loin dans
  // l'arbre, page tout juste créée en fin de liste) : la barre latérale défile jusqu'à elle,
  // une fois. Absente des lignes (arbre pas encore chargé, parent replié), elle le sera dès
  // qu'elle y apparaît.
  useLayoutEffect(() => {
    if (activeId === null) {
      revealed.current = null;
      return;
    }
    const scroller = scrollerRef.current;
    const tree = treeRef.current;
    if (revealed.current === activeId || !scroller || !tree) return;
    const index = rows.findIndex((row) => row.node.id === activeId);
    if (index < 0) return;
    revealed.current = activeId;
    // Haut de la ligne, mesuré depuis le haut de la partie visible.
    const top = tree.getBoundingClientRect().top + index * ROW_HEIGHT - viewTop(scroller);
    if (top >= 0 && top + ROW_HEIGHT <= scroller.clientHeight) return;
    scroller.scrollTop += top - (scroller.clientHeight - ROW_HEIGHT) / 2;
    measure();
  }, [activeId, rows, measure]);

  const toggle = useCallback((id: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }, []);

  const create = useCallback(
    (parentId: string | null) => {
      // La sous-page doit se voir : son parent se déplie. Déjà déplié, l'ensemble ne change
      // pas, et les lignes non plus.
      if (parentId !== null) {
        setCollapsed((current) => {
          if (!current.has(parentId)) return current;
          const next = new Set(current);
          next.delete(parentId);
          return next;
        });
      }
      onCreate(parentId);
    },
    [onCreate],
  );

  return (
    <nav ref={scrollerRef} className="mm-sidebar" aria-label="Barre latérale">
      <div className="mm-brand">
        <span className="mm-brand-tile" aria-hidden="true">
          M
        </span>
        <span>Mementum</span>
      </div>
      <button
        type="button"
        className="mm-side-action"
        disabled={readOnly}
        title={readOnly ? "Lecture seule : recharge la page pour écrire" : undefined}
        onClick={() => create(null)}
      >
        <PlusIcon size={16} />
        <span>Nouvelle page</span>
      </button>
      <div className="mm-side-section">Pages</div>
      {nodes === null ? (
        <p className="mm-side-note">Chargement…</p>
      ) : count === 0 ? (
        <p className="mm-side-note">Aucune page pour l'instant.</p>
      ) : (
        <div
          ref={treeRef}
          role="tree"
          aria-label="Pages"
          className="tree"
          style={{ height: count * ROW_HEIGHT }}
        >
          {rows.slice(range.start, range.end).map((row, i) => (
            <TreeRow
              key={row.node.id}
              row={row}
              index={range.start + i}
              active={row.node.id === activeId}
              readOnly={readOnly}
              onToggle={toggle}
              onCreate={create}
            />
          ))}
        </div>
      )}
      {notice && (
        <p className="mm-side-error" role="alert">
          {notice}
        </p>
      )}
    </nav>
  );
}
