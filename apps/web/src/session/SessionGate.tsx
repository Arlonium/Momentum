// La porte de l'application. `GET /api/v1/me` d'abord : sans session, l'écran de connexion,
// ou celui d'inscription s'il est permis (premier compte de l'instance, inscriptions
// ouvertes, ou invitation en cours) ; avec, l'application, qui reçoit le compte par
// `useSession`.
//
// Lien d'invitation (`/invite#<jeton>`, `InvitePage`), avec ou sans session. Le jeton est lu
// au chargement (ou quand un autre lien est ouvert dans l'onglet), puis effacé de l'adresse.
// Il ne vit qu'ici, en mémoire, le temps de l'invitation : sa page, puis l'inscription (qui
// l'envoie, et s'ouvre même quand les inscriptions sont fermées) ou la connexion (qui ramène à
// la page d'invitation, pour l'accepter). Il est oublié dès que l'invitation a servi, ou
// qu'on quitte ces trois écrans.
//
// Session perdue en cours de route (un appel répond `401 unauthenticated`, `api.ts`) : la
// connexion est demandée par-dessus l'application, qui reste montée. La page ouverte et ce
// qui y a été tapé restent dans l'onglet, et repartent une fois la session rouverte. Si
// c'est un autre compte qui se connecte, l'application est remontée : rien de ce que le
// premier avait ouvert ou tapé ne part sous le nom du second.
//
// La déconnexion recharge l'onglet (`SpaceSwitcher`) : rien du compte ne reste en mémoire.
import type { Info, InvitationInfo, Me } from "@mementum/protocol";
import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { getInfo, getMe, isSessionLost, onSessionLost } from "../api";
import { navigate, parseRoute, usePathname } from "../router";
import { AuthScreen, messageOf } from "./forms";
import { invitationTarget, InvitePage, takeInvitationToken } from "./InvitePage";
import { LoginDialog, LoginPage } from "./LoginPage";
import { SessionContext, type Session } from "./session";
import { SignupPage } from "./SignupPage";

type State =
  | { name: "loading" }
  /** Le serveur ne répond pas, ou pas comme prévu. */
  | { name: "failed"; message: string }
  | { name: "anonymous"; info: Info }
  /** `lost` : un appel a dit que la session n'est plus là. */
  | { name: "signedIn"; me: Me; lost: boolean };

export function SessionGate({ children }: { children: ReactNode }) {
  const route = parseRoute(usePathname());
  const [state, setState] = useState<State>({ name: "loading" });
  /** Le jeton du lien d'invitation ouvert dans cet onglet. */
  const [invitation, setInvitation] = useState<string | null>(null);
  /**
   * Ce que le serveur dit de cette invitation, une fois qu'on a choisi d'y répondre par
   * l'inscription ou la connexion : ces écrans le reprennent.
   */
  const [invited, setInvited] = useState<InvitationInfo | null>(null);
  const joining = invitation !== null && invited !== null;

  const forgetInvitation = useCallback(() => {
    setInvitation(null);
    setInvited(null);
  }, []);

  // Le jeton quitte l'adresse avant le premier affichage ; un autre lien d'invitation ouvert
  // dans l'onglet (seul le fragment change : pas de rechargement) le remplace.
  useLayoutEffect(() => {
    const take = () => {
      const token = takeInvitationToken();
      if (token === null) return;
      setInvitation(token);
      setInvited(null);
    };
    take();
    window.addEventListener("hashchange", take);
    return () => window.removeEventListener("hashchange", take);
  }, []);

  // Hors des écrans de l'invitation, son jeton est oublié.
  const inInvitationFlow =
    route.name === "invite" || route.name === "login" || route.name === "signup";
  useEffect(() => {
    if (!inInvitationFlow) forgetInvitation();
  }, [inInvitationFlow, forgetInvitation]);

  const enter = useCallback((me: Me) => setState({ name: "signedIn", me, lost: false }), []);

  /** L'inscription a reçu le jeton, et accepté l'invitation : il ne sert plus. */
  const signedUp = useCallback(
    (me: Me) => {
      forgetInvitation();
      enter(me);
    },
    [enter, forgetInvitation],
  );

  const load = useCallback(async () => {
    setState({ name: "loading" });
    try {
      enter(await getMe());
    } catch (error) {
      if (!isSessionLost(error)) {
        setState({ name: "failed", message: messageOf(error) });
        return;
      }
      try {
        setState({ name: "anonymous", info: await getInfo() });
      } catch (e) {
        setState({ name: "failed", message: messageOf(e) });
      }
    }
  }, [enter]);

  useEffect(() => {
    void load();
  }, [load]);

  // Connecté sur `/login` ou `/signup` (après s'être connecté là, ou par « Précédent ») : à
  // la place de cette adresse, avant que rien ne s'affiche, la page de l'invitation qu'on
  // venait accepter, sinon l'accueil.
  const signedIn = state.name === "signedIn";
  const onSessionRoute = route.name === "login" || route.name === "signup";
  useLayoutEffect(() => {
    if (signedIn && onSessionRoute) navigate(joining ? "/invite" : "/", { replace: true });
  }, [signedIn, onSessionRoute, joining]);

  // Tout appel qui répond `401 unauthenticated`, `getMe` de `refresh` compris.
  useEffect(
    () =>
      onSessionLost(() =>
        setState((current) =>
          current.name === "signedIn" && !current.lost ? { ...current, lost: true } : current,
        ),
      ),
    [],
  );

  const refresh = useCallback(async () => {
    // Session perdue : `getMe` rejette, et `onSessionLost` a déjà demandé la connexion.
    const me = await getMe();
    setState((current) => (current.name === "signedIn" ? { ...current, me } : current));
  }, []);

  const showSignup = useCallback(() => navigate("/signup"), []);
  const showLogin = useCallback(() => navigate("/login"), []);
  /** Depuis la page d'invitation : l'inscription ou la connexion, qui la reprennent. */
  const answerBySignup = useCallback((info: InvitationInfo) => {
    setInvited(info);
    navigate("/signup");
  }, []);
  const answerByLogin = useCallback((info: InvitationInfo) => {
    setInvited(info);
    navigate("/login");
  }, []);

  const me = state.name === "signedIn" ? state.me : null;
  const session = useMemo<Session | null>(() => (me ? { me, refresh } : null), [me, refresh]);

  if (state.name === "loading") return <main className="mm-auth" aria-busy="true" />;

  if (state.name === "failed") {
    return (
      <AuthScreen title="Impossible de joindre Mementum" lead={state.message}>
        <button
          type="button"
          className="mm-button mm-button-primary mm-auth-submit"
          onClick={() => void load()}
        >
          Réessayer
        </button>
      </AuthScreen>
    );
  }

  const invitePage = (
    <InvitePage
      // Un autre lien ouvert dans l'onglet : une autre invitation, relue.
      key={invitation ?? ""}
      token={invitation}
      session={session}
      onSignup={answerBySignup}
      onLogin={answerByLogin}
      onAccepted={forgetInvitation}
    />
  );

  if (state.name === "anonymous") {
    const { info } = state;
    // L'invitation en cours ouvre l'inscription, même fermée : le serveur l'accepte avec son
    // jeton.
    const signupAllowed = info.setup || info.signup === "open" || joining;
    /** L'invitation à laquelle on répond : l'adresse invitée, et ce qu'elle donne, en mots. */
    const answering = joining
      ? { email: invited.email, invitedTo: invitationTarget(invited) }
      : undefined;
    if (info.setup || (route.name === "signup" && signupAllowed)) {
      return (
        <SignupPage
          setup={info.setup}
          request={answering && invitation ? { ...answering, invitation } : {}}
          onSignedUp={signedUp}
          onLogin={showLogin}
        />
      );
    }
    if (route.name === "invite") return invitePage;
    return (
      <LoginPage
        signupOpen={signupAllowed}
        invitation={answering}
        onSignup={showSignup}
        onLoggedIn={enter}
      />
    );
  }

  return (
    <SessionContext.Provider value={session}>
      {route.name === "invite" ? (
        invitePage
      ) : (
        // Un autre compte connecté par-dessus : l'application repart de zéro.
        <Fragment key={state.me.user.id}>{children}</Fragment>
      )}
      {state.lost && <LoginDialog email={state.me.user.email} onLoggedIn={enter} />}
    </SessionContext.Provider>
  );
}
