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
// qu'on quitte ces trois écrans. Un compte créé par une invitation sur une page arrive sur
// cette page (`landing.ts`).
//
// Session perdue en cours de route (un appel répond `401 unauthenticated`, `api.ts`) : la
// connexion est demandée par-dessus l'application, qui reste montée. La page ouverte et ce
// qui y a été tapé restent dans l'onglet, et repartent une fois la session rouverte : par
// cette fenêtre, ou ailleurs (un autre onglet), ce que la relecture du compte constate ; elle
// ferme alors la fenêtre.
//
// Le compte de la session n'est plus celui que l'application a chargé (Ruling R115) : un
// autre s'est connecté par-dessus elle, ou dans un autre onglet du même navigateur, ce qu'elle
// apprend en relisant le compte (au retour sur l'onglet, Ruling R126 ; à l'ouverture du menu
// de l'espace ou de ses réglages). Elle ne reprend pas : elle est démontée aussitôt, et
// l'onglet rechargé. Rien de ce que le premier avait ouvert ou tapé ne reste en mémoire, ni
// ne part sous le nom du second. Un seul endroit en décide : `sameAccount`.
//
// La déconnexion recharge l'onglet (`SpaceSwitcher`) : rien du compte ne reste en mémoire.
import type { Info, InvitationInfo, Me } from "@mementum/protocol";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { flushSync } from "react-dom";
import { getInfo, getMe, isSessionLost, onSessionLost } from "../api";
import { navigate, pagePath, parseRoute, usePathname } from "../router";
import { forgetOtherRecents } from "../space/recents";
import { AuthScreen, messageOf } from "./forms";
import { invitationTarget, InvitePage, takeInvitationToken } from "./InvitePage";
import { invitedPage } from "./landing";
import { LoginDialog, LoginPage } from "./LoginPage";
import { SessionContext, type Session } from "./session";
import { SignupPage } from "./SignupPage";

/**
 * Ce que dit la connexion quand l'écran du premier compte apprend qu'un compte existe déjà
 * (R257, M5).
 */
const ACCOUNT_EXISTS =
  "Un compte a déjà été créé sur cette instance. Connecte-toi, ou demande une invitation.";

type State =
  | { name: "loading" }
  /** Le serveur ne répond pas, ou pas comme prévu. */
  | { name: "failed"; message: string }
  /**
   * `accountExists` : l'écran du premier compte était affiché, et l'instance relue après une
   * inscription qui a échoué a dit qu'un compte existe désormais.
   */
  | { name: "anonymous"; info: Info; accountExists?: boolean }
  /** `lost` : un appel a dit que la session n'est plus là. */
  | { name: "signedIn"; me: Me; lost: boolean };

/** Recharge l'onglet : plus rien de l'application ouverte ne reste en mémoire. */
const reloadTab = () => location.reload();

type Props = {
  children: ReactNode;
  /** Recharge l'onglet ; les tests en donnent un autre. */
  reload?: () => void;
};

export function SessionGate({ children, reload = reloadTab }: Props) {
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

  // Toute entrée d'un compte passe ici (chargement, connexion, inscription, session rouverte) :
  // les pages récentes des autres comptes quittent le navigateur, et les siennes s'écrivent
  // (R297).
  const enter = useCallback((me: Me) => {
    forgetOtherRecents(me.user.id);
    setState({ name: "signedIn", me, lost: false });
  }, []);

  /** Où aller en quittant l'inscription, quand ce n'est pas l'accueil : la page de l'invitation. */
  const landing = useRef<string | null>(null);

  /** L'inscription a reçu le jeton, et accepté l'invitation : il ne sert plus. */
  const signedUp = useCallback(
    async (me: Me) => {
      // Invité sur une page : il y arrive, comme le compte qui accepte avec une session.
      const page = invited !== null && invited.pageTitle !== null ? await invitedPage(me) : null;
      landing.current = page === null ? null : pagePath(page);
      forgetInvitation();
      enter(me);
    },
    [enter, forgetInvitation, invited],
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

  /**
   * L'inscription de l'écran du premier compte n'a pas créé de compte (refus, ou pas de
   * réponse) : l'instance est relue (R257, M5). Quelqu'un d'autre a pu créer ce compte
   * entre-temps ; il n'y a alors plus de premier compte à créer, et l'écran devient celui de la
   * connexion, qui le dit. Ce qui a été tapé n'est envoyé nulle part d'autre, et ne passe pas
   * dans la connexion. Une relecture qui échoue, ou une instance toujours sans compte, laisse
   * l'écran tel quel, avec le message de l'inscription.
   */
  const firstAccountFailed = useCallback(async () => {
    let info: Info;
    try {
      info = await getInfo();
    } catch {
      return;
    }
    if (info.setup) return;
    // Inscriptions ouvertes, l'adresse `/signup` montrerait l'inscription ordinaire.
    if (parseRoute(location.pathname).name === "signup") navigate("/login", { replace: true });
    setState((current) =>
      current.name === "anonymous" ? { name: "anonymous", info, accountExists: true } : current,
    );
  }, []);

  // Connecté sur `/login` ou `/signup` (après s'être connecté là, ou par « Précédent ») : à
  // la place de cette adresse, avant que rien ne s'affiche, la page de l'invitation qu'on
  // venait accepter, celle où elle mène si l'inscription vient de l'accepter, sinon l'accueil.
  const signedIn = state.name === "signedIn";
  const onSessionRoute = route.name === "login" || route.name === "signup";
  useLayoutEffect(() => {
    if (!signedIn || !onSessionRoute) return;
    const to = landing.current ?? (joining ? "/invite" : "/");
    landing.current = null;
    navigate(to, { replace: true });
  }, [signedIn, onSessionRoute, joining]);

  /** Le nombre de fois où un appel a dit la session perdue. */
  const losses = useRef(0);
  // Tout appel qui répond `401 unauthenticated`, `getMe` de `refresh` compris.
  useEffect(
    () =>
      onSessionLost(() => {
        losses.current += 1;
        setState((current) =>
          current.name === "signedIn" && !current.lost ? { ...current, lost: true } : current,
        );
      }),
    [],
  );

  /** Le compte que l'application a chargé ; `null` : elle n'est pas ouverte. */
  const loaded = useRef<string | null>(null);
  loaded.current = state.name === "signedIn" ? state.me.user.id : null;

  /**
   * Le serveur vient de dire quel compte a la session (`GET /api/v1/me`, ou la connexion
   * par-dessus l'application) : est-ce toujours celui que l'application a chargé ? Sinon
   * (Ruling R115), elle repart de zéro : l'onglet est rechargé. Elle est d'abord démontée,
   * sur-le-champ : ses connexions de synchro et ce qui attendait le retour du compte (un
   * texte, un titre) s'arrêtent là, avant d'avoir pu se servir de la session de l'autre.
   */
  const sameAccount = useCallback(
    (me: Me): boolean => {
      if (loaded.current === null || me.user.id === loaded.current) return true;
      flushSync(() => setState({ name: "loading" }));
      reload();
      return false;
    },
    [reload],
  );

  const refresh = useCallback(async () => {
    const before = losses.current;
    // Session perdue : `getMe` rejette, et `onSessionLost` a déjà demandé la connexion.
    const me = await getMe();
    if (!sameAccount(me)) return;
    // Le serveur a répondu pour ce compte : la session est là. Si la connexion était demandée
    // par-dessus l'application (session perdue ici, puis rouverte dans un autre onglet), la
    // fenêtre se ferme, sans mot de passe à retaper. Sauf si la session s'est perdue pendant
    // cette lecture : sa réponse, partie avant, ne prouve rien.
    const proven = losses.current === before;
    setState((current) =>
      current.name === "signedIn" ? { ...current, me, lost: current.lost && !proven } : current,
    );
  }, [sameAccount]);

  /** La session est rouverte par-dessus l'application (`LoginDialog`) : elle reprend. */
  const resume = useCallback(
    (me: Me) => {
      if (sameAccount(me)) enter(me);
    },
    [enter, sameAccount],
  );

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
          onFailed={info.setup ? () => void firstAccountFailed() : undefined}
        />
      );
    }
    if (route.name === "invite") return invitePage;
    return (
      <LoginPage
        signupOpen={signupAllowed}
        notice={state.accountExists ? ACCOUNT_EXISTS : undefined}
        invitation={answering}
        onSignup={showSignup}
        onLoggedIn={enter}
      />
    );
  }

  return (
    <SessionContext.Provider value={session}>
      {route.name === "invite" ? invitePage : children}
      {state.lost && <LoginDialog email={state.me.user.email} onLoggedIn={resume} />}
    </SessionContext.Provider>
  );
}
