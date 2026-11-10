// Fenêtre de confirmation dans la page, jamais `window.confirm` : ni en français ni à nos
// couleurs, et bloquante. Sert à retirer quelqu'un d'un espace, à le quitter, et à supprimer
// définitivement une page de la corbeille.
//
// Accessible (`alertdialog`, nommée par son titre et décrite par son texte) :
// - le focus reste dans la fenêtre, Tab et Maj+Tab tournent entre ses boutons ;
// - Échap annule, comme un clic sur le voile ;
// - le focus part sur « Annuler » : Entrée, tapée par réflexe ou restée enfoncée depuis le
//   bouton qui a ouvert la fenêtre, n'exécute jamais l'action. La confirmer demande d'aller
//   sur son bouton ;
// - à la fermeture, le focus revient où il était (s'il est encore là).
//
// La fenêtre ne se ferme pas d'elle-même : `onCancel` et `onConfirm` disent au parent de la
// retirer, après l'action pour une confirmation (`busy` pendant ce temps). Pendant l'action,
// les boutons restent focalisables (`aria-disabled`, clics ignorés) : un bouton désactivé
// perdrait le focus, qui sortirait de la fenêtre.
import {
  useEffect,
  useId,
  useRef,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";

type Props = {
  title: string;
  /** Ce que fera l'action, en une ou deux phrases. */
  children?: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  /** Une action qu'on ne défait pas d'un clic : son bouton est rouge. */
  destructive?: boolean;
  /** L'action est en cours : les boutons attendent, Échap aussi. */
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
};

export function Confirm({
  title,
  children,
  confirmLabel,
  cancelLabel = "Annuler",
  destructive = false,
  busy = false,
  onConfirm,
  onCancel,
}: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const bodyId = useId();

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || dialog.open) return;
    // Le parent retire la fenêtre sans `close()` : le focus est rendu ici.
    const previous = document.activeElement;
    dialog.showModal();
    cancelRef.current?.focus();
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  const cancel = () => {
    if (!busy) onCancel();
  };
  const confirm = () => {
    if (!busy) onConfirm();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDialogElement>) => {
    // Entrée restée enfoncée : ses répétitions ne valent pas un choix.
    if (event.key === "Enter" && event.repeat) {
      event.preventDefault();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = [
      ...event.currentTarget.querySelectorAll<HTMLElement>("button:not(:disabled), [href], input"),
    ];
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !event.currentTarget.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !event.currentTarget.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  };

  // Un clic sur le voile (hors du cadre de la fenêtre) annule.
  const onClick = (event: MouseEvent<HTMLDialogElement>) => {
    if (event.target !== event.currentTarget) return;
    const box = event.currentTarget.getBoundingClientRect();
    const inside =
      event.clientX >= box.left &&
      event.clientX <= box.right &&
      event.clientY >= box.top &&
      event.clientY <= box.bottom;
    if (!inside) cancel();
  };

  return (
    <dialog
      ref={ref}
      role="alertdialog"
      className="mm-dialog"
      aria-labelledby={titleId}
      aria-describedby={children ? bodyId : undefined}
      onKeyDown={onKeyDown}
      onClick={onClick}
      // Échap : le parent retire la fenêtre.
      onCancel={(event) => {
        event.preventDefault();
        cancel();
      }}
      // Chrome la ferme quand même au second Échap sans geste entre les deux : c'est une
      // annulation.
      onClose={cancel}
    >
      <h2 id={titleId} className="mm-dialog-title">
        {title}
      </h2>
      {children && (
        <div id={bodyId} className="mm-dialog-lead">
          {children}
        </div>
      )}
      <div className="mm-dialog-actions">
        <button
          ref={cancelRef}
          type="button"
          className="mm-button"
          aria-disabled={busy || undefined}
          onClick={cancel}
        >
          {cancelLabel}
        </button>
        <button
          type="button"
          className={`mm-button ${destructive ? "mm-button-danger" : "mm-button-primary"}`}
          aria-disabled={busy || undefined}
          onClick={confirm}
        >
          {confirmLabel}
        </button>
      </div>
    </dialog>
  );
}
