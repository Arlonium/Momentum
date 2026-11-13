// Titre de la page, au-dessus de l'éditeur. Enregistré (`PATCH`) à la perte du focus ou sur
// Entrée ; le serveur fait foi (espaces de bord retirées, vide → « Sans titre », 500
// caractères au plus).
//
// Seul ce que la personne a tapé part. Un clic dans le champ, puis un clic ailleurs,
// n'envoient rien : sinon le titre affiché à l'ouverture effacerait, sans que rien le dise, un
// renommage fait ailleurs entre-temps. Tant qu'elle n'a rien tapé, le champ suit le titre
// distant (la barre latérale en direct l'apporte), focus ou non ; dès qu'elle a tapé, sa
// saisie gagne, et rien de distant ne la remplace sous ses doigts.
//
// Un titre refusé parce que la session est perdue (`401`) n'est pas une erreur à afficher : la
// connexion est demandée par-dessus la page, le titre tapé reste dans le champ, et il repart
// au serveur quand le compte revient, comme le texte de la page.
import type { PageNode } from "@mementum/protocol";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ApiError, isSessionLost, MAX_TITLE_CHARS, titleLength, updatePage } from "../api";
import { useSession } from "../session/session";
import { useAfterLogin } from "../session/useAfterLogin";

type Props = {
  node: PageNode;
  /** Page qui vient d'être créée : le titre prend le focus, sélectionné, une seule fois. */
  autoFocus: boolean;
  onAutoFocused: () => void;
  readOnly: boolean;
  onSaved: (node: PageNode) => void;
};

/** Le curseur et la sélection d'un champ, gardés le temps d'y poser un autre texte. */
type Selection = { start: number; end: number; direction: "forward" | "backward" | "none" };

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export function TitleInput({ node, autoFocus, onAutoFocused, readOnly, onSaved }: Props) {
  const [value, setValue] = useState(node.title);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLTextAreaElement>(null);
  const saved = useRef(node.title);
  const sending = useRef<string | null>(null);
  /**
   * Le champ porte une saisie qui n'est pas le titre de la page : seule elle part, et elle n'est
   * pas remplacée par un titre distant. Levé par une frappe qui écrit autre chose que le titre
   * enregistré (ou que celui qui part) ; retombé quand une frappe le redit, et quand la saisie
   * est enregistrée.
   */
  const typed = useRef(false);
  /** Le champ est en lecture seule : rien de ce qui y est ne peut partir. */
  const locked = useRef(readOnly);
  /** Sélection à remettre en place une fois le texte posé (champ qui a le focus). */
  const keep = useRef<Selection | null>(null);
  const afterLogin = useAfterLogin(useSession().me);

  // Lu par l'effet du titre distant, qui ne doit pas repartir quand seul ce drapeau change.
  useLayoutEffect(() => {
    locked.current = readOnly;
  }, [readOnly]);

  /**
   * Pose `title` dans le champ. Le navigateur met le curseur à la fin quand on change la
   * valeur d'un champ : s'il a le focus, son curseur et sa sélection sont gardés pour être remis
   * à leur place (bornés au nouveau texte) avant l'affichage.
   */
  const show = (title: string) => {
    const el = ref.current;
    if (el && el.value !== title && document.activeElement === el) {
      keep.current = {
        start: el.selectionStart,
        end: el.selectionEnd,
        direction: el.selectionDirection,
      };
    }
    setValue(title);
  };

  // Un titre changé ailleurs (renommage apporté par la barre latérale, réponse du serveur) : le
  // champ le reprend tant que rien n'y a été tapé, même pendant le focus. Un champ en lecture
  // seule le reprend aussi : ce qui y a été tapé ne partira plus.
  useEffect(() => {
    saved.current = node.title;
    if (typed.current && !locked.current) return;
    typed.current = false;
    // `show` ne lit que des références : l'effet ne dépend que du titre.
    show(node.title);
  }, [node.title]);

  useEffect(() => {
    if (autoFocus && ref.current) {
      ref.current.focus();
      ref.current.select();
      onAutoFocused();
    }
  }, [autoFocus, onAutoFocused]);

  // Le champ grandit avec le titre (un titre long passe à la ligne) ; un texte posé pendant le
  // focus rend sa sélection.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
    const kept = keep.current;
    keep.current = null;
    if (kept && document.activeElement === el) {
      const end = el.value.length;
      el.setSelectionRange(Math.min(kept.start, end), Math.min(kept.end, end), kept.direction);
    }
  }, [value]);

  const save = async () => {
    const title = value;
    // Rien de tapé : rien ne part. Sans cette garde, un blur arrivé entre l'effet qui pose un
    // titre distant et le rendu qui l'affiche enverrait l'ancien texte, que voit encore `value`.
    if (readOnly || !typed.current || title === sending.current) return;
    if (title === saved.current) {
      // Le champ dit de nouveau le titre de la page : plus rien à envoyer, il suit le distant.
      typed.current = false;
      return;
    }
    sending.current = title;
    try {
      let updated: PageNode;
      for (;;) {
        try {
          updated = await updatePage(node.id, { title });
          break;
        } catch (e) {
          // Session perdue : le titre attend le retour du compte, puis repart.
          if (!isSessionLost(e)) throw e;
          await afterLogin();
        }
      }
      saved.current = updated.title;
      setError(null);
      // La saisie n'a pas changé pendant l'envoi : elle est enregistrée, et on affiche le titre
      // normalisé. Sinon, ce qui a été tapé depuis reste à envoyer.
      if (ref.current?.value === title) {
        typed.current = false;
        show(updated.title);
      }
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
          const next = e.target.value.replace(/[\r\n]+/g, " ");
          // Le champ redit le titre que la page a (ou aura : celui qui part) : plus rien de tapé à
          // protéger, il suit le distant. Une lettre tapée puis effacée ne garde pas le champ.
          typed.current = next !== (sending.current ?? saved.current);
          setValue(next);
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
