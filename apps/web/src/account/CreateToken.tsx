// La fenêtre « Créer un jeton » (lot 5, Task C1), puis, une fois le jeton créé, le jeton lui-même
// (`TokenCreated`), dans la même fenêtre.
//
// Ce qui se choisit : un nom ; un niveau, « Lecture » ou « Modification », et rien au-dessus (R1218) ;
// une portée, tous les espaces du compte, un espace, ou des pages d'un espace avec leurs sous-pages
// (R1219) ; la corbeille, permise ou non ; une échéance (90 jours par défaut, décision L1) ; et le mot
// de passe actuel, que la création redemande (décision L2). Le serveur fait foi : ses refus
// s'affichent tels qu'il les dit, et un mot de passe faux (`401 invalid_credentials`) n'est pas une
// session perdue.
//
// Les pages d'une portée « pages » sont celles de l'arbre que montre la barre latérale, et rien
// d'autre (`findPages`, R570) : aucune lecture du serveur ne part d'ici, et une page que le compte ne
// voit pas, ou à la corbeille, n'est jamais proposée. Elles sont donc d'un seul espace, celui qui est
// ouvert. La liste proposée est lue à l'ouverture et à chaque frappe dans sa recherche, jamais quand
// l'arbre change : rien n'y bouge sous le pointeur. Une page choisie reste à sa place, cochée.
//
// **Le texte du jeton ne vit que dans cette fenêtre** (`created`) : il part avec elle. Échap, une fois
// le jeton montré, demande avant de fermer ; Chrome, qui ferme quand même la fenêtre au second Échap
// sans geste entre les deux, la voit se rouvrir et demander. La confirmation est rendue à côté de la
// fenêtre dans l'arbre de React, jamais dedans : son Échap y remonterait.
//
// Une réponse perdue (`answerLost`) : le jeton a peut-être été créé, sans que personne en ait le texte
// ; la fenêtre le dit, et la liste est relue (`onChanged`), qui le montre s'il existe (R757, R1372).
import type { CreateToken as CreateTokenBody, SpaceInfo, TokenLevel, TokenScope } from "@mementum/protocol";
import { useEffect, useId, useRef, useState, type FormEvent, type RefObject } from "react";
import { answerLost, ApiError, createToken, isSessionLost, lostAnswer } from "../api";
import { Field, FormError, messageOf } from "../session/forms";
import { ROLE_LABELS } from "../share/levels";
import { findPages, publishedSpace, type FoundPage } from "../space/pageDirectory";
import { Confirm } from "../ui/Confirm";
import { TokenCreated } from "./TokenCreated";

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

type Props = {
  /** Les espaces du compte, pour une portée « espace ». */
  spaces: readonly SpaceInfo[];
  /** L'espace ouvert dans la barre latérale : celui d'une portée « pages » ; `null` : aucun. */
  current: SpaceInfo | null;
  /** Le compte, pour les gestionnaires de mots de passe. */
  email: string;
  /** La liste des jetons a pu changer (créé, ou peut-être créé) : elle est relue. */
  onChanged: () => void;
  /** Le parent retire la fenêtre. */
  onClose: () => void;
};

/** Les pages proposées pour `query` : celles de l'arbre de `space` s'il est l'arbre publié. */
function offer(space: SpaceInfo | null, query: string): FoundPage[] {
  return space !== null && publishedSpace() === space.id ? findPages(query, OFFERED) : [];
}

export function CreateToken({ spaces, current, email, onChanged, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const levelName = useId();
  const scopeName = useId();
  const expiryId = useId();
  const nameRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const levelRef = useRef<HTMLInputElement>(null);
  const scopeRef = useRef<HTMLInputElement>(null);
  const expiryRef = useRef<HTMLSelectElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const copyRef = useRef<HTMLDivElement>(null);

  const [name, setName] = useState("");
  const [level, setLevel] = useState<TokenLevel>("read");
  const [scope, setScope] = useState<TokenScope>(current !== null ? "space" : "all");
  const [spaceId, setSpaceId] = useState(current?.id ?? spaces[0]?.id ?? "");
  const [query, setQuery] = useState("");
  const [offered, setOffered] = useState<FoundPage[]>(() => offer(current, ""));
  const [chosen, setChosen] = useState<FoundPage[]>([]);
  const [canTrash, setCanTrash] = useState(false);
  const [expiry, setExpiry] = useState<number | null>(DEFAULT_EXPIRY);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** Le jeton créé, montré cette fois seulement : il part avec la fenêtre. */
  const [created, setCreated] = useState<{ token: string; name: string } | null>(null);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    nameRef.current?.focus();
  }, []);

  // Le formulaire, et le bouton qui avait le focus, sont partis : le focus va au bouton qui copie le
  // jeton, la première chose à faire.
  useEffect(() => {
    if (created !== null) copyRef.current?.querySelector<HTMLButtonElement>("[data-copy-token]")?.focus();
  }, [created]);

  const search = (next: string) => {
    setQuery(next);
    setOffered(offer(current, next));
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
        // Échap : le jeton montré, on demande d'abord ; sinon, la fenêtre part. Seulement le sien :
        // celui de la confirmation ne la concerne pas.
        onCancel={(event) => {
          if (event.target !== event.currentTarget) return;
          event.preventDefault();
          if (created !== null) setConfirming(true);
          else onClose();
        }}
        // Chrome la ferme quand même au second Échap sans geste entre les deux : le jeton montré, elle
        // se rouvre et demande.
        onClose={(event) => {
          if (event.target !== event.currentTarget) return;
          if (created === null) return onClose();
          event.currentTarget.showModal();
          setConfirming(true);
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
              <fieldset className="mm-token-choices">
                <legend>Niveau</legend>
                {LEVELS.map((item, i) => (
                  <label key={item.id} className="mm-token-choice">
                    <input
                      ref={i === 0 ? levelRef : undefined}
                      type="radio"
                      name={`${levelName}-level`}
                      value={item.id}
                      checked={level === item.id}
                      onChange={() => setLevel(item.id)}
                    />
                    <span className="mm-token-choice-text">
                      <span className="mm-token-choice-label">{item.label}</span>
                      <span className="mm-token-choice-hint">{item.hint}</span>
                    </span>
                  </label>
                ))}
              </fieldset>
              <fieldset className="mm-token-choices">
                <legend>Portée</legend>
                <label className="mm-token-choice">
                  <input
                    ref={scopeRef}
                    type="radio"
                    name={`${scopeName}-scope`}
                    value="all"
                    checked={scope === "all"}
                    onChange={() => setScope("all")}
                  />
                  <span className="mm-token-choice-text">
                    <span className="mm-token-choice-label">Tous mes espaces</span>
                  </span>
                </label>
                <label className="mm-token-choice">
                  <input
                    type="radio"
                    name={`${scopeName}-scope`}
                    value="space"
                    checked={scope === "space"}
                    disabled={spaces.length === 0}
                    onChange={() => setScope("space")}
                  />
                  <span className="mm-token-choice-text">
                    <span className="mm-token-choice-label">Un espace</span>
                  </span>
                </label>
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
                <label className="mm-token-choice">
                  <input
                    type="radio"
                    name={`${scopeName}-scope`}
                    value="pages"
                    checked={scope === "pages"}
                    disabled={current === null}
                    onChange={() => setScope("pages")}
                  />
                  <span className="mm-token-choice-text">
                    <span className="mm-token-choice-label">
                      {current === null ? (
                        "Des pages"
                      ) : (
                        <>
                          Des pages de «&nbsp;
                          <span className="mm-isolate" dir="auto">
                            {current.name}
                          </span>
                          &nbsp;», avec leurs sous-pages
                        </>
                      )}
                    </span>
                    <span className="mm-token-choice-hint">
                      Celles de l'espace ouvert dans la barre latérale ; pour un autre espace, ouvre-le
                      d'abord.
                    </span>
                  </span>
                </label>
                {scope === "pages" && (
                  <PagePicker
                    searchRef={searchRef}
                    query={query}
                    offered={offered}
                    chosen={chosen}
                    onSearch={search}
                    onToggle={toggle}
                  />
                )}
              </fieldset>
              <p className="mm-token-hint">
                Avec «&nbsp;Modification&nbsp;», un agent modifie les pages de sa portée, et crée des
                pages à la racine d'un espace où tu es membre, dans «&nbsp;Privé&nbsp;» par défaut ou
                dans «&nbsp;Espace&nbsp;» s'il le demande, ou sous une page qu'il peut modifier (limité à
                des pages, il ne crée que sous elles). Il ne déplace jamais une page sous une autre, ne
                supprime rien pour de bon et ne partage rien.
              </p>
              <label className="mm-token-check">
                <input type="checkbox" checked={canTrash} onChange={(e) => setCanTrash(e.target.checked)} />
                <span className="mm-token-choice-text">
                  <span className="mm-token-choice-label">Permettre la corbeille</span>
                  <span className="mm-token-choice-hint">
                    Mettre une page à la corbeille et l'en sortir.
                  </span>
                </span>
              </label>
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
                <button type="button" className="mm-button" onClick={onClose}>
                  Annuler
                </button>
                {/* Grisé par `aria-disabled` : il garde le focus pendant la demande. */}
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
              <button type="button" className="mm-button mm-button-primary" onClick={onClose}>
                Fermer
              </button>
            </div>
          </>
        )}
      </dialog>
      {confirming && (
        <Confirm
          title={"Fermer la fenêtre du jeton\u00a0?"}
          confirmLabel="Fermer"
          cancelLabel="Rester"
          onConfirm={onClose}
          onCancel={() => setConfirming(false)}
        >
          Le jeton ne sera plus jamais affiché. Si tu ne l'as pas copié, il faudra en créer un autre.
        </Confirm>
      )}
    </>
  );
}

type PickerProps = {
  searchRef: RefObject<HTMLInputElement | null>;
  query: string;
  offered: readonly FoundPage[];
  chosen: readonly FoundPage[];
  onSearch: (query: string) => void;
  onToggle: (page: FoundPage) => void;
};

/**
 * Les pages d'une portée : une recherche dans l'arbre affiché, les pages qu'elle propose (des cases,
 * qui restent à leur place une fois cochées), puis celles qui sont choisies, recherche changée
 * comprise. Cinquante au plus.
 */
function PagePicker({ searchRef, query, offered, chosen, onSearch, onToggle }: PickerProps) {
  const isChosen = (id: string) => chosen.some((p) => p.id === id);
  const full = chosen.length >= MAX_PAGES;
  const elsewhere = chosen.filter((p) => !offered.some((o) => o.id === p.id));
  return (
    <div className="mm-token-pages">
      <input
        ref={searchRef}
        className="mm-input"
        type="search"
        aria-label="Chercher une page"
        placeholder="Chercher une page"
        autoComplete="off"
        value={query}
        onChange={(e) => onSearch(e.target.value)}
      />
      {offered.length === 0 ? (
        <p className="mm-token-choice-hint">Aucune page de l'arbre ne porte ce titre.</p>
      ) : (
        <ul className="mm-token-offered" aria-label="Pages proposées">
          {offered.map((found) => (
            <li key={found.id}>
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
          ))}
        </ul>
      )}
      <p className="mm-token-choice-hint" role="status">
        {chosen.length === 0
          ? "Aucune page choisie."
          : `${chosen.length} ${chosen.length > 1 ? "pages choisies" : "page choisie"}${full ? ", le plus qu'un jeton admet : pour d'autres, crée un autre jeton ou choisis leur page parente." : "."}`}
      </p>
      {elsewhere.length > 0 && (
        <ul className="mm-token-offered" aria-label="Autres pages choisies">
          {elsewhere.map((found) => (
            <li key={found.id}>
              <label className="mm-token-check">
                <input type="checkbox" checked onChange={() => onToggle(found)} />
                <span className="mm-isolate" dir="auto">
                  {found.title || "Sans titre"}
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
