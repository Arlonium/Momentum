// Les sections de la barre latérale d'un espace, dans l'ordre « Espace », « Partagé »,
// « Privé » (`sections.ts`), chacune avec son titre et son arbre de pages. Un titre replie sa
// section, comme dans Notion ; l'état est gardé par espace, dans le navigateur. Le « + » d'un
// titre crée une page à la racine de la section (« Espace » et « Privé » seulement) ; le « + »
// d'une ligne, une sous-page.
//
// Les pages repliées (une page et ses sous-pages) ne sont pas gardées : tout est déplié au
// chargement.
import type { PageNode, Role, RootSection, Section } from "@mementum/protocol";
import { useCallback, useId, useMemo, useRef, useState, type RefObject } from "react";
import { ChevronIcon, PlusIcon } from "../icons";
import { PageTree } from "./PageTree";
import { flattenRows } from "./rows";
import {
  bySection,
  canCreateIn,
  readCollapsedSections,
  SECTION_LABELS,
  visibleSections,
  writeCollapsedSections,
} from "./sections";
import type { Viewport } from "./treeWindow";

/** Où créer une page : sous une page, ou à la racine d'une section. */
export type CreateTarget = { parentId: string } | { section: RootSection };

type Props = {
  spaceId: string;
  /** Le rôle de la personne dans l'espace : un invité n'a que « Partagé ». */
  role: Role;
  /** Les pages de l'espace ; `null` : pas encore chargées. */
  nodes: PageNode[] | null;
  /** La barre latérale, où les arbres défilent. */
  viewport: Viewport | null;
  activeId: string | null;
  /** Schéma du serveur différent (R35) : aucune création de page. */
  readOnly: boolean;
  /** Doit garder son identité d'un rendu à l'autre : les lignes sont mémoïsées. */
  onCreate: (target: CreateTarget) => void;
};

export function SectionList({ spaceId, role, nodes, viewport, activeId, readOnly, onCreate }: Props) {
  /** Les sections repliées. */
  const [closed, setClosed] = useState<readonly Section[]>(() => readCollapsedSections(spaceId));
  /** Les pages repliées, toutes sections confondues. */
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const groups = useMemo(() => bySection(nodes ?? []), [nodes]);
  /** La page active déjà montrée par l'un des arbres (`PageTree`). */
  const revealed = useRef<string | null>(null);

  const openSection = (section: Section, open: boolean) => {
    const next = closed.filter((other) => other !== section);
    if (!open) next.push(section);
    setClosed(next);
    writeCollapsedSections(spaceId, next);
  };

  const togglePage = useCallback((id: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }, []);

  const createUnder = useCallback(
    (parentId: string) => {
      // La sous-page doit se voir : son parent se déplie. Déjà déplié, l'ensemble ne change
      // pas, et les lignes non plus.
      setCollapsed((current) => {
        if (!current.has(parentId)) return current;
        const next = new Set(current);
        next.delete(parentId);
        return next;
      });
      onCreate({ parentId });
    },
    [onCreate],
  );

  const createIn = (section: RootSection) => {
    // La page doit se voir : sa section se déplie.
    if (closed.includes(section)) openSection(section, true);
    onCreate({ section });
  };

  if (nodes === null) {
    return (
      <div className="mm-sections">
        <p className="mm-side-note">Chargement…</p>
      </div>
    );
  }

  return (
    <div className="mm-sections">
      {visibleSections(role, groups).map((section) => (
        <SectionBlock
          key={section}
          section={section}
          nodes={groups[section]}
          collapsed={collapsed}
          open={!closed.includes(section)}
          viewport={viewport}
          activeId={activeId}
          revealed={revealed}
          readOnly={readOnly}
          onOpen={openSection}
          onToggle={togglePage}
          onCreateUnder={createUnder}
          onCreateIn={createIn}
        />
      ))}
    </div>
  );
}

type BlockProps = {
  section: Section;
  /** Les pages de la section. */
  nodes: PageNode[];
  collapsed: ReadonlySet<string>;
  open: boolean;
  viewport: Viewport | null;
  activeId: string | null;
  revealed: RefObject<string | null>;
  readOnly: boolean;
  onOpen: (section: Section, open: boolean) => void;
  onToggle: (id: string) => void;
  onCreateUnder: (parentId: string) => void;
  onCreateIn: (section: RootSection) => void;
};

function SectionBlock({
  section,
  nodes,
  collapsed,
  open,
  viewport,
  activeId,
  revealed,
  readOnly,
  onOpen,
  onToggle,
  onCreateUnder,
  onCreateIn,
}: BlockProps) {
  const titleId = useId();
  const bodyId = useId();
  const rows = useMemo(() => flattenRows(nodes, collapsed), [nodes, collapsed]);
  const label = SECTION_LABELS[section];
  return (
    <div role="group" aria-labelledby={titleId} className="mm-section">
      <div className="mm-section-head">
        <button
          id={titleId}
          type="button"
          className={open ? "mm-section-title is-open" : "mm-section-title"}
          aria-expanded={open}
          aria-controls={open ? bodyId : undefined}
          onClick={() => onOpen(section, !open)}
        >
          <span>{label}</span>
          <ChevronIcon />
        </button>
        {!readOnly && canCreateIn(section) && (
          <button
            type="button"
            className="mm-section-add"
            aria-label={`Ajouter une page dans « ${label} »`}
            title="Ajouter une page"
            onClick={() => onCreateIn(section)}
          >
            <PlusIcon />
          </button>
        )}
      </div>
      {open &&
        (rows.length === 0 ? (
          <p id={bodyId} className="mm-side-note">
            {section === "shared"
              ? "Rien n'est partagé avec toi pour l'instant."
              : "Aucune page pour l'instant."}
          </p>
        ) : (
          <PageTree
            id={bodyId}
            label={label}
            rows={rows}
            viewport={viewport}
            activeId={activeId}
            revealed={revealed}
            readOnly={readOnly}
            onToggle={onToggle}
            onCreate={onCreateUnder}
          />
        ))}
    </div>
  );
}
