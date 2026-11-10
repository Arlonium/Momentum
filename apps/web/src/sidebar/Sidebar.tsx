// La barre latérale. En tête, collés à son bord haut : le sélecteur d'espace (avec le menu du
// compte, Ruling R80) et « Nouvelle page ». Puis les sections de l'espace, chacune avec son
// arbre de pages, qui défilent dessous. En pied, collé à son bord bas, le message de la
// dernière action refusée.
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import { useMemo, useState } from "react";
import { ComposeIcon } from "../icons";
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
  /** Schéma du serveur différent (R35) : aucune création de page. */
  readOnly: boolean;
  /** Doit garder son identité d'un rendu à l'autre : les lignes sont mémoïsées. */
  onCreate: (target: CreateTarget) => void;
  /** Erreur de la dernière action (création), en français. */
  notice: string | null;
};

export function Sidebar({ space, nodes, activeId, settingsOpen, readOnly, onCreate, notice }: Props) {
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
          viewport={viewport}
          activeId={activeId}
          readOnly={readOnly}
          onCreate={onCreate}
        />
      )}
      {notice && (
        <div ref={setFooter} className="mm-side-footer">
          <p className="mm-side-error" role="alert">
            {notice}
          </p>
        </div>
      )}
    </nav>
  );
}
