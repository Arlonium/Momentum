// La fenêtre « Créer un jeton » (lot 5, Task C1), puis, une fois le jeton créé, le jeton lui-même
// (`TokenCreated`), dans la même fenêtre.
//
// Ce qui se choisit : un nom ; un niveau, « Lecture » ou « Modification », et rien au-dessus (R1218) ;
// une portée, proposée de la plus étroite à la plus large : des pages d'un espace avec leurs
// sous-pages, un espace, ou tous les espaces du compte (R1219, R1664), l'espace affiché coché d'abord
// (R1450) ; la corbeille, permise ou non, avec « Modification » seulement (sous « Lecture », la case
// est grisée et décochée, I4, R1665) ; une échéance (90 jours par défaut, décision L1) ; et le mot de
// passe actuel, que la création redemande (décision L2). Le serveur fait foi : ses refus s'affichent
// tels qu'il les dit, et un mot de passe faux (`401 invalid_credentials`) n'est pas une session perdue.
//
// Avant ces choix, l'écran dit le risque d'une page piégée (LA-c, R1664) : le texte d'une page peut
// pousser l'agent à agir avec les droits du jeton, et la portée est la seule barrière.
//
// Les pages d'une portée « pages » sont celles de l'arbre que montre la barre latérale, et rien
// d'autre (`outlinePages`, `findPages`, R570) : aucune lecture du serveur ne part d'ici, et une page
// que le compte ne voit pas, ou à la corbeille, n'est jamais proposée. Elles sont donc d'un seul
// espace, celui qui est ouvert. Sans recherche, elles viennent dans l'ordre de l'arbre, chaque
// sous-page en retrait sous sa page (R1553). Les listes de pages sont lues à l'ouverture et à chaque
// frappe de la recherche, jamais quand l'arbre change ni quand on coche : rien n'y bouge sous le
// pointeur, une page décochée reste à sa place.
//
// **Le texte du jeton ne vit que dans cette fenêtre** (`created`) : il part avec elle. Une fois le jeton
// montré, rien ne le fait partir sans demander (R1460) : Échap ouvre la confirmation ; Chrome, qui ferme
// quand même la fenêtre au second Échap sans geste entre les deux, la voit se rouvrir et demander ;
// Précédent et Suivant sont retenus par le routeur (`holdHistory`), qui remet l'adresse, et la fenêtre
// demande ; recharger ou fermer l'onglet passe par la question du navigateur (`beforeunload`). Pendant
// la création aussi (R1461) : sa réponse doit trouver la fenêtre pour montrer le jeton, si bien
// qu'Échap, « Annuler » et Précédent n'y font rien. La confirmation est rendue à côté de la fenêtre
// dans l'arbre de React, jamais dedans : son Échap y remonterait.
//
// Une réponse perdue (`answerLost`) : le jeton a peut-être été créé, sans que personne en ait le texte
// ; la fenêtre le dit, et la liste est relue (`onChanged`), qui le montre s'il existe (R757, R1372).
//
// Échap ne détruit rien : fermée sans créer, la fenêtre laisse à l'écran ce qui a été choisi
// (`onDraft`), qu'elle retrouve en se rouvrant, revu contre les espaces et l'arbre du moment (R1463) ;
// jamais le mot de passe, qui ne survit pas à la fenêtre. Un jeton créé repart d'un formulaire vide.
import type { CreateToken as CreateTokenBody, SpaceInfo, TokenLevel, TokenScope } from "@mementum/protocol";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { answerLost, ApiError, createToken, isSessionLost, lostAnswer } from "../api";
import { holdHistory } from "../router";
import { Field, FormError, messageOf } from "../session/forms";
import { ROLE_LABELS } from "../share/levels";
import { findPages, outlinePages, pageTitle, publishedSpace, type FoundPage } from "../space/pageDirectory";
import { Confirm } from "../ui/Confirm";
import { TokenCreated } from "./TokenCreated";

const NBSP = String.fromCharCode(0xa0);

const LEVELS: readonly { id: TokenLevel; label: string; hint: string }[] = [
  { id: "read", label: "Lecture", hint: "Il lit les pages de sa portée, et n'écrit rien." },
  { id: "edit", label: "Modification", hint: "Il les lit, les modifie, et crée des pages." },
];

/** Les échéances proposées, en jours ; `null` : aucune (« une expiration en option », §6.7). */
const EXPIRIES: readonly { days: number | null; label: string }[] = [
  { days: 7, label: "7 jours" },
  { days: 30, label: "30 jours" },
  { days: 90, label: "90 jours" },
  { days: 365, label: "1 an" },
  { days: null, label: "Aucune échéance" },
];
const DEFAULT_EXPIRY = 90;

/** Ce que le serveur admet de pages dans une portée (R1219). */
const MAX_PAGES = 50;
/** Combien de pages la recherche propose à la fois. */
const OFFERED = 8;

/** Ce que la fenêtre retrouve en se rouvrant, sauf le mot de passe. */
export type TokenDraft = {
  name: string;
  level: TokenLevel;
  scope: TokenScope;
  spaceId: string;
  chosen: FoundPage[];
  /** L'espace des pages choisies : un autre espace ouvert depuis, elles ne sont pas reprises. */
  pagesOf: string | null;
  canTrash: boolean;
  expiry: number | null;
};

type Props = {
  /** Les espaces du compte, pour une portée « espace ». */
  spaces: readonly SpaceInfo[];
  /** L'espace ouvert dans la barre latérale : celui d'une portée « pages » ; `null` : aucun. */
  current: SpaceInfo | null;
  /** Le compte, pour les gestionnaires de mots de passe. */
  email: string;
  /** Ce que la fenêtre a gardé à sa dernière fermeture sans création ; `null` : rien. */
  draft: TokenDraft | null;
  /** La fenêtre part sans avoir créé de jeton (`draft`), ou en a créé un (`null`). */
  onDraft: (draft: TokenDraft | null) => void;
  /** La liste des jetons a pu changer (créé, ou peut-être créé) : elle est relue. */
  onChanged: () => void;
  /** Le parent retire la fenêtre. */
  onClose: () => void;
};

/** L'arbre publié est celui de `space` : ses pages peuvent être proposées. */
function treeOf(space: SpaceInfo | null): boolean {
  return space !== null && publishedSpace() === space.id;
}

/**
 * Les pages proposées pour `query` : celles de l'arbre de `space` s'il est l'arbre publié. Sans
 * recherche, dans l'ordre de l'arbre, chaque sous-page sous sa page avec sa profondeur (R1553) ; une
 * recherche, à plat.
 */
function offer(space: SpaceInfo | null, query: string): (FoundPage & { depth?: number })[] {
  if (!treeOf(space)) return [];
  return query.trim() === "" ? outlinePages(OFFERED) : findPages(query, OFFERED);
}

/**
 * Le brouillon, revu contre ce qui vaut à la réouverture (R1463) : l'espace d'une portée « espace »
 * que le compte a quitté devient l'espace affiché ; une portée « pages » sans arbre lu de l'espace
 * affiché revient à la portée par défaut ; une page choisie qui n'est plus dans l'arbre part, les
 * autres prennent leur titre du moment.
 */
function resumed(draft: TokenDraft | null, spaces: readonly SpaceInfo[], current: SpaceInfo | null): TokenDraft | null {
  if (draft === null) return null;
  const fallback = current !== null ? "space" : "all";
  const spaceId = spaces.some((s) => s.id === draft.spaceId) ? draft.spaceId : (current?.id ?? spaces[0]?.id ?? "");
  const pagesHere = current !== null && draft.pagesOf === current.id && treeOf(current);
  const chosen = pagesHere
    ? draft.chosen.flatMap((p) => {
        const title = pageTitle(p.id);
        return title === null ? [] : [{ id: p.id, title }];
      })
    : [];
  let scope = draft.scope;
  if (scope === "pages" && !treeOf(current)) scope = fallback;
  if (scope === "space" && spaces.length === 0) scope = "all";
  return { ...draft, spaceId, scope, chosen };
}

export function CreateToken({ spaces, current, email, draft, onDraft, onChanged, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const levelName = useId();
  const scopeName = useId();
  const expiryId = useId();
  const warningId = useId();
  const nameRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const levelRef = useRef<HTMLInputElement>(null);
  const scopeRef = useRef<HTMLInputElement>(null);
  const expiryRef = useRef<HTMLSelectElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const copyRef = useRef<HTMLDivElement>(null);

  const [start] = useState(() => resumed(draft, spaces, current));
  const [name, setName] = useState(start?.name ?? "");
  const [level, setLevel] = useState<TokenLevel>(start?.level ?? "read");
  const [scope, setScope] = useState<TokenScope>(start?.scope ?? (current !== null ? "space" : "all"));
  const [spaceId, setSpaceId] = useState(start?.spaceId ?? current?.id ?? spaces[0]?.id ?? "");
  const [query, setQuery] = useState("");
  const [offered, setOffered] = useState<(FoundPage & { depth?: number })[]>(() => offer(current, ""));
  const [chosen, setChosen] = useState<FoundPage[]>(start?.chosen ?? []);
  /** Les pages choisies hors de la liste proposée, figées comme elle : décochées, elles restent. */
  const [kept, setKept] = useState<FoundPage[]>(() =>
    (start?.chosen ?? []).filter((p) => !offered.some((o) => o.id === p.id)),
  );
  // La corbeille ne vaut qu'avec « Modification » (mettre à la corbeille et en sortir demandent de
  // modifier, I4) : sous « Lecture », la case est grisée et décochée, et le serveur la refuse.
  const [canTrash, setCanTrash] = useState(start !== null && start.level === "edit" && start.canTrash);
  const [expiry, setExpiry] = useState<number | null>(start === null ? DEFAULT_EXPIRY : start.expiry);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** Le jeton créé, montré cette fois seulement : il part avec la fenêtre. */
  const [created, setCreated] = useState<{ token: string; name: string } | null>(null);
  const [confirming, setConfirming] = useState(false);
  /** Le jeton est montré (lu par la retenue de l'historique, posée une fois). */
  const shown = useRef(false);
  shown.current = created !== null;

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    nameRef.current?.focus();
  }, []);

  // Le formulaire, et le bouton qui avait le focus, sont partis : le focus va au premier geste de la
  // fenêtre (copier la ligne qui attend le jeton, R1462).
  useEffect(() => {
    if (created !== null) copyRef.current?.querySelector<HTMLButtonElement>("[data-first-gesture]")?.focus();
  }, [created]);

  // Rien ne fait partir la fenêtre sans demander tant qu'une création est en route ou que le jeton est
  // montré : ni Précédent (le routeur remet l'adresse), ni le rechargement ou la fermeture de l'onglet.
  const guarded = busy || created !== null;
  useEffect(() => {
    if (!guarded) return;
    const release = holdHistory(() => {
      if (shown.current) setConfirming(true);
    });
    const unload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", unload);
    return () => {
      release();
      window.removeEventListener("beforeunload", unload);
    };
  }, [guarded]);

  /** La fenêtre part : ce qui a été choisi reste à l'écran, si aucun jeton n'a été créé. */
  const leave = () => {
    onDraft(
      created !== null
        ? null
        : { name, level, scope, spaceId, chosen, pagesOf: current?.id ?? null, canTrash, expiry },
    );
    onClose();
  };

  /** Une demande de fermeture (Échap, « Annuler ») : attendue pendant la création, demandée après. */
  const askToLeave = () => {
    if (busy) return;
    if (created !== null) setConfirming(true);
    else leave();
  };

  const chooseLevel = (next: TokenLevel) => {
    setLevel(next);
    if (next === "read") setCanTrash(false);
  };

  const search = (next: string) => {
    const pages = offer(current, next);
    setQuery(next);
    setOffered(pages);
    setKept(chosen.filter((p) => !pages.some((o) => o.id === p.id)));
  };

  const toggle = (found: FoundPage) =>
    setChosen((was) =>
      was.some((p) => p.id === found.id) ? was.filter((p) => p.id !== found.id) : [...was, found],
    );

  /** Le champ à reprendre après un refus du serveur. */
  const fieldOf = (code: string | null): RefObject<HTMLInputElement | HTMLSelectElement | null> | null => {
    switch (code) {
      case "invalid_name":
        return nameRef;
      case "invalid_level":
        return levelRef;
      case "invalid_scope":
        return scope === "pages" ? searchRef : scopeRef;
      case "invalid_expiry":
        return expiryRef;
      case "invalid_credentials":
      case "too_many_attempts":
        return passwordRef;
      default:
        return null;
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setError(null);
    if (scope === "pages" && chosen.length === 0) {
      setError("Choisis au moins une page.");
      searchRef.current?.focus();
      return;
    }
    const body: CreateTokenBody = {
      name,
      level,
      scope,
      canTrash,
      expiresInDays: expiry,
      password,
      ...(scope === "all" ? {} : { spaceId: scope === "pages" ? (current?.id ?? "") : spaceId }),
      ...(scope === "pages" ? { pageIds: chosen.map((p) => p.id) } : {}),
    };
    setBusy(true);
    try {
      const { token } = await createToken(body);
      setCreated({ token, name: name.trim() });
      setPassword("");
      onChanged();
    } catch (e) {
      // Session perdue : la connexion est demandée par-dessus ; le formulaire reste rempli.
      if (isSessionLost(e)) return;
      if (answerLost(e)) {
        setError(
          lostAnswer(
            "le jeton a peut-être été créé",
            "Regarde la liste de tes jetons : s'il y est, tu n'en as pas le texte, révoque-le et crée-en un autre.",
          ),
        );
        onChanged();
        return;
      }
      setError(messageOf(e));
      const target = fieldOf(e instanceof ApiError ? e.code : null)?.current;
      target?.focus();
      if (target instanceof HTMLInputElement) target.select();
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <dialog
        ref={ref}
        className="mm-dialog mm-token-dialog"
        aria-labelledby={titleId}
        // Échap : attendu pendant la création, demandé une fois le jeton montré ; sinon, la fenêtre
        // part. Seulement le sien : celui de la confirmation ne la concerne pas.
        onCancel={(event) => {
          if (event.target !== event.currentTarget) return;
          event.preventDefault();
          askToLeave();
        }}
        // Chrome la ferme quand même au second Échap sans geste entre les deux : pendant la création ou
        // le jeton montré, elle se rouvre (et demande, une fois le jeton montré).
        onClose={(event) => {
          if (event.target !== event.currentTarget) return;
          if (!busy && created === null) return leave();
          event.currentTarget.showModal();
          if (created !== null) setConfirming(true);
        }}
      >
        {created === null ? (
          <>
            <h2 id={titleId} className="mm-dialog-title">
              Créer un jeton
            </h2>
            <p className="mm-dialog-lead">
              Un jeton laisse un agent (Claude Code, un script) lire ou modifier tes pages en ton nom. Il
              ne fait jamais plus que toi.
            </p>
            <form className="mm-auth-form" onSubmit={submit} noValidate>
              <Field
                ref={nameRef}
                label="Nom du jeton"
                name="token-name"
                autoComplete="off"
                hint="Pour le reconnaître dans la liste : « Claude Code au bureau »."
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
              <p id={warningId} className="mm-token-warning">
                Le texte d'une page peut tromper un agent qui la lit, et le pousser à agir avec les droits
                de ce jeton : recopier ce qu'il lit dans une page qu'il peut modifier, par exemple. La
                portée est la seule barrière : choisis la portée la plus étroite qui suffit, et
                «&nbsp;Lecture&nbsp;» s'il n'a rien à écrire.
              </p>
              <fieldset className="mm-token-choices" aria-describedby={warningId}>
                <legend>Niveau</legend>
                {LEVELS.map((item, i) => (
                  <Choice
                    key={item.id}
                    inputRef={i === 0 ? levelRef : undefined}
                    type="radio"
                    name={`${levelName}-level`}
                    value={item.id}
                    checked={level === item.id}
                    onChange={() => chooseLevel(item.id)}
                    label={item.label}
                    hint={item.hint}
                  />
                ))}
              </fieldset>
              <fieldset className="mm-token-choices" aria-describedby={warningId}>
                <legend>Portée</legend>
                <Choice
                  type="radio"
                  name={`${scopeName}-scope`}
                  value="pages"
                  checked={scope === "pages"}
                  disabled={current === null}
                  onChange={() => setScope("pages")}
                  label={
                    current === null ? (
                      "Des pages"
                    ) : (
                      <>
                        Des pages de «&nbsp;
                        <span className="mm-isolate" dir="auto">
                          {current.name}
                        </span>
                        &nbsp;», avec leurs sous-pages
                      </>
                    )
                  }
                  hint="Celles de l'espace ouvert dans la barre latérale ; pour un autre espace, ouvre-le d'abord."
                />
                {scope === "pages" && (
                  <PagePicker
                    searchRef={searchRef}
                    query={query}
                    offered={offered}
                    kept={kept}
                    chosen={chosen}
                    treeRead={treeOf(current)}
                    onSearch={search}
                    onToggle={toggle}
                  />
                )}
                <Choice
                  inputRef={scope === "space" ? scopeRef : undefined}
                  type="radio"
                  name={`${scopeName}-scope`}
                  value="space"
                  checked={scope === "space"}
                  disabled={spaces.length === 0}
                  onChange={() => setScope("space")}
                  label="Un espace"
                />
                {scope === "space" && (
                  <select
                    className="mm-select mm-select-boxed mm-token-space"
                    aria-label="Espace du jeton"
                    value={spaceId}
                    onChange={(e) => setSpaceId(e.target.value)}
                  >
                    {spaces.map((item) => (
                      <option key={item.id} value={item.id}>
                        {item.name} ({ROLE_LABELS[item.role]})
                      </option>
                    ))}
                  </select>
                )}
                <Choice
                  inputRef={scope === "all" ? scopeRef : undefined}
                  type="radio"
                  name={`${scopeName}-scope`}
                  value="all"
                  checked={scope === "all"}
                  onChange={() => setScope("all")}
                  label="Tous mes espaces"
                />
              </fieldset>
              <p className="mm-token-hint">
                Avec «&nbsp;Modification&nbsp;», un agent modifie les pages de sa portée, et crée des
                pages à la racine d'un espace où tu es membre, dans «&nbsp;Privé&nbsp;» par défaut ou
                dans «&nbsp;Espace&nbsp;» s'il le demande, ou sous une page qu'il peut modifier (limité à
                des pages, il ne crée que sous elles). Il ne déplace jamais une page sous une autre, ne
                supprime rien pour de bon et ne partage rien.
              </p>
              <Choice
                type="checkbox"
                checked={canTrash}
                disabled={level === "read"}
                onChange={(checked) => setCanTrash(checked)}
                label="Permettre la corbeille"
                hint="Mettre une page à la corbeille et l'en sortir, avec «&nbsp;Modification&nbsp;» seulement."
              />
              <div className="mm-field">
                <label htmlFor={expiryId}>Échéance</label>
                <select
                  ref={expiryRef}
                  id={expiryId}
                  className="mm-select mm-select-boxed mm-token-expiry"
                  value={expiry === null ? "" : String(expiry)}
                  onChange={(e) => setExpiry(e.target.value === "" ? null : Number(e.target.value))}
                >
                  {EXPIRIES.map((item) => (
                    <option key={item.label} value={item.days === null ? "" : String(item.days)}>
                      {item.label}
                    </option>
                  ))}
                </select>
              </div>
              {/* Pour les gestionnaires de mots de passe : le compte dont on redonne le mot de passe. */}
              <input type="email" name="username" autoComplete="username" value={email} readOnly hidden />
              <Field
                ref={passwordRef}
                label="Mot de passe actuel"
                type="password"
                name="current-password"
                autoComplete="current-password"
                hint="La création d'un jeton le redemande."
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
              <FormError message={error} />
              <div className="mm-dialog-actions">
                {/* Grisés par `aria-disabled` : ils gardent le focus pendant la demande, et « Annuler »
                    n'y fait rien (la réponse doit trouver la fenêtre). */}
                <button
                  type="button"
                  className="mm-button"
                  aria-disabled={busy || undefined}
                  onClick={askToLeave}
                >
                  Annuler
                </button>
                <button
                  type="submit"
                  className="mm-button mm-button-primary"
                  aria-disabled={busy || undefined}
                >
                  Créer le jeton
                </button>
              </div>
            </form>
          </>
        ) : (
          <>
            <h2 id={titleId} className="mm-dialog-title">
              Jeton créé
            </h2>
            <p className="mm-dialog-lead">
              «&nbsp;
              <span className="mm-isolate" dir="auto">
                {created.name}
              </span>
              &nbsp;» est prêt. Tu pourras le révoquer à tout moment depuis la liste de tes jetons.
            </p>
            <div ref={copyRef}>
              <TokenCreated token={created.token} origin={location.origin} />
            </div>
            <div className="mm-dialog-actions">
              <button type="button" className="mm-button mm-button-primary" onClick={leave}>
                Fermer
              </button>
            </div>
          </>
        )}
      </dialog>
      {confirming && (
        <Confirm
          title={`Fermer la fenêtre du jeton${NBSP}?`}
          confirmLabel="Fermer"
          cancelLabel="Rester"
          onConfirm={leave}
          onCancel={() => setConfirming(false)}
        >
          Le jeton ne sera plus jamais affiché. Si tu ne l'as pas copié, il faudra en créer un autre.
        </Confirm>
      )}
    </>
  );
}

type ChoiceProps = {
  type: "radio" | "checkbox";
  name?: string;
  value?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  /** Sa précision : dite à part (`aria-describedby`), hors de son nom. */
  hint?: string;
  inputRef?: RefObject<HTMLInputElement | null>;
};

/** Un choix : son nom, puis sa précision dessous, qui le décrit sans entrer dans son nom. */
function Choice({ type, name, value, checked, disabled, onChange, label, hint, inputRef }: ChoiceProps) {
  const id = useId();
  return (
    <label className={type === "radio" ? "mm-token-choice" : "mm-token-check"}>
      <input
        ref={inputRef}
        type={type}
        name={name}
        value={value}
        checked={checked}
        disabled={disabled}
        aria-labelledby={`${id}-label`}
        aria-describedby={hint === undefined ? undefined : `${id}-hint`}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="mm-token-choice-text">
        <span id={`${id}-label`} className="mm-token-choice-label">
          {label}
        </span>
        {hint !== undefined && (
          <span id={`${id}-hint`} className="mm-token-choice-hint">
            {hint}
          </span>
        )}
      </span>
    </label>
  );
}

type PickerProps = {
  searchRef: RefObject<HTMLInputElement | null>;
  query: string;
  /** Avec leur profondeur dans l'arbre quand on ne cherche rien (R1553). */
  offered: readonly (FoundPage & { depth?: number })[];
  kept: readonly FoundPage[];
  chosen: readonly FoundPage[];
  /** L'arbre publié est celui de l'espace de la portée. */
  treeRead: boolean;
  onSearch: (query: string) => void;
  onToggle: (page: FoundPage) => void;
};

/**
 * Les pages d'une portée : une recherche dans l'arbre affiché, les pages qu'elle propose, puis celles
 * choisies plus tôt hors de cette liste ; des cases, qui restent à leur place cochées ou non. Cinquante
 * pages au plus. Entrée dans la recherche n'envoie pas le formulaire.
 */
function PagePicker({ searchRef, query, offered, kept, chosen, treeRead, onSearch, onToggle }: PickerProps) {
  const isChosen = (id: string) => chosen.some((p) => p.id === id);
  const full = chosen.length >= MAX_PAGES;
  let empty: string | null = null;
  if (offered.length === 0) {
    if (!treeRead) {
      empty =
        "Les pages de cet espace ne sont pas encore lues : ouvre-le dans la barre latérale, puis rouvre cette fenêtre.";
    } else if (query.trim() === "") empty = "Cet espace n'a aucune page que tu vois.";
    else empty = "Aucune page de l'arbre ne porte ce titre.";
  }
  // Une sous-page en retrait de sa profondeur (`--depth`, comme une ligne de la barre latérale).
  const box = (found: FoundPage, depth = 0) => (
    <li key={found.id} style={depth > 0 ? ({ "--depth": depth } as CSSProperties) : undefined}>
      <label className="mm-token-check">
        <input
          type="checkbox"
          checked={isChosen(found.id)}
          disabled={full && !isChosen(found.id)}
          onChange={() => onToggle(found)}
        />
        <span className="mm-isolate" dir="auto">
          {found.title || "Sans titre"}
        </span>
      </label>
    </li>
  );
  return (
    <div className="mm-token-pages">
      <input
        ref={searchRef}
        className="mm-input"
        type="text"
        aria-label="Chercher une page"
        placeholder="Chercher une page"
        autoComplete="off"
        value={query}
        onChange={(e) => onSearch(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.preventDefault();
        }}
      />
      {empty !== null ? (
        <p className="mm-token-choice-hint">{empty}</p>
      ) : (
        <ul className="mm-token-offered" aria-label="Pages proposées">
          {offered.map((found) => box(found, found.depth))}
        </ul>
      )}
      <p className="mm-token-choice-hint" role="status">
        {chosen.length === 0
          ? "Aucune page choisie."
          : `${chosen.length} ${chosen.length > 1 ? "pages choisies" : "page choisie"}${full ? ", le plus qu'un jeton admet : pour d'autres, crée un autre jeton ou choisis leur page parente." : "."}`}
      </p>
      {kept.length > 0 && (
        <ul className="mm-token-offered" aria-label="Autres pages choisies">
          {kept.map((found) => box(found))}
        </ul>
      )}
    </div>
  );
}
