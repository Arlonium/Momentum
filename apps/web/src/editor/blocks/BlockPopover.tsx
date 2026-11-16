// La fenêtre où se règle un bloc riche (l'icône et la couleur d'un callout, l'adresse d'un signet) :
// non modale, posée près du bloc, hors du document. Elle prend le focus en s'ouvrant (l'élément
// marqué `data-autofocus`, sinon le premier qui se focalise), le garde (Tab et Maj+Tab tournent en
// dedans), et se ferme sur Échap ou un clic dehors : c'est `onClose` qui dit où va le focus
// (`BlockWindowHost`, qui retrouve le bloc par son id).
//
// Elle reste dans l'écran : au-dessous de son bloc s'il y a la place, au-dessus sinon, et, s'il n'y
// en a ni d'un côté ni de l'autre, collée au bas de l'écran, entière (`placeWindow`). Plus haute que
// l'écran, elle défile en dedans. Sa place suit le défilement de la page et la taille de la fenêtre
// du navigateur.
//
// Ce n'est pas un élément `dialog` : une fenêtre ouverte rend le reste de la page inerte pour les
// raccourcis (`underModal`, `comments/shortcut.ts`), et celle-ci ne l'est pas.
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

type Rect = { left: number; top: number; bottom: number };
type Size = { width: number; height: number };

/** Écart entre le bloc et la fenêtre, et marge gardée au bord de l'écran (px). */
const GAP = 6;
const MARGIN = 8;

/** Ce qui prend le focus dans la fenêtre. */
const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * La place d'une fenêtre de taille `size` près de l'élément `anchor`, dans un écran `viewport` :
 * au-dessous, sinon au-dessus, sinon collée au bas ; jamais hors de l'écran, à `MARGIN` près.
 * `maxHeight` : sa hauteur au plus (au-delà, elle défile en dedans).
 */
export function placeWindow(anchor: Rect, size: Size, viewport: Size): { top: number; left: number; maxHeight: number } {
  const maxHeight = Math.max(0, viewport.height - 2 * MARGIN);
  const height = Math.min(size.height, maxHeight);
  const lowest = viewport.height - MARGIN - height;
  const below = anchor.bottom + GAP;
  const above = anchor.top - GAP - height;
  let top: number;
  if (below <= lowest && below >= MARGIN) top = below;
  else if (above >= MARGIN && above <= lowest) top = above;
  else top = Math.max(MARGIN, Math.min(lowest, below));
  const rightmost = viewport.width - MARGIN - size.width;
  const left = Math.max(MARGIN, Math.min(anchor.left, rightmost));
  return { top, left, maxHeight };
}

type Props = {
  /** Le nom de la fenêtre, pour les lecteurs d'écran. */
  label: string;
  /**
   * L'élément près duquel la poser, relu à chaque mise en place : celui du bloc peut être recréé
   * pendant que la fenêtre est ouverte. `null` (le bloc est parti) : elle garde sa place.
   */
  anchor: () => HTMLElement | null;
  /**
   * L'élément où un clic ne ferme pas la fenêtre, parce qu'il l'ouvre et la ferme lui-même : par
   * défaut, `anchor` (le bouton d'un bloc). Une fenêtre sans bouton, posée près d'un bloc ou de
   * l'éditeur entier (le sélecteur du lien de page), rend `null` : tout clic dehors la ferme.
   */
  opener?: () => HTMLElement | null;
  onClose: (how: "escape" | "outside") => void;
  children: ReactNode;
};

export function BlockPopover({ label, anchor, opener = anchor, onClose, children }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ top: number; left: number; maxHeight: number } | null>(null);
  const latest = useRef({ anchor, opener, onClose });
  latest.current = { anchor, opener, onClose };

  const position = () => {
    const element = ref.current;
    const target = latest.current.anchor();
    if (element === null || target === null || !target.isConnected) return;
    const next = placeWindow(
      target.getBoundingClientRect(),
      { width: element.offsetWidth, height: element.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
    );
    setPlace((was) =>
      was && was.top === next.top && was.left === next.left && was.maxHeight === next.maxHeight ? was : next,
    );
  };

  // Après chaque rendu : la taille de la fenêtre, ou l'élément du bloc, a pu changer.
  useLayoutEffect(position);

  useEffect(() => {
    const element = ref.current!;
    const first =
      element.querySelector<HTMLElement>("[data-autofocus]") ?? element.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? element).focus();
    // Le bloc vient peut-être d'être inséré (le menu « / ») : son élément arrive au rendu suivant.
    const frame = requestAnimationFrame(position);
    const onMove = () => position();
    const onPointerDown = (event: Event) => {
      const target = event.target;
      if (!(target instanceof Node) || element.contains(target)) return;
      // Le bouton du bloc ouvre et ferme sa fenêtre lui-même.
      if (latest.current.opener()?.contains(target)) return;
      latest.current.onClose("outside");
    };
    window.addEventListener("scroll", onMove, true);
    window.addEventListener("resize", onMove);
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("scroll", onMove, true);
      window.removeEventListener("resize", onMove);
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
    // `position` lit ses entrées par `latest` : une fois au montage.
  }, []);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose("escape");
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = [...ref.current!.querySelectorAll<HTMLElement>(FOCUSABLE)];
    if (focusable.length === 0) {
      event.preventDefault();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !ref.current!.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !ref.current!.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      ref={ref}
      role="dialog"
      aria-modal="false"
      aria-label={label}
      tabIndex={-1}
      className="mm-block-window"
      // Une place calculée ici, jamais rien de lu dans le document.
      style={place ? { top: place.top, left: place.left, maxHeight: place.maxHeight } : undefined}
      onKeyDown={onKeyDown}
    >
      {children}
    </div>
  );
}
