// La zone où l'on rédige un commentaire : un commentaire de la page, un commentaire d'un bloc,
// une réponse, ou un message qu'on modifie. Un champ de texte (les sauts de ligne comptent) et
// son bouton ; Ctrl+Entrée (Cmd+Entrée) envoie aussi. Rien ne part tant que le texte est vide ou
// blanc. Un refus du serveur s'affiche tel quel, sous le champ, et le texte reste pour un nouvel
// essai ; une session perdue n'affiche rien : la connexion est demandée par-dessus la page. Le
// texte tapé va dans son brouillon (`drafts.ts`), qui survit à la zone : il part quand le texte
// est envoyé, ou annulé.
import { useEffect, useId, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
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
  autoFocus?: boolean;
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
  onSubmit,
  onCancel,
  clearOnSent = true,
}: Props) {
  const [text, setText] = useState(() => (draft && draft.store.get(draft.key)) || initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const errorId = useId();
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
  }, [autoFocus]);

  const empty = text.trim() === "";

  const send = async () => {
    if (empty || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(text);
      draft?.store.drop(draft.key);
      if (mounted.current && clearOnSent) setText("");
    } catch (e) {
      if (mounted.current && !isSessionLost(e)) setError(messageOf(e));
    } finally {
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
        aria-label={label}
        placeholder={`${label}…`}
        dir="auto"
        rows={2}
        value={text}
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
          aria-disabled={empty || busy || undefined}
        >
          {submitLabel}
        </button>
      </div>
    </form>
  );
}
