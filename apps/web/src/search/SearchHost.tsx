// Ce qui ouvre la fenêtre de recherche (lot 4, Task C1), pour `App` : le raccourci Ctrl+K
// (`shortcut.ts`), l'entrée « Rechercher » de la barre latérale, et la fenêtre elle-même, dont le
// code n'est chargé qu'à la demande (`loadSearch.ts`) : rendre l'application n'en demande rien.
//
// Ce qui est tapé dans le champ est tenu ici, pas dans la fenêtre : fermée par Échap, elle le
// retrouve, sélectionné, en se rouvrant (Échap ne détruit jamais un texte tapé). Il est celui d'un
// espace : un autre espace affiché, le champ est vide. Sans espace affiché (le compte n'en a
// aucun), rien ne s'ouvre, et Ctrl+K reste au navigateur.
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import {
  Component,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { loadSearch } from "./loadSearch";
import { useSearchShortcut } from "./shortcut";

const SearchDialog = lazy(async () => ({ default: (await loadSearch()).SearchDialog }));

export type SearchWindow = {
  /** Ouvre la fenêtre sur l'espace affiché. */
  open(): void;
  /** Demande le code de la fenêtre, sans l'ouvrir : l'entrée de la barre survolée, ou au focus. */
  preload(): void;
  /** La fenêtre, à rendre à côté du reste de l'application ; `null` : fermée. */
  window: ReactNode;
};

/**
 * La recherche de l'espace `space` (`null` : aucun) : son raccourci, écouté tant que le composant
 * est monté, et sa fenêtre. `nodes` : les pages de l'espace que le compte voit.
 */
export function useSearchWindow(space: SpaceInfo | null, nodes: PageNode[] | null): SearchWindow {
  const spaceId = space?.id ?? null;
  const [opened, setOpened] = useState(false);
  const [typed, setTyped] = useState({ spaceId, query: "" });
  // Un autre espace : le champ est vide, et la fenêtre, s'il n'y a plus d'espace, est fermée.
  if (typed.spaceId !== spaceId) setTyped({ spaceId, query: "" });
  if (opened && space === null) setOpened(false);
  const query = typed.spaceId === spaceId ? typed.query : "";

  const open = useCallback(() => setOpened(true), []);
  const close = useCallback(() => setOpened(false), []);
  const preload = useCallback(() => void loadSearch().catch(() => {}), []);
  const onQuery = useCallback((next: string) => setTyped({ spaceId, query: next }), [spaceId]);
  useSearchShortcut(space === null ? null : open);

  const window =
    opened && space !== null ? (
      <SearchBoundary onClose={close}>
        <Suspense fallback={null}>
          <SearchDialog
            // Une fenêtre par espace.
            key={space.id}
            space={space}
            nodes={nodes}
            query={query}
            onQuery={onQuery}
            onClose={close}
          />
        </Suspense>
      </SearchBoundary>
    ) : null;
  return { open, preload, window };
}

/**
 * Code de la fenêtre introuvable (réseau coupé, application mise à jour depuis) ou fenêtre en
 * erreur : un message, au lieu de rien, et l'application reste en place.
 */
class SearchBoundary extends Component<{ onClose: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: unknown): void {
    console.error("Mementum : la fenêtre de recherche n'a pas pu s'afficher.", error);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return <SearchFailed onClose={this.props.onClose} />;
  }
}

function SearchFailed({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      role="alertdialog"
      className="mm-dialog"
      aria-label="Rechercher"
      onCancel={(event) => {
        if (event.target !== event.currentTarget) return;
        event.preventDefault();
        onClose();
      }}
      onClose={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <h2 className="mm-dialog-title">Rechercher</h2>
      <p className="mm-dialog-lead">La recherche n'a pas pu s'afficher. Recharge la page pour réessayer.</p>
      <div className="mm-dialog-actions">
        <button type="button" className="mm-button" onClick={onClose}>
          Fermer
        </button>
        <button type="button" className="mm-button mm-button-primary" onClick={() => location.reload()}>
          Recharger
        </button>
      </div>
    </dialog>
  );
}
