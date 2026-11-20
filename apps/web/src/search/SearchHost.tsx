// Ce qui ouvre la fenêtre de recherche (lot 4, Task C1), pour `App` : le raccourci Ctrl+K
// (`shortcut.ts`), l'entrée « Rechercher » de la barre latérale, et la fenêtre elle-même, dont le
// code n'est chargé qu'à la demande (`loadSearch.ts`) : rendre l'application n'en demande rien.
//
// Ce qui est tapé dans le champ est tenu ici, pas dans la fenêtre : fermée par Échap, elle le
// retrouve, sélectionné, en se rouvrant (Échap ne détruit jamais un texte tapé). Il est celui d'un
// espace : un autre espace affiché, le champ est vide. Sans espace affiché (le compte n'en a
// aucun), rien ne s'ouvre, et Ctrl+K reste au navigateur.
//
// La coquille (R1095). Le temps que le code de la fenêtre arrive (son premier chargement), ce module,
// qui n'est pas chargé à la demande, ouvre dans le geste même une fenêtre modale réduite à son
// champ (`SearchShell`, le repli du `Suspense`) : ce qui est tapé entre Ctrl+K et l'arrivée du code
// va au champ, jamais à la page restée dessous. Quand le code arrive, la fenêtre entière prend sa
// place dans le même rendu, avec le texte (il est tenu ici), le focus, et le curseur que la coquille
// lui laisse (`caret`). La coquille reste petite : ni liste, ni recherche, ni rien de l'arbre.
//
// Ce qui avait le focus, et sa sélection, sont relevés ici, à la demande d'ouverture (`open`, dans
// le geste : Ctrl+K ou le clic), puis rendus par la fenêtre qui part, quelle qu'elle soit
// (`focusBack.ts`).
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import {
  Component,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type SyntheticEvent,
} from "react";
import { SearchIcon } from "../icons";
import { giveBack, takeBefore, type Before, type Caret } from "./focusBack";
import { loadSearch } from "./loadSearch";
import { isSearchKey, MAX_QUERY_LENGTH, useSearchShortcut } from "./shortcut";

const SearchDialog = lazy(async () => ({ default: (await loadSearch()).SearchDialog }));

export type SearchWindow = {
  /** Ouvre la fenêtre sur l'espace affiché. */
  open(): void;
  /** Demande le code de la fenêtre, sans l'ouvrir : l'entrée de la barre survolée, ou au focus. */
  preload(): void;
  /** La fenêtre, à rendre à côté du reste de l'application ; `null` : fermée. */
  window: ReactNode;
};

/** Une ouverture de la fenêtre : ce qui avait le focus avant elle, et le curseur de sa coquille. */
type Opening = { before: Before; caret: { current: Caret | null } };

/**
 * La recherche de l'espace `space` (`null` : aucun) : son raccourci, écouté tant que le composant
 * est monté, et sa fenêtre. `nodes` : les pages de l'espace que le compte voit.
 */
export function useSearchWindow(space: SpaceInfo | null, nodes: PageNode[] | null): SearchWindow {
  const spaceId = space?.id ?? null;
  const [opening, setOpening] = useState<Opening | null>(null);
  const [typed, setTyped] = useState({ spaceId, query: "" });
  // Un autre espace : le champ est vide, et la fenêtre, s'il n'y a plus d'espace, est fermée.
  if (typed.spaceId !== spaceId) setTyped({ spaceId, query: "" });
  if (opening !== null && space === null) setOpening(null);
  const query = typed.spaceId === spaceId ? typed.query : "";

  const open = useCallback(() => {
    // Dans le geste, avant tout rendu : le focus est encore où la personne l'avait.
    const before = takeBefore();
    setOpening((already) => already ?? { before, caret: { current: null } });
  }, []);
  const close = useCallback(() => setOpening(null), []);
  const preload = useCallback(() => void loadSearch().catch(() => {}), []);
  const onQuery = useCallback((next: string) => setTyped({ spaceId, query: next }), [spaceId]);
  useSearchShortcut(space === null ? null : open);

  const window =
    opening !== null && space !== null ? (
      <SearchBoundary before={opening.before} onClose={close}>
        <Suspense
          fallback={
            <SearchShell
              query={query}
              onQuery={onQuery}
              onClose={close}
              before={opening.before}
              caret={opening.caret}
            />
          }
        >
          <SearchDialog
            // Une fenêtre par espace.
            key={space.id}
            space={space}
            nodes={nodes}
            query={query}
            onQuery={onQuery}
            onClose={close}
            before={opening.before}
            caret={opening.caret}
          />
        </Suspense>
      </SearchBoundary>
    ) : null;
  return { open, preload, window };
}

/** `cancel` (Échap) d'une fenêtre, et d'elle seule : ceux d'une autre remontent dans l'arbre de React. */
function cancelling(onClose: () => void) {
  return (event: SyntheticEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    onClose();
  };
}

/**
 * La fenêtre de recherche le temps que son code arrive : une fenêtre modale et son champ, dans le
 * dessin de la fenêtre entière, rien d'autre. Elle laisse dans `caret` la place de son curseur en
 * partant, et rend le focus si c'est elle qu'on ferme.
 */
function SearchShell({
  query,
  onQuery,
  onClose,
  before,
  caret,
}: {
  query: string;
  onQuery: (query: string) => void;
  onClose: () => void;
  before: Before;
  caret: { current: Caret | null };
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);
  // Dans le rendu du geste : aucune image ne passe avec le focus resté dans la page.
  useLayoutEffect(() => {
    const dialog = ref.current;
    const field = fieldRef.current;
    if (dialog && !dialog.open) dialog.showModal();
    field?.focus();
    field?.select();
    return () => {
      if (field) caret.current = { start: field.selectionStart ?? 0, end: field.selectionEnd ?? 0 };
    };
  }, [caret]);
  // Remplacée par la fenêtre entière, elle ne rend rien : le focus est déjà dans le champ neuf.
  useEffect(() => () => giveBack(before), [before]);
  return (
    <dialog
      ref={ref}
      className="mm-dialog mm-search"
      aria-label="Rechercher"
      aria-busy="true"
      onKeyDown={(event) => {
        // Ctrl+K de nouveau : la touche est prise (le navigateur en ferait autre chose).
        if (isSearchKey(event)) event.preventDefault();
      }}
      onCancel={cancelling(onClose)}
      onClose={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="mm-search-head">
        <SearchIcon />
        <input
          ref={fieldRef}
          className="mm-search-field"
          type="text"
          aria-label="Rechercher dans l'espace"
          autoComplete="off"
          spellCheck={false}
          maxLength={MAX_QUERY_LENGTH}
          placeholder="Un titre, ou quelques mots d'une page"
          value={query}
          onChange={(event) => onQuery(event.target.value)}
        />
      </div>
    </dialog>
  );
}

/**
 * Code de la fenêtre introuvable (réseau coupé, application mise à jour depuis) ou fenêtre en
 * erreur : un message, au lieu de rien, et l'application reste en place.
 */
class SearchBoundary extends Component<
  { before: Before; onClose: () => void; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: unknown): void {
    console.error("Mementum : la fenêtre de recherche n'a pas pu s'afficher.", error);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return <SearchFailed before={this.props.before} onClose={this.props.onClose} />;
  }
}

function SearchFailed({ before, onClose }: { before: Before; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  // Elle prend le focus dans le rendu où elle remplace la coquille, et le rend en partant.
  useLayoutEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    closeRef.current?.focus();
  }, []);
  useEffect(() => () => giveBack(before), [before]);
  return (
    <dialog
      ref={ref}
      role="alertdialog"
      className="mm-dialog"
      aria-label="Rechercher"
      onCancel={cancelling(onClose)}
      onClose={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <h2 className="mm-dialog-title">Rechercher</h2>
      <p className="mm-dialog-lead">La recherche n'a pas pu s'afficher. Recharge la page pour réessayer.</p>
      <div className="mm-dialog-actions">
        <button ref={closeRef} type="button" className="mm-button" onClick={onClose}>
          Fermer
        </button>
        <button type="button" className="mm-button mm-button-primary" onClick={() => location.reload()}>
          Recharger
        </button>
      </div>
    </dialog>
  );
}
