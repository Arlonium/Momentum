// Une ligne de l'arbre des pages : repli, lien vers la page, puis, au survol et pour qui peut
// modifier la page, « … » (ses actions) et « + » (une sous-page). Qui peut la modifier peut
// aussi la saisir pour la déplacer (`PageTree`). Mémoïsée (Ruling R43) : d'une navigation à
// l'autre, seules les lignes dont `active` change se rendent de nouveau. Ses autres props
// gardent donc leur identité : la ligne vient de `flattenRows`, les rappels sont stables.
import { memo, type CSSProperties } from "react";
import { ChevronIcon, MoreIcon, PageIcon, PlusIcon } from "../icons";
import { pagePath } from "../router";
import { canEdit } from "../share/levels";
import { firstClick } from "../ui/firstClick";
import { ROW_HEIGHT, type Row } from "./rows";

type Props = {
  row: Row;
  /** Rang de la ligne dans l'arbre aplati : elle est placée à `index × ROW_HEIGHT`. */
  index: number;
  active: boolean;
  /** Le menu « … » de cette ligne est ouvert. */
  menuOpen: boolean;
  /** La page est en cours de déplacement à la souris. */
  dragging: boolean;
  /** La page déplacée, lâchée ici, deviendrait une sous-page de celle-ci. */
  dropInside: boolean;
  /** Schéma du serveur différent (R35) : ni action, ni création de sous-page. */
  readOnly: boolean;
  onToggle: (id: string) => void;
  onCreate: (parentId: string) => void;
  /** Ouvre ou ferme le menu « … » de la page ; `anchor` : son bouton. */
  onMenu: (id: string, anchor: HTMLElement) => void;
};

export const TreeRow = memo(function TreeRow({
  row,
  index,
  active,
  menuOpen,
  dragging,
  dropInside,
  readOnly,
  onToggle,
  onCreate,
  onMenu,
}: Props) {
  const { node, depth, hasChildren, open } = row;
  const editable = !readOnly && canEdit(node.level);
  // `--depth` : le retrait, dans `theme.css`.
  const style = { top: index * ROW_HEIGHT, height: ROW_HEIGHT, "--depth": depth } as CSSProperties;
  const classes = [
    "tree-row",
    active && "is-active",
    menuOpen && "is-menu-open",
    dragging && "is-dragging",
    dropInside && "is-drop-inside",
  ];
  return (
    <div
      role="treeitem"
      aria-level={depth + 1}
      aria-expanded={hasChildren ? open : undefined}
      data-page-id={node.id}
      className={classes.filter(Boolean).join(" ")}
      style={style}
      // Toute la ligne se saisit, pas seulement son lien.
      draggable={editable || undefined}
    >
      {hasChildren ? (
        <button
          type="button"
          className={open ? "tree-toggle is-open" : "tree-toggle"}
          aria-label={open ? "Replier" : "Déplier"}
          onClick={() => onToggle(node.id)}
        >
          <ChevronIcon />
        </button>
      ) : (
        <span className="tree-toggle" aria-hidden="true" />
      )}
      <a className="tree-link" href={pagePath(node.id)} aria-current={active ? "page" : undefined}>
        <PageIcon />
        <span className="tree-title">{node.title}</span>
      </a>
      {editable && (
        // Posées sur le bord droit de la ligne : invisibles, elles ne prennent pas de place au
        // titre.
        <span className="tree-actions">
          <button
            type="button"
            className="tree-action"
            aria-label={`Actions de « ${node.title} »`}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            title="Actions"
            onClick={(event) => onMenu(node.id, event.currentTarget)}
          >
            <MoreIcon />
          </button>
          <button
            type="button"
            className="tree-action"
            aria-label={`Ajouter une sous-page à « ${node.title} »`}
            title="Ajouter une sous-page"
            onClick={firstClick(() => onCreate(node.id))}
          >
            <PlusIcon />
          </button>
        </span>
      )}
    </div>
  );
});
