// Bandeaux d'une page en lecture seule, qui disent pourquoi.
//
// `ReadOnlyBanner` : l'onglet est resté ouvert pendant une mise à jour du serveur. Son schéma
// de blocs n'est plus celui du serveur, donc plus rien ne s'écrit d'ici (Review Focus 2) ;
// recharger la page prend la nouvelle version.
//
// `ReaderBanner` : le niveau du compte sur la page permet de la lire, pas de la modifier. Si ce
// niveau monte pendant que la page est ouverte, elle reste en lecture seule (rien ne remplace
// l'éditeur sous les yeux de qui lit) ; le bandeau le dit, et son bouton la rouvre.
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

/**
 * `BlockedBanner` : le serveur a refusé plusieurs fois de suite ce que cet onglet lui envoie,
 * et n'enregistre plus rien du compte sur cette page pendant quelques minutes (R725, R788).
 * L'onglet garde ce qui a été refusé et le renverrait à chaque retour : seul un rechargement
 * le remet d'accord avec le serveur, et ce qui a été tapé depuis le refus n'y sera plus. En
 * alerte : c'est une perte, si la personne ferme l'onglet sans copier.
 */
export const BLOCKED_MESSAGE =
  "Tes dernières modifications sur cette page ne s'enregistrent plus. Copie ce que tu viens d'écrire, puis recharge la page.";
export const BLOCKED_HINT = "Si ce message revient, attends quelques minutes avant d'écrire ici.";

export function BlockedBanner() {
  return (
    <div className="mm-banner mm-banner-blocked" role="alert">
      <InfoIcon />
      <div className="mm-banner-text">
        <p>{BLOCKED_MESSAGE}</p>
        <p>{BLOCKED_HINT}</p>
      </div>
      <button type="button" className="mm-button" onClick={() => location.reload()}>
        Recharger
      </button>
    </div>
  );
}

type ReaderProps = {
  /**
   * Le niveau est remonté depuis l'ouverture de la page : rouvre la page, qui se modifiera.
   * `null` : il ne permet toujours que de lire.
   */
  reopen: (() => void) | null;
};

export function ReaderBanner({ reopen }: ReaderProps) {
  return (
    <div className="mm-banner mm-banner-reader" role="status">
      <InfoIcon />
      <p>
        {reopen
          ? "Tu peux maintenant modifier cette page : rouvre-la."
          : "Tu peux lire cette page, pas la modifier."}
      </p>
      {reopen && (
        <button type="button" className="mm-button" onClick={reopen}>
          Rouvrir la page
        </button>
      )}
    </div>
  );
}
