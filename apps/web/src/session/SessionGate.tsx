// La porte de l'application. `GET /api/v1/me` d'abord : sans session, l'écran de connexion,
// ou celui d'inscription s'il est permis (premier compte de l'instance, inscriptions
// ouvertes, ou jeton d'invitation donné par un autre écran : `showSignup`) ; avec,
// l'application, qui reçoit le compte par `useSession`.
//
// Session perdue en cours de route (un appel répond `401 unauthenticated`, `api.ts`) : la
// connexion est demandée par-dessus l'application, qui reste montée. La page ouverte et ce
// qui y a été tapé restent dans l'onglet, et repartent une fois la session rouverte. Si
// c'est un autre compte qui se connecte, l'application est remontée : rien de ce que le
// premier avait ouvert ou tapé ne part sous le nom du second.
//
// La déconnexion recharge l'onglet (`AccountMenu`) : rien du compte ne reste en mémoire.
import type { Info, Me } from "@mementum/protocol";
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
import { LoginDialog, LoginPage } from "./LoginPage";
import { SessionContext, type Session } from "./session";
import { SignupPage, type SignupRequest } from "./SignupPage";

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
  /** Ce qu'un autre écran a demandé à l'inscription (adresse, jeton d'invitation). */
  const [signupRequest, setSignupRequest] = useState<SignupRequest>({});

  const enter = useCallback((me: Me) => {
    setState({ name: "signedIn", me, lost: false });
    // Servie ou non, la demande d'inscription (et son jeton d'invitation) ne survit pas à
    // l'écran qui l'a reçue.
    setSignupRequest({});
  }, []);

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

  // Connecté sur `/login` ou `/signup` (après s'être connecté là, ou par « Précédent ») :
  // l'accueil, à la place de cette adresse, avant que rien ne s'affiche.
  const signedIn = state.name === "signedIn";
  const onSessionRoute = route.name === "login" || route.name === "signup";
  useLayoutEffect(() => {
    if (signedIn && onSessionRoute) navigate("/", { replace: true });
  }, [signedIn, onSessionRoute]);

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

  /** Montre l'inscription, même si elle est fermée quand `request` porte une invitation. */
  const showSignup = useCallback((request: SignupRequest = {}) => {
    setSignupRequest(request);
    navigate("/signup");
  }, []);
  const showLogin = useCallback(() => navigate("/login"), []);

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

  if (state.name === "anonymous") {
    const { info } = state;
    const signupAllowed =
      info.setup || info.signup === "open" || signupRequest.invitation !== undefined;
    if (info.setup || (route.name === "signup" && signupAllowed)) {
      return (
        <SignupPage
          setup={info.setup}
          request={signupRequest}
          onSignedUp={enter}
          onLogin={showLogin}
        />
      );
    }
    return (
      <LoginPage signupOpen={info.signup === "open"} onSignup={showSignup} onLoggedIn={enter} />
    );
  }

  return (
    <SessionContext.Provider value={session}>
      {/* Un autre compte connecté par-dessus : l'application repart de zéro. */}
      <Fragment key={state.me.user.id}>{children}</Fragment>
      {state.lost && <LoginDialog email={state.me.user.email} onLoggedIn={enter} />}
    </SessionContext.Provider>
  );
}
