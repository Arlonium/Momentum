// « Commentaires », dans la barre du haut de la page : ouvre et ferme le panneau, et dit combien
// de fils sont ouverts (en toutes lettres pour les lecteurs d'écran : un nom qui se lit tel quel,
// par `aria-label`, qui commence par ce qui se lit sur le bouton).
import type { RefObject } from "react";
import { CommentIcon } from "../icons";

type Props = {
  buttonRef: RefObject<HTMLButtonElement | null>;
  /** Les fils ouverts ; `null` : pas encore lus. */
  count: number | null;
  expanded: boolean;
  panelId: string;
  onClick: () => void;
};

/** Le nombre de fils ouverts, en toutes lettres. */
export function openThreadsText(count: number): string {
  if (count === 0) return "aucun fil ouvert";
  return count === 1 ? "1 fil ouvert" : `${count} fils ouverts`;
}

export function CommentsButton({ buttonRef, count, expanded, panelId, onClick }: Props) {
  return (
    <button
      ref={buttonRef}
      type="button"
      className="mm-button mm-comments-button"
      aria-label={count === null ? undefined : `Commentaires, ${openThreadsText(count)}`}
      aria-expanded={expanded}
      aria-controls={expanded ? panelId : undefined}
      onClick={onClick}
    >
      <CommentIcon />
      <span>Commentaires</span>
      {count !== null && count > 0 && <span className="mm-count">{count}</span>}
    </button>
  );
}
