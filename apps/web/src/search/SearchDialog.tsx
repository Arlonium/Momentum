// La fenêtre de recherche d'un espace (lot 4, Task C1 ; décisions L2, L5, L7). Chargée à la demande
// (`loadSearch.ts`) : rien d'elle sur le chemin de l'ouverture de l'application ni d'une page.
//
// Ce qu'elle montre, une ligne par page (`rows.ts`) :
// - champ vide : les pages récentes du compte dans l'espace, celles de la barre latérale ;
// - dès la frappe : les pages de l'arbre dont le titre contient ce qui est tapé (`findPages`), tout
//   de suite et sans réseau ; puis, sous elles, les pages que le serveur trouve par leur titre ou
//   leur texte (`useSearch`), avec leur fil d'Ariane et un extrait aux mots surlignés.
//
// Rien ne bouge sous le pointeur (R296) : les lignes de l'arbre sont prises une fois par recherche
// (elles ne suivent pas l'arbre tant que la fenêtre montre cette recherche), celles du serveur ne
// font que s'ajouter dessous, ce qui se dit (recherche en cours, aucun résultat, indexation, refus)
// se dit sous la liste, et la fenêtre est tenue par son bord haut : elle grandit vers le bas. La
// ligne active est tenue par l'identifiant de sa page, jamais par son rang (R572) : fixée dès
// qu'une ligne est montrée, elle ne change qu'aux flèches, ou quand ce qui est tapé change.
//
// Un combobox : le champ garde le focus, les flèches choisissent la ligne active
// (`aria-activedescendant`), Entrée l'ouvre, Ctrl+Entrée (Cmd+Entrée) l'ouvre dans un nouvel onglet
// sans fermer, Échap ferme. Chaque ligne est un vrai lien (`<a href>`) : un clic simple ouvre la
// page dans l'onglet, un clic avec Ctrl, Cmd, Maj ou la molette reste au navigateur. Le focus ne
// tombe jamais sur `body` : fermée, la fenêtre le rend à ce qui l'avait, sinon à l'entrée
// « Rechercher » de la barre latérale (`data-search-entry`).
//
// Échap ne détruit pas ce qui est tapé : le texte du champ est tenu par qui rend la fenêtre
// (`query`, `onQuery`), qui le lui rend, sélectionné, quand elle se rouvre ; il n'est vidé que
// quand une page est ouverte.
//
// Les titres, les fils d'Ariane et les extraits viennent d'autres comptes : toujours des nœuds de
// texte, isolés (`dir="auto"`, `unicode-bidi: isolate`), jamais du HTML ni un `style`. Les mots
// surlignés sont des `<mark>` posés autour de nœuds de texte (`marks.ts`).
import type { PageNode, SpaceInfo } from "@mementum/protocol";
import {
  useEffect,
  useLayoutEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type SyntheticEvent,
} from "react";
import { PageIcon, SearchIcon } from "../icons";
import { navigate, pagePath } from "../router";
import { useSession } from "../session/session";
import { useAfterLogin } from "../session/useAfterLogin";
import { findPages } from "../space/pageDirectory";
import { useRecentPages } from "../space/recents";
import { markedParts } from "./marks";
import { treeRows, withHits, type Row } from "./rows";
import { giveBack, takeBefore, type Before, type Caret } from "./focusBack";
import { isSearchKey, MAX_QUERY_LENGTH, MOD } from "./shortcut";
import { useSearch } from "./useSearch";

/** Les pages de l'arbre montrées pour ce qui est tapé, au plus. */
export const MAX_TREE_ROWS = 8;

type Props = {
  /** L'espace où l'on cherche : celui qui est affiché (décision L2). */
  space: SpaceInfo;
  /** Les pages de l'espace que le compte voit ; `null` : pas encore lues. */
  nodes: PageNode[] | null;
  /** Ce qui est tapé dans le champ, tenu par qui rend la fenêtre. */
  query: string;
  onQuery: (query: string) => void;
  /** La fenêtre est à retirer : c'est qui la rend qui le fait. */
  onClose: () => void;
  /**
   * Ce qui avait le focus quand la recherche a été demandée, et sa sélection, relevés par l'hôte
   * (`useSearchWindow.open`) ; absent : la fenêtre les relève elle-même en s'ouvrant.
   */
  before?: Before;
  /**
   * Où était le curseur du champ de la coquille que la fenêtre remplace (`current` nul : elle n'en
   * remplace aucune, et sélectionne ce qui était resté dans le champ).
   */
  caret?: { readonly current: Caret | null };
};

/**
 * L'adresse qu'ouvre une ligne. La Task C2 y ajoutera le bloc trouvé (`row.blockId`, échappé, en
 * fragment) : c'est ici, et nulle part ailleurs, qu'une ligne devient une adresse.
 */
export function hrefOf(row: Row): string {
  return pagePath(row.id);
}

/** L'extrait d'une ligne : des nœuds de texte, les mots trouvés dans des `<mark>`. */
function Excerpt({ row }: { row: Row }) {
  const parts = useMemo(() => markedParts(row.excerpt, row.marks), [row.excerpt, row.marks]);
  return (
    <span className="mm-search-excerpt" dir="auto">
      {parts.map((part, i) =>
        part.marked ? (
          <mark key={i} className="mm-search-mark">
            {part.text}
          </mark>
        ) : (
          part.text
        ),
      )}
    </span>
  );
}

export function SearchDialog({ space, nodes, query, onQuery, onClose, before, caret }: Props) {
  const { me } = useSession();
  const afterLogin = useAfterLogin(me);
  const id = useId();
  const ref = useRef<HTMLDialogElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);
  const wanted = query.trim();
  const search = useSearch(space.id, query, { afterLogin });

  // Les lignes de l'arbre, prises une fois par recherche : elles ne suivent pas l'arbre (une page
  // renommée ou arrivée ailleurs ne déplace rien sous le pointeur). Champ vide : les pages récentes,
  // prises dès qu'il y en a (l'arbre a pu ne pas être lu à l'ouverture).
  const recents = useRecentPages(me.user.id, space.id, nodes);
  const taken = (): Row[] =>
    treeRows(wanted === "" ? recents : findPages(wanted, MAX_TREE_ROWS), nodes);
  const [held, setHeld] = useState(() => ({ query: wanted, rows: taken() }));
  let tree = held;
  if (held.query !== wanted || (wanted === "" && held.rows.length === 0 && recents.length > 0)) {
    tree = { query: wanted, rows: taken() };
    setHeld(tree);
  }
  // Puis celles du serveur, dessous, dans son ordre ; une page déjà montrée garde sa ligne.
  const rows = useMemo(() => withHits(tree.rows, search.hits), [tree.rows, search.hits]);

  // La ligne active, par l'identifiant de sa page. Fixée dès qu'une ligne est montrée : la première ;
  // ce qui est tapé change, la première de la liste neuve.
  const [chosen, setChosen] = useState<{ query: string; id: string | null }>({ query: wanted, id: null });
  const at = chosen.query === wanted ? rows.findIndex((row) => row.id === chosen.id) : -1;
  if (at === -1 && rows.length > 0) setChosen({ query: wanted, id: rows[0].id });
  const index = Math.max(0, at);
  const active: Row | undefined = rows[index];
  const optionId = (i: number) => `${id}-option-${i}`;

  // Ouverte : le focus entre dans le champ, ce qui y était resté est sélectionné (taper le
  // remplace). Retirée : le focus revient d'où il venait.
  // Ce qui avait le focus, et sa sélection (`focusBack.ts`) : l'hôte les relève à la demande
  // d'ouverture (`before`) ; sans lui, ils sont relevés ici, avant que la fenêtre s'ouvre.
  const noted = useRef<Before | null>(null);
  // Dans le rendu même (un effet de mise en page) : quand la fenêtre remplace sa coquille (le code
  // vient d'arriver), aucune image ne passe sans fenêtre ouverte, et aucune frappe sans champ. Le
  // curseur de la coquille est repris (`caret`) ; sinon ce qui était resté dans le champ est
  // sélectionné, et taper le remplace.
  useLayoutEffect(() => {
    noted.current ??= before ?? takeBefore();
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    const field = fieldRef.current;
    field?.focus();
    const at = caret?.current ?? null;
    if (at === null) field?.select();
    else field?.setSelectionRange(at.start, at.end);
  }, [before, caret]);
  // Retirée : le focus revient d'où il venait. Dans un effet passif, après les retraits du DOM.
  useEffect(
    () => () => {
      if (noted.current !== null) giveBack(noted.current);
    },
    [],
  );

  // Une flèche a changé la ligne active : elle est montrée, sans rien déplacer d'autre.
  const moved = useRef(false);
  useEffect(() => {
    if (!moved.current) return;
    moved.current = false;
    document.getElementById(optionId(index))?.scrollIntoView?.({ block: "nearest" });
  });

  // « Afficher plus de résultats » part avec la dernière réponse : s'il avait le focus, le champ le
  // reprend (React n'appelle pas `onBlur` pour un élément qu'il retire, R339).
  const moreFocused = useRef(false);
  useEffect(() => {
    if (search.more || !moreFocused.current) return;
    moreFocused.current = false;
    const now = document.activeElement;
    if (now === null || now === document.body) fieldRef.current?.focus();
  }, [search.more]);

  /** Ouvre la page d'une ligne dans l'onglet : la recherche est finie, le champ est vidé. */
  const open = (row: Row) => {
    navigate(hrefOf(row));
    onQuery("");
    onClose();
  };

  const onFieldKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (rows.length === 0) return;
      const next = event.key === "ArrowDown" ? Math.min(index + 1, rows.length - 1) : Math.max(index - 1, 0);
      moved.current = true;
      setChosen({ query: wanted, id: rows[next].id });
      // Entrée qui valide une composition (IME) appartient au champ, pas à la liste.
    } else if (event.key === "Enter" && !event.nativeEvent.isComposing) {
      event.preventDefault();
      if (active === undefined) return;
      // Dans un nouvel onglet, sans ouvreur ; la fenêtre reste, pour en ouvrir d'autres.
      if (event.ctrlKey || event.metaKey) window.open(hrefOf(active), "_blank", "noopener");
      else open(active);
    }
  };

  // Ctrl+K dans la fenêtre : rien à ouvrir, mais la touche est prise (le navigateur en ferait
  // autre chose), et le champ reprend la main.
  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (!isSearchKey(event)) return;
    event.preventDefault();
    fieldRef.current?.focus();
  };

  const onRowClick = (event: MouseEvent<HTMLAnchorElement>, row: Row) => {
    // Avec Ctrl, Cmd, Maj, Alt ou un autre bouton : au navigateur (nouvel onglet, nouvelle fenêtre).
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    // Le second clic d'un double clic a commencé ailleurs (« Afficher plus de résultats », dont la
    // réponse a mis cette ligne sous le pointeur) : il n'ouvre rien (R1094, la règle de `firstClick`).
    if (event.detail > 1) return;
    open(row);
  };

  /** L'événement de souris vise-t-il le voile : la fenêtre elle-même, hors de son cadre ? */
  const onVeil = (event: MouseEvent<HTMLDialogElement>): boolean => {
    if (event.target !== event.currentTarget) return false;
    const box = event.currentTarget.getBoundingClientRect();
    return (
      event.clientX < box.left || event.clientX > box.right || event.clientY < box.top || event.clientY > box.bottom
    );
  };
  // Un clic sur le voile la ferme, s'il y a commencé aussi : une sélection à la souris commencée
  // dans le champ et relâchée hors du cadre arrive comme un clic sur la fenêtre (R1097). Jamais le
  // second clic d'un double clic : celui d'un double clic sur « Rechercher » tombe sur le voile de la
  // fenêtre que le premier vient d'ouvrir (R1094).
  const pressedOnVeil = useRef(false);
  const onMouseDown = (event: MouseEvent<HTMLDialogElement>) => {
    pressedOnVeil.current = onVeil(event);
  };
  const onClick = (event: MouseEvent<HTMLDialogElement>) => {
    const pressed = pressedOnVeil.current;
    pressedOnVeil.current = false;
    if (pressed && event.detail <= 1 && onVeil(event)) onClose();
  };

  // `cancel` (Échap) et `close` remontent dans l'arbre de React : seuls ceux de cette fenêtre la
  // concernent, jamais ceux d'une fenêtre ouverte par-dessus.
  const onCancel = (event: SyntheticEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    onClose();
  };

  const searching = wanted !== "";
  const said: string[] = [];
  if (searching && search.pending) said.push("Recherche…");
  else if (searching && search.error === null) {
    if (rows.length === 0) said.push("Aucun résultat.");
    if (search.indexing) said.push("L'indexation est en cours : des pages peuvent manquer.");
  }

  return (
    <dialog
      ref={ref}
      className="mm-dialog mm-search"
      aria-label="Rechercher"
      onKeyDown={onKeyDown}
      onMouseDown={onMouseDown}
      onClick={onClick}
      onCancel={onCancel}
      // Chrome la ferme quand même au second Échap sans geste entre les deux.
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
          role="combobox"
          aria-label="Rechercher dans l'espace"
          aria-expanded={rows.length > 0}
          aria-controls={`${id}-list`}
          aria-autocomplete="list"
          aria-activedescendant={active === undefined ? undefined : optionId(index)}
          autoComplete="off"
          spellCheck={false}
          maxLength={MAX_QUERY_LENGTH}
          enterKeyHint="go"
          placeholder="Un titre, ou quelques mots d'une page"
          value={query}
          onChange={(event) => onQuery(event.target.value)}
          onKeyDown={onFieldKeyDown}
        />
      </div>
      <div className="mm-search-results">
        {!searching && rows.length > 0 && <p className="mm-search-heading">Récents</p>}
        <ul
          id={`${id}-list`}
          className="mm-search-list"
          role="listbox"
          aria-label={searching ? "Résultats" : "Pages récentes"}
          hidden={rows.length === 0}
          // Le champ garde le focus : un clic ouvre une ligne sans le lui prendre.
          onMouseDown={(event) => event.preventDefault()}
        >
          {rows.map((row, i) => (
            <li key={row.id} role="presentation">
              <a
                id={optionId(i)}
                className="mm-search-row"
                role="option"
                aria-selected={i === index}
                href={hrefOf(row)}
                tabIndex={-1}
                data-search-page={row.id}
                data-from={row.from}
                onClick={(event) => onRowClick(event, row)}
              >
                <PageIcon />
                <span className="mm-search-text">
                  <span className="mm-search-line">
                    <span className="mm-search-title" dir="auto">
                      {row.title}
                    </span>
                    {row.path.length > 0 && (
                      <span className="mm-search-path">
                        {row.path.map((ancestor) => (
                          <span key={ancestor.id} className="mm-search-ancestor" dir="auto">
                            {ancestor.title}
                          </span>
                        ))}
                      </span>
                    )}
                  </span>
                  {row.excerpt !== "" && <Excerpt row={row} />}
                </span>
              </a>
            </li>
          ))}
        </ul>
        {said.map((text) => (
          <p key={text} className="mm-search-status" role="status">
            {text}
          </p>
        ))}
        {searching && search.error !== null && (
          <p className="mm-search-status mm-search-error" role="alert">
            {search.error}
          </p>
        )}
      </div>
      {search.more && (
        // Hors de la zone qui défile : les lignes de la suite s'ajoutent dans la liste, au-dessus, sans
        // que le bouton descende sous le pointeur (R1094).
        <div className="mm-search-foot">
          <button
            type="button"
            className="mm-button mm-button-small mm-search-more"
            // Grisé sans être retiré pendant la demande : il garde le focus.
            aria-disabled={search.loadingMore || undefined}
            onFocus={() => {
              moreFocused.current = true;
            }}
            onBlur={() => {
              moreFocused.current = false;
            }}
            onClick={() => {
              if (!search.loadingMore) search.loadMore();
            }}
          >
            Afficher plus de résultats
          </button>
        </div>
      )}
      <p className="mm-search-keys" aria-hidden="true">
        <span>
          <kbd>↑</kbd> <kbd>↓</kbd> choisir
        </span>
        <span>
          <kbd>Entrée</kbd> ouvrir
        </span>
        <span>
          <kbd>{MOD}</kbd> <kbd>Entrée</kbd> nouvel onglet
        </span>
        <span>
          <kbd>Échap</kbd> fermer
        </span>
      </p>
    </dialog>
  );
}
