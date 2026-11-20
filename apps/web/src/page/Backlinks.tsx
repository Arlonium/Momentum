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
import { useId, useRef, useState, useSyncExternalStore, type FocusEvent, type MouseEvent } from "react";
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
  const hold = () => setHeld((was) => was ?? shown);
  const release = () => {
    if (!inside.current.pointer && !inside.current.focus) setHeld(null);
  };
  const onBlur = (event: FocusEvent) => {
    if (event.relatedTarget instanceof Node && nav.current?.contains(event.relatedTarget)) return;
    inside.current.focus = false;
    release();
  };

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
      onPointerEnter={() => {
        inside.current.pointer = true;
        hold();
      }}
      onPointerLeave={() => {
        inside.current.pointer = false;
        release();
      }}
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
