// Un écran de l'application qui ne montre qu'un message, à la place d'une page : rien n'est
// ouvert, la page n'existe pas, une lecture a échoué. Un titre, une ou deux phrases, et au
// besoin un bouton (`Retry`, ou un autre, dans un `mm-message-actions`).
import type { ReactNode } from "react";

type Props = {
  title: string;
  /** Un bandeau, au-dessus du titre (lecture seule). */
  banner?: ReactNode;
  children?: ReactNode;
};

export function Message({ title, banner, children }: Props) {
  return (
    <section className="mm-message">
      {banner}
      <h1>{title}</h1>
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
