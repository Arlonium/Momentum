// Connexion : adresse e-mail et mot de passe, validés par Entrée. En page, sans session
// (`LoginPage`) ; ou par-dessus l'application quand la session a pris fin en cours de route
// (`LoginDialog`), sans la démonter. Les messages d'erreur sont ceux du serveur, tels quels.
//
// Ce fichier porte aussi ce que partagent les écrans de session (inscription, mot de passe) :
// la mise en page (`AuthScreen`), les champs (`Field`) et le message d'erreur (`FormError`).
import type { Me } from "@mementum/protocol";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type FormEvent,
  type InputHTMLAttributes,
  type MouseEvent,
  type ReactNode,
} from "react";
import { ApiError, logIn } from "../api";

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

type FieldProps = { label: string; hint?: string } & InputHTMLAttributes<HTMLInputElement>;

/** Un champ et son vrai `label` ; `hint` en dessous, lié au champ. */
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

/** Le formulaire de connexion. `email` : l'adresse à préremplir. */
function LoginForm({ email: initialEmail = "", onLoggedIn }: { email?: string; onLoggedIn: (me: Me) => void }) {
  const [email, setEmail] = useState(initialEmail);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      // L'écran disparaît avec la session ouverte : rien à remettre à zéro.
      onLoggedIn(await logIn({ email, password }));
    } catch (e) {
      setError(messageOf(e));
      setBusy(false);
    }
  };

  // Sans validation du navigateur : ses bulles ne sont ni en français ni à nos couleurs, et
  // le serveur dit ce qui ne va pas.
  return (
    <form className="mm-auth-form" onSubmit={submit} noValidate>
      <Field
        label="Adresse e-mail"
        type="email"
        name="email"
        autoComplete="email"
        autoFocus={!initialEmail}
        value={email}
        onChange={(e) => setEmail(e.target.value)}
      />
      <Field
        label="Mot de passe"
        type="password"
        name="password"
        autoComplete="current-password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
      />
      <FormError message={error} />
      <button type="submit" className="mm-button mm-button-primary mm-auth-submit" disabled={busy}>
        {busy ? "Connexion…" : "Se connecter"}
      </button>
    </form>
  );
}

type LoginPageProps = {
  /** Inscriptions ouvertes (`info.signup === "open"`) : le lien vers l'inscription. */
  signupOpen: boolean;
  onSignup: () => void;
  onLoggedIn: (me: Me) => void;
};

/** La connexion, en page : sans session. */
export function LoginPage({ signupOpen, onSignup, onLoggedIn }: LoginPageProps) {
  useDocumentTitle("Connexion");
  return (
    <AuthScreen
      title="Se connecter"
      lead="Retrouve tes pages et les espaces où l'on t'a invité."
      footer={
        signupOpen ? (
          <>
            Pas encore de compte ?{" "}
            <ScreenLink href="/signup" onClick={onSignup}>
              Créer un compte
            </ScreenLink>
          </>
        ) : (
          "Pas encore de compte ? Sur cette instance, on s'inscrit avec le lien d'une invitation."
        )
      }
    >
      <LoginForm onLoggedIn={onLoggedIn} />
    </AuthScreen>
  );
}

/**
 * La connexion par-dessus l'application, quand un appel a répondu `401 unauthenticated` :
 * l'application reste montée derrière (la page ouverte, ce qui y a été tapé), mais inerte
 * tant que la session n'est pas rouverte. On ne peut que se reconnecter : Échap ne ferme
 * pas cette fenêtre.
 */
export function LoginDialog({ email, onLoggedIn }: { email: string; onLoggedIn: (me: Me) => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.open) return;
    dialog.showModal();
    // `showModal` donne le focus au premier champ, l'adresse, déjà remplie (`autoFocus` de
    // React a agi avant, au montage) : le mot de passe le reprend.
    dialog.querySelector<HTMLInputElement>('input[type="password"]')?.focus();
  }, []);

  return (
    <dialog
      ref={ref}
      className="mm-dialog mm-relogin"
      aria-labelledby={titleId}
      onCancel={(event) => event.preventDefault()}
      // Chrome ferme quand même la fenêtre au second Échap sans geste entre les deux : elle
      // se rouvre, la session n'étant pas revenue.
      onClose={() => ref.current?.showModal()}
    >
      <h1 id={titleId} className="mm-auth-title">
        Ta session a pris fin
      </h1>
      <p className="mm-auth-lead">
        Reconnecte-toi pour continuer. La page reste ouverte, avec ce que tu y as écrit.
      </p>
      <LoginForm email={email} onLoggedIn={onLoggedIn} />
    </dialog>
  );
}
