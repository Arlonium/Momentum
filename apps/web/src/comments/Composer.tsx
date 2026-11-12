// La zone où l'on rédige un commentaire : un commentaire de la page, un commentaire d'un bloc,
// une réponse, ou un message qu'on modifie. Un champ de texte (les sauts de ligne comptent) et
// son bouton ; Ctrl+Entrée (Cmd+Entrée) envoie aussi. Rien ne part tant que le texte est vide ou
// blanc. Un refus du serveur s'affiche tel quel, sous le champ, et le texte reste pour un nouvel
// essai ; une session perdue n'affiche rien : la connexion est demandée par-dessus la page. Le
// texte tapé va dans son brouillon (`drafts.ts`), qui survit à la zone : il part quand le texte
// est envoyé, ou annulé. Pendant l'envoi, le champ est en lecture seule (pas désactivé : il
// garde le focus) et le bouton inactif ; un second envoi ne part pas. L'envoi en cours est tenu
// par le brouillon (`Drafts.startSending`), pas par la zone : une zone recréée pendant l'envoi
// (son fil change de liste) est verrouillée elle aussi, et suit l'issue de l'envoi. À la
// réussite, le texte envoyé est retiré ; à l'échec, tout est rendu tel quel.
import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { isSessionLost } from "../api";
import { messageOf } from "../session/forms";
import type { Drafts } from "./drafts";

type Props = {
  /** Le nom du champ, lu par les lecteurs d'écran (et son texte d'attente). */
  label: string;
  /** Le texte du bouton d'envoi. */
  submitLabel: string;
  /** Le nom du bouton d'envoi, s'il dit plus que son texte (il le contient). */
  submitName?: string;
  /** Le texte de départ (un message qu'on modifie), s'il n'y a pas de brouillon. */
  initial?: string;
  /** Le brouillon de cette zone : repris à l'ouverture, tenu à jour à chaque frappe. */
  draft?: { store: Drafts; key: string };
  /** Le champ prend le focus à son montage : seulement sur un geste de la personne. */
  autoFocus?: boolean;
  /** Le champ a pris le focus à son montage : le geste qui l'a demandé est consommé. */
  onAutoFocused?: () => void;
  /** Envoie le texte ; rejette avec l'erreur du serveur s'il refuse. */
  onSubmit: (body: string) => Promise<void>;
  /** Donné : un bouton « Annuler » referme la zone. */
  onCancel?: () => void;
  /** Le champ se vide une fois le texte parti (un commentaire neuf) ; sinon il reste tel quel. */
  clearOnSent?: boolean;
};

export function Composer({
  label,
  submitLabel,
  submitName,
  initial = "",
  draft,
  autoFocus = false,
  onAutoFocused,
  onSubmit,
  onCancel,
  clearOnSent = true,
}: Props) {
  const [text, setText] = useState(() => (draft && draft.store.get(draft.key)) || initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const errorId = useId();
  /**
   * Sans brouillon : un envoi est en route, vrai dès le premier, avant tout rendu (deux
   * Ctrl+Entrée coup sur coup). Avec un brouillon, c'est lui qui le tient.
   */
  const sending = useRef(false);
  const store = draft?.store;
  const key = draft?.key;
  const subscribe = useCallback(
    (listener: () => void) => (store ? store.subscribe(listener) : () => {}),
    [store],
  );
  /** Un envoi du brouillon est en route, parti d'ici ou d'une zone d'avant un remontage. */
  const draftSending = useSyncExternalStore(subscribe, () =>
    store && key !== undefined ? store.isSending(key) : false,
  );
  // Un envoi s'achève : le texte suit le brouillon (retiré à la réussite, gardé à l'échec).
  const wasSending = useRef(draftSending);
  useEffect(() => {
    if (wasSending.current && !draftSending && store && key !== undefined) {
      setText(store.has(key) ? store.get(key) : clearOnSent ? "" : initial);
    }
    wasSending.current = draftSending;
  }, [draftSending, store, key, clearOnSent, initial]);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!autoFocus) return;
    const element = field.current;
    element?.focus();
    // Un message qu'on modifie : le curseur à la fin du texte.
    element?.setSelectionRange(element.value.length, element.value.length);
    // Le geste ne vaut que pour ce montage-ci : un montage plus tard ne prendra pas le focus.
    onAutoFocused?.();
  }, [autoFocus]);

  const empty = text.trim() === "";
  const locked = busy || draftSending;

  const send = async () => {
    if (empty) return;
    if (store && key !== undefined) {
      if (!store.startSending(key)) return;
    } else {
      if (sending.current) return;
      sending.current = true;
    }
    const sent = text;
    let succeeded = false;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(sent);
      succeeded = true;
      if (!store && mounted.current && clearOnSent) setText("");
    } catch (e) {
      if (mounted.current && !isSessionLost(e)) setError(messageOf(e));
    } finally {
      if (store && key !== undefined) store.finishSending(key, sent, succeeded);
      else sending.current = false;
      if (mounted.current) setBusy(false);
    }
  };

  const onFormSubmit = (event: FormEvent) => {
    event.preventDefault();
    void send();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void send();
    }
  };

  return (
    <form className="mm-composer" onSubmit={onFormSubmit}>
      <textarea
        ref={field}
        className="mm-composer-field"
        // Le brouillon du champ : s'il est recréé sous le focus, le panneau le lui rend.
        data-draft={draft?.key}
        aria-label={label}
        placeholder={`${label}…`}
        dir="auto"
        rows={2}
        value={text}
        readOnly={locked}
        aria-describedby={error ? errorId : undefined}
        onChange={(event) => {
          setText(event.target.value);
          draft?.store.set(draft.key, event.target.value);
        }}
        onKeyDown={onKeyDown}
      />
      {error && (
        <p id={errorId} className="mm-form-error" role="alert">
          {error}
        </p>
      )}
      <div className="mm-composer-actions">
        {onCancel && (
          <button
            type="button"
            className="mm-button mm-button-small"
            onClick={() => {
              draft?.store.drop(draft.key);
              onCancel();
            }}
          >
            Annuler
          </button>
        )}
        <button
          type="submit"
          className="mm-button mm-button-primary mm-button-small"
          aria-label={submitName}
          aria-disabled={empty || locked || undefined}
        >
          {submitLabel}
        </button>
      </div>
    </form>
  );
}
