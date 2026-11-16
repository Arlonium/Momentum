// Le champ où l'on écrit une formule (une équation en bloc, une formule en ligne) : le texte LaTeX,
// 10 000 caractères au plus, l'aperçu au-dessous (KaTeX à `trust: false`, mis en forme par
// `typeset`, qui refuse les définitions et mesure l'imbrication), « Terminer ». Rien ne s'écrit dans
// le document tant qu'on tape : « Terminer », ou Ctrl+Entrée (Cmd+Entrée), applique, par
// `onApply`. Le champ est tenu par qui l'appelle (`value`, `onChange`) : le brouillon d'une équation
// vit par id de bloc, rangé par page (`blockDraft`), celui d'une formule en ligne dans la fenêtre
// de l'éditeur (`InlineMath.tsx`). Une formule invalide se montre en rouge, avec ce que KaTeX en
// dit, et reste dans le champ.
import { useDeferredValue, useId, type KeyboardEvent, type ReactNode } from "react";
import { Formula } from "./Formula";
import { MAX_LATEX_CHARS, refusal, typeset } from "./tex";

type Props = {
  value: string;
  onChange: (value: string) => void;
  /** En bloc (une équation) ou en ligne. */
  display: boolean;
  onApply: () => void;
  /** Le bloc parti, ou l'éditeur en lecture seule : le texte se lit, se copie, rien ne s'applique. */
  readOnly?: boolean;
  /** Une phrase sous le champ (où vit le brouillon). */
  note?: ReactNode;
};

export function LatexField({ value, onChange, display, onApply, readOnly = false, note }: Props) {
  const id = useId();
  // L'aperçu suit la frappe sans la retenir : une formule longue se met en forme après.
  const shown = useDeferredValue(value);
  const outcome = typeset(shown, display);
  const why = refusal(outcome);
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== "Enter" || !(event.ctrlKey || event.metaKey) || event.altKey) return;
    event.preventDefault();
    if (!readOnly) onApply();
  };
  return (
    <div className="mm-block-window-body mm-latex">
      <label className="mm-block-window-heading" htmlFor={`${id}-latex`}>
        Formule (LaTeX)
      </label>
      <textarea
        id={`${id}-latex`}
        className="mm-input mm-latex-field"
        rows={3}
        maxLength={MAX_LATEX_CHARS}
        dir="ltr"
        spellCheck={false}
        autoComplete="off"
        autoCapitalize="off"
        value={value}
        readOnly={readOnly}
        aria-describedby={`${id}-preview${why ? ` ${id}-why` : ""}`}
        data-autofocus=""
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <div id={`${id}-preview`} className={display ? "mm-latex-preview" : "mm-latex-preview mm-latex-inline"}>
        {outcome.kind === "empty" ? (
          <span className="mm-placeholder">L'aperçu de la formule s'affiche ici.</span>
        ) : outcome.kind === "formula" ? (
          <Formula latex={shown} display={display} eager />
        ) : (
          // Refusée ou invalide : le texte, en rouge, et ce qui l'empêche au-dessous.
          <span className="mm-tex-source mm-tex-invalid" dir="ltr">
            {shown}
          </span>
        )}
      </div>
      {why !== null && (
        <p id={`${id}-why`} className="mm-block-window-error">
          {why}
        </p>
      )}
      {note && <p className="mm-latex-note">{note}</p>}
      <div className="mm-block-window-actions">
        <span className="mm-latex-hint">Ctrl+Entrée</span>
        <button
          type="button"
          className="mm-button mm-button-primary"
          aria-disabled={readOnly || undefined}
          onClick={() => {
            if (!readOnly) onApply();
          }}
        >
          Terminer
        </button>
      </div>
    </div>
  );
}
