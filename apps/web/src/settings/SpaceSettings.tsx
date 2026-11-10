// Réglages d'un espace (`/s/:spaceId/settings`) : son nom, ses membres et leurs rôles, les
// invitations par lien, et « Quitter l'espace ». Une page de l'application, barre latérale à
// gauche, contenu centré, comme les réglages de Notion.
//
// Ce que chacun y fait suit le tableau des rôles (`roles.ts`) ; le serveur fait foi, et ses
// refus s'affichent tels quels :
// - propriétaire : tout, le rôle de chacun compris ;
// - admin : renommer, inviter (`admin` ou `member`), révoquer, retirer qui n'est pas
//   propriétaire, faire passer quelqu'un d'`admin` à `member` et retour ;
// - membre : la liste des membres, sans aucune action sur elle ;
// - invité : renvoyé à l'accueil.
// Chacun peut quitter l'espace, sauf son dernier propriétaire (`409 last_owner`). Son propre
// rôle ne se change pas ici.
//
// Le rôle de la personne connectée est relu avec la liste des membres, pas pris de la
// session, qui peut dater (`useSpaceSettings`). Après chaque action, et après un refus du
// serveur (un rôle changé ailleurs, quelqu'un déjà parti), l'écran relit tout : il montre
// l'état du serveur, plus celui qu'il croyait. Une relecture qui échoue ne retire rien de
// l'écran. Un refus pour rôle insuffisant (`403`) ou pour quelqu'un qui n'est plus là (`404`)
// se dit en tête des réglages, pas dans la section d'où il vient : le rôle perdu peut la
// faire disparaître, et son message avec elle.
// Le lien d'une invitation n'est montré qu'à sa création : le serveur ne garde que le
// hachage de son jeton.
import type { InviteRole, Member, PendingInvitation, Role } from "@mementum/protocol";
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import {
  ApiError,
  inviteToSpace,
  removeMember,
  revokeInvitation,
  updateMember,
  updateSpace,
} from "../api";
import { MailIcon } from "../icons";
import { navigate } from "../router";
import { FormError, messageOf, useDocumentTitle } from "../session/forms";
import { useSession } from "../session/session";
import { CreatedInvitation } from "../share/CreatedInvitation";
import { invitationLink, LEVEL_LABELS, ROLE_LABELS } from "../share/levels";
import { SpaceNotFound } from "../space/SpaceNotFound";
import { Avatar } from "../ui/Avatar";
import { Confirm } from "../ui/Confirm";
import { isManager, mayRemove, rolesFor } from "./roles";
import { useSpaceSettings } from "./useSpaceSettings";

/** Une invitation tout juste créée, avec son lien : montré cette fois seulement. */
type Created = { email: string; role: InviteRole; link: string };

const INVITE_ROLES: InviteRole[] = ["member", "admin"];

const expiry = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "long",
  hour: "2-digit",
  minute: "2-digit",
});

/** Ce que donne une invitation en attente : un rôle dans l'espace, ou un niveau sur une page. */
function grantOf(invitation: PendingInvitation): string {
  if (invitation.role) return ROLE_LABELS[invitation.role];
  return invitation.level ? `${LEVEL_LABELS[invitation.level]} sur une page` : "Sur une page";
}

const roleOf = (members: Member[], self: string) => members.find((m) => m.userId === self)?.role;

/** Le serveur dit que la personne n'a pas (ou plus) ce rôle, ou que l'objet a disparu. */
const isRefusal = (e: unknown) => e instanceof ApiError && (e.status === 403 || e.status === 404);

export function SpaceSettings({ spaceId }: { spaceId: string }) {
  const { me, refresh } = useSession();
  const self = me.user.id;
  const space = me.spaces.find((s) => s.id === spaceId);
  useDocumentTitle("Réglages de l'espace");

  // Un invité n'a rien à faire ici : l'accueil, à la place de cette adresse.
  const guest = space?.role === "guest";
  const goHome = useCallback(() => navigate("/", { replace: true }), []);
  useLayoutEffect(() => {
    if (guest) goHome();
  }, [guest, goHome]);

  const { load, reloadError, reload } = useSpaceSettings(spaceId, self, goHome);
  /** Le dernier refus du serveur pour rôle insuffisant, ou pour quelqu'un qui n'est plus là. */
  const [refusal, setRefusal] = useState<string | null>(null);

  useEffect(() => {
    if (guest) return;
    void reload();
    // Le nom et le rôle de la session, à jour (barre latérale comprise).
    refresh().catch(() => {});
  }, [guest, reload, refresh]);

  // Le compte n'a plus de rôle dans cet espace (on l'en a retiré pendant que ses réglages
  // étaient ouverts) : la liste de ses espaces est relue, et l'écran est celui de tout espace
  // introuvable (R96), sans rien de celui-ci.
  const missing = load.name === "missing";
  useEffect(() => {
    if (missing) refresh().catch(() => {});
  }, [missing, refresh]);
  if (missing) {
    return <SpaceNotFound first={me.spaces.find((s) => s.id !== spaceId) ?? null} />;
  }

  const name = space?.name ?? "";
  const content =
    load.name === "loading" ? (
      <p className="mm-settings-lead">Chargement…</p>
    ) : load.name === "failed" ? (
      <>
        <FormError message={load.message} />
        <p className="mm-settings-actions">
          <button type="button" className="mm-button" onClick={() => void reload()}>
            Réessayer
          </button>
        </p>
      </>
    ) : (
      <Ready
        spaceId={spaceId}
        name={name}
        self={self}
        members={load.members}
        invitations={load.invitations}
        reload={reload}
        onRefused={setRefusal}
      />
    );

  return (
    <>
      <header className="mm-topbar">
        <span className="mm-topbar-title">
          {name && <span className="mm-topbar-crumb">{name} / </span>}
          Réglages
        </span>
      </header>
      <div className="mm-settings">
        <h1 className="mm-settings-title">Réglages de l'espace</h1>
        <FormError message={refusal} />
        {reloadError && (
          <div className="mm-settings-row mm-settings-reload">
            <FormError message={`Les réglages n'ont pas pu être relus. ${reloadError}`} />
            <button type="button" className="mm-button" onClick={() => void reload()}>
              Réessayer
            </button>
          </div>
        )}
        {content}
      </div>
    </>
  );
}

type ReadyProps = {
  spaceId: string;
  name: string;
  self: string;
  members: Member[];
  invitations: PendingInvitation[];
  /** Relit tout depuis le serveur : après chaque action, et après chaque refus. */
  reload: () => Promise<void>;
  /**
   * Le serveur a refusé une action (`403`, `404`) : son message, à dire en tête des réglages.
   * `null` : une nouvelle action commence, le refus d'avant n'a plus cours.
   */
  onRefused: (message: string | null) => void;
};

function Ready({ spaceId, name, self, members, invitations, reload, onRefused }: ReadyProps) {
  const role = roleOf(members, self);
  const manager = isManager(role);
  return (
    <>
      {role && (
        <p className="mm-settings-lead">
          Ton rôle dans cet espace : <strong>{ROLE_LABELS[role]}</strong>.
        </p>
      )}
      <NameSection
        spaceId={spaceId}
        name={name}
        editable={manager}
        reload={reload}
        onRefused={onRefused}
      />
      <MembersSection
        spaceId={spaceId}
        spaceName={name}
        self={self}
        role={role}
        members={members}
        reload={reload}
        onRefused={onRefused}
      />
      {manager && (
        <InvitationsSection
          spaceId={spaceId}
          invitations={invitations}
          reload={reload}
          onRefused={onRefused}
        />
      )}
      <LeaveSection spaceId={spaceId} spaceName={name} self={self} />
    </>
  );
}

type NameProps = {
  spaceId: string;
  name: string;
  editable: boolean;
  reload: () => Promise<void>;
  onRefused: (message: string | null) => void;
};

function NameSection({ spaceId, name, editable, reload, onRefused }: NameProps) {
  const { refresh } = useSession();
  const headingId = useId();
  /** Ce qui a été tapé ; `null` : le nom de l'espace, tel quel. */
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const value = draft ?? name;
  // Rogné, comme le serveur le rognera : c'est ce nom-là qui part, et qu'on compare.
  const next = value.trim();
  const changed = draft !== null && next !== name;

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (!changed || busy) return;
    setBusy(true);
    setError(null);
    onRefused(null);
    setSaved(false);
    try {
      await updateSpace(spaceId, { name: next });
      // Le nouveau nom, dans la session : ici, et partout où l'espace est nommé.
      await refresh().catch(() => {});
      setDraft(null);
      setSaved(true);
    } catch (e) {
      if (isRefusal(e)) {
        // Plus le rôle qu'il faut : le champ va disparaître, le refus se dit en tête.
        onRefused(messageOf(e));
        await reload();
      } else setError(messageOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="mm-settings-section" aria-labelledby={headingId}>
      <h2 id={headingId} className="mm-settings-heading">
        Nom
      </h2>
      {editable ? (
        <>
          <form className="mm-settings-row" onSubmit={save} noValidate>
            <input
              className="mm-input"
              aria-label="Nom de l'espace"
              value={value}
              onChange={(e) => {
                setDraft(e.target.value);
                setSaved(false);
              }}
            />
            <button type="submit" className="mm-button mm-button-field" disabled={!changed || busy}>
              Enregistrer
            </button>
          </form>
          <FormError message={error} />
          <p className="mm-settings-note" role="status">
            {saved ? "Nom enregistré." : ""}
          </p>
        </>
      ) : (
        <p className="mm-settings-value">{name}</p>
      )}
    </section>
  );
}

type MembersProps = {
  spaceId: string;
  spaceName: string;
  self: string;
  role: Role | undefined;
  members: Member[];
  reload: () => Promise<void>;
  onRefused: (message: string | null) => void;
};

function MembersSection({
  spaceId,
  spaceName,
  self,
  role,
  members,
  reload,
  onRefused,
}: MembersProps) {
  const headingId = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [error, setError] = useState<string | null>(null);
  /** La personne dont le rôle est en train de changer. */
  const [changing, setChanging] = useState<string | null>(null);
  /** La personne à retirer, le temps de la confirmation. */
  const [removing, setRemoving] = useState<Member | null>(null);
  const [busy, setBusy] = useState(false);
  /** Compte les fermetures de la confirmation, pour rattraper le focus après chacune. */
  const [closed, setClosed] = useState(0);

  // La fenêtre fermée rend le focus au bouton qui l'a ouverte (`Confirm`). Parti avec sa
  // ligne, ce bouton n'est plus là : le focus va au titre de la liste, pas dans le vide.
  useEffect(() => {
    if (closed === 0) return;
    const active = document.activeElement;
    if (!active || active === document.body) headingRef.current?.focus();
  }, [closed]);

  /** Le refus d'une action : en tête des réglages s'il tient au rôle, sinon ici. */
  const refused = (e: unknown) => {
    if (isRefusal(e)) onRefused(messageOf(e));
    else setError(messageOf(e));
  };

  const changeRole = async (member: Member, next: Role) => {
    setChanging(member.userId);
    setError(null);
    onRefused(null);
    try {
      await updateMember(spaceId, member.userId, { role: next });
    } catch (e) {
      refused(e);
    }
    // Réussi ou refusé : la liste telle que le serveur la voit.
    await reload();
    setChanging(null);
  };

  const remove = async () => {
    if (!removing) return;
    setBusy(true);
    setError(null);
    onRefused(null);
    try {
      await removeMember(spaceId, removing.userId);
    } catch (e) {
      refused(e);
    }
    // Réussi ou refusé : la liste relue (ses invitations en attente partent avec lui).
    await reload();
    setBusy(false);
    setRemoving(null);
    setClosed((n) => n + 1);
  };

  const count = members.length;
  // Qui gère l'espace a une colonne d'actions, vide sur sa ligne : les rôles restent alignés.
  const manager = isManager(role);
  return (
    <section className="mm-settings-section" aria-labelledby={headingId}>
      <div className="mm-settings-heading-row">
        <h2 id={headingId} ref={headingRef} className="mm-settings-heading" tabIndex={-1}>
          Membres
        </h2>
        <span className="mm-settings-count">
          {count} {count > 1 ? "personnes" : "personne"}
        </span>
      </div>
      <FormError message={error} />
      <ul className="mm-people" aria-labelledby={headingId}>
        {members.map((member) => {
          const roles = rolesFor(role, member, self);
          return (
            <li key={member.userId} className="mm-person">
              <Avatar id={member.userId} name={member.name} size={32} />
              <span className="mm-person-names">
                <span className="mm-person-name">
                  {member.name}
                  {member.userId === self && <span className="mm-person-you"> (toi)</span>}
                </span>
                <span className="mm-person-detail">{member.email}</span>
              </span>
              <span className="mm-person-role">
                {roles ? (
                  <select
                    className="mm-select"
                    aria-label={`Rôle de ${member.name}`}
                    value={member.role}
                    disabled={changing === member.userId}
                    onChange={(e) => void changeRole(member, e.target.value as Role)}
                  >
                    {roles.map((r) => (
                      <option key={r} value={r}>
                        {ROLE_LABELS[r]}
                      </option>
                    ))}
                  </select>
                ) : (
                  ROLE_LABELS[member.role]
                )}
              </span>
              {manager && (
                <span className="mm-person-action">
                  {mayRemove(role, member, self) && (
                    <button
                      type="button"
                      className="mm-button mm-button-quiet"
                      onClick={() => setRemoving(member)}
                    >
                      Retirer
                    </button>
                  )}
                </span>
              )}
            </li>
          );
        })}
      </ul>
      {removing && (
        <Confirm
          title={`Retirer ${removing.name} de l'espace\u00a0?`}
          confirmLabel="Retirer"
          destructive
          busy={busy}
          onConfirm={() => void remove()}
          onCancel={() => setRemoving(null)}
        >
          {removing.name} n'aura plus accès aux pages de «&nbsp;{spaceName}&nbsp;». Pour revenir,
          il lui faudra une nouvelle invitation.
        </Confirm>
      )}
    </section>
  );
}

type InvitationsProps = {
  spaceId: string;
  invitations: PendingInvitation[];
  reload: () => Promise<void>;
  onRefused: (message: string | null) => void;
};

function InvitationsSection({ spaceId, invitations, reload, onRefused }: InvitationsProps) {
  const headingId = useId();
  const emailId = useId();
  const roleId = useId();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<InviteRole>("member");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<Created | null>(null);
  /** L'invitation en cours de révocation : un double clic n'envoie qu'une requête. */
  const [revoking, setRevoking] = useState<string | null>(null);
  const [revokeError, setRevokeError] = useState<string | null>(null);

  const invite = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    onRefused(null);
    let token: string;
    try {
      ({ token } = await inviteToSpace(spaceId, { email, role }));
    } catch (e) {
      setBusy(false);
      if (isRefusal(e)) {
        // Plus le rôle qu'il faut : cette section va disparaître, le refus se dit en tête.
        onRefused(messageOf(e));
        await reload();
      } else setError(messageOf(e));
      return;
    }
    setCreated({ email: email.trim().toLowerCase(), role, link: invitationLink(token) });
    setEmail("");
    setBusy(false);
    // La nouvelle invitation, sans son jeton, dans la liste de celles qui attendent. Si la
    // relecture échoue, l'invitation existe quand même : son lien reste montré, avec ce qui
    // était déjà à l'écran (`useSpaceSettings`).
    await reload();
  };

  const revoke = async (invitation: PendingInvitation) => {
    if (revoking) return;
    setRevoking(invitation.id);
    setRevokeError(null);
    onRefused(null);
    try {
      await revokeInvitation(invitation.id);
    } catch (e) {
      // Déjà acceptée, échue ou révoquée ailleurs (`404`) : la liste relue le montre.
      if (!(e instanceof ApiError && e.status === 404)) setRevokeError(messageOf(e));
    }
    // Réussie ou refusée : la liste telle que le serveur la voit.
    await reload();
    setRevoking(null);
  };

  return (
    <section className="mm-settings-section" aria-labelledby={headingId}>
      <h2 id={headingId} className="mm-settings-heading">
        Invitations
      </h2>
      <p className="mm-settings-hint">
        Une invitation se fait par lien : il vaut 7 jours et ne sert qu'une fois. C'est le lien
        qui donne accès, pas l'adresse : ne l'envoie qu'à la personne invitée.
      </p>
      <form className="mm-settings-row" onSubmit={invite} noValidate>
        <label htmlFor={emailId} className="mm-visually-hidden">
          Adresse e-mail
        </label>
        <input
          id={emailId}
          className="mm-input"
          type="email"
          autoComplete="off"
          placeholder="Adresse e-mail"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <label htmlFor={roleId} className="mm-visually-hidden">
          Rôle
        </label>
        <select
          id={roleId}
          className="mm-select mm-select-boxed"
          value={role}
          onChange={(e) => setRole(e.target.value as InviteRole)}
        >
          {INVITE_ROLES.map((r) => (
            <option key={r} value={r}>
              {ROLE_LABELS[r]}
            </option>
          ))}
        </select>
        <button type="submit" className="mm-button mm-button-primary" disabled={busy}>
          Inviter
        </button>
      </form>
      <FormError message={error} />
      {created && (
        <CreatedInvitation email={created.email} grant={ROLE_LABELS[created.role]} link={created.link} />
      )}

      <h3 className="mm-settings-subheading">En attente</h3>
      <FormError message={revokeError} />
      {invitations.length === 0 ? (
        <p className="mm-settings-empty">Aucune invitation en attente.</p>
      ) : (
        <ul className="mm-people" aria-label="Invitations en attente">
          {invitations.map((invitation) => (
            <li key={invitation.id} className="mm-person">
              <span className="mm-person-tile">
                <MailIcon />
              </span>
              <span className="mm-person-names">
                <span className="mm-person-name">{invitation.email}</span>
                <span className="mm-person-detail">
                  {grantOf(invitation)} · expire le {expiry.format(new Date(invitation.expiresAt))}
                </span>
              </span>
              <span className="mm-person-action">
                <button
                  type="button"
                  className="mm-button mm-button-quiet"
                  aria-disabled={revoking !== null || undefined}
                  onClick={() => void revoke(invitation)}
                >
                  Révoquer
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function LeaveSection({ spaceId, spaceName, self }: { spaceId: string; spaceName: string; self: string }) {
  const { refresh } = useSession();
  const headingId = useId();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const leave = async () => {
    setBusy(true);
    setError(null);
    try {
      await removeMember(spaceId, self);
    } catch (e) {
      // Le dernier propriétaire (`409 last_owner`) : le message du serveur, tel quel.
      setError(messageOf(e));
      setBusy(false);
      setConfirming(false);
      return;
    }
    await refresh().catch(() => {});
    navigate("/", { replace: true });
  };

  return (
    <section className="mm-settings-section mm-settings-leave" aria-labelledby={headingId}>
      <h2 id={headingId} className="mm-settings-heading">
        Quitter l'espace
      </h2>
      <div className="mm-settings-row mm-settings-row-spread">
        <p className="mm-settings-hint">
          Tu n'auras plus accès à ses pages. Pour revenir, il faudra une nouvelle invitation.
        </p>
        <button type="button" className="mm-button mm-button-danger-quiet" onClick={() => setConfirming(true)}>
          Quitter l'espace
        </button>
      </div>
      <FormError message={error} />
      {confirming && (
        <Confirm
          title={`Quitter l'espace «\u00a0${spaceName}\u00a0»\u00a0?`}
          confirmLabel="Quitter l'espace"
          destructive
          busy={busy}
          onConfirm={() => void leave()}
          onCancel={() => setConfirming(false)}
        >
          Tu n'auras plus accès à ses pages. Pour revenir, il faudra une nouvelle invitation.
        </Confirm>
      )}
    </section>
  );
}
