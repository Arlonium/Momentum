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
import { createReactBlockSpec } from "@blocknote/react";
import { withMultiColumn } from "@blocknote/xl-multi-column";
import { Mention } from "../mentions/Mention";
import { Bookmark } from "./blocks/Bookmark";
import { Callout } from "./blocks/Callout";
import { Equation } from "./blocks/Equation";
import { MathInline } from "./blocks/InlineMath";
import { PageLink } from "./blocks/PageLink";
import { Toc } from "./blocks/Toc";
import { textProp } from "./blocks/props";
import type { ReactNode } from "react";
import {
  DatabaseIcon,
  SyncIcon,
  WarningIcon,
} from "../icons";

/** Version du schéma de blocs de ce client (`version.ts`), celle de `blocks.json`. */
export { CLIENT_SCHEMA_VERSION } from "./version";

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

/** Les langues que Shiki sait colorer : celles du menu, par leur nom ou un alias. */
const HIGHLIGHTED_LANGUAGES: ReadonlySet<string> = new Set(
  Object.entries(codeBlockOptions.supportedLanguages).flatMap(([name, { aliases }]) => [
    name,
    ...(aliases ?? []),
  ]),
);

/**
 * Bloc de code, dont seule une langue du menu est confiée à Shiki. Pour une autre langue,
 * le chargeur de `@blocknote/code-block` lève une erreur synchrone (« Language … is not
 * included in this bundle ») que BlockNote n'attrape pas : elle partait dans la console à
 * chaque rendu. Le bloc s'affiche alors sans coloration, et sa langue reste dans le document.
 */
function codeBlockSpec() {
  const spec = createCodeBlockSpec({
    ...codeBlockOptions,
    defaultLanguage: "text",
    supportedLanguages,
  });
  const highlight = spec.implementation.meta?.highlight;
  return {
    ...spec,
    implementation: {
      ...spec.implementation,
      meta: {
        ...spec.implementation.meta,
        highlight: (block: Parameters<NonNullable<typeof highlight>>[0]) => {
          const language = highlight?.(block);
          return language !== undefined && HIGHLIGHTED_LANGUAGES.has(language) ? language : undefined;
        },
      },
    },
  };
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

const DatabaseRef = createReactBlockSpec(
  {
    type: "databaseRef",
    propSchema: { databaseId: { default: "" }, view: { default: "" } },
    content: "none",
  },
  {
    render: ({ block }) => (
      <Frame icon={<DatabaseIcon />} muted>
        Base de données {textProp(block.props, "databaseId")}
        {textProp(block.props, "view") ? ` · vue ${textProp(block.props, "view")}` : ""} (bientôt disponible)
      </Frame>
    ),
  },
);

const SyncedBlock = createReactBlockSpec(
  { type: "syncedBlock", propSchema: { ref: { default: "" } }, content: "none" },
  {
    render: ({ block }) => (
      <Frame icon={<SyncIcon />} muted>
        Bloc synchronisé {textProp(block.props, "ref")} (bientôt disponible)
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
          {textProp(block.props, "originalType") ? ` (${textProp(block.props, "originalType")})` : ""}
        </span>
      </div>
    ),
  },
);

export const schema = withMultiColumn(
  BlockNoteSchema.create({
    blockSpecs: {
      ...defaultBlockSpecs,
      // Coloration par `@blocknote/code-block` (Shiki) ; le contenu reste du texte brut.
      // Ses options choisissent `javascript` par défaut : on garde celle du registre et de
      // BlockNote (`text`), sinon un bloc sans langue changerait de langue au rendu MMD.
      codeBlock: codeBlockSpec(),
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
