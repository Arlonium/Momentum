// Les titres d'une page, pour la table des matières (`Toc.tsx`) : ceux de niveau 1 à 4, dans l'ordre
// du document, enfants et colonnes compris, avec l'id de leur bloc et leur texte.
//
// Lus dans le document ProseMirror de l'éditeur, une fois par recalcul (jamais à chaque frappe :
// `Toc.tsx` groupe les recalculs), sans descendre dans le texte des autres blocs : environ 1 ms pour
// une page de 10 000 blocs. Ni le DOM (un titre de niveau 1 n'y porte pas `data-level`, BlockNote
// omet la valeur par défaut, et son texte affiché porte le rendu des contenus en ligne, KaTeX en
// double), ni `editor.document` (tous les blocs convertis).
//
// Le texte d'un titre : son texte, une mention par « @ » et son libellé, une formule en ligne par son
// LaTeX ; espaces repliées. Un titre vide n'y est pas. Tout ce qui vient du document est lu en
// texte, jamais en HTML : la vue le pose en nœud de texte.
import type { MementumEditor } from "../debug";

export type Heading = { readonly id: string; readonly level: number; readonly text: string };

/** Les niveaux de titre de la table des matières : 1 à 4 (5 et 6 n'y sont pas). */
export const TOC_LEVELS = 4;

type PmNode = MementumEditor["prosemirrorState"]["doc"];

/** Ce qui range des blocs sans en être un : on y descend. */
const LAYOUT = new Set(["blockGroup", "columnList", "column"]);

/** Le texte d'un contenu en ligne sans texte à lui (une mention, une formule, un saut de ligne). */
function leafText(node: PmNode): string {
  const { name } = node.type;
  const attr = (key: string) => {
    const value: unknown = node.attrs[key];
    return typeof value === "string" ? value : "";
  };
  if (name === "mention") return `@${attr("label")}`;
  if (name === "math") return attr("latex");
  if (name === "hardBreak") return " ";
  return "";
}

/** Les titres de niveau 1 à 4 du document `doc`, dans son ordre. */
export function headingsOf(doc: PmNode): Heading[] {
  const headings: Heading[] = [];
  let id = "";
  doc.descendants((node) => {
    const { name } = node.type;
    if (name === "blockContainer") {
      const attr: unknown = node.attrs.id;
      id = typeof attr === "string" ? attr : "";
      return true;
    }
    if (LAYOUT.has(name)) return true;
    // Le contenu d'un bloc : son texte n'est lu que si c'est un titre ; ses enfants sont dans le
    // `blockGroup` qui le suit, dans le même conteneur.
    if (name === "heading" && id !== "") {
      const level: unknown = node.attrs.level;
      if (typeof level === "number" && Number.isInteger(level) && level >= 1 && level <= TOC_LEVELS) {
        const text = node.textBetween(0, node.content.size, "", leafText).replace(/\s+/g, " ").trim();
        if (text !== "") headings.push({ id, level, text });
      }
    }
    return false;
  });
  return headings;
}

/** Les titres de la page de `editor`. Une lecture : `Toc.tsx` dit quand. */
export function readHeadings(editor: MementumEditor): Heading[] {
  return headingsOf(editor.prosemirrorState.doc);
}
