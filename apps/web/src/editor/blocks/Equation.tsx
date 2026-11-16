// L'équation en bloc (registre : `latex`) : une formule centrée, mise en forme par KaTeX quand elle
// est à l'écran (`Formula`). Pour qui peut modifier, la formule est un bouton, « Modifier
// l'équation », qui ouvre sa fenêtre (`EquationWindow`, rendue hors du document par
// `BlockWindowHost`, tenue par l'id du bloc) ; Entrée sur le bouton, ou Ctrl+Alt+Entrée quand
// l'équation est sélectionnée, l'ouvrent aussi. Posée par « / », elle s'insère vide et ouvre sa
// fenêtre. Une équation vide le dit (« Équation vide »).
//
// Un lecteur qui a tapé une formule avant de perdre le droit d'écrire garde un bouton (« Formule non
// enregistrée ») qui rouvre la fenêtre en lecture seule, le texte à copier (R472).
import { createReactBlockSpec } from "@blocknote/react";
import { useRef, useState } from "react";
import type { MementumEditor } from "../debug";
import {
  blockDraft,
  setBlockDraft,
  toggleBlockWindow,
  useBlockWindow,
  useHasBlockDraft,
  type BlockWindowProps,
} from "./blockWindows";
import { Formula } from "./Formula";
import { LatexField } from "./LatexField";
import { textProp } from "./props";
import { useEditable } from "./useEditable";

type ViewProps = {
  block: { id: string; type: string; props: Readonly<Record<string, unknown>> };
  editor: MementumEditor;
};

function EquationView({ block, editor }: ViewProps) {
  const latex = textProp(block.props, "latex");
  const editable = useEditable(editor);
  const open = useBlockWindow(editor, block.id);
  const unsaved = useHasBlockDraft(editor, block.id, "equation");
  const shown =
    latex.trim() === "" ? (
      <span className="mm-placeholder">Équation vide</span>
    ) : (
      <Formula latex={latex} display />
    );
  if (!editable && !unsaved) {
    return (
      <div className="mm-equation" contentEditable={false}>
        {shown}
      </div>
    );
  }
  return (
    <div className="mm-equation" contentEditable={false}>
      <button
        type="button"
        className="mm-equation-button"
        aria-label={editable ? "Modifier l'équation" : "Formule non enregistrée de l'équation"}
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
        {shown}
      </button>
    </div>
  );
}

export const Equation = createReactBlockSpec(
  { type: "equation", propSchema: { latex: { default: "" } }, content: "none" },
  {
    render: ({ block, editor }) => <EquationView block={block} editor={editor as unknown as MementumEditor} />,
  },
);

/**
 * La fenêtre d'une équation : son champ (`LatexField`). Tant qu'on n'y a rien tapé, il suit la
 * formule du bloc, relue par son id, et « Terminer » ferme sans rien écrire (R473) ; ce qu'on tape
 * vit en brouillon, rangé par page et par id de bloc, jamais remplacé par ce qui arrive d'ailleurs,
 * et Échap ne le perd pas. « Terminer » écrit la formule en une fois. Le bloc parti, ou l'éditeur en
 * lecture seule, le champ reste, en lecture seule, avec ce qu'on y a tapé.
 */
export function EquationWindow({ editor, blockId, block, editable, close }: BlockWindowProps) {
  const [draft, setDraft] = useState(() => blockDraft(editor, blockId, "equation"));
  // La dernière formule vue du bloc, gardée quand il part.
  const seen = useRef("");
  if (block !== null) seen.current = textProp(block.props, "latex");
  const writable = block !== null && editable;
  const apply = () => {
    if (!writable) return;
    if (draft !== undefined) {
      editor.updateBlock(blockId, { props: { latex: draft } });
      setBlockDraft(editor, blockId, "equation", undefined);
    }
    close("done");
  };
  return (
    <LatexField
      value={draft ?? seen.current}
      display
      readOnly={!writable}
      onApply={apply}
      onChange={(value) => {
        setDraft(value);
        setBlockDraft(editor, blockId, "equation", value);
      }}
    />
  );
}
