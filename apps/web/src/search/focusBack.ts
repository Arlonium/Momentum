// Ce qui avait le focus avant la fenêtre de recherche, et sa sélection (R1088, R1089, R1095) : relevés
// à la demande d'ouverture (`useSearchWindow.open`, dans le geste même : Ctrl+K ou le clic sur
// « Rechercher »), et rendus quand la fenêtre part, quelle qu'elle soit (la coquille, la fenêtre
// entière, celle qui dit que son code n'a pas pu être chargé).
//
// La sélection : ouverte depuis le texte d'une page, le navigateur la retire quand le focus passe au
// champ, et un `focus()` ne la rend pas (il pose le curseur au début du texte : la frappe suivante
// irait en tête de la page). Une plage du DOM suit ce qui s'écrit autour d'elle pendant que la
// fenêtre est ouverte.
//
// Où il va quand ce qui l'avait n'est plus là (R1096, recette du lot 4, E5) : au titre de l'écran
// qui a pris la place de la page sous la fenêtre (`LANDING` : « Tu n'as plus accès à cette page »,
// « Page introuvable », « Espace introuvable »…), qui dit ce qui s'est passé ; à défaut à l'entrée
// « Rechercher » de la barre latérale. Un invité qui perd son seul accès sort de l'espace : la
// fenêtre se ferme, l'entrée part avec l'espace, et le focus tombait sur `body`. Une page ouverte
// par la fenêtre, elle, prend le focus à son arrivée (le bloc trouvé, ou le fil d'Ariane : `open`
// de `SearchDialog`), après ce repli.
import { LANDING } from "../ui/Message";

/** Ce qui avait le focus (`null` : rien, ou `body`), et la sélection du document à cet instant. */
export type Before = { opener: HTMLElement | null; range: Range | null };

/** Relève ce qui a le focus et la sélection du document : avant que la fenêtre s'ouvre. */
export function takeBefore(): Before {
  const active = document.activeElement;
  const opener = active instanceof HTMLElement && active !== document.body ? active : null;
  const selected = document.getSelection();
  const range =
    opener !== null && selected !== null && selected.rangeCount > 0
      ? selected.getRangeAt(0).cloneRange()
      : null;
  return { opener, range };
}

/**
 * Rend le focus : à ce qui l'avait, avec sa sélection si le texte visé y est encore ; sinon (parti
 * avec la page d'avant, ou rien ne l'avait) au titre de l'écran qui a remplacé la page (`LANDING`),
 * à défaut à l'entrée « Rechercher » de la barre latérale (`data-search-entry`). **Seulement s'il
 * est tombé sur `body`** : ce qui l'a pris entre-temps (la fenêtre qui remplace la coquille, un bloc
 * où l'on arrive) le garde. À appeler du nettoyage d'un effet passif, après les retraits du DOM :
 * `isConnected` y dit vrai de ce que le même rendu a retiré.
 */
export function giveBack({ opener, range }: Before): void {
  const now = document.activeElement;
  if (now !== null && now !== document.body) return;
  const back = opener?.isConnected
    ? opener
    : (document.querySelector<HTMLElement>(`[${LANDING}]`) ??
      document.querySelector<HTMLElement>("[data-search-entry]"));
  back?.focus({ preventScroll: true });
  if (back === null || back !== opener || range === null) return;
  const { startContainer, endContainer } = range;
  if (startContainer === opener || !opener.contains(startContainer) || !opener.contains(endContainer)) return;
  const selection = document.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
}

/** Où était le curseur d'un champ : `setSelectionRange` le remet. */
export type Caret = { start: number; end: number };
