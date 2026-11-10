// L'arbre des pages d'une section de la barre latérale. Il est fenêtré (Ruling R43,
// `treeWindow.ts`) : il a la hauteur de toutes ses lignes, mais seules celles de la partie
// visible de la barre latérale sont dans le document. Une navigation ne rend de nouveau que
// les lignes dont `active` change (`TreeRow` est mémoïsée).
import { useLayoutEffect, useState, type RefObject } from "react";
import { ROW_HEIGHT, type Row } from "./rows";
import { TreeRow } from "./TreeRow";
import { useTreeWindow, type Viewport } from "./treeWindow";

type Props = {
  /** L'id de l'arbre dans le document : le titre de sa section le désigne (`aria-controls`). */
  id: string;
  /** Le nom de l'arbre : le titre de sa section. */
  label: string;
  /** Les lignes de l'arbre, de haut en bas (`flattenRows`). */
  rows: Row[];
  /** La barre latérale, où l'arbre défile ; `null` : pas encore dans le document. */
  viewport: Viewport | null;
  activeId: string | null;
  /**
   * La page active que la barre latérale a déjà montrée : elle n'y revient pas tant que la
   * page active ne change pas. Partagée par les arbres des sections : la page active est dans
   * l'un d'eux.
   */
  revealed: RefObject<string | null>;
  /** La page dont le menu « … » est ouvert, sinon `null`. */
  menuFor: string | null;
  /** Schéma du serveur différent (R35) : ni action, ni création de sous-page. */
  readOnly: boolean;
  /** Ces rappels gardent leur identité d'un rendu à l'autre : les lignes sont mémoïsées. */
  onToggle: (id: string) => void;
  onCreate: (parentId: string) => void;
  onMenu: (id: string, anchor: HTMLElement) => void;
};

export function PageTree({
  id,
  label,
  rows,
  viewport,
  activeId,
  revealed,
  menuFor,
  readOnly,
  onToggle,
  onCreate,
  onMenu,
}: Props) {
  const [tree, setTree] = useState<HTMLDivElement | null>(null);
  const { range, reveal } = useTreeWindow(viewport, tree, rows.length);

  // La page active est hors de la partie visible (lien direct vers une page loin dans
  // l'arbre, page tout juste créée en fin de liste) : la barre latérale défile jusqu'à elle,
  // une fois. Absente des lignes (une autre section, un parent replié), elle le sera dès
  // qu'elle y apparaît.
  useLayoutEffect(() => {
    if (activeId === null) {
      revealed.current = null;
      return;
    }
    if (revealed.current === activeId) return;
    const index = rows.findIndex((row) => row.node.id === activeId);
    if (index >= 0 && reveal(index)) revealed.current = activeId;
  }, [activeId, rows, reveal, revealed]);

  return (
    <div
      ref={setTree}
      id={id}
      role="tree"
      aria-label={label}
      className="tree"
      style={{ height: rows.length * ROW_HEIGHT }}
    >
      {rows.slice(range.start, range.end).map((row, i) => (
        <TreeRow
          key={row.node.id}
          row={row}
          index={range.start + i}
          active={row.node.id === activeId}
          menuOpen={row.node.id === menuFor}
          readOnly={readOnly}
          onToggle={onToggle}
          onCreate={onCreate}
          onMenu={onMenu}
        />
      ))}
    </div>
  );
}
