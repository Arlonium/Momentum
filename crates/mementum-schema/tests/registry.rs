use mementum_schema::{PropDefault, PropType, block, blocks, cell_props, schema_json};
use std::collections::HashSet;
use std::path::PathBuf;

const P1_TYPES: [&str; 24] = [
    "paragraph",
    "heading",
    "bulletListItem",
    "numberedListItem",
    "checkListItem",
    "toggleListItem",
    "quote",
    "codeBlock",
    "divider",
    "image",
    "video",
    "audio",
    "file",
    "table",
    "callout",
    "equation",
    "bookmark",
    "columnList",
    "column",
    "pageLink",
    "databaseRef",
    "syncedBlock",
    "toc",
    "unknown",
];

fn default_of(block_name: &str, prop: &str) -> (PropType, Option<PropDefault>) {
    let spec = block(block_name).unwrap_or_else(|| panic!("bloc {block_name} absent"));
    let p = spec
        .props
        .iter()
        .find(|p| p.name == prop)
        .unwrap_or_else(|| panic!("prop {block_name}.{prop} absente"));
    (p.ty, p.default)
}

#[test]
fn registry_covers_p1_types() {
    let names: Vec<&str> = blocks().iter().map(|b| b.name).collect();
    assert_eq!(names.len(), 24);
    let got: HashSet<&str> = names.iter().copied().collect();
    let want: HashSet<&str> = P1_TYPES.iter().copied().collect();
    assert_eq!(got.len(), names.len(), "noms de blocs en double");
    assert_eq!(got, want);
}

#[test]
fn defaults_match_blocknote() {
    assert_eq!(
        default_of("paragraph", "backgroundColor"),
        (PropType::String, Some(PropDefault::Str("default")))
    );
    assert_eq!(
        default_of("paragraph", "textColor"),
        (PropType::String, Some(PropDefault::Str("default")))
    );
    assert_eq!(
        default_of("paragraph", "textAlignment"),
        (PropType::String, Some(PropDefault::Str("left")))
    );
    assert_eq!(
        default_of("heading", "level"),
        (PropType::Int, Some(PropDefault::Int(1)))
    );
    assert_eq!(
        default_of("heading", "isToggleable"),
        (PropType::Bool, Some(PropDefault::Bool(false)))
    );
    assert_eq!(
        default_of("image", "showPreview"),
        (PropType::Bool, Some(PropDefault::Bool(true)))
    );
    assert_eq!(
        default_of("column", "width"),
        (PropType::Float, Some(PropDefault::Float(1.0)))
    );
    assert_eq!(
        default_of("unknown", "foreign"),
        (PropType::Bool, Some(PropDefault::Bool(false)))
    );
    assert_eq!(
        default_of("numberedListItem", "start"),
        (PropType::Int, None)
    );
}

#[test]
fn wrapped_is_false_only_for_columns() {
    for b in blocks() {
        let expected = !matches!(b.name, "columnList" | "column");
        assert_eq!(b.wrapped, expected, "wrapped de {}", b.name);
    }
}

#[test]
fn mmd_names_unique_per_block() {
    for b in blocks() {
        let mut seen = HashSet::new();
        let mut seen_bn = HashSet::new();
        for p in b.props {
            assert!(
                seen.insert(p.mmd),
                "nom MMD {} en double dans {}",
                p.mmd,
                b.name
            );
            assert!(
                seen_bn.insert(p.name),
                "prop {} en double dans {}",
                p.name,
                b.name
            );
        }
    }
}

/// Les props de cellule du registre et leurs défauts sont ceux d'une cellule
/// neuve de BlockNote (fixture de la P0).
#[test]
fn cell_props_match_blocknote() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spec/mmd/fixtures/blocknote/p0-all-blocks.json");
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let table = fixture
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "table")
        .expect("tableau dans la fixture");
    let cell = &table["content"]["rows"][0]["cells"][0]["props"];
    let names: HashSet<&str> = cell
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let registry: HashSet<&str> = cell_props().iter().map(|p| p.name).collect();
    assert_eq!(names, registry);
    for p in cell_props() {
        let expected = match p.default {
            Some(PropDefault::Str(s)) => serde_json::json!(s),
            Some(PropDefault::Int(i)) => serde_json::json!(i),
            other => panic!("défaut inattendu pour {} : {other:?}", p.name),
        };
        assert_eq!(cell[p.name], expected, "défaut de {}", p.name);
    }
}

#[test]
fn schema_json_is_committed() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/schema/blocks.json");
    let mut expected = serde_json::to_string_pretty(&schema_json()).unwrap();
    expected.push('\n');
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} illisible ({e}) : lancer avec UPDATE_SCHEMA=1",
            path.display()
        )
    });
    assert_eq!(
        committed, expected,
        "spec/schema/blocks.json périmé : lancer avec UPDATE_SCHEMA=1"
    );
}
