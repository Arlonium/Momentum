// « Pages qui mènent ici », sous le texte d'une page (lot 4, Task C3, décision L8) : les pages du
// même espace, que le compte voit, qui portent un lien vers elle. La liste vient de `useBacklinks` ;
// ce composant ne lit rien du serveur.
//
// Le titre d'une page vient de l'arbre de l'espace, et de lui seul (R570) : la réponse n'en porte
// pas. L'arbre et la réponse ne sont pas lus au même instant : une source que l'arbre ne porte pas
// (pas encore, ou plus : accès retiré, corbeille) n'est pas montrée, et jamais sous un titre gardé.
// Sans aucune page à montrer, il n'y a pas de section, pas même son titre.
//
// Rien ne bouge sous le pointeur ni sous le focus : tant que l'un ou l'autre est dans la section,
// elle garde les lignes qu'elle montrait, à leur place. Une ligne tenue dont la page quitte l'arbre
// reste, sous un libellé neutre et sans son extrait ; un titre renommé, lui, se met à jour en place.
//
// Chaque ligne est un vrai lien, vers la page source à son bloc (`blockPath`), que le routeur suit
// avec son fragment (`useLinkInterception`) ; Ctrl+clic et le clic du milieu restent au navigateur.
// Le lien suivi part avec la page quittée. Avec un bloc, l'éditeur de la page d'arrivée y va et y
// met le focus (`useArrival`). Sans bloc, rien ne le prendrait : le fil d'Ariane de la page ouverte
// le prend (`arriveAt`, R292), et lui seul dans ce cas, pour que les deux ne se disputent rien.
import type { Backlink } from "@mementum/protocol";
import {
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
  type FocusEvent,
  type MouseEvent,
} from "react";
import { blockPath } from "../router";
import { publishedTitles, watchPages } from "../space/pageDirectory";
import { arriveAt } from "./Breadcrumb";

/**
 * L'adresse d'un rétrolien : **le seul endroit** où une page source et son bloc deviennent une
 * adresse. Celle de la page, à son bloc (`blockPath`, qui échappe `blockId`, un texte venu
 * d'ailleurs) ; `null` : la page, sans bloc. Jamais de fragment bâti à la main ici.
 */
export function backlinkHref(pageId: string, blockId: string | null): string {
  return blockPath(pageId, blockId);
}

/** La borne du serveur pour un extrait, en points de code : il coupe sans marque. */
const EXCERPT_CODE_POINTS = 240;

/** L'extrait montré : à la borne, il a sans doute été coupé, et le dit. */
function shownExcerpt(excerpt: string): string {
  return [...excerpt].length >= EXCERPT_CODE_POINTS ? `${excerpt}…` : excerpt;
}

/** Un clic que le routeur suivra : bouton principal, sans touche. */
const followed = (event: MouseEvent) =>
  event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;

type Props = {
  /** Les pages qui mènent ici, telles que lues ; `null` : pas encore lues. */
  pages: readonly Backlink[] | null;
  /** Le message de la dernière lecture, si elle a échoué. */
  error: string | null;
};

export function Backlinks({ pages, error }: Props) {
  // Les titres de l'arbre publié : une table neuve à chaque arbre qui change les pages.
  const titles = useSyncExternalStore(watchPages, publishedTitles);
  // Ce que la section montrait quand le pointeur ou le focus y est entré : gardé tant qu'ils y sont.
  const [held, setHeld] = useState<readonly Backlink[] | null>(null);
  const inside = useRef({ pointer: false, focus: false });
  const nav = useRef<HTMLElement>(null);
  const headingId = useId();

  /** Ce que la dernière lecture et l'arbre donnent à montrer. */
  const seen = (pages ?? []).filter((link) => titles.has(link.pageId));
  const shown = held ?? seen;
  /** Ce qui est montré au dernier rendu : ce que le gel garde. */
  const latest = useRef(shown);
  latest.current = shown;
  const hold = () => setHeld((was) => was ?? latest.current);
  const release = () => {
    if (!inside.current.pointer && !inside.current.focus) setHeld(null);
  };
  const onBlur = (event: FocusEvent) => {
    if (event.relatedTarget instanceof Node && nav.current?.contains(event.relatedTarget)) return;
    // La fenêtre du navigateur a perdu le focus : le lien reste l'élément actif du document, et
    // reprendra le focus avec elle. Sa ligne ne part pas sous lui.
    if (nav.current?.contains(document.activeElement)) return;
    inside.current.focus = false;
    release();
  };

  // Le pointeur se suit sur le document, jamais par « enter » et « leave » de la section (R419) :
  // une fenêtre qui disparaît sous le pointeur ne fait envoyer que `pointerover` à ce qui est
  // dessous, et React n'en tire aucun « enter » ; le gel ne s'armerait pas.
  useEffect(() => {
    const over = (event: PointerEvent) => {
      const within = event.target instanceof Node && nav.current?.contains(event.target) === true;
      if (within === inside.current.pointer) return;
      inside.current.pointer = within;
      if (within) setHeld((was) => was ?? latest.current);
      else if (!inside.current.focus) setHeld(null);
    };
    // Sans élément d'arrivée : le pointeur a quitté la fenêtre.
    const out = (event: PointerEvent) => {
      if (event.relatedTarget !== null || !inside.current.pointer) return;
      inside.current.pointer = false;
      if (!inside.current.focus) setHeld(null);
    };
    document.addEventListener("pointerover", over, true);
    document.addEventListener("pointerout", out, true);
    return () => {
      document.removeEventListener("pointerover", over, true);
      document.removeEventListener("pointerout", out, true);
    };
  }, []);

  if (shown.length === 0 && error === null) return null;

  return (
    <nav
      ref={nav}
      className="mm-backlinks"
      aria-labelledby={headingId}
      onFocus={() => {
        inside.current.focus = true;
        hold();
      }}
      onBlur={onBlur}
    >
      <h2 id={headingId} className="mm-backlinks-title">
        Pages qui mènent ici
      </h2>
      {shown.length > 0 && (
        <ul className="mm-backlinks-list">
          {shown.map((link) => {
            const title = titles.get(link.pageId);
            return (
              <li key={link.pageId} className="mm-backlink">
                <a
                  className="mm-backlink-link"
                  href={backlinkHref(link.pageId, link.blockId)}
                  data-backlink-page={link.pageId}
                  onClick={(event) => {
                    // Avec un bloc, c'est l'arrivée sur le bloc qui prend le focus (`useArrival`).
                    if (link.blockId === null && followed(event)) arriveAt(link.pageId);
                  }}
                >
                  <span className="mm-backlink-name" dir="auto">
                    {title === undefined ? "Page indisponible" : title || "Sans titre"}
                  </span>
                  {title !== undefined && link.excerpt !== "" && (
                    <span className="mm-backlink-excerpt" dir="auto">
                      {shownExcerpt(link.excerpt)}
                    </span>
                  )}
                </a>
              </li>
            );
          })}
        </ul>
      )}
      {error !== null && (
        <p className="mm-backlinks-error" role="status">
          Les pages qui mènent ici n'ont pas pu être lues : {error}
        </p>
      )}
    </nav>
  );
}
