// La page d'un lien d'invitation (`/invite#<jeton>`), avec ou sans session.
//
// Le jeton est dans le fragment, que le navigateur n'envoie pas au serveur. `SessionGate`
// le lit dès le chargement (`takeInvitationToken`) puis efface le fragment de l'adresse
// (`history.replaceState`) : il ne reste ni dans l'historique, ni dans une capture d'écran,
// ni dans un lien copié depuis la barre d'adresse. Il ne vit plus qu'en mémoire, le temps de
// l'invitation (cette page, puis l'inscription ou la connexion), et part avec elle.
//
// Le serveur dit ce que donne le lien (`lookup`) : « <Nom> t'invite dans l'espace « … » »
// (ou sur une page). Avec une session, « Accepter » y mène. Sans session : « Créer mon
// compte » (adresse préremplie ; l'inscription envoie le jeton et accepte dans la foulée) ou
// « J'ai déjà un compte » (connexion, puis retour ici pour accepter). Un jeton inconnu, échu,
// révoqué ou déjà utilisé, ou dont l'auteur a perdu son rôle : « Cette invitation n'est plus
// valable », sans autre détail, comme le serveur.
import type { InvitationInfo } from "@mementum/protocol";
import { useCallback, useEffect, useState } from "react";
import { ApiError, acceptInvitation, isSessionLost, lookupInvitation } from "../api";
import { navigate, pagePath, parseRoute } from "../router";
import { invitationToken } from "../share/levels";
import { AuthScreen, FormError, messageOf, ScreenLink, useDocumentTitle } from "./forms";
import type { Session } from "./session";

/**
 * Le jeton d'invitation de l'adresse, si c'est `/invite#<jeton>`, sinon `null`. Sur
 * `/invite`, le fragment est effacé de l'adresse dans tous les cas, à la place de l'entrée
 * courante de l'historique.
 */
export function takeInvitationToken(): string | null {
  if (parseRoute(location.pathname).name !== "invite") return null;
  const token = invitationToken(location.hash);
  if (location.hash) history.replaceState(history.state, "", location.pathname + location.search);
  return token;
}

/**
 * Ce que donne l'invitation, en mots : « l'espace « Pro » » ou « la page « Plan » ». Les
 * guillemets tiennent à leur nom (espaces insécables) : une ligne ne se coupe pas entre eux.
 */
export function invitationTarget(info: InvitationInfo): string {
  return info.pageTitle === null
    ? `l'espace « ${info.spaceName} »`
    : `la page « ${info.pageTitle || "Sans titre"} »`;
}

type Props = {
  /** Le jeton lu dans l'adresse ; `null` : elle n'en portait pas, ou plus. */
  token: string | null;
  /** La session ; `null` : sans compte connecté. */
  session: Session | null;
  /** Sans session : créer un compte avec cette invitation. */
  onSignup: (info: InvitationInfo) => void;
  /** Sans session : se connecter, puis revenir ici pour accepter. */
  onLogin: (info: InvitationInfo) => void;
  /** L'invitation a servi : son jeton ne sert plus à rien. */
  onAccepted: () => void;
};

type State =
  | { name: "loading" }
  | { name: "invalid" }
  | { name: "failed"; message: string }
  | { name: "ready"; info: InvitationInfo };

export function InvitePage({ token, session, onSignup, onLogin, onAccepted }: Props) {
  const [state, setState] = useState<State>(token ? { name: "loading" } : { name: "invalid" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useDocumentTitle(state.name === "invalid" ? "Invitation expirée" : "Invitation");

  const lookup = useCallback(async () => {
    if (!token) return;
    setState({ name: "loading" });
    try {
      setState({ name: "ready", info: await lookupInvitation(token) });
    } catch (e) {
      setState(
        e instanceof ApiError && e.code === "invalid_invitation"
          ? { name: "invalid" }
          : { name: "failed", message: messageOf(e) },
      );
    }
  }, [token]);

  useEffect(() => {
    void lookup();
  }, [lookup]);

  const accept = async () => {
    if (!token || !session || busy) return;
    setBusy(true);
    setError(null);
    try {
      const accepted = await acceptInvitation(token);
      // L'espace rejoint, dans la liste du compte. En cas d'échec, l'application la relira.
      await session.refresh().catch(() => {});
      // À la place de cette adresse : Précédent ne revient pas sur une invitation qui a servi.
      navigate(accepted.pageId ? pagePath(accepted.pageId) : "/", { replace: true });
      onAccepted();
    } catch (e) {
      setBusy(false);
      if (e instanceof ApiError && e.code === "invalid_invitation") setState({ name: "invalid" });
      // Session perdue : la connexion est demandée par-dessus ; l'invitation attend.
      else if (!isSessionLost(e)) setError(messageOf(e));
    }
  };

  const home = session ? (
    <ScreenLink href="/" onClick={() => navigate("/")}>
      Revenir à mes pages
    </ScreenLink>
  ) : (
    <ScreenLink href="/login" onClick={() => navigate("/login")}>
      Se connecter
    </ScreenLink>
  );

  if (state.name === "loading") return <main className="mm-auth" aria-busy="true" />;

  if (state.name === "invalid") {
    return (
      <AuthScreen
        title="Cette invitation n'est plus valable"
        lead="Demande un nouveau lien à la personne qui t'a invité."
        footer={home}
      />
    );
  }

  if (state.name === "failed") {
    return (
      <AuthScreen title="Impossible de lire l'invitation" lead={state.message}>
        <button
          type="button"
          className="mm-button mm-button-primary mm-auth-submit"
          onClick={() => void lookup()}
        >
          Réessayer
        </button>
      </AuthScreen>
    );
  }

  const { info } = state;
  const title = `${info.invitedBy} t'invite ${info.pageTitle === null ? "dans" : "sur"} ${invitationTarget(info)}`;
  const where =
    info.pageTitle === null ? null : <>Elle est dans l'espace «&nbsp;{info.spaceName}&nbsp;». </>;

  if (session) {
    const { user } = session.me;
    return (
      <AuthScreen
        title={title}
        lead={
          <>
            {where}
            Tu l'accepteras avec ton compte <strong className="mm-invite-who">{user.name}</strong>{" "}
            ({user.email}).
          </>
        }
        footer={home}
      >
        <div className="mm-auth-form">
          <FormError message={error} />
          {/* Sans focus d'office : une Entrée tapée par mégarde n'accepte rien. */}
          <button
            type="button"
            className="mm-button mm-button-primary mm-auth-submit"
            disabled={busy}
            onClick={() => void accept()}
          >
            {busy ? "Un instant…" : "Accepter"}
          </button>
        </div>
      </AuthScreen>
    );
  }

  return (
    <AuthScreen
      title={title}
      lead={
        <>
          {where}
          Invitation pour <strong className="mm-invite-who">{info.email}</strong>. Crée ton compte
          pour l'accepter, ou connecte-toi si tu en as déjà un.
        </>
      }
    >
      <div className="mm-auth-form">
        <button
          type="button"
          className="mm-button mm-button-primary mm-auth-submit"
          onClick={() => onSignup(info)}
        >
          Créer mon compte
        </button>
        <button type="button" className="mm-button mm-auth-submit" onClick={() => onLogin(info)}>
          J'ai déjà un compte
        </button>
      </div>
    </AuthScreen>
  );
}
