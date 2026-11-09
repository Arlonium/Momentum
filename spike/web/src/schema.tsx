// Schéma BlockNote du spike : blocs par défaut + blocs custom du tableau de correspondance MMD.
import { BlockNoteSchema, defaultBlockSpecs, defaultInlineContentSpecs } from "@blocknote/core";
import { createReactBlockSpec, createReactInlineContentSpec } from "@blocknote/react";
import { withMultiColumn } from "@blocknote/xl-multi-column";

const box = { border: "1px dashed #c9c7c0", borderRadius: 6, padding: "6px 10px", fontSize: 13, color: "#4a4843" };

const Callout = createReactBlockSpec(
  { type: "callout", propSchema: { icon: { default: "💡" }, color: { default: "default" } }, content: "inline" },
  {
    render: (props) => (
      <div style={{ display: "flex", gap: 8, background: "#f4f3ef", borderRadius: 6, padding: "8px 12px", width: "100%" }}>
        <span>{props.block.props.icon}</span>
        <div ref={props.contentRef} style={{ flex: 1 }} />
      </div>
    ),
  },
);

const Equation = createReactBlockSpec(
  { type: "equation", propSchema: { latex: { default: "" } }, content: "none" },
  { render: (props) => <div style={box}>∑ {props.block.props.latex}</div> },
);

const Bookmark = createReactBlockSpec(
  { type: "bookmark", propSchema: { url: { default: "" } }, content: "none" },
  { render: (props) => <div style={box}>🔖 {props.block.props.url}</div> },
);

const PageLink = createReactBlockSpec(
  { type: "pageLink", propSchema: { pageId: { default: "" }, title: { default: "" } }, content: "none" },
  { render: (props) => <div style={box}>📄 {props.block.props.title}</div> },
);

const DatabaseRef = createReactBlockSpec(
  { type: "databaseRef", propSchema: { databaseId: { default: "" }, view: { default: "" } }, content: "none" },
  { render: (props) => <div style={box}>🗂 base {props.block.props.databaseId} · vue {props.block.props.view}</div> },
);

const SyncedBlock = createReactBlockSpec(
  { type: "syncedBlock", propSchema: { ref: { default: "" } }, content: "none" },
  { render: (props) => <div style={box}>⟳ bloc synchronisé ({props.block.props.ref})</div> },
);

const Toc = createReactBlockSpec(
  { type: "toc", propSchema: {}, content: "none" },
  { render: () => <div style={box}>Table des matières</div> },
);

/** Bloc inconnu (version plus récente) : conservé tel quel, jamais interprété. */
const Unknown = createReactBlockSpec(
  { type: "unknown", propSchema: { originalType: { default: "" }, data: { default: "" } }, content: "none" },
  { render: (props) => <div style={box}>Bloc non pris en charge : {props.block.props.originalType}</div> },
);

const Mention = createReactInlineContentSpec(
  { type: "mention", propSchema: { kind: { default: "page" }, id: { default: "" }, label: { default: "" } }, content: "none" },
  { render: (props) => <span style={{ color: "#23704F", fontWeight: 500 }}>@{props.inlineContent.props.label}</span> },
);

const MathInline = createReactInlineContentSpec(
  { type: "math", propSchema: { latex: { default: "" } }, content: "none" },
  { render: (props) => <span style={{ fontFamily: "serif", fontStyle: "italic" }}>{props.inlineContent.props.latex}</span> },
);

export const schema = withMultiColumn(
  BlockNoteSchema.create({
    blockSpecs: {
      ...defaultBlockSpecs,
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
