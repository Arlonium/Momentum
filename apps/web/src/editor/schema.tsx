// Schéma BlockNote de Mementum : les blocs natifs de BlockNote 0.57, les blocs custom du
// format MMD, les contenus en ligne `mention` et `math`, et le multi-colonnes de
// `@blocknote/xl-multi-column`. Il doit rester identique au registre du serveur
// (`spec/schema/blocks.json`), ce que vérifie `schema.test.ts`.
import { codeBlockOptions } from "@blocknote/code-block";
import {
  BlockNoteSchema,
  createCodeBlockSpec,
  defaultBlockSpecs,
  defaultInlineContentSpecs,
} from "@blocknote/core";
import { createReactBlockSpec, createReactInlineContentSpec } from "@blocknote/react";
import { withMultiColumn } from "@blocknote/xl-multi-column";
import katex from "katex";
import type { ReactNode } from "react";
import {
  BookmarkIcon,
  DatabaseIcon,
  InfoIcon,
  PageIcon,
  SyncIcon,
  TocIcon,
  WarningIcon,
} from "../icons";

/** Version du schéma de blocs de ce client (`version.ts`), celle de `blocks.json`. */
export { CLIENT_SCHEMA_VERSION } from "./version";

/** Icône par défaut d'un callout (défaut du registre), affichée en icône vectorielle. */
const DEFAULT_CALLOUT_ICON = "💡";

/** Fonds des callouts, par couleur BlockNote. */
const CALLOUT_BACKGROUNDS: Record<string, string> = {
  default: "#F4F3EF",
  gray: "#F1F1EF",
  brown: "#F4EEEE",
  red: "#FBE4E4",
  orange: "#FBEEDB",
  yellow: "#FBF3DB",
  green: "#EDF3EC",
  blue: "#E7F3F8",
  purple: "#F6F3F9",
  pink: "#FAF1F5",
};

/**
 * Langues du menu des blocs de code (celles de `@blocknote/code-block`). BlockNote refuse
 * d'afficher un bloc dont la langue n'est pas dans cette liste (« Language … is not
 * supported ») : l'erreur interrompt la synchro, et la page ne s'ouvre plus chez personne.
 * Or une langue quelconque arrive par MMD (`` ```ma-langue ``) ou d'un autre client. Toute
 * langue est donc admise : le menu n'en montre aucune de choisie, Shiki ne la colore pas,
 * et le document la garde telle quelle.
 */
export const supportedLanguages = new Proxy({ ...codeBlockOptions.supportedLanguages }, {
  has: (languages, key) => typeof key === "string" || key in languages,
});

/** Formule LaTeX rendue par KaTeX ; une formule invalide s'affiche en rouge, sans erreur. */
function Tex({ latex, display }: { latex: string; display: boolean }) {
  const html = katex.renderToString(latex, { displayMode: display, throwOnError: false });
  // KaTeX échappe son entrée (`trust: false` par défaut) : sa sortie est sûre.
  return <span className="mm-tex" dangerouslySetInnerHTML={{ __html: html }} />;
}

/** URL web (http ou https) sûre pour un lien, sinon `null` (pas de `javascript:`). */
function webUrl(raw: string): string | null {
  try {
    const url = new URL(raw);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch {
    return null;
  }
}

/** Cadre commun des blocs sans contenu modifiable. */
function Frame({ icon, children, muted }: { icon: ReactNode; children: ReactNode; muted?: boolean }) {
  return (
    <div className={muted ? "mm-frame mm-frame-muted" : "mm-frame"} contentEditable={false}>
      <span className="mm-frame-icon" aria-hidden="true">
        {icon}
      </span>
      <span className="mm-frame-body">{children}</span>
    </div>
  );
}

const Callout = createReactBlockSpec(
  {
    type: "callout",
    propSchema: { icon: { default: DEFAULT_CALLOUT_ICON }, color: { default: "default" } },
    content: "inline",
  },
  {
    render: ({ block, contentRef }) => (
      <div
        className="mm-callout"
        style={{ background: CALLOUT_BACKGROUNDS[block.props.color] ?? CALLOUT_BACKGROUNDS.default }}
      >
        <span className="mm-callout-icon" contentEditable={false}>
          {block.props.icon === DEFAULT_CALLOUT_ICON ? <InfoIcon /> : block.props.icon}
        </span>
        <div className="mm-callout-content" ref={contentRef} />
      </div>
    ),
  },
);

const Equation = createReactBlockSpec(
  { type: "equation", propSchema: { latex: { default: "" } }, content: "none" },
  {
    render: ({ block }) =>
      block.props.latex.trim() === "" ? (
        <div className="mm-equation mm-placeholder" contentEditable={false}>
          Équation vide
        </div>
      ) : (
        <div className="mm-equation" contentEditable={false}>
          <Tex latex={block.props.latex} display />
        </div>
      ),
  },
);

const Bookmark = createReactBlockSpec(
  { type: "bookmark", propSchema: { url: { default: "" } }, content: "none" },
  {
    render: ({ block }) => {
      const href = webUrl(block.props.url);
      return (
        <Frame icon={<BookmarkIcon />}>
          {href ? (
            <a href={href} target="_blank" rel="noopener noreferrer">
              {block.props.url}
            </a>
          ) : (
            block.props.url || "Signet sans adresse"
          )}
        </Frame>
      );
    },
  },
);

const PageLink = createReactBlockSpec(
  {
    type: "pageLink",
    propSchema: { pageId: { default: "" }, title: { default: "" } },
    content: "none",
  },
  {
    render: ({ block }) => (
      <Frame icon={<PageIcon />}>
        {block.props.pageId ? (
          <a href={`/p/${encodeURIComponent(block.props.pageId)}`}>
            {block.props.title || "Sans titre"}
          </a>
        ) : (
          block.props.title || "Sans titre"
        )}
      </Frame>
    ),
  },
);

const DatabaseRef = createReactBlockSpec(
  {
    type: "databaseRef",
    propSchema: { databaseId: { default: "" }, view: { default: "" } },
    content: "none",
  },
  {
    render: ({ block }) => (
      <Frame icon={<DatabaseIcon />} muted>
        Base de données {block.props.databaseId}
        {block.props.view ? ` · vue ${block.props.view}` : ""} (bientôt disponible)
      </Frame>
    ),
  },
);

const SyncedBlock = createReactBlockSpec(
  { type: "syncedBlock", propSchema: { ref: { default: "" } }, content: "none" },
  {
    render: ({ block }) => (
      <Frame icon={<SyncIcon />} muted>
        Bloc synchronisé {block.props.ref} (bientôt disponible)
      </Frame>
    ),
  },
);

const Toc = createReactBlockSpec(
  { type: "toc", propSchema: {}, content: "none" },
  {
    render: () => (
      <Frame icon={<TocIcon />} muted>
        Table des matières
      </Frame>
    ),
  },
);

/**
 * Bloc d'une version plus récente, ou nœud que le serveur ne connaît pas (`foreign`,
 * R4) : conservé tel quel, jamais interprété ni modifié.
 */
const Unknown = createReactBlockSpec(
  {
    type: "unknown",
    propSchema: {
      originalType: { default: "" },
      data: { default: "" },
      foreign: { default: false },
    },
    content: "none",
  },
  {
    render: ({ block }) => (
      <div className="mm-unknown" contentEditable={false}>
        <WarningIcon />
        <span>
          Bloc non pris en charge
          {block.props.originalType ? ` (${block.props.originalType})` : ""}
        </span>
      </div>
    ),
  },
);

const Mention = createReactInlineContentSpec(
  {
    type: "mention",
    propSchema: { kind: { default: "page" }, id: { default: "" }, label: { default: "" } },
    content: "none",
  },
  {
    render: ({ inlineContent }) => <span className="mm-mention">@{inlineContent.props.label}</span>,
  },
);

const MathInline = createReactInlineContentSpec(
  { type: "math", propSchema: { latex: { default: "" } }, content: "none" },
  { render: ({ inlineContent }) => <Tex latex={inlineContent.props.latex} display={false} /> },
);

export const schema = withMultiColumn(
  BlockNoteSchema.create({
    blockSpecs: {
      ...defaultBlockSpecs,
      // Coloration par `@blocknote/code-block` (Shiki) ; le contenu reste du texte brut.
      // Ses options choisissent `javascript` par défaut : on garde celle du registre et de
      // BlockNote (`text`), sinon un bloc sans langue changerait de langue au rendu MMD.
      codeBlock: createCodeBlockSpec({
        ...codeBlockOptions,
        defaultLanguage: "text",
        supportedLanguages,
      }),
      callout: Callout(),
      equation: Equation(),
      bookmark: Bookmark(),
      pageLink: PageLink(),
      databaseRef: DatabaseRef(),
      syncedBlock: SyncedBlock(),
      toc: Toc(),
      unknown: Unknown(),
    },
    inlineContentSpecs: { ...defaultInlineContentSpecs, mention: Mention, math: MathInline },
  }),
);

export type MementumSchema = typeof schema;
