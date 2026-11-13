// Le fil d'Ariane de la barre du haut (R256) : les pages parentes que le compte voit, de la
// plus haute à la page parente, chacune un lien, puis la page ouverte, qui n'en est pas un.
// Au-delà de quatre éléments, la première page, « … » et les deux derniers : le bouton « … »
// ouvre le menu des pages masquées (`RowMenu` : au clavier, il prend le focus sur sa première
// entrée, les flèches passent de l'une à l'autre, Échap le ferme et rend le focus au bouton).
//
// Les pages parentes viennent de l'arbre déjà chargé (`ancestorsOf`), que le serveur a filtré
// par les droits : rien n'est lu pour le fil, et le fil d'un invité commence à la plus haute
// page qu'il voit. Une page hors de l'arbre (à la corbeille, arbre pas encore lu) ne montre que
// son titre.
//
// Les titres viennent d'autres comptes : du texte, chacun isolé (`dir="auto"`,
// `unicode-bidi: isolate`), coupé à l'affichage et entier dans l'infobulle, entre U+2068 et
// U+2069.
//
// Suivre un lien du fil ouvre une autre page, dont la barre du haut est neuve (`PageView` est
// rendu sous la clé de sa page) : le lien quitte le document. Le focus ne tombe pas sur `body` :
// il va à la page atteinte, dans le fil de sa barre du haut.
import type { PageNode } from "@mementum/protocol";
import { useCallback, useEffect, useRef, useState, type MouseEvent } from "react";
import { isolate } from "../comments/dom";
import { PageIcon } from "../icons";
import { navigate, pagePath } from "../router";
import { RowMenu, type MenuAction } from "../sidebar/RowMenu";

/** Au-delà de ce nombre d'éléments (pages parentes et page), le milieu du fil se replie. */
export const MAX_CRUMBS = 4;

/** Le nom du bouton « … » et de son menu. */
const MORE = "Pages parentes masquées";

type Props = {
  /** La page ouverte. */
  node: PageNode;
  /** Ses pages parentes que le compte voit, de la plus haute à la page parente. */
  ancestors: PageNode[];
};

/**
 * La page qu'un lien du fil vient d'ouvrir : le fil de sa barre du haut, quand il sera monté,
 * prendra le focus. Tout fil qui se monte l'efface.
 */
let arriving: string | null = null;

/** Un clic que le routeur suivra (`useLinkInterception`) : bouton principal, sans touche. */
function followed(event: MouseEvent): boolean {
  return (
    event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey
  );
}

/** Le titre montré : le serveur n'en rend pas de vide, mais un fil ne reste jamais muet. */
const titleOf = (node: PageNode) => node.title || "Sans titre";

export function Breadcrumb({ node, ancestors }: Props) {
  /** La page ouverte, dans le fil : elle prend le focus quand un lien du fil l'a ouverte. */
  const current = useRef<HTMLSpanElement>(null);
  /** Le bouton « … » du menu ouvert ; `null` : fermé. */
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);

  useEffect(() => {
    if (arriving === node.id) current.current?.focus();
    arriving = null;
  }, [node.id]);

  const trail = [...ancestors, node];
  const folded = trail.length > MAX_CRUMBS;
  /** Les pages parentes que le fil replie dans son menu. */
  const hidden = folded ? trail.slice(1, -2) : [];
  const shown: (PageNode | null)[] = folded ? [trail[0], null, ...trail.slice(-2)] : trail;

  // D'autres pages masquées sous un menu ouvert (une page déplacée ailleurs) : ses entrées ne
  // valent plus. Il se ferme, et le focus, qui y était, revient au bouton « … », ou à la page
  // si le fil ne se replie plus. Un titre renommé, lui, se met à jour dans le menu.
  const hiddenIds = hidden.map((page) => page.id).join(" ");
  const shownIds = useRef(hiddenIds);
  useEffect(() => {
    if (shownIds.current === hiddenIds) return;
    shownIds.current = hiddenIds;
    if (anchor === null) return;
    setAnchor(null);
    (anchor.isConnected ? anchor : current.current)?.focus();
  }, [hiddenIds, anchor]);
  const closeMenu = useCallback(() => setAnchor(null), []);

  const actions: MenuAction[] = hidden.map((page) => ({
    key: page.id,
    label: titleOf(page),
    icon: <PageIcon />,
    foreign: true,
    onSelect: () => {
      arriving = page.id;
      navigate(pagePath(page.id));
    },
  }));

  return (
    <nav className="mm-breadcrumb" aria-label="Fil d'Ariane">
      <ol className="mm-breadcrumb-list">
        {shown.map((page, index) => (
          <li
            key={page?.id ?? "more"}
            className={page === node ? "mm-crumb is-current" : "mm-crumb"}
          >
            {index > 0 && (
              <span className="mm-crumb-separator" aria-hidden="true">
                /
              </span>
            )}
            {page === null ? (
              <button
                type="button"
                className="mm-crumb-more"
                aria-label={MORE}
                title={MORE}
                aria-haspopup="menu"
                aria-expanded={anchor !== null}
                onClick={(event) => {
                  // Un clic sur le bouton d'un menu ouvert le ferme (`RowMenu` le laisse passer).
                  setAnchor(anchor === null ? event.currentTarget : null);
                }}
              >
                …
              </button>
            ) : page === node ? (
              <span
                ref={current}
                className="mm-crumb-page mm-isolate"
                dir="auto"
                aria-current="page"
                title={isolate(titleOf(page))}
                // Focalisable par le script seulement : à l'arrivée par un lien du fil.
                tabIndex={-1}
              >
                {titleOf(page)}
              </span>
            ) : (
              <a
                className="mm-crumb-link"
                href={pagePath(page.id)}
                title={isolate(titleOf(page))}
                onClick={(event) => {
                  if (followed(event)) arriving = page.id;
                }}
              >
                <span className="mm-isolate" dir="auto">
                  {titleOf(page)}
                </span>
              </a>
            )}
          </li>
        ))}
      </ol>
      {anchor && (
        <RowMenu label={MORE} anchor={anchor} actions={actions} onClose={closeMenu} />
      )}
    </nav>
  );
}
