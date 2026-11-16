// Le signet (registre : `url`) : une carte qui montre l'hôte et l'adresse d'une page web, en lien
// (`target="_blank"`, `rel="noopener noreferrer"`). Pas d'aperçu distant (titre, image) : le serveur
// ne va chercher aucune adresse (R250).
//
// Dans l'éditeur, son adresse s'écrit par sa fenêtre (`BookmarkWindow`, « Modifier le signet », ou
// Ctrl+Alt+Entrée quand le signet est sélectionné), et seulement si c'est une adresse web entière ;
// l'adresse écrite est celle que rend `webUrl` (`https://mementum.dev` devient
// `https://mementum.dev/`). La barre de mise en forme n'offre rien sur un signet (`Toolbar.tsx`) :
// BlockNote y mettait « Remplacer le fichier », qui écrivait l'adresse sans la vérifier.
//
// D'autres chemins écrivent une adresse sans passer par cette fenêtre (R477) : le collage d'un bloc
// copié dans BlockNote (son HTML porte l'adresse telle quelle), MMD, un agent, un autre client. La
// sûreté est donc dans la vue : ce qui n'est pas une adresse web se montre en texte, isolé, jamais
// en lien ; une prop qui n'est pas une chaîne est une adresse vide.
import { createReactBlockSpec } from "@blocknote/react";
import { useId, useRef, useState, type FormEvent } from "react";
import { shorten } from "../../comments/dom";
import { BookmarkIcon } from "../../icons";
import type { MementumEditor } from "../debug";
import { blockDraft, setBlockDraft, toggleBlockWindow, useBlockWindow, type BlockWindowProps } from "./blockWindows";
import { textProp } from "./props";
import { hostOf, webUrl } from "./urls";
import { useEditable } from "./useEditable";

/** Le message d'une adresse refusée. */
export const ADDRESS_REFUSED = "Une adresse commence par http:// ou https://";

/** Une adresse qui n'est pas une adresse web n'est montrée qu'en partie. */
const MAX_RAW_CHARS = 200;

type ViewProps = {
  block: { id: string; type: string; props: Readonly<Record<string, unknown>> };
  editor: MementumEditor;
};

function BookmarkView({ block, editor }: ViewProps) {
  const url = textProp(block.props, "url");
  const href = webUrl(url);
  const editable = useEditable(editor);
  const open = useBlockWindow(editor, block.id);
  return (
    <div className="mm-frame mm-bookmark" contentEditable={false}>
      <span className="mm-frame-icon" aria-hidden="true">
        <BookmarkIcon />
      </span>
      <span className="mm-frame-body mm-bookmark-body">
        {href !== null ? (
          <a className="mm-bookmark-link" href={href} target="_blank" rel="noopener noreferrer">
            <span className="mm-bookmark-host" dir="auto">
              {hostOf(href)}
            </span>
            <span className="mm-bookmark-url" dir="auto">
              {href}
            </span>
          </a>
        ) : url === "" ? (
          <span className="mm-bookmark-empty">Signet sans adresse</span>
        ) : (
          <span className="mm-bookmark-raw" dir="auto">
            {shorten(url, MAX_RAW_CHARS)}
          </span>
        )}
      </span>
      {editable && (
        <button
          type="button"
          className="mm-button mm-button-small mm-bookmark-edit"
          aria-label="Modifier le signet"
          aria-haspopup="dialog"
          aria-expanded={open}
          data-block-window-anchor=""
          onClick={(event) => {
            // Un clic ne donne pas le focus à un bouton partout (Safari) : la fenêtre qu'il ferme
            // l'avait, il le garde.
            event.currentTarget.focus();
            toggleBlockWindow(editor, block);
          }}
        >
          Modifier
        </button>
      )}
    </div>
  );
}

export const Bookmark = createReactBlockSpec(
  { type: "bookmark", propSchema: { url: { default: "" } }, content: "none" },
  {
    render: ({ block, editor }) => <BookmarkView block={block} editor={editor as unknown as MementumEditor} />,
  },
);

/**
 * La fenêtre d'un signet : un champ d'adresse. Tant qu'on n'y a rien tapé, il suit l'adresse du bloc,
 * relue par son id (un autre peut la changer pendant que la fenêtre est ouverte), et « Enregistrer »
 * ferme sans rien écrire (R473). Ce qu'on tape vit en brouillon, rangé par page et par id de bloc
 * (`setBlockDraft`), et n'est jamais remplacé par ce qui arrive d'ailleurs ; « Enregistrer » (ou
 * Entrée) écrit alors l'adresse que rend `webUrl`, ou dit pourquoi elle est refusée, sans rien écrire.
 * Échap ferme la fenêtre sans perdre le brouillon, que la rouvrir retrouve, tant que la page est
 * ouverte. Le bloc parti (supprimé, ou changé en autre chose par un autre), ou l'éditeur en lecture
 * seule, le champ reste, en lecture seule, avec ce qu'on y a tapé, ou la dernière adresse vue.
 */
export function BookmarkWindow({ editor, blockId, block, editable, close }: BlockWindowProps) {
  const id = useId();
  const [draft, setDraft] = useState(() => blockDraft(editor, blockId, "bookmark"));
  const [refused, setRefused] = useState(false);
  // La dernière adresse vue du bloc, gardée quand il part.
  const seen = useRef("");
  if (block !== null) seen.current = textProp(block.props, "url");
  const shown = draft ?? seen.current;
  const writable = block !== null && editable;
  const onSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!writable) return;
    // Rien de tapé : rien à écrire, l'adresse du bloc reste telle qu'un autre a pu la laisser.
    if (draft === undefined) {
      close("done");
      return;
    }
    const href = webUrl(draft);
    if (href === null) {
      setRefused(true);
      return;
    }
    editor.updateBlock(blockId, { props: { url: href } });
    setBlockDraft(editor, blockId, "bookmark", undefined);
    close("done");
  };
  return (
    <form className="mm-block-window-body" onSubmit={onSubmit} noValidate>
      <label className="mm-block-window-heading" htmlFor={`${id}-url`}>
        Adresse de la page
      </label>
      <input
        id={`${id}-url`}
        className="mm-input mm-bookmark-field"
        type="text"
        inputMode="url"
        autoComplete="off"
        spellCheck={false}
        placeholder="https://…"
        dir="ltr"
        value={shown}
        readOnly={!writable}
        aria-invalid={refused || undefined}
        aria-describedby={refused ? `${id}-error` : undefined}
        data-autofocus=""
        onChange={(event) => {
          setDraft(event.target.value);
          setBlockDraft(editor, blockId, "bookmark", event.target.value);
          setRefused(false);
        }}
      />
      {refused && (
        <p id={`${id}-error`} className="mm-block-window-error" role="alert">
          {ADDRESS_REFUSED}
        </p>
      )}
      <div className="mm-block-window-actions">
        <button type="submit" className="mm-button mm-button-primary" aria-disabled={!writable || undefined}>
          Enregistrer
        </button>
      </div>
    </form>
  );
}
