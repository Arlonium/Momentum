// La barre latérale. En tête, collés à son bord haut : le sélecteur d'espace (avec le menu du
// compte, Ruling R80) et « Nouvelle page ». Puis les sections de l'espace, chacune avec son
// arbre de pages, qui défilent dessous. En pied, collés à son bord bas : le message de la
// dernière action refusée (à défaut, celui d'une relecture des pages qui a échoué), et
// « Corbeille ».
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import { useMemo, useState } from "react";
import { ComposeIcon, TrashIcon } from "../icons";
import { trashPath } from "../router";
import type { Drop } from "./drag";
import { SectionList, type CreateTarget } from "./SectionList";
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
  // Un invité ne crée pas de page à la racine d'un espace.
  const member = space !== null && space.role !== "guest";
  // Le refus d'une action dit déjà ce qui ne va pas ; sinon, la lecture qui a échoué.
  const message =
    notice ??
    (readError === null
      ? null
      : `La liste des pages n'a pas pu être ${nodes === null ? "lue" : "mise à jour"}. ${readError}`);
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
            onClick={() => onCreate({ section: "private" })}
          >
            <ComposeIcon />
          </button>
        )}
      </div>
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
