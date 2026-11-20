//! Ce que les modules inclus de `mementum-crdt` (`foreign.rs`) prennent de `crate::guard`, recopié
//! de `crates/mementum-crdt/src/guard.rs` (la garde elle-même n'est pas incluse : elle a un
//! dossier de sous-modules, qu'une inclusion par chemin ne trouve pas). Le banc n'écrit aucun
//! nœud étranger : seule la forme compte.

use mementum_md::{MAX_COLOR_BYTES, MAX_HREF_BYTES};

#[derive(Clone, Copy)]
pub(crate) enum Carried {
    Nothing,
    Color,
    Href,
}

pub(crate) const MARKS: &[(&str, Carried)] = &[
    ("bold", Carried::Nothing),
    ("italic", Carried::Nothing),
    ("underline", Carried::Nothing),
    ("strike", Carried::Nothing),
    ("code", Carried::Nothing),
    ("textColor", Carried::Color),
    ("backgroundColor", Carried::Color),
    ("link", Carried::Href),
];

pub(crate) fn value_refusal((_, carried): (&'static str, Carried), value: &str) -> Option<()> {
    let bound = match carried {
        Carried::Nothing => 0,
        Carried::Color => MAX_COLOR_BYTES,
        Carried::Href => MAX_HREF_BYTES,
    };
    (value.len() > bound * 6 + 32).then_some(())
}
