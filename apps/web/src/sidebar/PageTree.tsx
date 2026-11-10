// L'arbre des pages d'une section de la barre latérale. Il est fenêtré (Ruling R43,
// `treeWindow.ts`) : il a la hauteur de toutes ses lignes, mais seules celles de la partie
// visible de la barre latérale sont dans le document. Une navigation ne rend de nouveau que
// les lignes dont `active` change (`TreeRow` est mémoïsée).
//
// Une ligne se saisit et se déplace à la souris, à l'intérieur de cet arbre (`drag.ts`) :
// lâchée au milieu d'une ligne, la page devient sa sous-page (la ligne s'éclaire) ; sur son
// bord haut ou bas, elle se range avant ou après elle (un trait le montre, là où elle ira :
// après une page dépliée, c'est sous sa dernière sous-page). Un dépôt refusé ne montre rien,
// et le pointeur le dit. Les autres arbres n'acceptent pas ce dépôt : une
// page ne change pas de section à la souris.
import type { PageNode } from "@mementum/protocol";
import { useLayoutEffect, useMemo, useRef, useState, type DragEvent, type RefObject } from "react";
import { canEdit } from "../share/levels";
import { dropTarget, zonesAt, type Drop, type Zone } from "./drag";
import { flattenRows, NONE, ROW_HEIGHT, subtreeEnd, type Row } from "./rows";
import { TreeRow } from "./TreeRow";
import { useTreeWindow, type Viewport } from "./treeWindow";

/** Où irait la page déplacée si on la lâchait : la ligne visée, la zone, et le dépôt. */
export type DropAt = { id: string; zone: Zone; drop: Drop };

/** Un glisser-déposer en cours dans cet arbre : la page déplacée, et où elle irait. */
export type TreeDrag = { id: string; over: DropAt | null };

type Props = {
  /** L'id de l'arbre dans le document : le titre de sa section le désigne (`aria-controls`). */
  id: string;
  /** Le nom de l'arbre : le titre de sa section. */
  label: string;
  /** Les pages de la section. */
  nodes: PageNode[];
  /** Les lignes de l'arbre, de haut en bas (`flattenRows` de `nodes`, sans les repliées). */
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
  /** Le glisser-déposer en cours, si la page déplacée est de cet arbre. */
  drag: TreeDrag | null;
  /** Schéma du serveur différent (R35) : ni action, ni création de sous-page, ni déplacement. */
  readOnly: boolean;
  /** Ces rappels gardent leur identité d'un rendu à l'autre : les lignes sont mémoïsées. */
  onToggle: (id: string) => void;
  onCreate: (parentId: string) => void;
  onMenu: (id: string, anchor: HTMLElement) => void;
  /** Une page de cet arbre est saisie. */
  onDragStart: (id: string) => void;
  /** Le pointeur vise un dépôt permis (`null` : rien, ou un dépôt refusé). */
  onDragOver: (over: DropAt | null) => void;
  /** La page `id` est lâchée : sur un dépôt permis, ou nulle part (`null`). */
  onDrop: (id: string, over: DropAt | null) => void;
};

/** La ligne de l'arbre sous le pointeur, et la hauteur du pointeur sur elle. */
function rowUnder(event: DragEvent<HTMLElement>): { id: string; offset: number; height: number } | null {
  const row = event.target instanceof Element ? event.target.closest("[data-page-id]") : null;
  const id = row?.getAttribute("data-page-id");
  if (!row || !id || !event.currentTarget.contains(row)) return null;
  const box = row.getBoundingClientRect();
  return { id, offset: event.clientY - box.top, height: box.height };
}

export function PageTree({
  id,
  label,
  nodes,
  rows,
  viewport,
  activeId,
  revealed,
  menuFor,
  drag,
  readOnly,
  onToggle,
  onCreate,
  onMenu,
  onDragStart,
  onDragOver,
  onDrop,
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

  const dragged = drag?.id ?? null;
  // Un dépôt se décide sur l'arbre entier, pages repliées comprises : la dernière sous-page
  // d'une page repliée n'est pas dans les lignes montrées.
  const all = useMemo(() => (dragged === null ? null : flattenRows(nodes, NONE)), [dragged, nodes]);

  /** Où irait la page déplacée, lâchée sous le pointeur ; `null` : nulle part. */
  const dropAt = (event: DragEvent<HTMLElement>): DropAt | null => {
    const under = rowUnder(event);
    if (all === null || dragged === null || under === null) return null;
    const index = all.findIndex((row) => row.node.id === under.id);
    // La ligne telle qu'elle est montrée : dépliée, son bord bas vise l'intérieur.
    const shown = rows.find((row) => row.node.id === under.id);
    if (index < 0 || shown === undefined) return null;
    for (const zone of zonesAt(under.offset, under.height, shown)) {
      const drop = dropTarget(all, dragged, index, zone);
      if (drop !== null) return { id: under.id, zone, drop };
    }
    return null;
  };

  const startDrag = (event: DragEvent<HTMLElement>) => {
    const under = rowUnder(event);
    const node = under === null ? undefined : nodes.find((n) => n.id === under.id);
    // Une page qu'on ne peut pas modifier ne se déplace pas ; son lien se glisse toujours
    // ailleurs, comme tout lien.
    if (node === undefined || readOnly || !canEdit(node.level)) return;
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("application/x-mementum-page", node.id);
    const row = (event.target as Element).closest("[data-page-id]");
    // L'image qui suit le pointeur : la ligne entière, saisie là où elle l'a été.
    if (row && under) event.dataTransfer.setDragImage(row, event.clientX - row.getBoundingClientRect().left, under.offset);
    onDragStart(node.id);
  };

  // La page déplacée reste dans le document même hors de la fenêtre (le défilement pendant le
  // glisser) : sans sa ligne, le navigateur ne dirait pas la fin du glisser.
  const windowed = rows.slice(range.start, range.end);
  const draggedIndex = dragged === null ? -1 : rows.findIndex((row) => row.node.id === dragged);
  const shownRows =
    draggedIndex >= 0 && (draggedIndex < range.start || draggedIndex >= range.end)
      ? [...windowed.map((row, i) => ({ row, index: range.start + i })), { row: rows[draggedIndex], index: draggedIndex }]
      : windowed.map((row, i) => ({ row, index: range.start + i }));

  const over = drag?.over ?? null;
  const overIndex = over === null ? -1 : rows.findIndex((row) => row.node.id === over.id);
  // Le trait d'insertion se dessine là où la page ira : « après » une page dépliée, c'est
  // après ses sous-pages montrées, pas entre elle et la première. La ligne visée a pu quitter
  // l'arbre pendant le glisser (une relecture) : `overIndex` vaut alors -1, et il n'y a pas
  // de trait.
  const lineIndex =
    overIndex >= 0 && over?.zone === "after" ? subtreeEnd(rows, overIndex) : overIndex;

  /** Le dernier élément de l'arbre où le pointeur est entré pendant un glisser. */
  const entered = useRef<EventTarget | null>(null);

  /** Le pointeur vise un endroit de l'arbre : le dépôt y est permis, ou non. */
  const aim = (event: DragEvent<HTMLElement>) => {
    // Un autre glisser (un fichier, du texte, une page d'une autre section) n'est pas pour cet
    // arbre.
    if (dragged === null) return;
    const target = dropAt(event);
    if (target !== null) {
      // Dépôt permis : sans cela, le navigateur le refuse.
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
    }
    onDragOver(target);
  };

  return (
    <div
      ref={setTree}
      id={id}
      role="tree"
      aria-label={label}
      className="tree"
      style={{ height: rows.length * ROW_HEIGHT }}
      onDragStart={startDrag}
      // En entrant sur un élément comme en le survolant : le navigateur n'annonce le survol
      // qu'au deuxième événement sur le même élément.
      onDragEnter={(event) => {
        entered.current = event.target;
        aim(event);
      }}
      onDragOver={aim}
      onDragLeave={(event) => {
        // Le pointeur a-t-il quitté l'arbre, ou seulement changé d'élément ? Le navigateur
        // annonce l'entrée sur le nouvel élément avant la sortie de l'ancien : seule la
        // sortie du dernier élément où il est entré est celle de l'arbre. `relatedTarget` le
        // dirait aussi, mais tous les navigateurs ne le donnent pas.
        if (event.target !== entered.current) return;
        entered.current = null;
        if (dragged !== null) onDragOver(null);
      }}
      onDrop={(event) => {
        if (dragged === null) return;
        event.preventDefault();
        onDrop(dragged, dropAt(event));
      }}
    >
      {shownRows.map(({ row, index }) => (
        <TreeRow
          key={row.node.id}
          row={row}
          index={index}
          active={row.node.id === activeId}
          menuOpen={row.node.id === menuFor}
          dragging={row.node.id === dragged}
          dropInside={over !== null && over.zone === "inside" && over.id === row.node.id}
          readOnly={readOnly}
          onToggle={onToggle}
          onCreate={onCreate}
          onMenu={onMenu}
        />
      ))}
      {over !== null && over.zone !== "inside" && overIndex >= 0 && (
        // Le trait d'insertion, au retrait de la ligne visée.
        <div
          className="tree-drop-line"
          aria-hidden="true"
          style={{
            top: lineIndex * ROW_HEIGHT - 1,
            left: rows[overIndex].depth * 14 + 22,
          }}
        />
      )}
    </div>
  );
}
