// Titre de la page, au-dessus de l'éditeur. Enregistré (`PATCH`) à la perte du focus ou sur
// Entrée ; le serveur fait foi (espaces de bord retirées, vide → « Sans titre », 500
// caractères au plus).
import type { PageNode } from "@mementum/protocol";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ApiError, MAX_TITLE_CHARS, titleLength, updatePage } from "../api";

type Props = {
  node: PageNode;
  /** Page qui vient d'être créée : le titre prend le focus, sélectionné, une seule fois. */
  autoFocus: boolean;
  onAutoFocused: () => void;
  readOnly: boolean;
  onSaved: (node: PageNode) => void;
};

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export function TitleInput({ node, autoFocus, onAutoFocused, readOnly, onSaved }: Props) {
  const [value, setValue] = useState(node.title);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  const saved = useRef(node.title);
  const sending = useRef<string | null>(null);

  // Un titre changé ailleurs (réponse du serveur) remplace la saisie seulement hors édition.
  useEffect(() => {
    saved.current = node.title;
    if (document.activeElement !== ref.current) setValue(node.title);
  }, [node.title]);

  useEffect(() => {
    if (autoFocus && ref.current) {
      ref.current.focus();
      ref.current.select();
      onAutoFocused();
    }
  }, [autoFocus, onAutoFocused]);

  // Le champ grandit avec le titre (un titre long passe à la ligne).
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);

  const save = async () => {
    const title = value;
    if (readOnly || title === saved.current || title === sending.current) return;
    sending.current = title;
    try {
      const updated = await updatePage(node.id, { title });
      saved.current = updated.title;
      setError(null);
      // La saisie n'a pas changé pendant l'envoi : on affiche le titre normalisé.
      if (ref.current?.value === title) setValue(updated.title);
      onSaved(updated);
    } catch (e) {
      setError(
        e instanceof ApiError ? capitalize(e.message) : "Le titre n'a pas pu être enregistré.",
      );
    } finally {
      sending.current = null;
    }
  };

  const length = titleLength(value);
  const tooLong = length > MAX_TITLE_CHARS;
  const message =
    error ??
    (tooLong ? `Un titre a ${MAX_TITLE_CHARS} caractères au plus (celui-ci en a ${length}).` : null);

  return (
    <div className="mm-title">
      <textarea
        ref={ref}
        className="mm-title-input"
        aria-label="Titre de la page"
        aria-invalid={message ? true : undefined}
        aria-describedby={message ? "mm-title-error" : undefined}
        placeholder="Sans titre"
        rows={1}
        spellCheck
        readOnly={readOnly}
        value={value}
        onChange={(e) => {
          setValue(e.target.value.replace(/[\r\n]+/g, " "));
          setError(null);
        }}
        onBlur={() => void save()}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.nativeEvent.isComposing) {
            e.preventDefault();
            void save();
          }
        }}
      />
      {message && (
        <p id="mm-title-error" className="mm-title-error" role="alert">
          {message}
        </p>
      )}
    </div>
  );
}
