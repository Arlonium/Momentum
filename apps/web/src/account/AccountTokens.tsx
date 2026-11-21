// L'écran « Jetons d'API » du compte (`/account/tokens`, lot 5, Task C1), ouvert depuis le menu du
// compte (sélecteur d'espace, R80 ; §4.2 : « jetons personnels ») et chargé à la demande : rien de lui
// sur le chemin de l'ouverture de l'application ni d'une page.
//
// La liste des jetons du compte (`useTokens`), les vivants d'abord : leur nom, leurs quatre derniers
// caractères (jamais leur texte, que le serveur ne garde pas), leur niveau, leur portée, leurs dates ;
// « Créer un jeton » (`CreateToken`, où le jeton est montré une fois, puis part avec la fenêtre) ;
// « Révoquer », après une confirmation ; « Activité », qui déplie sous la ligne les appels du jeton
// (`TokenActivity`, Task C2), lus seulement à ce moment-là. Un jeton mort (révoqué ou échu) reste dans
// la liste quatre-vingt-dix jours, pour son activité (R1220).
//
// Rien ne bouge sous le pointeur ni sous le focus : tant que l'un ou l'autre est dans la liste, une
// relecture garde chaque ligne à sa place (un jeton révoqué le dit là où il est, un jeton neuf vient à
// la fin) ; l'ordre du serveur revient quand ils en sortent. Le pointeur se suit sur le document, jamais
// par « enter » et « leave » de la liste (R419).
//
// Ce qui vient d'ailleurs (le nom d'un jeton, d'un espace, le titre d'une page) est isolé (`dir`).
import type { SpaceInfo, TokenInfo } from "@mementum/protocol";
import { useEffect, useId, useRef, useState, type FocusEvent } from "react";
import { answerLost, isSessionLost, lostAnswer, revokeToken } from "../api";
import { FormError, messageOf, useDocumentTitle } from "../session/forms";
import { useSession } from "../session/session";
import { useAfterLogin } from "../session/useAfterLogin";
import { Confirm } from "../ui/Confirm";
import { CreateToken, type TokenDraft } from "./CreateToken";
import { NBSP, quoted } from "./quoted";
import { TokenActivity } from "./TokenActivity";
import { useTokens } from "./useTokens";

const day = new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "long", year: "numeric" });
const moment = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "long",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

/** Une date du serveur, ou « date inconnue » si elle ne s'écrit pas. */
function written(format: Intl.DateTimeFormat, at: number): string {
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? "date inconnue" : format.format(date);
}

const LEVEL_LABELS = { read: "Lecture", edit: "Modification" } as const;

/** Combien de titres de pages une ligne nomme avant « et N autres ». */
const NAMED_PAGES = 3;

/** Un texte venu d'ailleurs, isolé, entre guillemets. */
function Quoted({ text }: { text: string }) {
  return (
    <>
      «&nbsp;
      <span className="mm-isolate" dir="auto">
        {text}
      </span>
      &nbsp;»
    </>
  );
}

/** Ce que le jeton ouvre, en mots. */
function ScopeText({ token }: { token: TokenInfo }) {
  if (token.scope === "all") return <>tous tes espaces</>;
  const where =
    token.space === null || token.space.name === null ? (
      <>un espace où tu n'as plus de rôle</>
    ) : (
      <>
        l'espace <Quoted text={token.space.name} />
      </>
    );
  if (token.scope === "space") return where;
  const count = token.pages.length;
  const named = token.pages.slice(0, NAMED_PAGES);
  return (
    <>
      {count === 0 ? "aucune page (elles ont été supprimées)" : `${count} ${count > 1 ? "pages" : "page"}`} de{" "}
      {where}
      {named.length > 0 && " : "}
      {named.map((p, i) => (
        <span key={p.id}>
          {i > 0 && ", "}
          {p.title === null ? "une page que tu ne vois pas" : <Quoted text={p.title || "Sans titre"} />}
        </span>
      ))}
      {count > NAMED_PAGES && ` et ${count - NAMED_PAGES} de plus`}
    </>
  );
}

/** Ses dates : créé, échéance, dernière utilisation ; pour un jeton mort, sa mort. */
function datesOf(token: TokenInfo): string {
  const parts = [`créé le ${written(day, token.createdAt)}`];
  if (token.revokedAt !== null) parts.push(`révoqué le ${written(day, token.revokedAt)}`);
  else if (token.expiresAt === null) parts.push("sans échéance");
  else parts.push(`${token.live ? "expire" : "échu"} le ${written(day, token.expiresAt)}`);
  parts.push(
    token.lastUsedAt === null ? "jamais utilisé" : `dernière utilisation le ${written(moment, token.lastUsedAt)}`,
  );
  return parts.join(" · ");
}

/**
 * La liste montrée quand elle est gelée : ce qu'on montrait au début du gel, chaque jeton à jour
 * (`incoming`), ceux que la relecture n'a plus restant tels qu'on les montrait ; puis, à la fin, ceux
 * qu'elle apporte.
 */
function heldTokens(frozen: readonly TokenInfo[], incoming: readonly TokenInfo[]): TokenInfo[] {
  const byId = new Map(incoming.map((t) => [t.id, t]));
  const shown = new Set(frozen.map((t) => t.id));
  return [...frozen.map((t) => byId.get(t.id) ?? t), ...incoming.filter((t) => !shown.has(t.id))];
}

export default function AccountTokens({ space }: { space: SpaceInfo | null }) {
  const { me } = useSession();
  useDocumentTitle("Jetons d'API");
  const afterLogin = useAfterLogin(me);
  const { tokens, error, reread } = useTokens(afterLogin);
  const [creating, setCreating] = useState(false);
  /** Ce que la fenêtre a laissé en partant sans créer de jeton (Échap ne détruit rien). */
  const [draft, setDraft] = useState<TokenDraft | null>(null);
  /** Compte les fermetures de la fenêtre, pour rendre le focus après chacune. */
  const [closed, setClosed] = useState(0);
  const openerRef = useRef<HTMLButtonElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);

  // L'arrivée depuis le menu du compte : le lien suivi part avec le menu, et le focus tomberait sur
  // `body` ; le titre de l'écran le prend (R1621).
  useEffect(() => {
    const active = document.activeElement;
    if (active === null || active === document.body) titleRef.current?.focus();
  }, []);

  // La fenêtre partie, le focus revient au bouton qui l'a ouverte (pas dans le vide).
  useEffect(() => {
    if (closed === 0) return;
    const active = document.activeElement;
    if (active === null || active === document.body) openerRef.current?.focus();
  }, [closed]);

  return (
    <>
      <header className="mm-topbar">
        <span className="mm-topbar-title">
          <span className="mm-topbar-crumb">Compte / </span>
          Jetons d'API
        </span>
      </header>
      <div className="mm-settings">
        <h1 ref={titleRef} className="mm-settings-title" tabIndex={-1}>
          Jetons d'API
        </h1>
        <p className="mm-settings-lead">
          Un jeton laisse un agent (Claude Code, un script) lire ou modifier tes pages en ton nom, dans la
          portée que tu choisis, sans jamais faire plus que toi. Tu le révoques quand tu veux ; changer ton
          mot de passe les révoque tous.
        </p>
        <p className="mm-settings-actions">
          <button
            ref={openerRef}
            type="button"
            className="mm-button mm-button-primary"
            onClick={() => setCreating(true)}
          >
            Créer un jeton
          </button>
        </p>
        <TokenList tokens={tokens} error={error} reread={reread} afterLogin={afterLogin} />
        {creating && (
          <CreateToken
            spaces={me.spaces}
            current={space}
            email={me.user.email}
            draft={draft}
            onDraft={setDraft}
            onChanged={() => void reread()}
            onClose={() => {
              setCreating(false);
              setClosed((n) => n + 1);
            }}
          />
        )}
      </div>
    </>
  );
}

type ListProps = {
  tokens: TokenInfo[] | null;
  error: string | null;
  reread: () => Promise<void>;
  /** Tenue quand le compte revient, après une session perdue : l'activité d'un jeton est relue. */
  afterLogin: () => Promise<void>;
};

function TokenList({ tokens, error, reread, afterLogin }: ListProps) {
  const headingId = useId();
  /** Les jetons dont l'activité est dépliée : plusieurs à la fois, rien ne se replie de lui-même. */
  const [opened, setOpened] = useState<ReadonlySet<string>>(() => new Set());
  const toggle = (id: string) =>
    setOpened((was) => {
      const next = new Set(was);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const headingRef = useRef<HTMLHeadingElement>(null);
  const section = useRef<HTMLElement>(null);
  /** Ce que la liste montrait quand le pointeur ou le focus y est entré : gardé tant qu'ils y sont. */
  const [held, setHeld] = useState<readonly TokenInfo[] | null>(null);
  const inside = useRef({ pointer: false, focus: false });
  const [revoking, setRevoking] = useState<TokenInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [revokeError, setRevokeError] = useState<string | null>(null);
  /** Le jeton dont la ligne prend le focus, son bouton « Révoquer » parti avec la révocation. */
  const [landing, setLanding] = useState<{ id: string } | null>(null);
  /** Un « Réessayer » a demandé la lecture qui est en route : il reste là, grisé, et garde le focus. */
  const [retrying, setRetrying] = useState(false);
  /** Un « Réessayer » a le focus : parti avec la lecture réussie, il le laisserait sur `body` (R339). */
  const retryFocused = useRef(false);

  const shown = held === null || tokens === null ? (tokens ?? []) : heldTokens(held, tokens);
  /** Ce qui est montré au dernier rendu : ce que le gel garde. */
  const latest = useRef(shown);
  latest.current = shown;
  const release = () => {
    if (!inside.current.pointer && !inside.current.focus) setHeld(null);
  };

  useEffect(() => {
    const over = (event: Event) => {
      const within = event.target instanceof Node && section.current?.contains(event.target) === true;
      if (within === inside.current.pointer) return;
      inside.current.pointer = within;
      if (within) setHeld((was) => was ?? latest.current);
      else release();
    };
    // Sans élément d'arrivée : le pointeur a quitté la fenêtre.
    const out = (event: Event) => {
      if ((event as MouseEvent).relatedTarget !== null || !inside.current.pointer) return;
      inside.current.pointer = false;
      release();
    };
    document.addEventListener("pointerover", over, true);
    document.addEventListener("pointerout", out, true);
    return () => {
      document.removeEventListener("pointerover", over, true);
      document.removeEventListener("pointerout", out, true);
    };
  }, []);

  const onBlur = (event: FocusEvent) => {
    if (event.relatedTarget instanceof Node && section.current?.contains(event.relatedTarget)) return;
    // La fenêtre du navigateur a perdu le focus : l'élément actif reste dans la liste.
    if (section.current?.contains(document.activeElement)) return;
    inside.current.focus = false;
    release();
  };

  // La confirmation partie avec un bouton « Révoquer » qui n'est plus là : le focus va à la ligne du
  // jeton, ou au titre de la liste si elle ne le montre plus, jamais sur `body`.
  useEffect(() => {
    if (landing === null) return;
    const active = document.activeElement;
    if (active !== null && active !== document.body) return;
    const line = [...(section.current?.querySelectorAll<HTMLElement>("li[data-token-id]") ?? [])].find(
      (li) => li.dataset.tokenId === landing.id,
    );
    (line?.querySelector<HTMLElement>(".mm-token-name") ?? headingRef.current)?.focus();
  }, [landing]);

  // La lecture réussie retire « Réessayer » : s'il avait le focus, le titre de la liste le prend.
  useEffect(() => {
    if (error !== null || !retryFocused.current) return;
    retryFocused.current = false;
    const active = document.activeElement;
    if (active === null || active === document.body) headingRef.current?.focus({ preventScroll: true });
  }, [error, tokens]);

  const retry = async () => {
    if (retrying) return;
    setRetrying(true);
    await reread();
    setRetrying(false);
  };
  const retryButton = (
    <button
      type="button"
      className="mm-button"
      // Grisé sans être retiré pendant le nouvel essai : il garde le focus.
      aria-disabled={retrying || undefined}
      onFocus={() => {
        retryFocused.current = true;
      }}
      onBlur={() => {
        retryFocused.current = false;
      }}
      onClick={() => void retry()}
    >
      Réessayer
    </button>
  );

  const revoke = async () => {
    if (revoking === null || busy) return;
    const target = revoking;
    setBusy(true);
    setRevokeError(null);
    try {
      await revokeToken(target.id);
    } catch (e) {
      // Session perdue : la connexion est demandée par-dessus ; rien de plus à dire ici.
      if (isSessionLost(e)) {
        setBusy(false);
        setRevoking(null);
        return;
      }
      setRevokeError(
        answerLost(e) ? lostAnswer("le jeton a peut-être été révoqué", "La liste relue le dit.") : messageOf(e),
      );
    }
    // Réussie ou refusée : la liste telle que le serveur la voit, avant de rendre la main.
    await reread();
    setBusy(false);
    setRevoking(null);
    setLanding({ id: target.id });
  };

  let content;
  if (tokens === null && error !== null) {
    content = (
      <>
        <FormError message={`La liste des jetons n'a pas pu être lue. ${error}`} />
        <p className="mm-settings-actions">{retryButton}</p>
      </>
    );
  } else if (tokens === null) {
    content = <p className="mm-settings-empty">Chargement…</p>;
  } else if (shown.length === 0) {
    content = <p className="mm-settings-empty">Aucun jeton pour l'instant.</p>;
  } else {
    content = (
      <ul className="mm-people mm-tokens" aria-labelledby={headingId}>
        {shown.map((token) => (
          <TokenLine
            key={token.id}
            token={token}
            open={opened.has(token.id)}
            afterLogin={afterLogin}
            onToggle={() => toggle(token.id)}
            onRevoke={() => setRevoking(token)}
          />
        ))}
      </ul>
    );
  }

  return (
    <section
      ref={section}
      className="mm-settings-section"
      aria-labelledby={headingId}
      aria-busy={tokens === null && error === null}
      onFocus={() => {
        inside.current.focus = true;
        setHeld((was) => was ?? latest.current);
      }}
      onBlur={onBlur}
    >
      <h2 id={headingId} ref={headingRef} className="mm-settings-heading" tabIndex={-1}>
        Tes jetons
      </h2>
      {tokens !== null && error !== null && (
        <>
          <FormError message={`La liste des jetons n'a pas pu être relue. ${error}`} />
          <p className="mm-settings-actions mm-settings-reload">{retryButton}</p>
        </>
      )}
      <FormError message={revokeError} />
      {content}
      {revoking !== null && (
        <Confirm
          title={`Révoquer le jeton ${quoted(revoking.name)}${NBSP}?`}
          confirmLabel="Révoquer"
          destructive
          busy={busy}
          onConfirm={() => void revoke()}
          onCancel={() => setRevoking(null)}
        >
          Les agents qui s'en servent n'auront plus accès à rien, dès leur prochain appel. Une révocation ne
          se défait pas : il faudra créer un autre jeton.
        </Confirm>
      )}
    </section>
  );
}

type LineProps = {
  token: TokenInfo;
  /** Son activité est dépliée sous la ligne. */
  open: boolean;
  afterLogin: () => Promise<void>;
  onToggle: () => void;
  onRevoke: () => void;
};

function TokenLine({ token, open, afterLogin, onToggle, onRevoke }: LineProps) {
  const dead = !token.live;
  const activityId = useId();
  return (
    <li className={dead ? "mm-person mm-token is-dead" : "mm-person mm-token"} data-token-id={token.id}>
      <span className="mm-person-names">
        <span className="mm-person-name">
          {/* La ligne prend le focus quand son bouton part avec la révocation. */}
          <span className="mm-token-name mm-isolate" dir="auto" tabIndex={-1}>
            {token.name}
          </span>
          {dead && <span className="mm-tag mm-token-state">{token.revokedAt !== null ? "Révoqué" : "Échu"}</span>}
        </span>
        <span className="mm-token-detail">
          {LEVEL_LABELS[token.level]} · <ScopeText token={token} />
          {token.canTrash && " · corbeille permise"}
        </span>
        <span className="mm-token-detail">
          se termine par …{token.hint} · {datesOf(token)}
        </span>
      </span>
      <span className="mm-person-action">
        {!dead && (
          <button
            type="button"
            className="mm-button mm-button-quiet"
            aria-label={`Révoquer ${quoted(token.name)}`}
            onClick={onRevoke}
          >
            Révoquer
          </button>
        )}
        {/* En dernier, à la même place sur chaque ligne, jeton mort compris. */}
        <button
          type="button"
          className="mm-button mm-button-quiet mm-token-activity-toggle"
          aria-expanded={open}
          aria-controls={activityId}
          aria-label={`Activité de ${quoted(token.name)}`}
          onClick={(event) => {
            // Le second clic d'un double clic ne la replie pas aussitôt.
            if (event.detail > 1) return;
            // Repliée, l'activité part avec ce qui avait le focus en elle : le bouton le prend, que le
            // clic ne lui donne pas partout (Safari).
            if (open) event.currentTarget.focus();
            onToggle();
          }}
        >
          Activité
        </button>
      </span>
      {open && <TokenActivity token={token} id={activityId} afterLogin={afterLogin} />}
    </li>
  );
}
