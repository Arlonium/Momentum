// Bandeau d'un onglet resté ouvert pendant une mise à jour du serveur : son schéma de blocs
// n'est plus celui du serveur, donc l'éditeur est en lecture seule (Review Focus 2).
import { InfoIcon } from "../icons";

export const READ_ONLY_MESSAGE =
  "Une nouvelle version de Mementum est disponible. Recharge la page pour continuer à écrire.";

export function ReadOnlyBanner() {
  return (
    <div className="mm-banner" role="status">
      <InfoIcon />
      <p>{READ_ONLY_MESSAGE}</p>
      <button type="button" className="mm-button" onClick={() => location.reload()}>
        Recharger
      </button>
    </div>
  );
}
