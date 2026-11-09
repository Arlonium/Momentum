//! Registre unique des types de blocs de Mementum.
//!
//! Toutes les autres crates (MMD, CRDT, serveur) et le client web lisent ce
//! registre : noms de blocs, props, défauts et forme MMD. Il est aussi publié
//! sous forme de `spec/schema/blocks.json`.

mod blocks;
mod inline;

pub use blocks::{
    BlockSpec, ContentKind, MmdForm, PropDefault, PropSpec, PropType, block, blocks, cell_props,
};
pub use inline::{INLINE_TYPES, STYLES};

use serde_json::{Value, json};

/// Version du schéma. Un client plus ancien que le serveur passe en lecture seule.
pub const SCHEMA_VERSION: u32 = 1;

fn default_json(default: Option<PropDefault>) -> Value {
    match default {
        None => Value::Null,
        Some(PropDefault::Str(s)) => json!(s),
        Some(PropDefault::Int(i)) => json!(i),
        Some(PropDefault::Float(f)) => json!(f),
        Some(PropDefault::Bool(b)) => json!(b),
    }
}

fn props_json(props: &[PropSpec]) -> Vec<Value> {
    props
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "type": p.ty.as_str(),
                "default": default_json(p.default),
            })
        })
        .collect()
}

/// Le registre au format JSON (`spec/schema/blocks.json`).
pub fn schema_json() -> Value {
    let blocks: Vec<Value> = blocks()
        .iter()
        .map(|b| json!({ "name": b.name, "content": b.content.as_str(), "props": props_json(b.props) }))
        .collect();
    json!({
        "version": SCHEMA_VERSION,
        "blocks": blocks,
        "tableCell": { "props": props_json(cell_props()) },
        "styles": STYLES,
        "inline": INLINE_TYPES,
    })
}
