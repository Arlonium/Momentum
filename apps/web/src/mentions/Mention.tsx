// La mention dans le texte (registre : contenu en ligne `mention`, props `kind`, `id`, `label` ; MMD
// `<mention type="page" id="…">Plan</mention>`). Une page, une personne ou une date ; le libellé est
// copié dans le document quand on pose la mention (R251, R615).
//
// - Une page : qui la voit (elle est dans l'arbre de l'espace affiché, `pageDirectory.ts`) lit son
//   titre à jour ; qui ne la voit pas ne lit que le libellé copié, jamais un titre venu d'ailleurs
//   (preuve 4 du plan). Elle n'est un lien que si `id` est un identifiant de page (un UUID) ; le
//   clic l'ouvre sans recharger l'application (`useLinkInterception`, qui suit les liens des îlots
//   non modifiables), et Ctrl+Alt+Entrée sur la mention sélectionnée met le focus sur son lien.
// - Une personne : le nom copié. Rien n'est lu au serveur pour une mention.
// - Une date : le jour que dit son `id` (`AAAA-MM-JJ`), écrit « 1 déc. 2026 » ; un `id` illisible
//   montre le libellé tel qu'il est écrit.
//
// Le libellé vient d'ailleurs : un nœud de texte, isolé (`dir="auto"`), coupé à l'affichage.
import { createReactInlineContentSpec } from "@blocknote/react";
import { NodeSelection } from "prosemirror-state";
import { shorten } from "../comments/dom";
import type { MementumEditor } from "../editor/debug";
import { textProp } from "../editor/blocks/props";
import { pagePath } from "../router";
import { isPageId, usePageTitle } from "../space/pageDirectory";
import { dateLabel, dateOfId } from "./dates";

/** Un titre de page a 500 caractères au plus ; un libellé écrit par un autre chemin, peut-être plus. */
const MAX_LABEL_SHOWN = 500;

/** Le libellé, isolé du texte voisin. */
function Label({ text }: { text: string }) {
  return (
    <span className="mm-mention-label" dir="auto">
      {shorten(text, MAX_LABEL_SHOWN)}
    </span>
  );
}

function PageMention({ id, label }: { id: string; label: string }) {
  const linked = isPageId(id);
  // Le titre à jour, seulement pour une page de l'arbre ; jamais lu pour ce qui n'est pas un id.
  const current = usePageTitle(linked ? id : "");
  const text = <Label text={(linked ? current : null) ?? (label || "Sans titre")} />;
  if (!linked) {
    return (
      <span className="mm-mention" contentEditable={false}>
        @{text}
      </span>
    );
  }
  return (
    <a className="mm-mention mm-mention-link" href={pagePath(id)} contentEditable={false}>
      @{text}
    </a>
  );
}

function MentionView({ props }: { props: Readonly<Record<string, unknown>> }) {
  const kind = textProp(props, "kind");
  const id = textProp(props, "id");
  const label = textProp(props, "label");
  if (kind === "page") return <PageMention id={id} label={label} />;
  let text = label;
  if (kind === "date") {
    const date = dateOfId(id);
    text = date !== null ? dateLabel(date) : label || id;
  } else if (kind === "user") text = label || "Sans nom";
  return (
    <span className="mm-mention" contentEditable={false}>
      @<Label text={text || "…"} />
    </span>
  );
}

export const Mention = createReactInlineContentSpec(
  {
    type: "mention",
    propSchema: { kind: { default: "page" }, id: { default: "" }, label: { default: "" } },
    content: "none",
  },
  {
    render: ({ inlineContent }) => <MentionView props={inlineContent.props} />,
  },
);

/**
 * Ctrl+Alt+Entrée sur une mention de page sélectionnée (les flèches la sélectionnent) : le focus va
 * à son lien, qu'Entrée suit. Dans le texte d'un éditeur modifiable, aucun autre chemin au clavier
 * n'y mène (Tab imbrique les blocs). Rend vrai si le focus y est allé.
 */
export function focusMentionLink(editor: MementumEditor): boolean {
  const { selection } = editor.prosemirrorState;
  if (!(selection instanceof NodeSelection) || selection.node.type.name !== "mention") return false;
  const dom = editor.prosemirrorView?.nodeDOM(selection.from);
  const link = dom instanceof HTMLElement ? dom.querySelector<HTMLElement>("a[href]") : null;
  if (link === null) return false;
  link.focus();
  return document.activeElement === link;
}
