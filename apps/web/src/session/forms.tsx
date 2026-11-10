// Ce que partagent les écrans et les fenêtres de session (connexion, inscription, mot de
// passe, invitation) : la mise en page en écran (`AuthScreen`), les champs (`Field`), le
// message d'erreur du serveur (`FormError`), les liens internes (`ScreenLink`), le titre de
// l'onglet et le message d'une erreur d'appel.
import {
  useEffect,
  useId,
  type ComponentPropsWithRef,
  type MouseEvent,
  type ReactNode,
} from "react";
import { ApiError } from "../api";

/** Le message d'une erreur d'appel : celui du serveur quand il répond. */
export function messageOf(error: unknown): string {
  return error instanceof ApiError ? error.message : "Une erreur inattendue est survenue.";
}

/** Le titre de l'onglet, le temps que l'écran est affiché. */
export function useDocumentTitle(title: string): void {
  useEffect(() => {
    document.title = `${title} · Mementum`;
  }, [title]);
}

/** Un lien interne qui appelle `onClick` au lieu de suivre son adresse (clic simple). */
export function ScreenLink({
  href,
  onClick,
  children,
}: {
  href: string;
  onClick: () => void;
  children: ReactNode;
}) {
  const follow = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    event.preventDefault();
    onClick();
  };
  return (
    <a href={href} onClick={follow}>
      {children}
    </a>
  );
}

/** Écran de session : la marque, un titre, une phrase, le formulaire, puis une ligne de pied. */
export function AuthScreen({
  title,
  lead,
  footer,
  children,
}: {
  title: string;
  lead?: ReactNode;
  footer?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <main className="mm-auth">
      <div className="mm-auth-column">
        <div className="mm-auth-brand">
          <span className="mm-brand-tile" aria-hidden="true">
            M
          </span>
          <span>Mementum</span>
        </div>
        <h1 className="mm-auth-title">{title}</h1>
        {lead && <p className="mm-auth-lead">{lead}</p>}
        {children}
        {footer && <p className="mm-auth-footer">{footer}</p>}
      </div>
    </main>
  );
}

type FieldProps = { label: string; hint?: string } & ComponentPropsWithRef<"input">;

/** Un champ et son vrai `label` ; `hint` en dessous, lié au champ. `ref` va au champ. */
export function Field({ label, hint, ...input }: FieldProps) {
  const id = useId();
  return (
    <div className="mm-field">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        className="mm-input"
        aria-describedby={hint ? `${id}-hint` : undefined}
        {...input}
      />
      {hint && (
        <p id={`${id}-hint`} className="mm-field-hint">
          {hint}
        </p>
      )}
    </div>
  );
}

/** Le message d'erreur du serveur, tel quel (seule l'initiale passe en capitale, en CSS). */
export function FormError({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <p className="mm-form-error" role="alert">
      {message}
    </p>
  );
}
