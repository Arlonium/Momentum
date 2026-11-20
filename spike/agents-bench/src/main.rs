//! Banc jetable de la Task S0 du lot 5. Voir `README.md`.
//!
//! Les modules d'écriture de `mementum-crdt` (`write_nodes`, `write_inline`, `write_table`…) sont
//! `pub(crate)` : le banc les inclut par leur chemin, à sa racine, pour que leurs `crate::…` se
//! résolvent ici. La garde (`guard.rs`, qui a un dossier de sous-modules) ne s'inclut pas ainsi :
//! `src/guard.rs` en recopie ce que `foreign.rs` demande.
#![allow(dead_code, unused_imports, clippy::all)]

#[path = "../../../crates/mementum-crdt/src/attrs.rs"]
mod attrs;
#[path = "../../../crates/mementum-crdt/src/blocknote.rs"]
mod blocknote;
#[path = "../../../crates/mementum-crdt/src/foreign.rs"]
mod foreign;
mod guard;
#[path = "../../../crates/mementum-crdt/src/kept_ids.rs"]
mod kept_ids;
#[path = "../../../crates/mementum-crdt/src/table.rs"]
mod table;

mod diff;
mod mcp;
mod narrow;
mod stats;
mod step2;
mod step3;
mod version;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = &args[1.min(args.len())..];
    match args.first().map(String::as_str) {
        Some("diff") => step2::run(rest),
        Some("inline-base") => step3::base(rest),
        Some("inline-apply") => step3::apply(rest),
        Some("narrow") => narrow::run(rest),
        Some("version") => version::run(rest),
        Some("mcp") => mcp::run(rest),
        _ => {
            eprintln!(
                "usage : agents-bench diff | inline-base <dossier> | inline-apply <dossier> | \
                 narrow | version <dossier> | mcp <port> [--serve both|2026|2025] [--log <fichier>]"
            );
            std::process::exit(2);
        }
    }
}
