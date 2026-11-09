//! SPIKE jetable (Task 10) : ce que yrs 0.28.1 laisse lire d'un bloc
//! supprimé avec `skip_gc`. Non commité.

use mementum_crdt::PageDoc;
use mementum_md::{block_text, parse};
use yrs::types::text::YChange;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encoder, EncoderV1};
use yrs::{
    Assoc, BranchID, Doc, IdSet, IndexScope, OffsetKind, Options, ReadTxn, Snapshot, StateVector,
    StickyIndex, Text, Transact, Update, Xml, XmlElementRef, XmlFragment, XmlOut, XmlTextRef, ID,
};

fn ghost(txn: &mut yrs::TransactionMut, text: &XmlTextRef) -> Vec<(String, Option<ID>)> {
    let hi = Snapshot::new(txn.state_vector(), IdSet::new());
    let lo = Snapshot::new(StateVector::default(), IdSet::new());
    text.diff_range(txn, Some(&hi), Some(&lo), YChange::identity)
        .into_iter()
        .map(|d| {
            let s = match d.insert {
                yrs::Out::Any(yrs::Any::String(s)) => s.to_string(),
                other => format!("<{other:?}>"),
            };
            (s, d.ychange.map(|c| c.id))
        })
        .collect()
}

fn scenario(history: bool) {
    println!("\n===== historique dans X : {history} =====");
    let (base, _) = PageDoc::new_page();
    let first = if history { "Bloc Xx" } else { "Bloc X" };
    base.write_blocks(&parse(&format!("{first}\n\nBloc Y\n\nBloc Z\n")).unwrap())
        .unwrap();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
    if history {
        // Une faute corrigée avant le déplacement : suppression ancienne.
        base.replace_text(&ids[0], 6, 1, "").unwrap();
    }
    let state = base.encode_state();
    let a = PageDoc::from_updates([state.as_slice()]).unwrap();
    let b = PageDoc::from_updates([state.as_slice()]).unwrap();
    println!("texte commun de X : {:?}", block_text(&a.read_blocks()[0]));
    let moved = a.move_block(&ids[0], Some(&ids[2])).unwrap();
    let edited = b.replace_text(&ids[0], 6, 0, " modifié par B").unwrap();

    let server = PageDoc::from_updates([state.as_slice()]).unwrap();
    server.apply_update(&moved).unwrap();
    let copy = server
        .read_blocks()
        .into_iter()
        .find(|blk| blk.id == ids[0])
        .unwrap();
    println!("copie vivante après le déplacement : {:?}", block_text(&copy));

    let doc = server.yrs_doc();
    let mut txn = doc.transact_mut();
    txn.apply_update(Update::decode_v1(&edited).unwrap()).unwrap();
    let inserted = txn.insert_set().clone();
    println!("insérés par B : {inserted:?} ; supprimés dans la transaction : {:?}", txn.delete_set());
    let (client, ranges) = inserted.iter().next().unwrap();
    let start = ranges.iter().next().unwrap().start;
    let id = ID::new(*client, start);
    let offset = StickyIndex::new(IndexScope::Relative(id), Assoc::After)
        .get_offset(&txn)
        .expect("parent retrouvé");
    let text = XmlTextRef::from(offset.branch);
    println!("XmlText parent supprimé : {}", offset.branch.is_deleted());
    let Some(XmlOut::Element(node)) = text.parent() else { panic!() };
    let Some(XmlOut::Element(container)) = node.parent() else { panic!() };
    println!(
        "conteneur : <{}> supprimé={} ; get_attribute(id) = {:?} ; enfants visibles = {}",
        container.tag(),
        AsRef::<yrs::branch::Branch>::as_ref(&container).is_deleted(),
        container.get_attribute(&txn, "id"),
        container.children(&txn).count()
    );
    println!("texte « fantôme » (diff_range, tout l'historique) : {:?}", ghost(&mut txn, &text));

    // Seul moyen public de relire l'id : rematérialiser tout le document
    // sans suppressions, dans un autre Doc.
    let mut enc = EncoderV1::new();
    txn.encode_state_from_snapshot(&Snapshot::new(txn.state_vector(), IdSet::new()), &mut enc)
        .unwrap();
    let all = enc.to_vec();
    let BranchID::Nested(container_id) = AsRef::<yrs::branch::Branch>::as_ref(&container).id() else { panic!() };
    drop(txn);
    let ghost_doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        skip_gc: true,
        ..Options::default()
    });
    let _ = ghost_doc.get_or_insert_xml_fragment(mementum_crdt::FRAGMENT);
    let _ = ghost_doc.get_or_insert_map(mementum_crdt::META);
    ghost_doc
        .transact_mut()
        .apply_update(Update::decode_v1(&all).unwrap())
        .unwrap();
    let t = ghost_doc.transact();
    let el = XmlElementRef::from(BranchID::get_nested(&t, &container_id).unwrap());
    println!(
        "id relu dans le document rematérialisé ({} octets) : {:?}",
        all.len(),
        el.get_attribute(&t, "id")
    );
}

#[test]
fn spike_read_deleted_block() {
    scenario(false);
    scenario(true);
}

fn timed_load(state: &[u8]) -> std::time::Duration {
    let mut times = Vec::new();
    for _ in 0..9 {
        let start = std::time::Instant::now();
        let page = PageDoc::from_updates([state]).unwrap();
        times.push(start.elapsed());
        assert!(!page.read_blocks().is_empty());
    }
    times.sort();
    times[4]
}

#[test]
fn spike_measure_state() {
    let gc = std::env::var_os("SPIKE_GC").is_some();
    let mmd = (1..=10_000)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let blocks = parse(&mmd).unwrap();
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&blocks).unwrap();
    let report = |label: &str, page: &PageDoc| {
        let state = page.encode_state();
        println!(
            "MESURE gc={gc} {label} : encode_state {} Ko, chargement (médiane de 9) {:?}",
            state.len() / 1024,
            timed_load(&state)
        );
    };
    report("page neuve", &page);
    // Frappe : 2 000 éditions qui insèrent 20 caractères puis les retirent.
    let ids: Vec<String> = page.read_blocks().into_iter().map(|b| b.id).collect();
    for k in 0..2_000 {
        let id = &ids[(k * 7919) % ids.len()];
        page.replace_text(id, 3, 0, "abcdefghijklmnopqrst").unwrap();
        page.replace_text(id, 3, 20, "").unwrap();
    }
    report("après 40 000 caractères tapés puis effacés", &page);
    for n in 1..=3 {
        page.write_blocks(&blocks).unwrap();
        report(&format!("après {n} réécriture(s) complète(s) (PUT /mmd)"), &page);
    }
}

#[test]
fn spike_measure_rewrite_in_journal() {
    let gc = std::env::var_os("SPIKE_GC").is_some();
    let mmd = (1..=10_000)
        .map(|i| format!("Paragraphe {i} de la page de mesure, avec un peu de texte ordinaire."))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let blocks = parse(&mmd).unwrap();
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&blocks).unwrap();
    let snapshot = page.encode_state();
    let start = std::time::Instant::now();
    let rewrite = page.write_blocks(&blocks).unwrap();
    println!("MESURE gc={gc} réécriture dans l'acteur (write_blocks) : {:?}", start.elapsed());
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let p = PageDoc::from_updates([snapshot.as_slice(), rewrite.as_slice()]).unwrap();
        times.push(start.elapsed());
        drop(p);
    }
    times.sort();
    println!("MESURE gc={gc} chargement instantané + réécriture au journal : médiane {:?}", times[2]);
}

#[test]
fn spike_pure_deletion_after_move_is_invisible() {
    let (base, _) = PageDoc::new_page();
    base.write_blocks(&parse("Bloc X\n\nBloc Y\n\nBloc Z\n").unwrap()).unwrap();
    let ids: Vec<String> = base.read_blocks().into_iter().map(|b| b.id).collect();
    let state = base.encode_state();
    let a = PageDoc::from_updates([state.as_slice()]).unwrap();
    let b = PageDoc::from_updates([state.as_slice()]).unwrap();
    let moved = a.move_block(&ids[0], Some(&ids[2])).unwrap();
    // B efface « Bloc » : une suppression seule, rien d'inséré.
    let erased = b.replace_text(&ids[0], 0, 5, "").unwrap();
    let server = PageDoc::from_updates([state.as_slice()]).unwrap();
    server.apply_update(&moved).unwrap();
    let before = (server.revision(), server.encode_state());
    let doc = server.yrs_doc();
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(Update::decode_v1(&erased).unwrap()).unwrap();
        println!(
            "MESURE suppression de B après le déplacement : insérés {:?}, supprimés dans la transaction {:?}",
            txn.insert_set(),
            txn.delete_set()
        );
    }
    println!(
        "MESURE révision inchangée : {} ; état inchangé : {}",
        server.revision() == before.0,
        server.encode_state() == before.1
    );
}
