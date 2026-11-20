// La barre latérale. En tête, collés à son bord haut : le sélecteur d'espace (avec le menu du
// compte, Ruling R80) et « Nouvelle page », puis, dessous, « Rechercher », avec son raccourci écrit
// (lot 4, décision L5 : la fenêtre de recherche de l'espace, `search/SearchHost.tsx`). Puis, qui défilent dessous : « Favoris », les pages
// favorites du compte dans l'espace (R254, absente quand elle est vide), les sections de
// l'espace, chacune avec son arbre de pages, et « Récents », les pages que le compte a ouvertes
// dans l'espace, gardées dans le navigateur (R255, sous les sections depuis R296 ; absente quand
// elle est vide, rien n'est lu du serveur pour elle). « Favoris » est au-dessus des arbres : tant
// que le pointeur est dans la barre, une relecture ne la fait ni grandir, ni rapetisser, ni
// changer d'ordre (`heldPages.ts`, R411) ; un favori posé ou retiré dans l'onglet (menu d'une
// ligne ou de la page) se voit tout de suite. Le pointeur est suivi au niveau du document (R419) :
// le menu d'une ligne, posé dans `body`, compte pour la barre, et il disparaît sous le pointeur
// sans que rien n'annonce de sortie ; pendant un glisser, plus aucun événement de pointeur
// n'arrive, et la page tenue garde la liste gelée. En pied, collés à son bord bas : le
// message de la dernière action refusée (à défaut, celui d'une lecture des pages qui a échoué),
// avec « Réessayer » tant que les pages n'ont pas pu être lues ; une nouvelle de la dernière action
// qui n'est pas un refus (une copie prête qui ne s'est pas ouverte, R646), en texte secondaire ; et
// « Corbeille ».
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import { useEffect, useLayoutEffect, useMemo, useState } from "react";
import { ComposeIcon, SearchIcon, TrashIcon } from "../icons";
import { trashPath } from "../router";
import { MOD } from "../search/shortcut";
import { useSession } from "../session/session";
import { useRecentPages } from "../space/recents";
import { favoritePages } from "../space/useFavorites";
import { firstClick } from "../ui/firstClick";
import type { Drop } from "./drag";
import { useHeldPages } from "./heldPages";
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
  /** Nouvelle de la dernière action, qui n'est pas un refus (`PageActions.info`). */
  info?: string | null;
  /** Les pages n'ont pas pu être lues, ou relues : le message du serveur, en français. */
  readError: string | null;
  /** Relit les pages de l'espace. */
  onRetry: () => void;
  /**
   * Les pages favorites du compte dans l'espace (`useFavorites`), si leur liste est lue, et le rang
   * du dernier geste fait dans l'onglet sur chaque page.
   */
  favorites: { pages: string[]; loaded: boolean; toggles: ReadonlyMap<string, number> };
  /** Pose ou retire le favori d'une page (R254). */
  onFavorite: (id: string) => void;
  /** Duplique une page (R252), depuis le menu de sa ligne. */
  onDuplicate?: (id: string) => void;
  /** Une duplication est en route : « Dupliquer » est grisé. */
  duplicating?: boolean;
  /** Ouvre la recherche de l'espace ; absent : pas d'entrée « Rechercher ». */
  onSearch?: () => void;
  /** L'entrée « Rechercher » est survolée ou prend le focus : le code de la fenêtre peut être demandé. */
  onSearchIntent?: () => void;
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
  info = null,
  readError,
  onRetry,
  favorites,
  onFavorite,
  onDuplicate,
  duplicating,
  onSearch,
  onSearchIntent,
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
  const recents = useRecentPages(me.user.id, space?.id ?? null, nodes);
  // Les favoris, filtrés par l'arbre, et gelés tant que le pointeur est dans la barre (R411), à
  // partir du moment où la liste de l'espace est lue (sinon le gel garderait une liste vide).
  const favoriteNodes = useMemo(() => favoritePages(favorites.pages, nodes), [favorites.pages, nodes]);
  const [pointerIn, setPointerIn] = useState(false);
  /** Une page de la barre est tenue au bout de la souris (glisser-déposer). */
  const [dragging, setDragging] = useState(false);
  // Le pointeur, suivi au niveau du document (R419), pas par `pointerenter`/`pointerleave` de la
  // barre : le menu d'une ligne est un portail dans `body`, et quand il disparaît sous le pointeur
  // (une entrée cliquée là où il déborde sur le contenu, Échap, Entrée), Chromium n'envoie aucun
  // `pointerout` depuis lui, seulement `pointerover` à ce qui est dessous. Chaque `pointerover` dit
  // donc où il est : dans la barre, ou sur un menu qu'elle a ouvert (`data-sidebar-menu`). Un
  // `pointerout` sans élément d'arrivée : il a quitté la fenêtre. Un glisser commence par
  // `pointercancel` et n'envoie plus rien jusqu'à sa fin (`dragend`, `drop`, ou le premier
  // `pointerover` d'après, si la ligne tenue a quitté le document et ne dit pas la fin).
  useEffect(() => {
    if (!scroller) return;
    const inside = (target: EventTarget | null) =>
      target instanceof Element &&
      (scroller.contains(target) || target.closest("[data-sidebar-menu]") !== null);
    const over = (event: PointerEvent) => {
      setPointerIn(inside(event.target));
      setDragging(false);
    };
    const out = (event: PointerEvent) => {
      if (event.relatedTarget === null) setPointerIn(false);
    };
    const dragStart = (event: DragEvent) => {
      if (inside(event.target)) setDragging(true);
    };
    const dragEnd = () => setDragging(false);
    document.addEventListener("pointerover", over, true);
    document.addEventListener("pointerout", out, true);
    document.addEventListener("dragstart", dragStart, true);
    document.addEventListener("dragend", dragEnd, true);
    document.addEventListener("drop", dragEnd, true);
    return () => {
      document.removeEventListener("pointerover", over, true);
      document.removeEventListener("pointerout", out, true);
      document.removeEventListener("dragstart", dragStart, true);
      document.removeEventListener("dragend", dragEnd, true);
      document.removeEventListener("drop", dragEnd, true);
    };
  }, [scroller]);
  const shownFavorites = useHeldPages(
    space?.id ?? "",
    favoriteNodes,
    (pointerIn || dragging) && favorites.loaded,
    favorites.toggles,
  );
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
        {space && onSearch && (
          // Sous le sélecteur, sur toute la largeur. C'est aussi là que le focus revient quand la
          // fenêtre se ferme et que ce qui l'avait n'est plus là (`data-search-entry`).
          <button
            type="button"
            className="mm-side-action mm-side-search"
            data-search-entry=""
            aria-haspopup="dialog"
            aria-keyshortcuts={MOD === "Ctrl" ? "Control+K" : "Meta+K"}
            onPointerEnter={onSearchIntent}
            onFocus={onSearchIntent}
            onClick={onSearch}
          >
            <SearchIcon />
            <span className="mm-side-search-label">Rechercher</span>
            <span className="mm-side-search-keys">
              <kbd>{MOD}</kbd>
              <kbd>K</kbd>
            </span>
          </button>
        )}
      </div>
      {space && (
        <ShortList
          // Une clé à elle, comme « Récents » (R290) ; l'état replié est celui de cet espace.
          key={`favorites:${space.id}`}
          spaceId={space.id}
          label="Favoris"
          group="favorites"
          pages={shownFavorites}
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
          favorites={favorites.pages}
          onFavorite={onFavorite}
          onDuplicate={onDuplicate}
          duplicating={duplicating}
        />
      )}
      {space && (
        <ShortList
          // Sous les sections : elle grandit d'une ligne à chaque page ouverte, et ce qui grandit
          // en bas ne déplace aucune ligne des arbres sous le pointeur (R296). L'état replié est
          // celui de cet espace. Une clé à elle : React rapproche par leur clé les enfants de la
          // barre, et celle des sections est déjà l'id de l'espace (deux fois la même clé, la
          // liste d'un espace restait à l'écran dans le suivant).
          key={`recents:${space.id}`}
          spaceId={space.id}
          label="Récents"
          group="recents"
          pages={recents}
          activeId={activeId}
        />
      )}
      {(space || message) && (
        <div ref={setFooter} className="mm-side-footer">
          {message && (
            <p className="mm-side-error" role="alert">
              {message}
            </p>
          )}
          {info && (
            <p className="mm-side-note" role="status">
              {info}
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
