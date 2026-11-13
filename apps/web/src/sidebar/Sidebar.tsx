// La barre latérale. En tête, collés à son bord haut : le sélecteur d'espace (avec le menu du
// compte, Ruling R80) et « Nouvelle page ». Puis, qui défilent dessous : « Récents », les pages
// que le compte a ouvertes dans l'espace, gardées dans le navigateur (R255, absente quand elle
// est vide, rien n'est lu du serveur pour elle), et les sections de l'espace, chacune avec son
// arbre de pages. En pied, collés à son bord bas : le message de la dernière action refusée (à
// défaut, celui d'une lecture des pages qui a échoué), avec « Réessayer » tant que les pages
// n'ont pas pu être lues, et « Corbeille ».
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import { useLayoutEffect, useMemo, useState } from "react";
import { ComposeIcon, TrashIcon } from "../icons";
import { trashPath } from "../router";
import { useSession } from "../session/session";
import { useRecentPages } from "../space/recents";
import { firstClick } from "../ui/firstClick";
import type { Drop } from "./drag";
import { SectionList, type CreateTarget } from "./SectionList";
import { ShortList } from "./ShortList";
import { SpaceSwitcher } from "./SpaceSwitcher";
import type { Viewport } from "./treeWindow";

type Props = {
  /** L'espace affiché ; `null` : le compte n'en a aucun. */
  space: SpaceInfo | null;
  /** Les pages de l'espace ; `null` : pas encore chargées. */
  nodes: PageNode[] | null;
  activeId: string | null;
  /** Les réglages de l'espace affiché sont à l'écran. */
  settingsOpen: boolean;
  /** La corbeille de l'espace affiché est à l'écran. */
  trashOpen: boolean;
  /** Schéma du serveur différent (R35) : aucune création de page. */
  readOnly: boolean;
  /** Doit garder son identité d'un rendu à l'autre : les lignes sont mémoïsées. */
  onCreate: (target: CreateTarget) => void;
  onTrash: (id: string) => void;
  onMove: (id: string, drop: Drop) => void;
  /** Erreur de la dernière action (création, corbeille, déplacement), en français. */
  notice: string | null;
  /** Les pages n'ont pas pu être lues, ou relues : le message du serveur, en français. */
  readError: string | null;
  /** Relit les pages de l'espace. */
  onRetry: () => void;
};

export function Sidebar({
  space,
  nodes,
  activeId,
  settingsOpen,
  trashOpen,
  readOnly,
  onCreate,
  onTrash,
  onMove,
  notice,
  readError,
  onRetry,
}: Props) {
  // La barre latérale défile ; sa tête et son pied restent collés à ses bords, par-dessus les
  // arbres, qui en tiennent compte pour montrer la page active (`treeWindow.ts`).
  const [scroller, setScroller] = useState<HTMLElement | null>(null);
  const [head, setHead] = useState<HTMLElement | null>(null);
  const [footer, setFooter] = useState<HTMLElement | null>(null);
  const viewport = useMemo<Viewport | null>(
    () => (scroller ? { scroller, top: head, bottom: footer } : null),
    [scroller, head, footer],
  );
  // Ce qui prend le focus au clavier (Tab) est montré par le navigateur, qui ne sait pas que
  // la tête et le pied recouvrent les bords de la barre : on lui dit de combien, à chaque fois
  // qu'ils changent de hauteur (un message en pied). `theme.css` en fait la marge de
  // défilement (`scroll-margin`) de ce qui se focalise dans les sections.
  useLayoutEffect(() => {
    if (!scroller) return;
    const measure = () => {
      scroller.style.setProperty("--mm-side-head-height", `${head?.offsetHeight ?? 0}px`);
      scroller.style.setProperty("--mm-side-footer-height", `${footer?.offsetHeight ?? 0}px`);
    };
    measure();
    const resized = new ResizeObserver(measure);
    if (head) resized.observe(head);
    if (footer) resized.observe(footer);
    return () => resized.disconnect();
  }, [scroller, head, footer]);
  // Un invité ne crée pas de page à la racine d'un espace.
  const member = space !== null && space.role !== "guest";
  // Les pages récentes du compte dans l'espace, filtrées par son arbre (R255).
  const { me } = useSession();
  const recents = useRecentPages(me.user.id, space?.id ?? null, nodes, activeId);
  // Le refus d'une action dit déjà ce qui ne va pas ; sinon, la lecture qui a échoué.
  const readFailure =
    readError === null
      ? null
      : `La liste des pages n'a pas pu être ${nodes === null ? "lue" : "mise à jour"}. ${readError}`;
  const message = notice ?? readFailure;
  // « Réessayer » refait la lecture, pas l'action refusée : il n'est que sous le message de la
  // lecture, qui revient quand le refus n'est plus dit.
  const retryable = notice === null && readFailure !== null;
  return (
    <nav ref={setScroller} className="mm-sidebar" aria-label="Barre latérale">
      <div ref={setHead} className="mm-side-head">
        <SpaceSwitcher space={space} settingsOpen={settingsOpen} />
        {member && (
          <button
            type="button"
            className="mm-icon-button"
            aria-label="Nouvelle page"
            title={
              readOnly
                ? "Lecture seule : recharge la page pour écrire"
                : "Nouvelle page, dans « Privé »"
            }
            disabled={readOnly}
            onClick={firstClick(() => onCreate({ section: "private" }))}
          >
            <ComposeIcon />
          </button>
        )}
      </div>
      {space && (
        <ShortList
          // L'état replié est celui de cet espace. Une clé à elle : React rapproche par leur
          // clé les enfants de la barre, et celle des sections est déjà l'id de l'espace (deux
          // fois la même clé, la liste d'un espace restait à l'écran dans le suivant).
          key={`recents:${space.id}`}
          spaceId={space.id}
          label="Récents"
          group="recents"
          pages={recents}
          activeId={activeId}
        />
      )}
      {space && (
        <SectionList
          // Les sections repliées sont celles de cet espace.
          key={space.id}
          spaceId={space.id}
          role={space.role}
          nodes={nodes}
          unavailable={readError !== null}
          viewport={viewport}
          activeId={activeId}
          readOnly={readOnly}
          onCreate={onCreate}
          onTrash={onTrash}
          onMove={onMove}
        />
      )}
      {(space || message) && (
        <div ref={setFooter} className="mm-side-footer">
          {message && (
            <p className="mm-side-error" role="alert">
              {message}
            </p>
          )}
          {retryable && (
            // Sans attendre le retour sur l'onglet ou celui du réseau : la lecture se refait ici.
            <button type="button" className="mm-button mm-button-small mm-side-retry" onClick={onRetry}>
              Réessayer
            </button>
          )}
          {space && (
            <a
              href={trashPath(space.id)}
              className="mm-side-action mm-side-link"
              aria-current={trashOpen ? "page" : undefined}
            >
              <TrashIcon />
              <span>Corbeille</span>
            </a>
          )}
        </div>
      )}
    </nav>
  );
}
