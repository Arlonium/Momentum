// Un écran de l'application qui ne montre qu'un message, à la place d'une page : rien n'est
// ouvert, la page n'existe pas, une lecture a échoué. Un titre, une ou deux phrases, et au
// besoin un bouton (`Retry`, ou un autre, dans un `mm-message-actions`).
//
// Son titre est l'endroit où va le focus quand cet écran a pris la place de la page sous une
// fenêtre qui se ferme, et que ce qui avait le focus est parti avec la page (`ui/landing.ts`, lu par
// `giveBack`, recette du lot 4, E5) : focalisable par le script seulement. Pas celui d'un écran
// de passage (« Chargement… », `transient`), qui partirait avec le focus.
import { useRef, type ReactNode } from "react";
import { LANDING, useLanding } from "./landing";

type Props = {
  title: string;
  /** Un bandeau, au-dessus du titre (lecture seule). */
  banner?: ReactNode;
  /** Un écran de passage (« Chargement… ») : son titre ne prend pas le focus. */
  transient?: boolean;
  children?: ReactNode;
};

export function Message({ title, banner, transient = false, children }: Props) {
  const heading = useRef<HTMLHeadingElement>(null);
  useLanding(heading, !transient);
  return (
    <section className="mm-message">
      {banner}
      <h1 ref={heading} {...(transient ? {} : { [LANDING]: "", tabIndex: -1 })}>
        {title}
      </h1>
      {children && <p>{children}</p>}
    </section>
  );
}

/** « Réessayer », sous le message d'une lecture qui a échoué : elle se refait sans recharger. */
export function Retry({ onRetry }: { onRetry: () => void }) {
  return (
    <span className="mm-message-actions">
      <button type="button" className="mm-button mm-button-primary" onClick={onRetry}>
        Réessayer
      </button>
    </span>
  );
}
