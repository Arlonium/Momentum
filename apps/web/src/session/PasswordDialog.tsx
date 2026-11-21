// La fenêtre « Changer le mot de passe », ouverte depuis le menu du compte (dans le sélecteur
// d'espace). Même dessin que les autres fenêtres (`mm-dialog`).
//
// Changer de mot de passe révoque aussi tous les jetons d'API du compte (R1221) : la fenêtre le dit
// avant (décision L3), puis dit combien en sont partis, et l'écran des jetons, s'il est ouvert
// derrière, relit sa liste (`tokensChanged.ts`).
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { announceTokensChanged } from "../account/tokensChanged";
import { ApiError, changePassword, isSessionLost } from "../api";
import { Field, FormError, messageOf } from "./forms";

/** Ce que la fenêtre dit des jetons révoqués ; `null` : la réponse n'a pas dit combien (R1375). */
function revokedTokens(count: number | null): string {
  if (count === null) return " Tes jetons d'API, s'il y en avait, sont révoqués.";
  if (count === 0) return "";
  return count === 1
    ? " 1 jeton d'API révoqué : l'agent qui s'en servait en demandera un neuf."
    : ` ${count} jetons d'API révoqués : les agents qui s'en servaient en demanderont un neuf.`;
}

/**
 * Changer son mot de passe : le serveur ferme les autres sessions du compte, et celle-ci
 * repart avec un cookie neuf. Un mot de passe actuel faux répond `401 invalid_credentials` :
 * son message s'affiche, la session est toujours là.
 */
export function PasswordDialog({ email, onClose }: { email: string; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** Le changement est fait ; `revoked`, les jetons d'API qu'il a révoqués (`null` : inconnu). */
  const [done, setDone] = useState<{ revoked: number | null } | null>(null);
  const currentRef = useRef<HTMLInputElement>(null);
  const nextRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const changed = await changePassword({ current, new: next });
      const revoked = changed === null ? null : changed.tokensRevoked;
      setDone({ revoked });
      if (revoked !== 0) announceTokensChanged();
    } catch (e) {
      // Session perdue : la connexion est demandée par-dessus ; les champs restent remplis.
      if (!isSessionLost(e)) {
        setError(messageOf(e));
        // Le champ à reprendre : le nouveau mot de passe s'il est refusé, sinon l'actuel.
        const field = e instanceof ApiError && e.code === "invalid_password" ? nextRef : currentRef;
        field.current?.focus();
        field.current?.select();
      }
    } finally {
      setBusy(false);
    }
  };

  const close = () => ref.current?.close();

  return (
    <dialog ref={ref} className="mm-dialog" aria-labelledby={titleId} onClose={onClose}>
      <h2 id={titleId} className="mm-dialog-title">
        Changer le mot de passe
      </h2>
      {done !== null ? (
        <>
          <p className="mm-dialog-lead">
            Mot de passe changé. Les autres sessions de ce compte sont fermées : sur tes autres
            appareils, il faudra te reconnecter.{revokedTokens(done.revoked)}
          </p>
          <div className="mm-dialog-actions">
            <button type="button" className="mm-button mm-button-primary" autoFocus onClick={close}>
              Fermer
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="mm-dialog-lead">
            Sur tes autres appareils, il faudra te reconnecter avec le nouveau. Tes jetons d'API seront
            tous révoqués : chaque agent qui s'en sert en demandera un neuf.
          </p>
          <form className="mm-auth-form" onSubmit={submit} noValidate>
            {/* Pour les gestionnaires de mots de passe : le compte dont on change le mot de passe. */}
            <input type="email" name="username" autoComplete="username" value={email} readOnly hidden />
            <Field
              ref={currentRef}
              label="Mot de passe actuel"
              type="password"
              name="current-password"
              autoComplete="current-password"
              autoFocus
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
            />
            <Field
              ref={nextRef}
              label="Nouveau mot de passe"
              type="password"
              name="new-password"
              autoComplete="new-password"
              hint="10 caractères au moins."
              value={next}
              onChange={(e) => setNext(e.target.value)}
            />
            <FormError message={error} />
            <div className="mm-dialog-actions">
              <button type="button" className="mm-button" onClick={close}>
                Annuler
              </button>
              <button type="submit" className="mm-button mm-button-primary" disabled={busy}>
                Changer le mot de passe
              </button>
            </div>
          </form>
        </>
      )}
    </dialog>
  );
}
