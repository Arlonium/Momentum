// Le sélecteur de page (Task C3) : un champ de recherche et la liste des pages que le compte voit
// dans l'espace affiché (`pageDirectory.ts`, l'arbre déjà filtré par les droits), huit au plus, les
// titres qui commencent par la requête d'abord, sans accents ni casse. Une page qui n'est pas dans
// l'arbre (qu'on ne voit pas, d'un autre espace, à la corbeille) n'y est jamais (R251).
//
// La liste suit l'arbre, sans jamais changer la page visée (R572, complété) : la page active est tenue
// par son id, pas par son rang (une page arrivée ou renommée avant elle ne la déplace pas ; partie de
// la liste, la première la remplace) ; et tant que le pointeur est sur la liste, elle garde ce qu'elle
// montrait, et suit l'arbre quand il en sort (R296, R298). Une page choisie est prise sous son titre du
// moment ; partie de l'arbre entre-temps, elle ne se choisit pas.
//
// Un combobox (R572) : le champ garde le focus, les flèches haut et bas choisissent la page active
// (`aria-activedescendant`), Entrée la prend ; un clic aussi. Le champ est tenu par qui le rend
// (`query`, `onQuery`) : la fenêtre du lien de page garde ce qu'on y a tapé quand on la ferme.
//
// Les titres viennent d'autres comptes : du texte, isolé (`dir="auto"`), entier dans l'infobulle,
// entre U+2068 et U+2069.
import { useId, useMemo, useState, type KeyboardEvent } from "react";
import { isolate } from "../../comments/dom";
import { PageIcon } from "../../icons";
import { findPages, pageTitle, usePublishedPages, type FoundPage } from "../../space/pageDirectory";

/** Les pages proposées, au plus. */
export const MAX_PICKED_PAGES = 8;

type Props = {
  /** Ce qu'on a tapé dans le champ. */
  query: string;
  onQuery: (query: string) => void;
  onPick: (page: FoundPage) => void;
  /** Rien ne se choisit (la page n'est pas modifiable pour l'instant). */
  disabled?: boolean;
};

export function PagePicker({ query, onQuery, onPick, disabled = false }: Props) {
  const id = useId();
  // Suivi : la liste change quand l'arbre change (une page renommée, partie, arrivée).
  const published = usePublishedPages();
  const found = useMemo(() => findPages(query, MAX_PICKED_PAGES), [query, published]);
  // Ce que la liste montrait quand le pointeur y est entré, pour cette requête : gardé tant qu'il y
  // est. Ce qu'on tape change la liste, pointeur dessus ou non, et c'est la liste neuve qui est tenue.
  const [held, setHeld] = useState<{ query: string; pages: FoundPage[] } | null>(null);
  if (held !== null && held.query !== query) setHeld({ query, pages: found });
  const pages = held !== null && held.query === query ? held.pages : found;
  // La page active, par son id ; une autre requête, ou la page partie : la première.
  const [chosen, setChosen] = useState<{ query: string; id: string | null }>({ query, id: null });
  const at = chosen.query === query ? pages.findIndex((page) => page.id === chosen.id) : -1;
  const index = Math.max(0, at);
  const active = pages[index];
  const optionId = (i: number) => `${id}-option-${i}`;

  const pick = (page: FoundPage | undefined) => {
    if (page === undefined || disabled) return;
    // Sous son titre du moment ; une page tenue sous le pointeur a pu quitter l'arbre.
    const title = pageTitle(page.id);
    if (title !== null) onPick({ id: page.id, title });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next = event.key === "ArrowDown" ? Math.min(index + 1, pages.length - 1) : Math.max(index - 1, 0);
      setChosen({ query, id: pages[next]?.id ?? null });
      // Entrée qui valide une composition (IME) appartient au champ, pas à la liste.
    } else if (event.key === "Enter" && !event.nativeEvent.isComposing) {
      event.preventDefault();
      pick(active);
    }
  };

  return (
    <div className="mm-page-picker">
      <label className="mm-block-window-heading" htmlFor={`${id}-query`}>
        Chercher une page
      </label>
      <input
        id={`${id}-query`}
        className="mm-input"
        type="text"
        role="combobox"
        aria-expanded={pages.length > 0}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={active === undefined ? undefined : optionId(index)}
        autoComplete="off"
        spellCheck={false}
        placeholder="Titre de la page"
        value={query}
        data-autofocus=""
        onChange={(event) => onQuery(event.target.value)}
        onKeyDown={onKeyDown}
      />
      {pages.length === 0 ? (
        <p className="mm-page-picker-empty" role="status">
          {published.length === 0 ? "Aucune page à proposer." : "Aucune page ne porte ce titre."}
        </p>
      ) : (
        <ul
          id={`${id}-list`}
          className="mm-page-picker-list"
          role="listbox"
          aria-label="Pages"
          onPointerEnter={() => {
            setHeld({ query, pages });
          }}
          onPointerLeave={() => {
            setHeld(null);
          }}
        >
          {pages.map((page, i) => (
            <li
              key={page.id}
              id={optionId(i)}
              className="mm-page-picker-option"
              role="option"
              aria-selected={i === index}
              aria-disabled={disabled || undefined}
              title={isolate(page.title)}
              // Le champ garde le focus : un clic choisit sans le lui prendre.
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => pick(page)}
            >
              <PageIcon />
              <span className="mm-page-picker-title" dir="auto">
                {page.title}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
