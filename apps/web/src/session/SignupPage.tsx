// Inscription : nom, adresse e-mail et mot de passe, validés par Entrée. Le compte créé a sa
// session ouverte dans la foulée. Trois cas (`SessionGate`) : le premier compte de
// l'instance (`setup`), les inscriptions ouvertes, ou l'arrivée par une invitation (Task 13),
// qui préremplit l'adresse et envoie son jeton avec l'inscription.
import type { Me } from "@mementum/protocol";
import { useState, type FormEvent } from "react";
import { signUp } from "../api";
import { AuthScreen, Field, FormError, messageOf, ScreenLink, useDocumentTitle } from "./LoginPage";

/** Ce qu'un autre écran (la page d'invitation) donne à l'inscription. */
export type SignupRequest = {
  /** L'adresse à préremplir. */
  email?: string;
  /** Le jeton d'invitation, envoyé avec l'inscription (`SignUp.invitation`). */
  invitation?: string;
};

type Props = {
  /** Aucun compte n'existe encore (`info.setup`) : celui-ci administrera l'instance. */
  setup: boolean;
  request: SignupRequest;
  onSignedUp: (me: Me) => void;
  onLogin: () => void;
};

export function SignupPage({ setup, request, onSignedUp, onLogin }: Props) {
  useDocumentTitle(setup ? "Premier compte" : "Créer un compte");
  const [name, setName] = useState("");
  const [email, setEmail] = useState(request.email ?? "");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      onSignedUp(
        await signUp({
          email,
          name,
          password,
          ...(request.invitation ? { invitation: request.invitation } : {}),
        }),
      );
    } catch (e) {
      setError(messageOf(e));
      setBusy(false);
    }
  };

  let lead = "Ton compte aura son propre espace, où tu pourras inviter d'autres personnes.";
  if (setup) lead = "Créer le premier compte : il administrera cette instance.";
  else if (request.invitation) lead = "Crée ton compte pour rejoindre ceux qui t'ont invité.";

  return (
    <AuthScreen
      title={setup ? "Bienvenue dans Mementum" : "Créer un compte"}
      lead={lead}
      // Aucun compte n'existe encore : il n'y a personne à connecter.
      footer={
        setup ? undefined : (
          <>
            Déjà un compte ?{" "}
            <ScreenLink href="/login" onClick={onLogin}>
              Se connecter
            </ScreenLink>
          </>
        )
      }
    >
      <form className="mm-auth-form" onSubmit={submit} noValidate>
        <Field
          label="Nom"
          type="text"
          name="name"
          autoComplete="name"
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <Field
          label="Adresse e-mail"
          type="email"
          name="email"
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <Field
          label="Mot de passe"
          type="password"
          name="password"
          autoComplete="new-password"
          hint="10 caractères au moins."
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <FormError message={error} />
        <button type="submit" className="mm-button mm-button-primary mm-auth-submit" disabled={busy}>
          {busy ? "Création du compte…" : setup ? "Créer le compte administrateur" : "Créer mon compte"}
        </button>
      </form>
    </AuthScreen>
  );
}
