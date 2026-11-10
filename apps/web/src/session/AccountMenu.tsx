// Le compte, en bas de la barre latérale : son nom, et au clic un menu (adresse, « Changer le
// mot de passe », « Se déconnecter »). Ouvrir le menu relit le compte (`refresh`) : c'est
// aussi ce qui s'aperçoit d'une session fermée ailleurs, et fait demander la connexion.
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { changePassword, isSessionLost, logOut } from "../api";
import { ChevronsIcon, KeyIcon, LogOutIcon } from "../icons";
import { Field, FormError, messageOf } from "./LoginPage";
import { colorOf, useSession } from "./session";

/** Les initiales d'un nom : celles des deux premiers mots, ou la première lettre. */
function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const first = (word: string) => Array.from(word)[0] ?? "";
  const letters = words.length >= 2 ? first(words[0]) + first(words[1]) : first(words[0] ?? "");
  return letters.toLocaleUpperCase("fr");
}

function Avatar({ id, name, size }: { id: string; name: string; size: number }) {
  return (
    <span
      className="mm-avatar"
      aria-hidden="true"
      style={{ background: colorOf(id), width: size, height: size, fontSize: size <= 24 ? 10 : 11 }}
    >
      {initials(name)}
    </span>
  );
}

export function AccountMenu() {
  const { me, refresh } = useSession();
  const { user } = me;
  const [open, setOpen] = useState(false);
  const [changing, setChanging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  // Fermé par un clic hors du menu, ou par Échap (le focus revient au bouton).
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setOpen(false);
      buttonRef.current?.focus();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  const toggle = () => {
    if (open) {
      setOpen(false);
      return;
    }
    setOpen(true);
    setError(null);
    // Le compte tel que le serveur le connaît maintenant. Session perdue : la connexion est
    // demandée par `SessionGate` ; serveur injoignable : le menu garde ce qu'il sait.
    refresh().catch(() => {});
  };

  const signOut = async () => {
    setError(null);
    try {
      await logOut();
    } catch (e) {
      setError(`La déconnexion a échoué. ${messageOf(e)}`);
      return;
    }
    // Un onglet neuf : plus rien du compte en mémoire, ni page, ni connexion de synchro.
    location.assign("/login");
  };

  return (
    <div
      ref={rootRef}
      className="mm-account"
      // Le focus part ailleurs (Tab, ou une fenêtre qui s'ouvre) : le menu se ferme.
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOpen(false);
      }}
    >
      {open && (
        <div id={menuId} className="mm-account-menu">
          <div className="mm-account-who">
            <Avatar id={user.id} name={user.name} size={28} />
            <span className="mm-account-names">
              <span className="mm-account-name">{user.name}</span>
              <span className="mm-account-email">{user.email}</span>
              {user.instanceAdmin && <span className="mm-account-role">Admin de l'instance</span>}
            </span>
          </div>
          <div className="mm-account-actions">
            <button
              type="button"
              className="mm-side-action"
              onClick={() => {
                setOpen(false);
                setChanging(true);
              }}
            >
              <KeyIcon />
              <span>Changer le mot de passe</span>
            </button>
            <button type="button" className="mm-side-action" onClick={() => void signOut()}>
              <LogOutIcon />
              <span>Se déconnecter</span>
            </button>
          </div>
          {error && (
            <p className="mm-side-error" role="alert">
              {error}
            </p>
          )}
        </div>
      )}
      <button
        ref={buttonRef}
        type="button"
        className="mm-side-action mm-account-button"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={toggle}
      >
        <Avatar id={user.id} name={user.name} size={22} />
        <span className="mm-account-label">{user.name}</span>
        <ChevronsIcon />
      </button>
      {changing && <PasswordDialog email={user.email} onClose={() => setChanging(false)} />}
    </div>
  );
}

/**
 * Changer son mot de passe : le serveur ferme les autres sessions du compte, et celle-ci
 * repart avec un cookie neuf. Un mot de passe actuel faux répond `401 invalid_credentials` :
 * son message s'affiche, la session est toujours là.
 */
function PasswordDialog({ email, onClose }: { email: string; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await changePassword({ current, new: next });
      setDone(true);
    } catch (e) {
      // Session perdue : la connexion est demandée par-dessus ; les champs restent remplis.
      if (!isSessionLost(e)) setError(messageOf(e));
    } finally {
      setBusy(false);
    }
  };

  const close = () => ref.current?.close();

  return (
    <dialog ref={ref} className="mm-dialog" aria-labelledby={titleId} onClose={onClose}>
      <h2 id={titleId} className="mm-dialog-title">
        Changer le mot de passe
      </h2>
      {done ? (
        <>
          <p className="mm-dialog-lead">
            Mot de passe changé. Les autres sessions de ce compte sont fermées : sur tes autres
            appareils, il faudra te reconnecter.
          </p>
          <div className="mm-dialog-actions">
            <button type="button" className="mm-button mm-button-primary" autoFocus onClick={close}>
              Fermer
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="mm-dialog-lead">
            Sur tes autres appareils, il faudra te reconnecter avec le nouveau.
          </p>
          <form className="mm-auth-form" onSubmit={submit} noValidate>
            {/* Pour les gestionnaires de mots de passe : le compte dont on change le mot de passe. */}
            <input type="email" name="username" autoComplete="username" value={email} readOnly hidden />
            <Field
              label="Mot de passe actuel"
              type="password"
              name="current-password"
              autoComplete="current-password"
              autoFocus
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
            />
            <Field
              label="Nouveau mot de passe"
              type="password"
              name="new-password"
              autoComplete="new-password"
              hint="10 caractères au moins."
              value={next}
              onChange={(e) => setNext(e.target.value)}
            />
            <FormError message={error} />
            <div className="mm-dialog-actions">
              <button type="button" className="mm-button" onClick={close}>
                Annuler
              </button>
              <button type="submit" className="mm-button mm-button-primary" disabled={busy}>
                Changer le mot de passe
              </button>
            </div>
          </form>
        </>
      )}
    </dialog>
  );
}
