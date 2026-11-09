//! Registre des types de blocs de la P1 : noms, props, défauts et forme MMD.
//!
//! Les défauts reprennent ceux de BlockNote 0.57 (et de `schema.tsx` du
//! prototype pour les blocs maison). Une prop sans défaut (`None`) est absente
//! tant que l'utilisateur ne l'a pas fixée.

/// Type d'une prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropType {
    String,
    Int,
    Float,
    Bool,
}

impl PropType {
    /// Nom du type dans `spec/schema/blocks.json`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PropType::String => "string",
            PropType::Int => "int",
            PropType::Float => "float",
            PropType::Bool => "bool",
        }
    }
}

/// Valeur par défaut d'une prop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropDefault {
    Str(&'static str),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Une prop d'un type de bloc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropSpec {
    /// Nom BlockNote (celui du nœud Yjs).
    pub name: &'static str,
    /// Nom dans le format MMD.
    pub mmd: &'static str,
    pub ty: PropType,
    pub default: Option<PropDefault>,
    /// Vrai si la forme MMD elle-même porte la prop (`#` × niveau, `url`…).
    pub in_form: bool,
}

/// Nature du contenu d'un bloc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Inline,
    Table,
    None,
}

impl ContentKind {
    /// Nom dans `spec/schema/blocks.json`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ContentKind::Inline => "inline",
            ContentKind::Table => "table",
            ContentKind::None => "none",
        }
    }
}

/// Forme d'un bloc dans le format MMD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmdForm {
    Line,
    Heading,
    Bullet,
    Numbered,
    Check,
    Quote,
    Code,
    Divider,
    Image,
    Table,
    Equation,
    /// Balise ouvrante et fermante : `<nom …> … </nom>`.
    Tag(&'static str),
    /// Balise autofermante : `<nom …/>`.
    SelfClosing(&'static str),
    /// Bloc inconnu : `<block type="…" id="…" data="…"/>`.
    Unknown,
}

/// Un type de bloc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockSpec {
    pub name: &'static str,
    pub content: ContentKind,
    pub form: MmdForm,
    pub props: &'static [PropSpec],
    /// Vrai si le bloc est dans un `blockContainer` (faux pour `columnList` et `column`).
    pub wrapped: bool,
}

const fn prop(
    name: &'static str,
    mmd: &'static str,
    ty: PropType,
    default: Option<PropDefault>,
) -> PropSpec {
    PropSpec {
        name,
        mmd,
        ty,
        default,
        in_form: false,
    }
}

const fn string(name: &'static str, mmd: &'static str, default: &'static str) -> PropSpec {
    prop(name, mmd, PropType::String, Some(PropDefault::Str(default)))
}

const fn boolean(name: &'static str, mmd: &'static str, default: bool) -> PropSpec {
    prop(name, mmd, PropType::Bool, Some(PropDefault::Bool(default)))
}

/// Marque la prop comme portée par la forme MMD.
const fn in_form(p: PropSpec) -> PropSpec {
    PropSpec { in_form: true, ..p }
}

const BG: PropSpec = string("backgroundColor", "bg", "default");
const COLOR: PropSpec = string("textColor", "color", "default");
const ALIGN: PropSpec = string("textAlignment", "align", "left");
const URL: PropSpec = in_form(string("url", "url", ""));
const CAPTION: PropSpec = in_form(string("caption", "caption", ""));
const NAME: PropSpec = string("name", "name", "");
const PREVIEW: PropSpec = boolean("showPreview", "preview", true);
const PREVIEW_WIDTH: PropSpec = prop("previewWidth", "width", PropType::Float, None);

const TEXT_PROPS: &[PropSpec] = &[BG, COLOR, ALIGN];

const BLOCKS: &[BlockSpec] = &[
    BlockSpec {
        name: "paragraph",
        content: ContentKind::Inline,
        form: MmdForm::Line,
        props: TEXT_PROPS,
        wrapped: true,
    },
    BlockSpec {
        name: "heading",
        content: ContentKind::Inline,
        form: MmdForm::Heading,
        props: &[
            BG,
            COLOR,
            ALIGN,
            in_form(prop(
                "level",
                "level",
                PropType::Int,
                Some(PropDefault::Int(1)),
            )),
            boolean("isToggleable", "toggle", false),
        ],
        wrapped: true,
    },
    BlockSpec {
        name: "bulletListItem",
        content: ContentKind::Inline,
        form: MmdForm::Bullet,
        props: TEXT_PROPS,
        wrapped: true,
    },
    BlockSpec {
        name: "numberedListItem",
        content: ContentKind::Inline,
        form: MmdForm::Numbered,
        props: &[
            BG,
            COLOR,
            ALIGN,
            in_form(prop("start", "start", PropType::Int, None)),
        ],
        wrapped: true,
    },
    BlockSpec {
        name: "checkListItem",
        content: ContentKind::Inline,
        form: MmdForm::Check,
        props: &[
            BG,
            COLOR,
            ALIGN,
            in_form(boolean("checked", "checked", false)),
        ],
        wrapped: true,
    },
    BlockSpec {
        name: "toggleListItem",
        content: ContentKind::Inline,
        form: MmdForm::Tag("toggle"),
        props: TEXT_PROPS,
        wrapped: true,
    },
    BlockSpec {
        name: "quote",
        content: ContentKind::Inline,
        form: MmdForm::Quote,
        props: &[BG, COLOR],
        wrapped: true,
    },
    BlockSpec {
        name: "codeBlock",
        content: ContentKind::Inline,
        form: MmdForm::Code,
        props: &[in_form(string("language", "language", "text"))],
        wrapped: true,
    },
    BlockSpec {
        name: "divider",
        content: ContentKind::None,
        form: MmdForm::Divider,
        props: &[],
        wrapped: true,
    },
    BlockSpec {
        name: "image",
        content: ContentKind::None,
        form: MmdForm::Image,
        props: &[URL, CAPTION, NAME, PREVIEW, PREVIEW_WIDTH, ALIGN, BG],
        wrapped: true,
    },
    BlockSpec {
        name: "video",
        content: ContentKind::None,
        form: MmdForm::SelfClosing("video"),
        props: &[URL, CAPTION, NAME, PREVIEW, PREVIEW_WIDTH, ALIGN, BG],
        wrapped: true,
    },
    BlockSpec {
        name: "audio",
        content: ContentKind::None,
        form: MmdForm::SelfClosing("audio"),
        props: &[URL, CAPTION, NAME, PREVIEW, BG],
        wrapped: true,
    },
    BlockSpec {
        name: "file",
        content: ContentKind::None,
        form: MmdForm::SelfClosing("file"),
        props: &[URL, in_form(NAME), CAPTION, BG],
        wrapped: true,
    },
    BlockSpec {
        name: "table",
        content: ContentKind::Table,
        form: MmdForm::Table,
        props: &[COLOR],
        wrapped: true,
    },
    BlockSpec {
        name: "callout",
        content: ContentKind::Inline,
        form: MmdForm::Tag("callout"),
        props: &[
            in_form(string("icon", "icon", "💡")),
            in_form(string("color", "color", "default")),
        ],
        wrapped: true,
    },
    BlockSpec {
        name: "equation",
        content: ContentKind::None,
        form: MmdForm::Equation,
        props: &[in_form(string("latex", "latex", ""))],
        wrapped: true,
    },
    BlockSpec {
        name: "bookmark",
        content: ContentKind::None,
        form: MmdForm::SelfClosing("bookmark"),
        props: &[URL],
        wrapped: true,
    },
    BlockSpec {
        name: "columnList",
        content: ContentKind::None,
        form: MmdForm::Tag("columns"),
        props: &[],
        wrapped: false,
    },
    BlockSpec {
        name: "column",
        content: ContentKind::None,
        form: MmdForm::Tag("column"),
        props: &[in_form(prop(
            "width",
            "width",
            PropType::Float,
            Some(PropDefault::Float(1.0)),
        ))],
        wrapped: false,
    },
    BlockSpec {
        name: "pageLink",
        content: ContentKind::None,
        form: MmdForm::Tag("page"),
        props: &[
            in_form(string("pageId", "id", "")),
            in_form(string("title", "title", "")),
        ],
        wrapped: true,
    },
    BlockSpec {
        name: "databaseRef",
        content: ContentKind::None,
        form: MmdForm::SelfClosing("database"),
        props: &[
            in_form(string("databaseId", "id", "")),
            in_form(string("view", "view", "")),
        ],
        wrapped: true,
    },
    BlockSpec {
        name: "syncedBlock",
        content: ContentKind::None,
        form: MmdForm::Tag("synced"),
        props: &[in_form(string("ref", "ref", ""))],
        wrapped: true,
    },
    BlockSpec {
        name: "toc",
        content: ContentKind::None,
        form: MmdForm::SelfClosing("toc"),
        props: &[],
        wrapped: true,
    },
    BlockSpec {
        name: "unknown",
        content: ContentKind::None,
        form: MmdForm::Unknown,
        props: &[
            string("originalType", "type", ""),
            string("data", "data", ""),
            boolean("foreign", "foreign", false),
        ],
        wrapped: true,
    },
];

/// Tous les types de blocs de la P1, dans l'ordre de la conception.
pub fn blocks() -> &'static [BlockSpec] {
    BLOCKS
}

/// Le type de bloc de ce nom, s'il est connu.
pub fn block(name: &str) -> Option<&'static BlockSpec> {
    BLOCKS.iter().find(|b| b.name == name)
}
