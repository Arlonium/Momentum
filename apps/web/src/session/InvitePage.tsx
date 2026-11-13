// La page d'un lien d'invitation (`/invite#<jeton>`), avec ou sans session.
//
// Le jeton est dans le fragment, que le navigateur n'envoie pas au serveur. `SessionGate`
// le lit dès le chargement (`takeInvitationToken`) puis efface le fragment de l'adresse
// (`history.replaceState`) avant le premier affichage : il n'est plus dans la barre
// d'adresse (ni dans une capture d'écran, ni dans un lien recopié depuis elle), ni dans
// l'entrée de l'historique de l'onglet. L'historique global du navigateur et les suggestions
// de la barre d'adresse peuvent, eux, garder l'adresse ouverte avec son fragment : c'est le
// navigateur qui les tient. Le jeton ne vit ensuite qu'en mémoire, le temps de l'invitation
// (cette page, puis l'inscription ou la connexion), et part avec elle. Un jeton qui a servi
// ne vaut plus rien.
//
// L'adresse e-mail de l'invitation n'est jamais vérifiée (pas de SMTP, Ruling R47) : le lien
// s'accepte avec n'importe quel compte, et l'écran le dit.
//
// Le serveur dit ce que donne le lien (`lookup`) : « <Nom> t'invite dans l'espace « … » »
// (ou sur une page). Avec une session, « Accepter » y mène. Sans session : « Créer mon
// compte » (adresse préremplie ; l'inscription envoie le jeton et accepte dans la foulée) ou
// « J'ai déjà un compte » (connexion, puis retour ici pour accepter). Un jeton inconnu, échu,
// révoqué ou déjà utilisé, ou dont l'auteur a perdu son rôle : « Cette invitation n'est plus
// valable », sans autre détail, comme le serveur.
//
// Une invitation qui ne donne rien au compte qui l'accepte (il a déjà ce rôle, ou ce niveau sur
// la page, par une ligne à son nom, une page parente ou l'accès général) n'est pas consommée
// (R257, M7 : `changed` faux) : l'écran le dit, et offre un lien vers ce qu'elle donne, sans y
// aller d'office. Le jeton reste en mémoire tant qu'on est sur cet écran.
//
// Sans jeton dans l'adresse (la page rechargée, puisqu'il en a été effacé, ou un lien
// tronqué), l'écran ne sait rien de l'invitation, qui vaut peut-être encore : il dit le lien
// incomplet et demande de le rouvrir, sans rien demander au serveur.
import type { Accepted, InvitationInfo } from "@mementum/protocol";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ApiError, acceptInvitation, isSessionLost, lookupInvitation } from "../api";
import { navigate, pagePath, parseRoute, spacePath } from "../router";
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
    ? `l'espace «\u00a0${info.spaceName}\u00a0»`
    : `la page «\u00a0${info.pageTitle || "Sans titre"}\u00a0»`;
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
  /** L'adresse ne porte pas de jeton : rien à demander au serveur. */
  | { name: "incomplete" }
  | { name: "invalid" }
  | { name: "failed"; message: string }
  | { name: "ready"; info: InvitationInfo }
  /** Acceptée sans effet : le compte avait déjà ce qu'elle donne, et le lien vaut encore. */
  | { name: "kept"; info: InvitationInfo; accepted: Accepted };

export function InvitePage({ token, session, onSignup, onLogin, onAccepted }: Props) {
  const [state, setState] = useState<State>(token ? { name: "loading" } : { name: "incomplete" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Le message d'une invitation sans effet : il prend le focus d'« Accepter », qui part. */
  const kept = useRef<HTMLSpanElement>(null);
  useDocumentTitle(
    state.name === "invalid"
      ? "Invitation non valable"
      : state.name === "incomplete"
        ? "Lien d'invitation incomplet"
        : "Invitation",
  );

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
      if (!accepted.changed) {
        // Rien n'a été écrit : rien à relire, et le lien vaut encore pour quelqu'un d'autre.
        setBusy(false);
        setState((current) =>
          current.name === "ready" ? { name: "kept", info: current.info, accepted } : current,
        );
        return;
      }
      // L'espace rejoint, dans la liste du compte. En cas d'échec, l'application la relira.
      await session.refresh().catch(() => {});
      // La page ou l'espace rejoint, à la place de cette adresse : Précédent ne revient pas
      // sur une invitation qui a servi.
      navigate(accepted.pageId ? pagePath(accepted.pageId) : spacePath(accepted.spaceId), {
        replace: true,
      });
      onAccepted();
    } catch (e) {
      setBusy(false);
      if (e instanceof ApiError && e.code === "invalid_invitation") setState({ name: "invalid" });
      // Session perdue : la connexion est demandée par-dessus ; l'invitation attend.
      else if (!isSessionLost(e)) setError(messageOf(e));
    }
  };

  const keptShown = state.name === "kept";
  useLayoutEffect(() => {
    if (keptShown) kept.current?.focus();
  }, [keptShown]);

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

  if (state.name === "incomplete") {
    return (
      <AuthScreen
        title="Ce lien d'invitation est incomplet"
        lead="Rouvre le lien que tu as reçu."
        footer={home}
      />
    );
  }

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

  if (state.name === "kept") {
    const { pageId, spaceId } = state.accepted;
    const to = pageId ? pagePath(pageId) : spacePath(spaceId);
    return (
      <AuthScreen
        title={title}
        lead={
          <span ref={kept} role="status" tabIndex={-1}>
            Tu as déjà cet accès. Le lien reste valable pour quelqu'un d'autre.
          </span>
        }
        footer={
          // Le nom vient d'ailleurs : isolé. À la place de cette adresse, comme une invitation
          // qui a servi : quitter l'écran oublie le jeton, et « Précédent » y trouverait « Ce lien
          // d'invitation est incomplet ».
          <ScreenLink href={to} onClick={() => navigate(to, { replace: true })}>
            {info.pageTitle === null ? "Ouvrir l'espace" : "Ouvrir la page"} «&nbsp;
            <span className="mm-isolate" dir="auto">
              {info.pageTitle === null ? info.spaceName : info.pageTitle || "Sans titre"}
            </span>
            &nbsp;»
          </ScreenLink>
        }
      />
    );
  }

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
          L'invitation a été préparée pour <strong className="mm-invite-who">{info.email}</strong>.
          Tu peux l'accepter avec le compte de ton choix : crée-en un, ou{" "}
          <span className="mm-nowrap">connecte-toi</span>.
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
