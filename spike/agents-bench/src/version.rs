//! Step 4 bis : ce que coûte une version d'avant un agent (`before_agent`, Ruling R1234) d'une page
//! de 10 000 blocs : l'état encodé depuis le document, sa relecture par `PageDoc::check_state`
//! (R816), son écriture par `Store::put_version` (la base du banc, `synchronous = FULL`, dans le
//! dossier donné). Les chiffres de R816 et R367, relevés de nouveau.

use std::path::Path;
use std::time::Instant;

use mementum_crdt::PageDoc;
use mementum_md::parse;
use mementum_store::{NewNode, NodeKind, Store, VersionReason};

use crate::stats::summary;

fn page_of(mmd: &str) -> PageDoc {
    let blocks = parse(mmd).unwrap();
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&blocks).unwrap();
    page
}

pub fn run(args: &[String]) {
    let dir = Path::new(&args[0]);
    let reps: usize = args
        .iter()
        .position(|a| a == "--reps")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(11);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        std::fs::create_dir_all(dir).unwrap();
        let store = Store::open(&dir.join("mementum.db")).await.unwrap();
        let space = store.create_space("Banc", None).await.unwrap();
        let plain = (1..=10_000)
            .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
            .collect::<Vec<_>>()
            .join("\n\n")
            + "\n";
        let rich_line = "**du texte** ordinaire et **du texte** ordinaire et **du texte** fin";
        let rich = vec![rich_line; 10_000].join("\n\n") + "\n";
        let small = (1..=1_000)
            .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
            .collect::<Vec<_>>()
            .join("\n\n")
            + "\n";
        for (label, mmd) in [
            ("1 000 paragraphes", &small),
            ("10 000 paragraphes (page des budgets)", &plain),
            ("10 000 paragraphes de six plages", &rich),
        ] {
            let page = page_of(mmd);
            let node = store
                .create_node(NewNode {
                    space_id: space.id,
                    parent_id: None,
                    after: None,
                    kind: NodeKind::Page,
                    title: label.to_string(),
                    created_by: None,
                    grants: vec![],
                    doc: Some(page.encode_state()),
                })
                .await
                .unwrap()
                .id;
            let mut encode = Vec::new();
            let mut check = Vec::new();
            let mut write = Vec::new();
            let mut whole = Vec::new();
            let mut bytes = 0;
            for seq in 0..reps {
                let start = Instant::now();
                let state = page.encode_state();
                encode.push(start.elapsed());
                let t = Instant::now();
                PageDoc::check_state(&state).unwrap();
                check.push(t.elapsed());
                let t = Instant::now();
                store
                    .put_version(node, seq as i64 + 1, &state, &[], VersionReason::BeforeRestore)
                    .await
                    .unwrap();
                write.push(t.elapsed());
                whole.push(start.elapsed());
                bytes = state.len();
            }
            println!("=== {label} : état {bytes} octets");
            println!("  encodage (encode_state) : {}", summary(&encode));
            println!("  relecture (PageDoc::check_state) : {}", summary(&check));
            println!("  écriture (Store::put_version, before_restore) : {}", summary(&write));
            println!("  les trois, de suite (ce que l'acteur tiendrait) : {}", summary(&whole));
        }
        store.close().await;
    });
}
