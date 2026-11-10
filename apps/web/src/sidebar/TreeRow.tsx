// Une ligne de l'arbre des pages : repli, lien vers la page, « + » au survol (sous-page) pour
// qui peut modifier la page. Mémoïsée (Ruling R43) : d'une navigation à l'autre, seules les
// lignes dont `active` change se rendent de nouveau. Ses autres props gardent donc leur
// identité : la ligne vient de `flattenRows`, les rappels sont stables.
import { memo, type CSSProperties } from "react";
import { ChevronIcon, PageIcon, PlusIcon } from "../icons";
import { pagePath } from "../router";
import { canEdit } from "../share/levels";
import { ROW_HEIGHT, type Row } from "./rows";

type Props = {
  row: Row;
  /** Rang de la ligne dans l'arbre aplati : elle est placée à `index × ROW_HEIGHT`. */
  index: number;
  active: boolean;
  /** Schéma du serveur différent (R35) : aucune création de sous-page. */
  readOnly: boolean;
  onToggle: (id: string) => void;
  onCreate: (parentId: string) => void;
};

export const TreeRow = memo(function TreeRow({
  row,
  index,
  active,
  readOnly,
  onToggle,
  onCreate,
}: Props) {
  const { node, depth, hasChildren, open } = row;
  // `--depth` : le retrait, dans `theme.css`.
  const style = { top: index * ROW_HEIGHT, height: ROW_HEIGHT, "--depth": depth } as CSSProperties;
  return (
    <div
      role="treeitem"
      aria-level={depth + 1}
      aria-expanded={hasChildren ? open : undefined}
      data-page-id={node.id}
      className={active ? "tree-row is-active" : "tree-row"}
      style={style}
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
      {!readOnly && canEdit(node.level) && (
        <button
          type="button"
          className="tree-add"
          aria-label={`Ajouter une sous-page à « ${node.title} »`}
          title="Ajouter une sous-page"
          onClick={() => onCreate(node.id)}
        >
          <PlusIcon />
        </button>
      )}
    </div>
  );
});
