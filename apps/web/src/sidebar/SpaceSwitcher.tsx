// Le sélecteur d'espace, en tête de barre latérale, comme dans Notion et dans la maquette
// (Ruling R80) : l'espace ouvert (une tuile à son initiale, et son nom), et au clic un menu.
// De haut en bas : les espaces du compte, chacun avec le rôle qu'il y a ; « Créer un espace »
// et « Réglages de l'espace » (pour tout membre, pas pour un invité) ; puis le compte (nom,
// adresse), « Changer le mot de passe » et « Se déconnecter », qui efface aussi les pages
// récentes du compte gardées dans le navigateur (R255).
//
// Ouvrir le menu relit le compte (`refresh`) : la liste des espaces est à jour, et c'est
// aussi ce qui s'aperçoit d'une session fermée ailleurs, et fait demander la connexion.
//
// Au clavier : le menu suit le bouton dans le document, Tab y entre donc depuis le bouton ;
// ouvert par Entrée ou Espace, il donne le focus à son premier élément. Échap le ferme et
// rend le focus au bouton, comme les fenêtres qu'il ouvre.
import type { SpaceInfo } from "@mementum/protocol";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { createSpace, isSessionLost, logOut } from "../api";
import { CheckIcon, ChevronDownIcon, KeyIcon, LogOutIcon, PlusIcon, SettingsIcon } from "../icons";
import { navigate, spacePath, spaceSettingsPath } from "../router";
import { Field, FormError, messageOf } from "../session/forms";
import { PasswordDialog } from "../session/PasswordDialog";
import { useSession } from "../session/session";
import { ROLE_LABELS } from "../share/levels";
import { forgetRecents } from "../space/recents";
import { Avatar } from "../ui/Avatar";

type Props = {
  /** L'espace ouvert ; `null` : le compte n'en a aucun. */
  space: SpaceInfo | null;
  /** Les réglages de l'espace ouvert sont à l'écran. */
  settingsOpen: boolean;
};

/** La tuile d'un espace : son initiale, à la couleur d'accent. */
function SpaceTile({ name, size }: { name: string; size: "small" | "large" }) {
  const initial = Array.from(name.trim())[0]?.toLocaleUpperCase("fr") ?? "";
  return (
    <span className={`mm-space-tile mm-space-tile-${size}`} aria-hidden="true">
      {initial}
    </span>
  );
}

export function SpaceSwitcher({ space, settingsOpen }: Props) {
  const { me, refresh } = useSession();
  const { user, spaces } = me;
  const [open, setOpen] = useState(false);
  /** La fenêtre ouverte depuis le menu. */
  const [dialog, setDialog] = useState<"space" | "password" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  /** Le menu vient d'être ouvert au clavier : son premier élément prendra le focus. */
  const focusFirst = useRef(false);
  const menuId = useId();

  // Fermé par un clic hors du menu, ou par Échap (le focus revient au bouton).
  useEffect(() => {
    if (!open) return;
    if (focusFirst.current) {
      focusFirst.current = false;
      menuRef.current?.querySelector<HTMLElement>("a, button")?.focus();
    }
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

  /** `byKeyboard` : ouvert par Entrée ou Espace (un clic du clavier n'a pas de `detail`). */
  const toggle = (byKeyboard: boolean) => {
    if (open) {
      setOpen(false);
      return;
    }
    focusFirst.current = byKeyboard;
    setOpen(true);
    setError(null);
    // Le compte et ses espaces tels que le serveur les connaît maintenant. Session perdue :
    // la connexion est demandée par `SessionGate` ; serveur injoignable : le menu garde ce
    // qu'il sait.
    refresh().catch(() => {});
  };

  const openDialog = (which: "space" | "password") => {
    setOpen(false);
    setDialog(which);
  };

  const closeDialog = () => {
    setDialog(null);
    // Le menu d'où venait la fenêtre est fermé : le focus revient au sélecteur.
    buttonRef.current?.focus();
  };

  const signOut = async () => {
    setError(null);
    try {
      await logOut();
    } catch (e) {
      setError(`La déconnexion a échoué. ${messageOf(e)}`);
      return;
    }
    // Les pages récentes du compte qui part ne restent pas dans le navigateur (R255).
    forgetRecents(user.id);
    // Un onglet neuf : plus rien du compte en mémoire, ni page, ni connexion de synchro. À la
    // place de l'entrée courante : Précédent ne revient pas sur l'application connectée (et
    // un document gardé par le cache arrière-avant se recharge, `main.tsx`).
    location.replace("/login");
  };

  // Sans espace (le compte a quitté le dernier) : la marque, et de quoi en créer un.
  const name = space?.name ?? "Mementum";
  return (
    <div
      ref={rootRef}
      className="mm-switcher"
      // Le focus part ailleurs (Tab, ou une fenêtre qui s'ouvre) : le menu se ferme.
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOpen(false);
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        className="mm-switcher-button"
        // L'espace ouvert d'abord, puis ce que fait le bouton.
        aria-label={`${name}, espaces et compte`}
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={(event) => toggle(event.detail === 0)}
      >
        <SpaceTile name={name} size="small" />
        <span className="mm-switcher-label">{name}</span>
        <ChevronDownIcon />
      </button>
      {open && (
        <div ref={menuRef} id={menuId} className="mm-switcher-menu">
          {spaces.length > 0 && (
            <ul className="mm-switcher-spaces" aria-label="Espaces">
              {spaces.map((item) => {
                const current = item.id === space?.id;
                return (
                  <li key={item.id}>
                    <a
                      href={spacePath(item.id)}
                      className="mm-switcher-space"
                      aria-current={current ? "true" : undefined}
                      onClick={() => setOpen(false)}
                    >
                      <SpaceTile name={item.name} size="large" />
                      <span className="mm-switcher-names">
                        <span className="mm-switcher-name">{item.name}</span>
                        <span className="mm-switcher-role">{ROLE_LABELS[item.role]}</span>
                      </span>
                      {current && <CheckIcon />}
                    </a>
                  </li>
                );
              })}
            </ul>
          )}
          <div className="mm-switcher-group">
            <button type="button" className="mm-side-action" onClick={() => openDialog("space")}>
              <PlusIcon size={16} />
              <span>Créer un espace</span>
            </button>
            {space && space.role !== "guest" && (
              <a
                href={spaceSettingsPath(space.id)}
                className="mm-side-action mm-side-link"
                aria-current={settingsOpen ? "page" : undefined}
                onClick={() => setOpen(false)}
              >
                <SettingsIcon />
                <span>Réglages de l'espace</span>
              </a>
            )}
          </div>
          <div className="mm-switcher-group" role="group" aria-label="Compte">
            <div className="mm-account-who">
              <Avatar id={user.id} name={user.name} size={28} />
              <span className="mm-account-names">
                <span className="mm-account-name">{user.name}</span>
                <span className="mm-account-email">{user.email}</span>
                {user.instanceAdmin && <span className="mm-account-role">Admin de l'instance</span>}
              </span>
            </div>
            <button type="button" className="mm-side-action" onClick={() => openDialog("password")}>
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
      {dialog === "space" && <CreateSpaceDialog onClose={closeDialog} />}
      {dialog === "password" && <PasswordDialog email={user.email} onClose={closeDialog} />}
    </div>
  );
}

/**
 * Créer un espace : le compte en devient propriétaire, et y arrive. Le nom est celui que le
 * serveur accepte (rogné, 1 à 100 caractères) : son refus s'affiche tel quel.
 */
export function CreateSpaceDialog({ onClose }: { onClose: () => void }) {
  const { refresh } = useSession();
  const ref = useRef<HTMLDialogElement>(null);
  const fieldRef = useRef<HTMLInputElement>(null);
  const titleId = useId();
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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
      const space = await createSpace({ name });
      // Le nouvel espace, dans la liste du compte, avant d'y aller.
      await refresh().catch(() => {});
      navigate(spacePath(space.id));
      ref.current?.close();
    } catch (e) {
      setBusy(false);
      // Session perdue : la connexion est demandée par-dessus ; le nom reste dans le champ.
      if (isSessionLost(e)) return;
      setError(messageOf(e));
      fieldRef.current?.focus();
      fieldRef.current?.select();
    }
  };

  return (
    <dialog ref={ref} className="mm-dialog" aria-labelledby={titleId} onClose={onClose}>
      <h2 id={titleId} className="mm-dialog-title">
        Créer un espace
      </h2>
      <p className="mm-dialog-lead">
        Un espace a ses propres pages et ses propres membres : «&nbsp;Perso&nbsp;»,
        «&nbsp;Pro&nbsp;», «&nbsp;Asso&nbsp;»… Tu en seras propriétaire.
      </p>
      <form className="mm-auth-form" onSubmit={submit} noValidate>
        <Field
          ref={fieldRef}
          label="Nom de l'espace"
          name="space-name"
          autoComplete="off"
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <FormError message={error} />
        <div className="mm-dialog-actions">
          <button type="button" className="mm-button" onClick={() => ref.current?.close()}>
            Annuler
          </button>
          <button type="submit" className="mm-button mm-button-primary" disabled={busy}>
            Créer l'espace
          </button>
        </div>
      </form>
    </dialog>
  );
}
