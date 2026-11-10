// Le lien d'une invitation tout juste créée, à copier tout de suite : le serveur ne garde
// que le hachage du jeton, le lien ne sera plus jamais montré. Réglages de l'espace, et
// bientôt le menu Partager d'une page.
//
// Chaque lien a son propre cadre (`key`) : une nouvelle invitation repart de « Copier le
// lien », jamais de « Lien copié », qui dirait copié un lien qui ne l'est pas (le
// presse-papiers aurait encore celui d'avant).
import { useRef, useState } from "react";
import { CheckIcon, LinkIcon, WarningIcon } from "../icons";

type Props = {
  /** L'adresse pour laquelle le lien a été préparé (jamais vérifiée : le lien fait foi). */
  email: string;
  /** Ce que donne l'invitation, en mots (« Membre », « Peut modifier »). */
  grant: string;
  link: string;
};

export function CreatedInvitation(props: Props) {
  return <LinkPanel key={props.link} {...props} />;
}

function LinkPanel({ email, grant, link }: Props) {
  const linkRef = useRef<HTMLInputElement>(null);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState<string | null>(null);

  const copy = async () => {
    setCopyError(null);
    try {
      // Absent hors d'un contexte sécurisé (instance servie en `http` sur le réseau local).
      await navigator.clipboard.writeText(link);
      setCopied(true);
    } catch {
      linkRef.current?.focus();
      linkRef.current?.select();
      setCopyError("Le navigateur a refusé la copie : le lien est sélectionné, copie-le avec Ctrl+C.");
    }
  };

  return (
    <div className="mm-invite-created">
      <p className="mm-invite-warning">
        <WarningIcon />
        <strong>Ce lien ne sera plus affiché : copie-le maintenant.</strong>
      </p>
      <p className="mm-invite-for">
        Lien d'invitation préparé pour {email} · {grant}
      </p>
      <div className="mm-settings-row">
        <input
          ref={linkRef}
          className="mm-input mm-invite-link"
          aria-label="Lien d'invitation"
          readOnly
          value={link}
          onFocus={(e) => e.currentTarget.select()}
        />
        <button
          type="button"
          className="mm-button mm-button-primary mm-button-icon"
          onClick={() => void copy()}
        >
          {copied ? <CheckIcon /> : <LinkIcon />}
          {copied ? "Lien copié" : "Copier le lien"}
        </button>
      </div>
      <p className="mm-visually-hidden" role="status">
        {copied ? "Lien copié dans le presse-papiers." : ""}
      </p>
      {copyError && <p className="mm-invite-copy-error">{copyError}</p>}
    </div>
  );
}
