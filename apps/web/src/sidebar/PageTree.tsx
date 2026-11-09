// Barre latérale : l'arbre imbriqué des pages, « Nouvelle page » (à la racine) et « + » au
// survol d'une page (sous-page). Déplacer les pages à la souris viendra au lot 2.
import type { PageNode } from "@mementum/protocol";
import { useMemo, useState } from "react";
import { ChevronIcon, PageIcon, PlusIcon } from "../icons";
import { pagePath } from "../router";

type Props = {
  /** `null` : l'arbre n'est pas encore chargé. */
  nodes: PageNode[] | null;
  activeId: string | null;
  onCreate: (parentId: string | null) => void;
  /** Erreur de la dernière action (création), en français. */
  notice: string | null;
};

/** Enfants de chaque page (`null` : la racine), dans l'ordre de `position`. */
function childrenByParent(nodes: PageNode[]): Map<string | null, PageNode[]> {
  const ids = new Set(nodes.map((n) => n.id));
  const map = new Map<string | null, PageNode[]>();
  for (const node of nodes) {
    // Un parent absent de l'arbre : la page reste visible, à la racine.
    const parent = node.parentId !== null && ids.has(node.parentId) ? node.parentId : null;
    const list = map.get(parent) ?? [];
    list.push(node);
    map.set(parent, list);
  }
  // Index fractionnaires comparés unité par unité (ASCII).
  for (const list of map.values()) {
    list.sort((a, b) => (a.position < b.position ? -1 : a.position > b.position ? 1 : 0));
  }
  return map;
}

export function PageTree({ nodes, activeId, onCreate, notice }: Props) {
  const children = useMemo(() => childrenByParent(nodes ?? []), [nodes]);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());

  const toggle = (id: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const create = (parentId: string | null) => {
    if (parentId !== null) {
      setCollapsed((current) => {
        const next = new Set(current);
        next.delete(parentId);
        return next;
      });
    }
    onCreate(parentId);
  };

  const renderItem = (node: PageNode) => {
    const kids = children.get(node.id) ?? [];
    const open = !collapsed.has(node.id);
    const active = node.id === activeId;
    return (
      <li key={node.id} data-page-id={node.id}>
        <div className={active ? "tree-row is-active" : "tree-row"}>
          {kids.length > 0 ? (
            <button
              type="button"
              className={open ? "tree-toggle is-open" : "tree-toggle"}
              aria-label={open ? "Replier" : "Déplier"}
              aria-expanded={open}
              onClick={() => toggle(node.id)}
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
          <button
            type="button"
            className="tree-add"
            aria-label={`Ajouter une sous-page à « ${node.title} »`}
            title="Ajouter une sous-page"
            onClick={() => create(node.id)}
          >
            <PlusIcon />
          </button>
        </div>
        {kids.length > 0 && open && <ul className="tree-children">{kids.map(renderItem)}</ul>}
      </li>
    );
  };

  const roots = children.get(null) ?? [];
  return (
    <nav className="mm-sidebar" aria-label="Barre latérale">
      <div className="mm-brand">
        <span className="mm-brand-tile" aria-hidden="true">
          M
        </span>
        <span>Mementum</span>
      </div>
      <button type="button" className="mm-side-action" onClick={() => create(null)}>
        <PlusIcon size={16} />
        <span>Nouvelle page</span>
      </button>
      <div className="mm-side-section">Pages</div>
      {nodes === null ? (
        <p className="mm-side-note">Chargement…</p>
      ) : roots.length === 0 ? (
        <p className="mm-side-note">Aucune page pour l'instant.</p>
      ) : (
        <ul className="tree">{roots.map(renderItem)}</ul>
      )}
      {notice && (
        <p className="mm-side-error" role="alert">
          {notice}
        </p>
      )}
    </nav>
  );
}
