// Le menu « … » d'une ligne de l'arbre : les actions sur la page, pour qui peut la modifier.
// Le menu « … » de la barre du haut d'une page est le même (`page/PageMenu.tsx`), comme celui
// des pages parentes que le fil d'Ariane replie (`page/Breadcrumb.tsx`).
// Il s'ouvre sous son bouton, par-dessus la page (il est posé dans `body`, hors de la barre
// latérale, qui le couperait), ou au-dessus quand la place manque en bas, et jamais au-delà du
// bord droit de la fenêtre.
//
// Au clavier, c'est un menu (`role="menu"`) : ouvert, il prend le focus sur sa première
// action ; les flèches passent de l'une à l'autre, Début et Fin vont aux extrémités ; Échap et
// Tab le ferment et rendent le focus à son bouton. Un clic ailleurs le ferme aussi.
//
// Un défilement (ou un redimensionnement) le ferme s'il déplace son bouton : la barre latérale qui
// défile sous le menu d'une ligne, le document sous celui de la barre du haut. Le focus revient
// alors au bouton, sans faire défiler jusqu'à lui ce que la personne vient de faire défiler. Tout
// autre défilement le laisse ouvert (R764) : celui d'un autre conteneur (le document sous le menu
// d'une ligne, la barre latérale étant collée au haut de la fenêtre), et celui qu'on a fait juste
// avant de l'ouvrir, dont l'événement n'est distribué qu'à l'image suivante, une fois le menu
// ouvert (un clic sur un bouton sorti de l'écran le ramène d'abord, un focus porté au clavier aussi,
// un défilement à l'inertie finit après le clic). Il se fermait sur tout défilement, et le focus
// tombait sur `body`.
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { isolate } from "../comments/dom";

export type MenuAction = {
  /** Distingue l'action quand deux libellés peuvent se ressembler (deux pages du même titre). */
  key?: string;
  label: string;
  icon: ReactNode;
  /**
   * Le libellé vient d'ailleurs (un titre de page, fil d'Ariane) : un texte isolé (`dir="auto"`,
   * `unicode-bidi: isolate`), coupé à l'affichage, et entier dans l'infobulle, entre U+2068 et
   * U+2069.
   */
  foreign?: boolean;
  /** L'action n'a pas d'effet ici (la première page ne monte pas plus haut) : grisée. */
  disabled?: boolean;
  /** Une action qui retire quelque chose : rouge au survol. */
  destructive?: boolean;
  onSelect: () => void;
};

type Props = {
  /** Le nom du menu, celui de son bouton : « Actions de « Titre » ». */
  label: string;
  /** Le bouton qui l'a ouvert : le menu se place d'après lui, et lui rend le focus. */
  anchor: HTMLElement;
  actions: MenuAction[];
  /** Ferme le menu (le parent le retire). */
  onClose: () => void;
  /**
   * Le menu est celui d'une ligne de la barre latérale : marqué `data-sidebar-menu`, il compte pour
   * la barre quand on suit le pointeur (« Favoris » ne bouge pas sous lui, R411, R419).
   */
  inSidebar?: boolean;
};

/** Écart entre le menu et son bouton, et marge gardée avec les bords de la fenêtre (px). */
const GAP = 4;
const MARGIN = 8;

export function RowMenu({ label, anchor, actions, onClose, inSidebar = false }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  /** Où poser le menu ; `null` : pas encore mesuré, il est rendu sans être montré. */
  const [place, setPlace] = useState<{ top: number; left: number } | null>(null);
  /** Où était le bouton quand le menu s'est posé : un défilement qui le déplace ferme le menu. */
  const anchorAt = useRef<DOMRect | null>(null);

  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    const button = anchor.getBoundingClientRect();
    anchorAt.current = button;
    const below = button.bottom + GAP;
    const fits = below + menu.offsetHeight <= window.innerHeight - MARGIN;
    setPlace({
      top: fits ? below : Math.max(MARGIN, button.top - GAP - menu.offsetHeight),
      left: Math.max(MARGIN, Math.min(button.left, window.innerWidth - menu.offsetWidth - MARGIN)),
    });
  }, [anchor]);

  /** Les actions possibles, dans l'ordre. */
  const enabled = () => [
    ...(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])') ??
      []),
  ];

  // Une fois montré, le menu prend le focus, sur sa première action possible.
  const placed = place !== null;
  useEffect(() => {
    if (placed) enabled()[0]?.focus();
  }, [placed]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      // Un clic sur son bouton le ferme lui-même (il bascule).
      if (!ref.current?.contains(target) && !anchor.contains(target)) onClose();
    };
    // Ce qui déplace le bouton laisserait le menu en l'air ; le reste ne le touche pas.
    const onMove = () => {
      const at = anchorAt.current;
      const now = anchor.getBoundingClientRect();
      const moved =
        !anchor.isConnected || at === null || Math.abs(now.top - at.top) >= 1 || Math.abs(now.left - at.left) >= 1;
      if (!moved) return;
      // Le focus était dans le menu (ou nulle part) : il revient au bouton, pas sur `body`.
      const active = document.activeElement;
      const lost = active === null || active === document.body || (ref.current?.contains(active) ?? false);
      onClose();
      if (lost) focusAnchor();
    };
    document.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("scroll", onMove, { capture: true, passive: true });
    window.addEventListener("resize", onMove);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("scroll", onMove, { capture: true });
      window.removeEventListener("resize", onMove);
    };
  }, [anchor, onClose]);

  /**
   * Rend le focus au bouton s'il est toujours là, sans faire défiler jusqu'à lui : fermé par un
   * défilement, le menu ne ramène pas ce que la personne vient de faire défiler.
   */
  function focusAnchor() {
    if (anchor.isConnected) anchor.focus({ preventScroll: true });
  }

  /** Ferme, et rend le focus au bouton. */
  const dismiss = () => {
    onClose();
    focusAnchor();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = enabled();
    const at = items.indexOf(document.activeElement as HTMLElement);
    let next: HTMLElement | undefined;
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        // Échap ferme ce menu, pas ce qui est dessous.
        event.stopPropagation();
        dismiss();
        return;
      case "Tab":
        // Le focus repart du bouton, vers l'élément suivant (ou précédent).
        dismiss();
        return;
      case "ArrowDown":
        next = items[(at + 1) % items.length];
        break;
      case "ArrowUp":
        next = items[(at <= 0 ? items.length : at) - 1];
        break;
      case "Home":
        next = items[0];
        break;
      case "End":
        next = items[items.length - 1];
        break;
      default:
        return;
    }
    event.preventDefault();
    next?.focus();
  };

  const style: CSSProperties = place ?? { top: 0, left: 0, visibility: "hidden" };
  return createPortal(
    <div
      ref={ref}
      role="menu"
      aria-label={label}
      className="mm-menu"
      data-sidebar-menu={inSidebar || undefined}
      style={style}
      onKeyDown={onKeyDown}
    >
      {actions.map((action) => (
        <button
          key={action.key ?? action.label}
          type="button"
          role="menuitem"
          // Focalisable par le menu seulement (les flèches), pas par Tab.
          tabIndex={-1}
          className={action.destructive ? "mm-menu-item is-destructive" : "mm-menu-item"}
          aria-disabled={action.disabled || undefined}
          title={action.foreign ? isolate(action.label) : undefined}
          onClick={() => {
            if (action.disabled) return;
            dismiss();
            action.onSelect();
          }}
        >
          {action.icon}
          {action.foreign ? (
            <span className="mm-menu-text mm-isolate" dir="auto">
              {action.label}
            </span>
          ) : (
            <span>{action.label}</span>
          )}
        </button>
      ))}
    </div>,
    document.body,
  );
}
