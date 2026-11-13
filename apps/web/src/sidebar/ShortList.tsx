// Une liste courte de la barre latérale, au-dessus des sections : « Récents » (R255), et
// « Favoris » quand ils viendront. Une section repliable de liens vers des pages de l'arbre,
// qui a l'allure de « Espace », « Partagé » et « Privé » : son titre la replie, et l'état est
// gardé par espace avec le leur (`sections.ts`). Absente quand elle est vide. Ni menu, ni
// glisser : ce sont des raccourcis, la page se range dans son arbre.
//
// Les titres viennent d'autres comptes : du texte, chacun isolé (`dir="auto"`,
// `unicode-bidi: isolate`), coupé à l'affichage et entier dans l'infobulle, entre U+2068 et
// U+2069. Les liens ne portent pas `data-page-id`, qui reste aux lignes de l'arbre.
//
// Une page qui quitte la liste (à la corbeille, plus dans l'arbre) emporte son lien. S'il avait
// le focus, il va au lien qui prend sa place, sinon à celui d'avant ; si la liste disparaît, au
// titre de ce qui la suit dans la barre latérale. Seulement s'il s'est perdu : on ne le reprend
// pas à qui l'a posé ailleurs entre-temps.
import type { PageNode } from "@mementum/protocol";
import { useEffect, useId, useLayoutEffect, useRef, useState, type FocusEvent } from "react";
import { isolate } from "../comments/dom";
import { ChevronIcon, PageIcon } from "../icons";
import { pagePath } from "../router";
import { readCollapsedGroup, writeCollapsedGroup, type ShortGroup } from "./sections";

type Props = {
  /** L'espace affiché : l'état replié de la liste est gardé pour lui. */
  spaceId: string;
  /** Le titre de la liste, qui la nomme. */
  label: string;
  group: ShortGroup;
  /** Les pages, dans l'ordre où les montrer, toutes de l'arbre affiché. */
  pages: PageNode[];
  activeId: string | null;
};

/** Le titre de section qu'est `element`, ou le premier qu'il contient, s'il est encore là. */
function sectionTitle(element: Element | null): HTMLElement | null {
  if (element === null || !element.isConnected) return null;
  if (element.matches(".mm-section-title")) return element as HTMLElement;
  return element.querySelector<HTMLElement>(".mm-section-title");
}

export function ShortList({ spaceId, label, group, pages, activeId }: Props) {
  const [open, setOpen] = useState(() => !readCollapsedGroup(spaceId, group));
  const titleId = useId();
  const bodyId = useId();
  const root = useRef<HTMLDivElement>(null);
  /** Ce qui suit la liste, et ce qui la contient : où rendre le focus quand elle disparaît. */
  const after = useRef<Element | null>(null);
  const container = useRef<Element | null>(null);
  /** La page dont le lien a le focus. */
  const focused = useRef<string | null>(null);
  /** Les pages au rendu d'avant : la place d'un lien parti s'y lit. */
  const before = useRef(pages);

  useLayoutEffect(() => {
    if (root.current === null) return;
    after.current = root.current.nextElementSibling;
    container.current = root.current.parentElement;
  });

  useEffect(() => {
    const previous = before.current;
    before.current = pages;
    const id = focused.current;
    if (id === null || pages.some((page) => page.id === id)) return;
    focused.current = null;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    const at = previous.findIndex((page) => page.id === id);
    const links = [...(root.current?.querySelectorAll<HTMLElement>("a") ?? [])];
    const target =
      links[Math.min(at, links.length - 1)] ??
      root.current?.querySelector<HTMLElement>(".mm-section-title") ??
      sectionTitle(after.current) ??
      sectionTitle(container.current);
    target?.focus();
  }, [pages]);

  /** La page du lien où est `target`, dans cette liste ; `null` : ce n'en est pas un. */
  const linkOf = (target: EventTarget | null): string | null => {
    if (!(target instanceof Element) || !root.current?.contains(target)) return null;
    return target.closest("[data-list-page]")?.getAttribute("data-list-page") ?? null;
  };
  const onFocus = (event: FocusEvent<HTMLDivElement>) => {
    focused.current = linkOf(event.target);
  };
  const onBlur = (event: FocusEvent<HTMLDivElement>) => {
    // Le focus va ailleurs : le lien ne l'a plus. S'il ne va nulle part, c'est peut-être que
    // le lien vient de quitter le document : l'effet ci-dessus tranche.
    if (event.relatedTarget !== null) focused.current = linkOf(event.relatedTarget);
  };

  const toggle = () => {
    setOpen(!open);
    writeCollapsedGroup(spaceId, group, open);
  };

  if (pages.length === 0) return null;
  return (
    <div
      ref={root}
      role="group"
      aria-labelledby={titleId}
      className="mm-section mm-short"
      data-group={group}
      onFocus={onFocus}
      onBlur={onBlur}
    >
      <div className="mm-section-head">
        <button
          id={titleId}
          type="button"
          className={open ? "mm-section-title is-open" : "mm-section-title"}
          aria-expanded={open}
          aria-controls={open ? bodyId : undefined}
          onClick={toggle}
        >
          <span>{label}</span>
          <ChevronIcon />
        </button>
      </div>
      {open && (
        <ul id={bodyId} className="mm-short-list">
          {pages.map((page) => {
            const title = page.title || "Sans titre";
            return (
              <li key={page.id}>
                <a
                  className="mm-short-link"
                  href={pagePath(page.id)}
                  data-list-page={page.id}
                  aria-current={page.id === activeId ? "page" : undefined}
                  title={isolate(title)}
                >
                  <PageIcon />
                  <span className="mm-short-title mm-isolate" dir="auto">
                    {title}
                  </span>
                </a>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
