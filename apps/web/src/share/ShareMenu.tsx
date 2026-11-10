// Le menu Partager d'une page (conception, §4.3) : le bouton « Partager », en haut à droite, et
// le panneau qu'il ouvre sous lui, comme dans Notion. Pour qui a l'accès complet sur la page.
// De haut en bas :
// 1. inviter une adresse, à un niveau : l'invitation est un lien, montré cette fois seulement ;
// 2. les personnes qui ont un accès nommé, avec leur niveau, qui se change ici, et
//    « Retirer » ; un accès posé sur une page parente est marqué « hérité », et se change sur
//    elle ; puis les invitations de la page qui attendent, avec « Révoquer » ;
// 3. l'accès général : « Seulement les invités », ou « Tous les membres de l'espace », à un
//    niveau. Celui qu'une page parente donne déjà est un plancher (R93) : il se relève, sans se
//    baisser ni se retirer. Un invité de l'espace le lit sans le régler (R85) ;
// 4. « Copier le lien » de la page.
// Le partage vaut pour la page et ses sous-pages. Pas d'onglet « Publier » : la publication
// vient en P3.
//
// Le serveur fait foi : ses refus s'affichent tels quels, et les accès reviennent à ce qu'il
// en dit (`useShare`). Quand la personne n'a plus l'accès complet (elle s'est baissée ou
// retirée elle-même, ou on le lui a retiré), le menu se ferme et la page est relue.
//
// Ce n'est pas une fenêtre : le panneau s'ouvre sous son bouton, sans voile. Ouvert, il donne
// le focus au champ d'invitation ; Tab y passe d'un réglage au suivant ; Échap le ferme et
// rend le focus au bouton ; un clic dehors, ou le focus qui en sort, le ferment aussi.
import type { AccessEntry, Level, PageNode, PendingInvitation } from "@mementum/protocol";
import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type FocusEvent,
  type FormEvent,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { CheckIcon, LinkIcon, MailIcon } from "../icons";
import { pagePath } from "../router";
import { FormError } from "../session/forms";
import { useSession } from "../session/session";
import { Avatar } from "../ui/Avatar";
import { Confirm } from "../ui/Confirm";
import { CreatedInvitation } from "./CreatedInvitation";
import { generalAccess, LEVEL_LABELS, LEVELS } from "./levels";
import { useShare, type Share } from "./useShare";

type Props = {
  /** La page ouverte, où la personne a l'accès complet. */
  node: PageNode;
  /** Les accès de la page ont changé : la barre latérale est à relire. */
  onChanged: () => void;
  /**
   * La personne n'a plus l'accès complet à la page, ou plus la page : elle est à relire.
   * `refusal` : le refus du serveur qui l'a révélé, s'il y en a un.
   */
  onAccessLost: (refusal: string | null) => void;
};

/** Le niveau proposé d'abord à une invitation. */
const INVITE_LEVEL: Level = "edit";
/** Le niveau donné aux membres quand la page leur est ouverte : le plus faible, qui se relève ensuite. */
const OPENING_LEVEL: Level = "read";

const expiry = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "long",
  hour: "2-digit",
  minute: "2-digit",
});

export function ShareMenu({ node, onChanged, onAccessLost }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const panelId = useId();
  /** La personne dont on retire l'accès, le temps de la confirmation. */
  const [removing, setRemoving] = useState<AccessEntry | null>(null);
  /** Compte les fermetures de la confirmation, pour rattraper le focus après chacune. */
  const [confirmed, setConfirmed] = useState(0);

  // La confirmation fermée rend le focus au bouton « Retirer » qui l'a ouverte (`Confirm`).
  // Parti avec sa ligne, ce bouton n'est plus là : le focus va au panneau, pas dans le vide.
  useEffect(() => {
    if (confirmed === 0) return;
    const active = document.activeElement;
    if (active === null || active === document.body) panelRef.current?.focus();
  }, [confirmed]);

  const lost = useCallback(
    (refusal: string | null) => {
      setOpen(false);
      setRemoving(null);
      onAccessLost(refusal);
    },
    [onAccessLost],
  );
  const share = useShare(node.id, open, { onChanged, onAccessLost: lost });

  // Un clic hors du menu le ferme. Le focus revient au bouton, sauf si le clic l'a posé
  // ailleurs (dans la page, par exemple) : on ne le lui reprend pas.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (rootRef.current?.contains(event.target as Node)) return;
      setOpen(false);
      // Après le clic : le navigateur a alors posé le focus là où il devait aller.
      setTimeout(() => {
        const active = document.activeElement;
        if (active === null || active === document.body) buttonRef.current?.focus();
      }, 0);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // La fenêtre de confirmation a son propre Échap.
    if (event.key !== "Escape" || !open || removing !== null) return;
    event.preventDefault();
    event.stopPropagation();
    setOpen(false);
    buttonRef.current?.focus();
  };

  const onBlur = (event: FocusEvent<HTMLDivElement>) => {
    // Le focus part vers un autre élément de la page (Tab) : le menu se ferme. S'il ne va
    // nulle part (une autre fenêtre du système, le temps d'y copier une adresse), il reste.
    const next = event.relatedTarget;
    if (next instanceof Node && !event.currentTarget.contains(next)) setOpen(false);
  };

  return (
    <div ref={rootRef} className="mm-share" onKeyDown={onKeyDown} onBlur={onBlur}>
      <button
        ref={buttonRef}
        type="button"
        className="mm-button"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        onClick={() => setOpen((was) => !was)}
      >
        Partager
      </button>
      {open && (
        <Panel id={panelId} panelRef={panelRef} node={node} share={share} onRemove={setRemoving} />
      )}
      {removing && (
        <RemoveConfirm
          entry={removing}
          busy={share.busy !== null}
          onConfirm={async () => {
            await share.remove(removing.userId);
            setRemoving(null);
            setConfirmed((n) => n + 1);
          }}
          onCancel={() => setRemoving(null)}
        />
      )}
    </div>
  );
}

type RemoveProps = {
  entry: AccessEntry;
  busy: boolean;
  onConfirm: () => Promise<void>;
  onCancel: () => void;
};

/** « Retirer » demande une confirmation dans la page : pour un invité, c'est sans retour simple. */
function RemoveConfirm({ entry, busy, onConfirm, onCancel }: RemoveProps) {
  const self = useSession().me.user.id === entry.userId;
  return (
    <Confirm
      // Espace insécable : le « ? » ne passe pas seul à la ligne.
      title={self ? "Retirer ton accès à cette page\u00a0?" : `Retirer l'accès de ${entry.name}\u00a0?`}
      confirmLabel="Retirer"
      destructive
      busy={busy}
      onConfirm={() => void onConfirm()}
      onCancel={onCancel}
    >
      {self
        ? "Tu ne pourras plus ouvrir cette page ni ses sous-pages, sauf si un autre accès te les laisse voir."
        : `${entry.name} ne pourra plus ouvrir cette page ni ses sous-pages, sauf si un autre accès les lui laisse voir. Pour un invité de l'espace, il faudra une nouvelle invitation.`}
    </Confirm>
  );
}

type PanelProps = {
  id: string;
  panelRef: RefObject<HTMLElement | null>;
  node: PageNode;
  share: Share;
  onRemove: (entry: AccessEntry) => void;
};

function Panel({ id, panelRef, node, share, onRemove }: PanelProps) {
  const { me } = useSession();
  const emailRef = useRef<HTMLInputElement>(null);
  const { access, busy } = share;
  /** Un invité de l'espace ne règle pas l'accès général (R85) ; le serveur fait foi. */
  const guest = me.spaces.find((space) => space.id === node.spaceId)?.role === "guest";

  // Ouvert : le champ d'invitation a le focus.
  useEffect(() => {
    emailRef.current?.focus();
  }, []);

  // Une invitation révoquée part avec son bouton, qui avait le focus : il va au panneau, pas
  // dans le vide, d'où Tab repart.
  const acting = busy !== null;
  useEffect(() => {
    if (acting) return;
    const active = document.activeElement;
    if (active === null || active === document.body) panelRef.current?.focus();
  }, [acting, access, panelRef]);

  return (
    <section
      ref={panelRef}
      id={id}
      role="dialog"
      aria-label="Partager"
      // Occupé dès l'ouverture, avant même que la première lecture soit partie.
      aria-busy={share.reading || (access === null && share.readError === null)}
      className="mm-share-panel"
      tabIndex={-1}
    >
      <div className="mm-share-body">
        <InviteForm share={share} emailRef={emailRef} />
        {share.readError && (
          <div className="mm-share-read-error">
            <FormError
              message={`Les accès n'ont pas pu être ${access === null ? "lus" : "relus"}. ${share.readError}`}
            />
            <button type="button" className="mm-button mm-button-small" onClick={() => void share.reload()}>
              Réessayer
            </button>
          </div>
        )}
        <FormError message={share.error} />
        {access === null ? (
          !share.readError && <p className="mm-share-note">Chargement…</p>
        ) : (
          <>
            <People entries={access.entries} self={me.user.id} share={share} onRemove={onRemove} />
            {access.invitations.length > 0 && (
              <Pending invitations={access.invitations} share={share} />
            )}
            <General share={share} guest={guest} />
          </>
        )}
      </div>
      <Footer pageId={node.id} />
    </section>
  );
}

type InviteProps = { share: Share; emailRef: RefObject<HTMLInputElement | null> };

function InviteForm({ share, emailRef }: InviteProps) {
  const emailId = useId();
  const levelId = useId();
  const [email, setEmail] = useState("");
  const [level, setLevel] = useState<Level>(INVITE_LEVEL);
  const { created } = share;

  const invite = async (event: FormEvent) => {
    event.preventDefault();
    if (await share.invite(email, level)) setEmail("");
  };

  return (
    <div className="mm-share-invite">
      <form className="mm-settings-row" onSubmit={invite} noValidate>
        <label htmlFor={emailId} className="mm-visually-hidden">
          Adresse e-mail
        </label>
        <input
          ref={emailRef}
          id={emailId}
          className="mm-input"
          type="email"
          autoComplete="off"
          placeholder="Adresse e-mail"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <label htmlFor={levelId} className="mm-visually-hidden">
          Niveau de l'invitation
        </label>
        <select
          id={levelId}
          className="mm-select mm-select-boxed"
          value={level}
          onChange={(e) => setLevel(e.target.value as Level)}
        >
          {LEVELS.map((choice) => (
            <option key={choice} value={choice}>
              {LEVEL_LABELS[choice]}
            </option>
          ))}
        </select>
        <button
          type="submit"
          className="mm-button mm-button-primary"
          aria-disabled={share.busy !== null || undefined}
        >
          Inviter
        </button>
      </form>
      <p className="mm-share-note">
        Un lien à envoyer toi-même : il vaut 7 jours et ne sert qu'une fois.
      </p>
      <FormError message={share.inviteError} />
      {created && (
        <CreatedInvitation
          email={created.email}
          grant={LEVEL_LABELS[created.level]}
          link={created.link}
          copyLabel="Copier le lien d'invitation"
        />
      )}
    </div>
  );
}

type PeopleProps = {
  entries: AccessEntry[];
  self: string;
  share: Share;
  onRemove: (entry: AccessEntry) => void;
};

function People({ entries, self, share, onRemove }: PeopleProps) {
  if (entries.length === 0) {
    return <p className="mm-share-note">Personne n'a d'accès à son nom sur cette page.</p>;
  }
  const busy = share.busy !== null;
  return (
    <div>
      <ul className="mm-people mm-share-people" aria-label="Personnes qui ont accès">
        {entries.map((entry) => (
          <li key={entry.userId} className="mm-person">
            <Avatar id={entry.userId} name={entry.name} size={28} />
            <span className="mm-person-names">
              <span className="mm-person-name">
                {entry.name}
                {entry.userId === self && <span className="mm-person-you"> (toi)</span>}
              </span>
              <span className="mm-person-detail">{entry.email}</span>
            </span>
            <span className="mm-person-role">
              {entry.inherited ? (
                LEVEL_LABELS[entry.level]
              ) : (
                <select
                  className="mm-select"
                  aria-label={`Niveau de ${entry.name}`}
                  value={entry.level}
                  disabled={busy}
                  onChange={(e) => void share.setLevel(entry.userId, e.target.value as Level)}
                >
                  {LEVELS.map((level) => (
                    <option key={level} value={level}>
                      {LEVEL_LABELS[level]}
                    </option>
                  ))}
                </select>
              )}
            </span>
            <span className="mm-person-action">
              {entry.inherited ? (
                <span className="mm-tag">hérité</span>
              ) : (
                <button
                  type="button"
                  className="mm-button mm-button-quiet"
                  aria-disabled={busy || undefined}
                  onClick={() => !busy && onRemove(entry)}
                >
                  Retirer
                </button>
              )}
            </span>
          </li>
        ))}
      </ul>
      {entries.some((entry) => entry.inherited) && (
        <p className="mm-share-note">
          Un accès hérité vient d'une page parente : il se change sur la page parente.
        </p>
      )}
    </div>
  );
}

function Pending({ invitations, share }: { invitations: PendingInvitation[]; share: Share }) {
  const busy = share.busy !== null;
  return (
    <ul className="mm-people mm-share-people" aria-label="Invitations en attente">
      {invitations.map((invitation) => (
        <li key={invitation.id} className="mm-person">
          <span className="mm-person-tile">
            <MailIcon />
          </span>
          <span className="mm-person-names">
            <span className="mm-person-name">{invitation.email}</span>
            <span className="mm-person-detail">
              {invitation.level ? `${LEVEL_LABELS[invitation.level]} · ` : ""}expire le{" "}
              {expiry.format(new Date(invitation.expiresAt))}
            </span>
          </span>
          <span className="mm-person-action">
            <button
              type="button"
              className="mm-button mm-button-quiet"
              aria-disabled={busy || undefined}
              onClick={() => !busy && void share.revoke(invitation.id)}
            >
              Révoquer
            </button>
          </span>
        </li>
      ))}
    </ul>
  );
}

function General({ share, guest }: { share: Share; guest: boolean }) {
  const headingId = useId();
  const invitedId = useId();
  const membersId = useId();
  const name = useId();
  const invitedRadio = `${invitedId}-radio`;
  const membersRadio = `${membersId}-radio`;
  const { access } = share;
  if (access === null) return null;
  const general = generalAccess(access);
  const busy = share.busy !== null;
  const open = general.level !== null;
  const floor = access.inheritedGeneral !== null;

  if (guest) {
    return (
      <div className="mm-share-general">
        <p className="mm-share-heading">Accès général</p>
        <div className="mm-share-fixed">
          <p className="mm-share-choice-label">
            {general.level === null
              ? "Seulement les invités"
              : `Tous les membres de l'espace · ${LEVEL_LABELS[general.level]}`}
          </p>
          <p className="mm-share-choice-hint">
            Seuls les membres de l'espace règlent l'accès général.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="mm-share-general">
      <p id={headingId} className="mm-share-heading">
        Accès général
      </p>
      <div role="radiogroup" aria-labelledby={headingId} className="mm-share-choices">
        <div
          className={[
            "mm-share-choice",
            !open && "is-checked",
            !general.invitedOnly && "is-disabled",
          ]
            .filter(Boolean)
            .join(" ")}
        >
          <div className="mm-share-choice-main">
            <label>
              <input
                id={invitedRadio}
                type="radio"
                name={name}
                checked={!open}
                disabled={busy || !general.invitedOnly}
                aria-labelledby={invitedId}
                aria-describedby={`${invitedId}-hint`}
                onChange={() => void share.setGeneral(null)}
              />
              <span className="mm-share-dot" aria-hidden="true" />
              <span id={invitedId} className="mm-share-choice-label">
                Seulement les invités
              </span>
            </label>
          </div>
          {/* La précision, sous le choix : elle le coche aussi. */}
          <label id={`${invitedId}-hint`} htmlFor={invitedRadio} className="mm-share-choice-hint">
            {!floor
              ? "Les personnes nommées ci-dessus."
              : general.invitedOnly
                ? "Retire l'accès propre à cette page : celui de la page parente reste."
                : "Une page parente ouvre déjà cette page aux membres de l'espace."}
          </label>
        </div>
        <div className={open ? "mm-share-choice is-checked" : "mm-share-choice"}>
          <div className="mm-share-choice-main">
            <label>
              <input
                id={membersRadio}
                type="radio"
                name={name}
                checked={open}
                disabled={busy}
                aria-labelledby={membersId}
                aria-describedby={`${membersId}-hint`}
                onChange={() => void share.setGeneral(OPENING_LEVEL)}
              />
              <span className="mm-share-dot" aria-hidden="true" />
              <span id={membersId} className="mm-share-choice-label">
                Tous les membres de l'espace
              </span>
            </label>
            {general.inherited && <span className="mm-tag">hérité</span>}
            {general.level !== null && (
              <select
                className="mm-select"
                aria-label="Niveau des membres de l'espace"
                value={general.level}
                disabled={busy}
                onChange={(e) => void share.setGeneral(e.target.value as Level)}
              >
                {general.levels.map(({ level, allowed }) => (
                  <option key={level} value={level} disabled={!allowed}>
                    {LEVEL_LABELS[level]}
                  </option>
                ))}
              </select>
            )}
          </div>
          <label id={`${membersId}-hint`} htmlFor={membersRadio} className="mm-share-choice-hint">
            {general.inherited
              ? "Cet accès vient d'une page parente : il se relève ici, sans se baisser."
              : "Ils voient la page dans la section « Espace »."}
          </label>
        </div>
      </div>
    </div>
  );
}

/** Le pied du menu : ce sur quoi porte le partage, et « Copier le lien » de la page. */
function Footer({ pageId }: { pageId: string }) {
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);

  const copy = async () => {
    setCopyError(false);
    try {
      // Absent hors d'un contexte sécurisé (instance servie en `http` sur le réseau local).
      await navigator.clipboard.writeText(`${location.origin}${pagePath(pageId)}`);
      setCopied(true);
    } catch {
      setCopyError(true);
    }
  };

  return (
    <div className="mm-share-footer">
      <p className="mm-share-note" role="status">
        {copyError
          ? "Le navigateur a refusé la copie : l'adresse de la page est dans la barre d'adresse."
          : copied
            ? "Lien de la page copié dans le presse-papiers."
            : "Vaut pour cette page et ses sous-pages."}
      </p>
      <button type="button" className="mm-button mm-button-icon" onClick={() => void copy()}>
        {copied ? <CheckIcon /> : <LinkIcon />}
        {copied ? "Lien copié" : "Copier le lien"}
      </button>
    </div>
  );
}
