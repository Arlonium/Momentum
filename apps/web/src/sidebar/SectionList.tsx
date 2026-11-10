// Les sections de la barre latérale d'un espace, dans l'ordre « Espace », « Partagé »,
// « Privé » (`sections.ts`), chacune avec son titre et son arbre de pages. Un titre replie sa
// section, comme dans Notion ; l'état est gardé par espace, dans le navigateur. Le « + » d'un
// titre crée une page à la racine de la section (« Espace » et « Privé » seulement) ; le « + »
// d'une ligne, une sous-page ; son « … », le menu de ses actions (`RowMenu`) : « Déplacer vers
// le haut », « Déplacer vers le bas », « Imbriquer dans la page précédente », « Sortir de la
// page parente » (ce que la souris fait en glissant, au clavier, R95), « Mettre à la
// corbeille ». La page mise à la corbeille emporte sa ligne : le focus va à celle qui prend sa
// place.
//
// Une page se déplace aussi à la souris, à l'intérieur de sa section (`PageTree`, `drag.ts`).
// Le glisser-déposer en cours est tenu ici : seul l'arbre d'où vient la page accepte le dépôt.
//
// Les pages repliées (une page et ses sous-pages) ne sont pas gardées : tout est déplié au
// chargement.
import type { PageNode, Role, RootSection, Section } from "@mementum/protocol";
import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  ChevronIcon,
  NestIcon,
  PlusIcon,
  TrashIcon,
  UnnestIcon,
} from "../icons";
import { nestTarget, stepTarget, unnestTarget, type Drop } from "./drag";
import { PageTree, type DropAt, type TreeDrag } from "./PageTree";
import { RowMenu, type MenuAction } from "./RowMenu";
import { flattenRows, neighbourOf, NONE } from "./rows";
import {
  bySection,
  canCreateIn,
  readCollapsedSections,
  SECTION_LABELS,
  SECTIONS,
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
  /** Leur lecture a échoué : sans pages chargées, on ne dit pas qu'elles arrivent. */
  unavailable: boolean;
  /** La barre latérale, où les arbres défilent. */
  viewport: Viewport | null;
  activeId: string | null;
  /** Schéma du serveur différent (R35) : aucune création de page. */
  readOnly: boolean;
  /** Doit garder son identité d'un rendu à l'autre : les lignes sont mémoïsées. */
  onCreate: (target: CreateTarget) => void;
  /** Met la page à la corbeille, avec ses sous-pages. */
  onTrash: (id: string) => void;
  /** Range la page ailleurs dans sa section (`drag.ts`). */
  onMove: (id: string, drop: Drop) => void;
};

/** Le menu « … » ouvert : sa page, et le bouton qui l'a ouvert. */
type OpenMenu = { id: string; anchor: HTMLElement };

/**
 * La page que le menu vient de mettre à la corbeille : le bouton « … » de sa ligne, à qui le
 * menu a rendu le focus, et ce qui le recevra quand la ligne partira : la ligne voisine
 * (`neighbour`), sinon le titre de sa section.
 */
type Leaving = { id: string; anchor: HTMLElement; section: Section; neighbour: string | null };

/** Le même dépôt visé ? */
function sameDrop(a: DropAt | null, b: DropAt | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.id === b.id &&
    a.zone === b.zone &&
    a.drop.parentId === b.drop.parentId &&
    a.drop.afterId === b.drop.afterId
  );
}

export function SectionList({
  spaceId,
  role,
  nodes,
  unavailable,
  viewport,
  activeId,
  readOnly,
  onCreate,
  onTrash,
  onMove,
}: Props) {
  /** Les sections repliées. */
  const [closed, setClosed] = useState<readonly Section[]>(() => readCollapsedSections(spaceId));
  /** Les pages repliées, toutes sections confondues. */
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const groups = useMemo(() => bySection(nodes ?? []), [nodes]);
  /** La page active déjà montrée par l'un des arbres (`PageTree`). */
  const revealed = useRef<string | null>(null);
  const [menu, setMenu] = useState<OpenMenu | null>(null);
  /** La page du menu ouvert ; partie de l'arbre entre-temps, le menu n'a plus lieu d'être. */
  const menuNode = menu === null ? undefined : nodes?.find((node) => node.id === menu.id);

  /** Le bouton « … » d'une ligne : ouvre son menu, ou le ferme s'il est déjà ouvert. */
  const toggleMenu = useCallback((id: string, anchor: HTMLElement) => {
    setMenu((current) => (current?.id === id ? null : { id, anchor }));
  }, []);
  const closeMenu = useCallback(() => setMenu(null), []);

  const root = useRef<HTMLDivElement>(null);
  const leaving = useRef<Leaving | null>(null);
  // La ligne d'une page mise à la corbeille part avec le focus, que le menu avait rendu à son
  // bouton « … » : il va à la ligne qui prend sa place, sinon au titre de la section. Seulement
  // s'il s'est perdu : on ne le reprend pas à qui l'a posé ailleurs entre-temps.
  useEffect(() => {
    const left = leaving.current;
    if (left === null || nodes === null) return;
    if (nodes.some((node) => node.id === left.id)) {
      // Toujours là (le serveur n'a pas encore répondu, ou il a refusé). Le focus a quitté son
      // bouton : la personne est passée à autre chose.
      if (document.activeElement !== left.anchor) leaving.current = null;
      return;
    }
    leaving.current = null;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    const row =
      left.neighbour === null
        ? null
        : root.current?.querySelector<HTMLElement>(`[data-page-id="${left.neighbour}"] .tree-link`);
    const title = root.current?.querySelector<HTMLElement>(
      `[data-section="${left.section}"] .mm-section-title`,
    );
    (row ?? title)?.focus();
  }, [nodes]);
  // Sans sa page, le menu est oublié : il ne se rouvre pas si elle revient.
  const menuGone = menu !== null && menuNode === undefined;
  useEffect(() => {
    if (menuGone) setMenu(null);
  }, [menuGone]);

  /** L'arbre déplié de la section de la page du menu : ses sœurs disent où elle peut aller. */
  const menuRows = useMemo(() => {
    const section = SECTIONS.find((s) => menuNode !== undefined && groups[s].includes(menuNode));
    return section === undefined ? [] : flattenRows(groups[section], NONE);
  }, [menuNode, groups]);

  /** Déplie la page `id`. Déjà dépliée, l'ensemble ne change pas, et les lignes non plus. */
  const expand = useCallback((id: string) => {
    setCollapsed((current) => {
      if (!current.has(id)) return current;
      const next = new Set(current);
      next.delete(id);
      return next;
    });
  }, []);

  const move = useCallback(
    (id: string, drop: Drop) => {
      // La page doit se voir à sa nouvelle place : sa nouvelle page parente se déplie.
      if (drop.parentId !== null) expand(drop.parentId);
      onMove(id, drop);
    },
    [onMove, expand],
  );

  /** Le glisser-déposer en cours : la page saisie, et où elle irait. */
  const [drag, setDrag] = useState<TreeDrag | null>(null);
  const startDrag = useCallback((id: string) => setDrag({ id, over: null }), []);
  const dragOver = useCallback((over: DropAt | null) => {
    setDrag((current) =>
      current === null || sameDrop(current.over, over) ? current : { ...current, over },
    );
  }, []);
  const drop = useCallback(
    (id: string, over: DropAt | null) => {
      setDrag(null);
      if (over !== null) move(id, over.drop);
    },
    [move],
  );
  // Lâchée ailleurs, ou glisser abandonné (Échap) : le navigateur le dit à la ligne saisie.
  const dragging = drag !== null;
  useEffect(() => {
    if (!dragging) return;
    const end = () => setDrag(null);
    document.addEventListener("dragend", end);
    return () => document.removeEventListener("dragend", end);
  }, [dragging]);

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
      // La sous-page doit se voir : sa page parente se déplie.
      expand(parentId);
      onCreate({ parentId });
    },
    [onCreate, expand],
  );

  const createIn = (section: RootSection) => {
    // La page doit se voir : sa section se déplie.
    if (closed.includes(section)) openSection(section, true);
    onCreate({ section });
  };

  if (nodes === null) {
    return (
      <div ref={root} className="mm-sections">
        {!unavailable && <p className="mm-side-note">Chargement…</p>}
      </div>
    );
  }

  const actions: MenuAction[] = [];
  if (menu !== null && menuNode !== undefined) {
    const { id } = menuNode;
    // Au clavier, ce que la souris fait en glissant : un cran parmi ses sœurs, sous la sœur
    // qui précède, ou hors de sa page parente. Un geste refusé (niveau, section) est grisé.
    const moves: [label: string, icon: ReactNode, drop: Drop | null][] = [
      ["Déplacer vers le haut", <ArrowUpIcon />, stepTarget(menuRows, id, "up")],
      ["Déplacer vers le bas", <ArrowDownIcon />, stepTarget(menuRows, id, "down")],
      ["Imbriquer dans la page précédente", <NestIcon />, nestTarget(menuRows, id, role)],
      ["Sortir de la page parente", <UnnestIcon />, unnestTarget(menuRows, id, role)],
    ];
    for (const [label, icon, drop] of moves) {
      actions.push({
        label,
        icon,
        disabled: drop === null,
        onSelect: () => drop !== null && move(id, drop),
      });
    }
    actions.push(
      {
        label: "Mettre à la corbeille",
        icon: <TrashIcon />,
        destructive: true,
        onSelect: () => {
          const section = SECTIONS.find((s) => groups[s].includes(menuNode));
          if (section !== undefined) {
            // Les lignes telles qu'elles sont montrées : la voisine est celle qu'on voit.
            const neighbour = neighbourOf(flattenRows(groups[section], collapsed), id);
            leaving.current = { id, anchor: menu.anchor, section, neighbour };
          }
          onTrash(id);
        },
      },
    );
  }

  return (
    <div ref={root} className="mm-sections">
      {visibleSections(role, groups).map((section) => (
        <SectionBlock
          key={section}
          section={section}
          nodes={groups[section]}
          role={role}
          collapsed={collapsed}
          open={!closed.includes(section)}
          viewport={viewport}
          activeId={activeId}
          revealed={revealed}
          menuFor={menuNode?.id ?? null}
          // Seul l'arbre d'où vient la page saisie connaît le glisser-déposer.
          drag={drag !== null && groups[section].some((n) => n.id === drag.id) ? drag : null}
          readOnly={readOnly}
          onOpen={openSection}
          onToggle={togglePage}
          onCreateUnder={createUnder}
          onCreateIn={createIn}
          onMenu={toggleMenu}
          onDragStart={startDrag}
          onDragOver={dragOver}
          onDrop={drop}
        />
      ))}
      {menu !== null && menuNode !== undefined && (
        <RowMenu
          label={`Actions de « ${menuNode.title} »`}
          anchor={menu.anchor}
          actions={actions}
          onClose={closeMenu}
        />
      )}
    </div>
  );
}

type BlockProps = {
  section: Section;
  /** Les pages de la section. */
  nodes: PageNode[];
  role: Role;
  collapsed: ReadonlySet<string>;
  open: boolean;
  viewport: Viewport | null;
  activeId: string | null;
  revealed: RefObject<string | null>;
  menuFor: string | null;
  drag: TreeDrag | null;
  readOnly: boolean;
  onOpen: (section: Section, open: boolean) => void;
  onToggle: (id: string) => void;
  onCreateUnder: (parentId: string) => void;
  onCreateIn: (section: RootSection) => void;
  onMenu: (id: string, anchor: HTMLElement) => void;
  onDragStart: (id: string) => void;
  onDragOver: (over: DropAt | null) => void;
  onDrop: (id: string, over: DropAt | null) => void;
};

function SectionBlock({
  section,
  nodes,
  role,
  collapsed,
  open,
  viewport,
  activeId,
  revealed,
  menuFor,
  drag,
  readOnly,
  onOpen,
  onToggle,
  onCreateUnder,
  onCreateIn,
  onMenu,
  onDragStart,
  onDragOver,
  onDrop,
}: BlockProps) {
  const titleId = useId();
  const bodyId = useId();
  const rows = useMemo(() => flattenRows(nodes, collapsed), [nodes, collapsed]);
  const label = SECTION_LABELS[section];
  return (
    <div role="group" aria-labelledby={titleId} className="mm-section" data-section={section}>
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
            nodes={nodes}
            role={role}
            rows={rows}
            viewport={viewport}
            activeId={activeId}
            revealed={revealed}
            menuFor={menuFor}
            drag={drag}
            readOnly={readOnly}
            onToggle={onToggle}
            onCreate={onCreateUnder}
            onMenu={onMenu}
            onDragStart={onDragStart}
            onDragOver={onDragOver}
            onDrop={onDrop}
          />
        ))}
    </div>
  );
}
