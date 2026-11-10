// L'écran d'une adresse qui nomme un espace où le compte n'a pas de rôle (`/s/:spaceId`, sa
// corbeille, ses réglages), Ruling R96. Le même écran pour un espace qui n'existe pas et pour
// un espace où l'on n'est pas : rien ne dit lequel. L'adresse reste : un lien collé qui
// mènerait ailleurs sans un mot égarerait. Son bouton ouvre le premier espace du compte.
import type { SpaceInfo } from "@mementum/protocol";
import { navigate, spacePath } from "../router";
import { useDocumentTitle } from "../session/forms";

type Props = {
  /** Le premier espace du compte ; `null` : il n'en a aucun. */
  first: SpaceInfo | null;
};

export function SpaceNotFound({ first }: Props) {
  useDocumentTitle("Espace introuvable");
  return (
    <section className="mm-message">
      <h1>Espace introuvable</h1>
      <p>
        Cet espace n'existe pas, ou tu n'en fais pas partie.
        <span className="mm-message-actions">
          <button
            type="button"
            className="mm-button mm-button-primary"
            onClick={() => navigate(first === null ? "/" : spacePath(first.id))}
          >
            {first === null ? "Revenir à l'accueil" : `Ouvrir « ${first.name} »`}
          </button>
        </span>
      </p>
    </section>
  );
}
