//! L'écriture d'une page par différence (`PageDoc::write_diff`, Rulings R1227, R1254, R1255) :
//! un bloc inchangé n'est pas touché, liste de frères par liste de frères ; un contenu en ligne
//! changé s'écrit par préfixe et suffixe communs quand le milieu changé tient dans un seul texte
//! sans élément en ligne ; les suppressions, puis les écritures, en deux transactions ; une page
//! que `read_blocks` normalise retombe sur la récriture complète.
//!
//! « Touché » se juge dans le document, élément par élément (la règle de mesure de R299) :
//! l'identité Yjs de chaque élément et de chaque texte d'un bloc (`identities`), et les unités
//! qu'une écriture crée ou supprime (`inserted_units`, `deleted_units`), jamais par les seuls
//! identifiants de blocs.
//!
//! Les frappes concurrentes viennent du vrai Yjs : voir la partie « octets du vrai Yjs », plus
//! bas, et `apps/web/src/editor/diffWrites.test.ts`.

#[path = "common/live.rs"]
mod live;

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use mementum_crdt::{DiffWritten, FRAGMENT, PageDoc, diff_cost, write_cost};
use mementum_md::{Block, Content, Inline, RenderOpts, arb, parse, render};
use proptest::prelude::*;
use yrs::branch::Branch;
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, OffsetKind, Options, ReadTxn, StateVector, Transact, Update, Xml, XmlElementPrelim,
    XmlElementRef, XmlFragment, XmlOut, XmlTextPrelim,
};

// ------------------------------------------------------------------------------------ outils

/// Une page qui porte `mmd`, écrite par le serveur.
fn page_with(mmd: &str) -> PageDoc {
    let (page, _) = PageDoc::new_page();
    page.write_blocks(&parse(mmd).unwrap()).unwrap();
    page
}

/// Le MMD de la page, sans ancres, comme `GET /mmd`.
fn mmd_of(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

/// Écrit `mmd` par différence, et rend ce que l'écriture dit.
fn diff(page: &PageDoc, mmd: &str) -> DiffWritten {
    page.write_diff(&parse(mmd).unwrap())
        .unwrap_or_else(|e| panic!("écriture de {mmd:?} : {e}"))
}

/// Les identifiants des blocs de premier niveau, dans l'ordre.
fn top_ids(page: &PageDoc) -> Vec<String> {
    page.read_blocks().into_iter().map(|b| b.id).collect()
}

fn branch_id(branch: &Branch) -> String {
    format!("{:?}", branch.id())
}

/// L'identité Yjs des éléments et des textes de chaque bloc, par identifiant de bloc : l'élément
/// du bloc (conteneur, colonne, liste de colonnes), son nœud de contenu et tout ce que celui-ci
/// porte (textes, éléments en ligne, lignes et cellules d'un tableau), sans les blocs enfants.
fn identities(page: &PageDoc) -> BTreeMap<String, Vec<String>> {
    fn content<T: ReadTxn>(txn: &T, node: &XmlElementRef, out: &mut Vec<String>) {
        out.push(branch_id(node.as_ref()));
        for child in node.children(txn) {
            match child {
                XmlOut::Element(e) => content(txn, &e, out),
                XmlOut::Text(t) => out.push(branch_id(t.as_ref())),
                XmlOut::Fragment(_) => {}
            }
        }
    }
    fn walk<T: ReadTxn>(txn: &T, list: &XmlElementRef, into: &mut BTreeMap<String, Vec<String>>) {
        for child in list.children(txn) {
            let XmlOut::Element(element) = child else {
                continue;
            };
            let id = match element.get_attribute(txn, "id") {
                Some(yrs::Out::Any(yrs::Any::String(s))) => s.to_string(),
                _ => String::new(),
            };
            let mut own = vec![branch_id(element.as_ref())];
            match element.tag().as_ref() {
                "columnList" | "column" => walk(txn, &element, into),
                _ => {
                    for part in element.children(txn) {
                        match part {
                            XmlOut::Element(e) if e.tag().as_ref() == "blockGroup" => {
                                walk(txn, &e, into)
                            }
                            XmlOut::Element(e) => content(txn, &e, &mut own),
                            XmlOut::Text(t) => own.push(branch_id(t.as_ref())),
                            XmlOut::Fragment(_) => {}
                        }
                    }
                }
            }
            into.insert(id, own);
        }
    }
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let mut out = BTreeMap::new();
    for child in fragment.children(&txn) {
        if let XmlOut::Element(group) = child {
            walk(&txn, &group, &mut out);
        }
    }
    out
}

/// Les blocs dont l'identité Yjs a changé entre deux relevés (partis, arrivés, ou recréés).
fn changed(
    before: &BTreeMap<String, Vec<String>>,
    after: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    let ids: HashSet<&String> = before.keys().chain(after.keys()).collect();
    let mut out: Vec<String> = ids
        .into_iter()
        .filter(|id| before.get(*id) != after.get(*id))
        .cloned()
        .collect();
    out.sort();
    out
}

/// Les unités Yjs (horloges) qu'une écriture a créées : la différence des vecteurs d'état.
fn inserted_units(before: &[u8], after: &[u8]) -> u32 {
    let (before, after) = (
        StateVector::decode_v1(before).unwrap(),
        StateVector::decode_v1(after).unwrap(),
    );
    after
        .iter()
        .map(|(client, clock)| clock - before.get(client))
        .sum()
}

/// Les unités Yjs que la mise à jour supprime.
fn deleted_units(update: &[u8]) -> u32 {
    Update::decode_v1(update)
        .unwrap()
        .delete_set()
        .iter()
        .flat_map(|(_, ranges)| ranges.iter())
        .map(|range| range.end - range.start)
        .sum()
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

/// La page relue de son journal (l'état de départ, puis la mise à jour) est la page vivante, à
/// l'arbre près (`live::tree` : chaque élément, chaque plage et ses marques), comme la page relue
/// de son propre état ; une seule mise à jour porte toute l'écriture.
fn assert_update_is_whole(page: &PageDoc, before: &[u8], written: &DiffWritten, what: &str) {
    let replica = PageDoc::from_updates([before, written.update.as_slice()])
        .unwrap_or_else(|e| panic!("{what} : la mise à jour ne se relit pas : {e}"));
    assert_eq!(replica.read_blocks(), page.read_blocks(), "{what} : relue");
    assert_eq!(
        live::tree(replica.yrs_doc()),
        live::tree(page.yrs_doc()),
        "{what} : l'arbre relu"
    );
    assert_eq!(replica.revision(), page.revision(), "{what} : révision");
    if let Some(why) = live::reloaded_from_state_differs(page) {
        panic!("{what} : {why}");
    }
}

const TEN: &str = "Un\n\nDeux\n\nTrois\n\nQuatre\n\nCinq\n\nSix\n\nSept\n\nHuit\n\nNeuf\n\nDix\n";

// ---------------------------------------------------------------------- blocs et listes

/// Une page récrite telle qu'elle se lit n'écrit rien : mise à jour vide, révision égale, aucun
/// bloc touché ; avec ou sans ancres, sur tous les types de blocs.
#[test]
fn an_identical_page_writes_nothing() {
    let fixture = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../spec/mmd/fixtures/all-blocks.mmd"),
    )
    .unwrap();
    for what in ["sans ancres", "avec les identifiants"] {
        let page = page_with(&fixture);
        let before = (page.encode_state(), page.revision(), identities(&page));
        let blocks = if what == "sans ancres" {
            parse(&mmd_of(&page)).unwrap()
        } else {
            page.read_blocks()
        };
        let written = page.write_diff(&blocks).unwrap();
        assert!(
            !written.fell_back,
            "{what} : la page se lit élément pour bloc"
        );
        assert_eq!(written.update, [0, 0], "{what} : la mise à jour est vide");
        assert!(written.touched.is_empty(), "{what} : {:?}", written.touched);
        assert_eq!(page.encode_state(), before.0, "{what} : l'état");
        assert_eq!(page.revision(), before.1, "{what} : la révision");
        assert_eq!(identities(&page), before.2, "{what}");
    }
}

/// Un bloc dont le texte change : lui seul est touché, son texte garde son élément (préfixe et
/// suffixe communs), et l'écriture crée et supprime le seul caractère qui change.
#[test]
fn one_changed_block_touches_one_block() {
    let page = page_with(TEN);
    let (state, before) = (page.encode_state(), identities(&page));
    let third = top_ids(&page)[2].clone();
    let written = diff(&page, &TEN.replace("Trois", "Trais"));
    assert!(!written.fell_back);
    assert_eq!(written.touched, [third.clone()]);
    assert_eq!(mmd_of(&page), TEN.replace("Trois", "Trais"));
    assert_eq!(
        changed(&before, &identities(&page)),
        Vec::<String>::new(),
        "aucun élément recréé, pas même le texte du bloc changé"
    );
    assert_eq!(
        inserted_units(&state_vector(&state), &page.state_vector()),
        1
    );
    assert_eq!(deleted_units(&written.update), 1);
    assert_update_is_whole(&page, &state, &written, "un bloc changé");
}

/// Le vecteur d'état d'un état encodé.
fn state_vector(state: &[u8]) -> Vec<u8> {
    PageDoc::from_updates([state]).unwrap().state_vector()
}

/// Des props changées s'écrivent en attributs du nœud de contenu : une entrée par prop changée,
/// l'ancienne supprimée ; ni le bloc ni son contenu ne sont recréés.
#[test]
fn changed_props_are_written_as_attributes() {
    let base = "# Titre\n\nUn paragraphe\n\nAutre\n";
    let page = page_with(base);
    let (state, before) = (page.encode_state(), identities(&page));
    let ids = top_ids(&page);
    let target = "### Titre\n\nUn paragraphe {align=\"center\"}\n\nAutre\n";
    let written = diff(&page, target);
    assert!(!written.fell_back);
    assert_eq!(
        sorted(written.touched.clone()),
        sorted(vec![ids[0].clone(), ids[1].clone()])
    );
    assert_eq!(mmd_of(&page), target);
    assert_eq!(changed(&before, &identities(&page)), Vec::<String>::new());
    assert_eq!(
        inserted_units(&state_vector(&state), &page.state_vector()),
        2,
        "une entrée pour le niveau, une pour l'alignement"
    );
    assert_eq!(
        deleted_units(&written.update),
        2,
        "les deux entrées remplacées"
    );
    assert_update_is_whole(&page, &state, &written, "props");

    // Revenir au défaut : la valeur du défaut est écrite, rien d'autre.
    let state = page.encode_state();
    let written = diff(&page, base);
    assert_eq!(mmd_of(&page), base);
    assert_eq!(
        inserted_units(&state_vector(&state), &page.state_vector()),
        2
    );
    assert_update_is_whole(&page, &state, &written, "props au défaut");
}

/// Un bloc déplacé parmi ses frères est recréé sous son identifiant ; les autres ne sont pas
/// touchés (la règle de R223 : le moins de blocs recréés).
#[test]
fn a_moved_block_is_recreated_under_its_id_and_the_others_stay() {
    let page = page_with(TEN);
    let (state, before) = (page.encode_state(), identities(&page));
    let ids = top_ids(&page);
    let target = TEN.replace("Deux\n\n", "") + "\nDeux\n";
    let written = diff(&page, &target);
    assert!(!written.fell_back);
    assert_eq!(mmd_of(&page), target);
    let mut moved_ids = ids.clone();
    let moved = moved_ids.remove(1);
    moved_ids.push(moved.clone());
    assert_eq!(
        top_ids(&page),
        moved_ids,
        "chaque bloc garde son identifiant"
    );
    assert_eq!(changed(&before, &identities(&page)), [moved.clone()]);
    assert_eq!(written.touched, [moved]);
    assert_update_is_whole(&page, &state, &written, "un bloc déplacé");
}

/// Un bloc inséré reçoit un identifiant neuf ; un bloc retiré part ; les autres ne sont pas
/// touchés.
#[test]
fn inserted_and_removed_blocks() {
    let page = page_with(TEN);
    let (state, before) = (page.encode_state(), identities(&page));
    let ids = top_ids(&page);
    let target = TEN
        .replace("Quatre\n\n", "")
        .replace("Sept\n", "Sept\n\nNeuf et demi\n");
    let written = diff(&page, &target);
    assert!(!written.fell_back);
    assert_eq!(mmd_of(&page), target);
    let now = top_ids(&page);
    assert!(!now.contains(&ids[3]), "le bloc retiré est parti");
    let fresh: Vec<&String> = now.iter().filter(|id| !ids.contains(id)).collect();
    assert_eq!(fresh.len(), 1, "un seul identifiant neuf : {now:?}");
    assert_eq!(
        changed(&before, &identities(&page)),
        sorted(vec![ids[3].clone(), fresh[0].clone()])
    );
    assert_eq!(
        sorted(written.touched.clone()),
        sorted(vec![ids[3].clone(), fresh[0].clone()])
    );
    assert_update_is_whole(&page, &state, &written, "inséré et retiré");
}

/// Les enfants se comparent liste par liste : un petit-enfant changé ne touche que lui ; des
/// enfants ajoutés à un bloc qui n'en avait pas, ou tous retirés, ne touchent pas le bloc
/// lui-même au-delà de son groupe d'enfants.
#[test]
fn children_are_diffed_list_by_list() {
    let base = "- Un\n  - Un un\n    - Un un un\n  - Un deux\n- Deux\n- Trois\n  - Trois un\n";
    let page = page_with(base);
    let (state, before) = (page.encode_state(), identities(&page));
    let target = base
        .replace("Un un un", "Un un trois")
        .replace("- Deux\n", "- Deux\n  - Deux un\n")
        .replace("  - Trois un\n", "");
    let written = diff(&page, &target);
    assert!(!written.fell_back);
    assert_eq!(mmd_of(&page), target);
    let after = identities(&page);
    let gone: Vec<&String> = before
        .keys()
        .filter(|id| !after.contains_key(*id))
        .collect();
    let fresh: Vec<&String> = after
        .keys()
        .filter(|id| !before.contains_key(*id))
        .collect();
    assert_eq!(
        (gone.len(), fresh.len()),
        (1, 1),
        "« Trois un » parti, « Deux un » arrivé"
    );
    let blocks = page.read_blocks();
    let deepest = blocks[0].children[0].children[0].id.clone();
    assert_eq!(
        changed(&before, &after),
        sorted(vec![gone[0].clone(), fresh[0].clone()]),
        "ni « Deux » ni « Trois » ne sont recréés ; le petit-enfant changé garde ses éléments"
    );
    assert!(written.touched.contains(&deepest));
    assert_update_is_whole(&page, &state, &written, "enfants");
}

/// Un bloc qui change de parent : sans ancre, `keep_ids` ne le suit pas, et il reçoit un
/// identifiant neuf (R1227, cas 3) ; avec son ancre, il garde son identifiant, recréé sous son
/// nouveau parent.
#[test]
fn a_block_that_changes_parent_is_removed_and_inserted() {
    let base = "- Parent\n- Mobile\n";
    let target = "- Parent\n  - Mobile\n";
    let page = page_with(base);
    let ids = top_ids(&page);
    let written = diff(&page, target);
    assert_eq!(mmd_of(&page), target);
    let blocks = page.read_blocks();
    assert_eq!(blocks[0].id, ids[0]);
    assert_ne!(
        blocks[0].children[0].id, ids[1],
        "sans ancre : un identifiant neuf"
    );
    assert!(written.touched.contains(&ids[1]));

    let page = page_with(base);
    let ids = top_ids(&page);
    let before = identities(&page);
    let mut blocks = page.read_blocks();
    let mobile = blocks.remove(1);
    blocks[0].children.push(mobile);
    page.write_diff(&blocks).unwrap();
    assert_eq!(mmd_of(&page), target);
    let blocks = page.read_blocks();
    assert_eq!(
        blocks[0].children[0].id, ids[1],
        "avec ancre : le même identifiant"
    );
    assert_ne!(
        identities(&page)[&ids[1]],
        before[&ids[1]],
        "recréé sous son nouveau parent"
    );
    assert_eq!(identities(&page)[&ids[0]], before[&ids[0]]);
}

/// Colonnes et tableaux : le texte d'un bloc d'une colonne ne touche que lui ; une colonne
/// ajoutée est insérée dans sa liste ; une cellule changée récrit le tableau, son nœud de
/// contenu gardé ; la page relue suit le schéma de BlockNote.
#[test]
fn columns_and_tables_follow_the_schema() {
    let base = "<columns>\n<column>\nGauche\n</column>\n<column>\nDroite\n</column>\n</columns>\n\n\
                | a | b |\n| --- | --- |\n| c | d |\n\nFin\n";
    let page = page_with(base);
    let before = identities(&page);
    let blocks = page.read_blocks();
    let (list, table) = (blocks[0].clone(), blocks[1].id.clone());
    let left = list.children[0].children[0].id.clone();

    let target = base.replace("Gauche", "Gaucher");
    let written = diff(&page, &target);
    assert!(!written.fell_back);
    assert_eq!(mmd_of(&page), target);
    assert_eq!(written.touched, [left]);
    assert_eq!(changed(&before, &identities(&page)), Vec::<String>::new());

    let state = page.encode_state();
    let before = identities(&page);
    let target = target.replace(
        "<column>\nDroite\n</column>\n",
        "<column>\nDroite\n</column>\n<column>\nTroisième\n</column>\n",
    );
    let written = diff(&page, &target);
    assert_eq!(mmd_of(&page), target);
    let fresh = changed(&before, &identities(&page));
    assert_eq!(fresh.len(), 2, "la colonne et son bloc : {fresh:?}");
    assert!(
        !fresh.contains(&list.id),
        "la liste de colonnes n'est pas recréée"
    );
    assert_update_is_whole(&page, &state, &written, "une colonne ajoutée");

    let state = page.encode_state();
    let before = identities(&page);
    let target = target.replace("| c | d |", "| c | e |");
    let written = diff(&page, &target);
    assert_eq!(mmd_of(&page), target);
    assert_eq!(written.touched, [table.clone()]);
    let after = identities(&page);
    assert_eq!(
        changed(&before, &after),
        [table.clone()],
        "le tableau est récrit"
    );
    assert_eq!(
        before[&table][..2],
        after[&table][..2],
        "le conteneur et le nœud `table` sont gardés"
    );
    assert_update_is_whole(&page, &state, &written, "une cellule changée");
}

/// Un bloc dont le type change, sous son identifiant (une ancre, ou `replace_block`), garde son
/// conteneur et ses enfants ; seul son nœud de contenu est recréé. (Sans identifiant, `keep_ids`
/// ne donne pas le sien à un bloc d'un autre type : c'est un bloc retiré et un bloc inséré.)
#[test]
fn a_changed_type_recreates_the_content_node_only() {
    let base = "- Un\n  - Enfant\n\nDeux\n";
    let page = page_with(base);
    let (state, before) = (page.encode_state(), identities(&page));
    let blocks = page.read_blocks();
    let mut target = blocks.clone();
    target[0].kind = "numberedListItem".into();
    let written = page.write_diff(&target).unwrap();
    assert!(!written.fell_back);
    assert_eq!(page.read_blocks(), target);
    let after = identities(&page);
    assert_eq!(changed(&before, &after), [blocks[0].id.clone()]);
    assert_eq!(
        before[&blocks[0].id][0], after[&blocks[0].id][0],
        "le conteneur est gardé"
    );
    assert_eq!(written.touched, [blocks[0].id.clone()]);
    assert_update_is_whole(&page, &state, &written, "type changé");
}

/// Une mention dans le milieu changé fait récrire tout le contenu (R1255) ; un texte changé
/// après une mention, sans elle dans le milieu, s'écrit par préfixe et suffixe.
#[test]
fn an_inline_element_in_the_changed_middle_rewrites_the_whole_content() {
    let mention = "<mention type=\"page\" id=\"pg_a\">A</mention>";
    let base = format!("Avant {mention} après.\n\nAutre\n");
    let page = page_with(&base);
    let first = top_ids(&page)[0].clone();
    let before = identities(&page);
    let target = base.replace("après", "ensuite");
    diff(&page, &target);
    assert_eq!(mmd_of(&page), target);
    assert_eq!(
        identities(&page)[&first],
        before[&first],
        "après la mention : les textes gardent leur élément"
    );

    let before = identities(&page);
    let target = target.replace("Avant ", &format!("Avant {mention} "));
    let written = diff(&page, &target);
    assert_eq!(mmd_of(&page), target);
    let after = identities(&page);
    assert_eq!(
        before[&first][0], after[&first][0],
        "le conteneur est gardé"
    );
    assert_eq!(
        before[&first][1], after[&first][1],
        "le nœud de contenu est gardé"
    );
    assert!(
        before[&first][2..]
            .iter()
            .all(|id| !after[&first].contains(id)),
        "tout le contenu en ligne est récrit : {:?} puis {:?}",
        before[&first],
        after[&first]
    );
    assert_eq!(written.touched, [first]);
}

/// Les suppressions, puis les écritures, en deux transactions (R367, R1254) : la première ne
/// crée rien.
#[test]
fn deletions_and_writes_are_two_transactions() {
    let page = page_with(TEN);
    let seen: Arc<Mutex<Vec<(bool, bool)>>> = Arc::default();
    let log = seen.clone();
    let _watch = page
        .yrs_doc()
        .observe_transaction_cleanup("diff-test", move |_, event| {
            let deleted = event.delete_set.iter().next().is_some();
            log.lock()
                .unwrap()
                .push((event.before_state != event.after_state, deleted));
        })
        .unwrap();
    // L'abonnement ouvre lui-même une transaction, vide.
    seen.lock().unwrap().clear();
    let target = TEN
        .replace("Deux\n\n", "")
        .replace("Cinq", "Cinquante")
        .replace("Huit", "Huit\n\nHuit et demi");
    let written = diff(&page, &target);
    assert_eq!(mmd_of(&page), target);
    assert!(!written.fell_back);
    assert_eq!(
        *seen.lock().unwrap(),
        [(false, true), (true, false)],
        "une transaction qui supprime sans rien créer, puis une qui crée"
    );
}

// --------------------------------------------------------------- pages que la lecture normalise

/// Une page dont le fragment porte `build` en plus de ce que le serveur y a écrit.
fn page_built(build: impl FnOnce(&mut yrs::TransactionMut, &yrs::XmlFragmentRef)) -> PageDoc {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    build(&mut doc.transact_mut(), &fragment);
    let state = doc
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
    PageDoc::from_updates([state.as_slice()]).unwrap()
}

/// Un conteneur de paragraphe d'identifiant `id`, au texte `text`, au bout de `list`.
fn push_paragraph(
    txn: &mut yrs::TransactionMut,
    list: &XmlElementRef,
    id: &str,
    text: &str,
) -> XmlElementRef {
    let container = list.push_back(txn, XmlElementPrelim::empty("blockContainer"));
    container.insert_attribute(txn, "id", id);
    let paragraph = container.push_back(txn, XmlElementPrelim::empty("paragraph"));
    paragraph.push_back(txn, XmlTextPrelim::new(text));
    container
}

/// Les formes que `read_blocks` normalise : ses éléments ne répondent pas un à un à ses blocs.
fn normalized_pages() -> Vec<(&'static str, PageDoc)> {
    vec![
        (
            "une seconde racine",
            page_built(|txn, fragment| {
                for (n, id) in ["a", "b"].iter().enumerate() {
                    let root =
                        fragment.insert(txn, n as u32, XmlElementPrelim::empty("blockGroup"));
                    push_paragraph(txn, &root, id, &format!("Racine {id}"));
                }
            }),
        ),
        (
            "un texte hors de la racine",
            page_built(|txn, fragment| {
                let root = fragment.insert(txn, 0, XmlElementPrelim::empty("blockGroup"));
                push_paragraph(txn, &root, "a", "Dans la racine");
                fragment.insert(txn, 1, XmlTextPrelim::new("hors de la racine"));
            }),
        ),
        (
            "une liste de colonnes défaite",
            page_built(|txn, fragment| {
                let root = fragment.insert(txn, 0, XmlElementPrelim::empty("blockGroup"));
                push_paragraph(txn, &root, "a", "Avant");
                let list = root.push_back(txn, XmlElementPrelim::empty("columnList"));
                list.insert_attribute(txn, "id", "l");
                let column = list.push_back(txn, XmlElementPrelim::empty("column"));
                column.insert_attribute(txn, "id", "c");
                push_paragraph(txn, &column, "b", "Seule colonne");
            }),
        ),
        (
            "des blocs aplatis au-delà de la profondeur",
            page_built(|txn, fragment| {
                let mut list = fragment.insert(txn, 0, XmlElementPrelim::empty("blockGroup"));
                for level in 0..=mementum_md::MAX_DEPTH {
                    let container = push_paragraph(txn, &list, &format!("n{level}"), "x");
                    list = container.push_back(txn, XmlElementPrelim::empty("blockGroup"));
                }
            }),
        ),
        (
            "un nœud étranger hors conteneur",
            page_built(|txn, fragment| {
                let root = fragment.insert(txn, 0, XmlElementPrelim::empty("blockGroup"));
                push_paragraph(txn, &root, "a", "Avant");
                let foreign = root.push_back(txn, XmlElementPrelim::empty("mystere"));
                foreign.insert_attribute(txn, "id", "m");
            }),
        ),
    ]
}

/// Une page que `read_blocks` normalise retombe sur la récriture complète, et se lit ensuite
/// comme les blocs écrits.
#[test]
fn a_page_that_read_blocks_normalizes_falls_back_on_the_full_rewrite() {
    for (what, page) in normalized_pages() {
        let state = page.encode_state();
        let target = mmd_of(&page) + "\nAjouté\n";
        let written = diff(&page, &target);
        assert!(written.fell_back, "{what}");
        assert_eq!(mmd_of(&page), target, "{what}");
        assert_update_is_whole(&page, &state, &written, what);
        // Une fois récrite, la page se lit élément pour bloc, et l'écriture suivante n'y retombe
        // plus ; sauf un nœud étranger hors conteneur, que la récriture recrée tel quel.
        let again = diff(&page, &target.replace("Ajouté", "Ajoutée"));
        assert_eq!(
            again.fell_back,
            what == "un nœud étranger hors conteneur",
            "{what}, ensuite"
        );
    }
}

/// Ce que `read_blocks` ne montre pas (une seconde racine, un contenu hors de la première
/// racine) est retiré par l'écriture, comme par la récriture complète : il ne reste qu'une racine.
#[test]
fn a_diff_write_removes_what_read_blocks_does_not_show() {
    for (what, page) in normalized_pages().into_iter().take(2) {
        let target = mmd_of(&page);
        diff(&page, &target);
        let doc = page.yrs_doc();
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let txn = doc.transact();
        assert_eq!(fragment.len(&txn), 1, "{what} : une seule racine");
        drop(txn);
        assert_eq!(mmd_of(&page), target, "{what}");
    }
}

// -------------------------------------------------------------------------------- le coût

/// `n` paragraphes, d'identifiants `<prefix>-<i>`.
fn paragraphs(n: usize, prefix: &str) -> Vec<Block> {
    let mmd: Vec<String> = (0..n).map(|i| format!("{prefix} {i}")).collect();
    let mut blocks = parse(&(mmd.join("\n\n") + "\n")).unwrap();
    for (i, b) in blocks.iter_mut().enumerate() {
        b.id = format!("{prefix}-{i}");
    }
    blocks
}

/// `current`, un bloc de `added` inséré après chacun.
fn interleaved(current: &[Block], added: &[Block]) -> Vec<Block> {
    current
        .iter()
        .zip(added)
        .flat_map(|(a, b)| [a.clone(), b.clone()])
        .collect()
}

/// `diff_cost` compte ce que la différence écrit : un bloc changé sur 10 000 coûte la lecture de
/// la page, loin de la récriture ; des blocs insérés un sur deux coûtent le parcours de la liste à
/// chaque insertion (yrs) : quatre fois plus de blocs, plus de quatre fois plus ; des blocs retirés
/// un sur deux aussi : la validation fond chaque plage ramassée dans la liste des blocs de yrs, en
/// décalant ce qui suit (Ruling R1284) ; des paires voisines échangées recréent un bloc sur deux, et
/// coûtent ses insertions.
#[test]
fn diff_cost_counts_what_the_write_does() {
    let current = paragraphs(10_000, "p");
    let mut one = current.clone();
    one[5_000].content = Content::Inline(vec![Inline::Text {
        text: "changé".into(),
        styles: Default::default(),
    }]);
    let full = write_cost(&current).nanos;
    let one_cost = diff_cost(&current, &one).nanos;
    assert!(
        one_cost < full / 4,
        "un bloc : {one_cost} ns, la récriture {full} ns"
    );
    assert!(diff_cost(&current, &current).nanos <= one_cost);

    let inserted = |n: usize| {
        let current = paragraphs(n, "p");
        diff_cost(&current, &interleaved(&current, &paragraphs(n, "n"))).nanos
    };
    let (small, large) = (inserted(500), inserted(2_000));
    assert!(large > 4 * small, "insérés : {small} puis {large} ns");

    let removed = |n: usize| {
        let current = paragraphs(n, "p");
        let half: Vec<Block> = current.iter().step_by(2).cloned().collect();
        diff_cost(&current, &half).nanos
    };
    let (small, large) = (removed(2_000), removed(8_000));
    assert!(large > 4 * small, "retirés : {small} puis {large} ns");

    let current = paragraphs(2_000, "p");
    let swapped: Vec<Block> = current
        .chunks(2)
        .flat_map(|pair| pair.iter().rev().cloned())
        .collect();
    let swaps = diff_cost(&current, &swapped).nanos;
    assert!(
        swaps > diff_cost(&current, &current).nanos + 1_000 * 20_000,
        "mille blocs recréés : {swaps} ns"
    );
}

// ---------------------------------------------------------------------------- propriétés

/// Les blocs, sans aucun identifiant (le MMD écrit celui d'un bloc `unknown`, tiré au hasard
/// pour un bloc neuf).
fn shown(blocks: &[Block]) -> String {
    fn clear(blocks: &mut [Block]) {
        for b in blocks {
            b.id.clear();
            clear(&mut b.children);
        }
    }
    let mut blocks = blocks.to_vec();
    clear(&mut blocks);
    render(&blocks, RenderOpts::default()).markdown
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Sur des arbres quelconques, l'écriture par différence se lit comme la récriture complète
    /// des mêmes blocs ; sa mise à jour, appliquée à une copie de la page d'avant, la mène au même
    /// contenu, et la page relue de son journal est la page vivante.
    #[test]
    fn a_diff_write_reads_back_as_the_blocks_written(
        before in arb::tree(),
        after in arb::tree(),
        edits in prop::collection::vec((0..4usize, any::<usize>()), 0..4),
    ) {
        let (page, created) = PageDoc::new_page();
        prop_assume!(page.write_blocks(&before).is_ok());
        let journal = vec![created, page.encode_state()];
        // La moitié des cas récrivent la page d'avant, retouchée ; les autres, d'autres blocs.
        let target = if edits.is_empty() { after } else { retouched(page.read_blocks(), &after, &edits) };
        let oracle = PageDoc::from_updates(journal.iter().map(Vec::as_slice)).unwrap();
        let state = page.encode_state();
        let written = page.write_diff(&target);
        let expected = oracle.rewrite_blocks(&target);
        match (written, expected) {
            (Ok(written), Ok(_)) => {
                prop_assert_eq!(shown(&page.read_blocks()), shown(&oracle.read_blocks()));
                let replica = PageDoc::from_updates([state.as_slice(), written.update.as_slice()]).unwrap();
                prop_assert_eq!(replica.read_blocks(), page.read_blocks());
                prop_assert_eq!(live::tree(replica.yrs_doc()), live::tree(page.yrs_doc()));
                prop_assert_eq!(replica.revision(), page.revision());
                prop_assert_eq!(live::reloaded_from_state_differs(&page), None);
            }
            (Err(a), Err(b)) => prop_assert_eq!(a, b),
            (a, b) => prop_assert!(false, "la différence {:?}, la récriture {:?}", a.map(|_| ()), b.map(|_| ())),
        }
    }
}

/// Les blocs de la page, retouchés : un texte changé, un bloc retiré, un bloc de `other` inséré,
/// un bloc déplacé ; les identifiants retirés une fois sur deux (un MMD sans ancres).
fn retouched(mut blocks: Vec<Block>, other: &[Block], edits: &[(usize, usize)]) -> Vec<Block> {
    for &(kind, at) in edits {
        let n = blocks.len();
        match kind {
            0 if n > 0 => {
                if let Content::Inline(content) = &mut blocks[at % n].content {
                    content.push(Inline::Text {
                        text: "ajout".into(),
                        styles: Default::default(),
                    });
                }
            }
            1 if n > 0 => {
                blocks.remove(at % n);
            }
            2 if !other.is_empty() => blocks.insert(at % (n + 1), other[at % other.len()].clone()),
            3 if n > 1 => {
                let moved = blocks.remove(at % n);
                blocks.insert((at / n) % n, moved);
            }
            _ => {}
        }
    }
    if edits.len() % 2 == 1 {
        fn clear(blocks: &mut [Block]) {
            for b in blocks {
                b.id.clear();
                clear(&mut b.children);
            }
        }
        clear(&mut blocks);
    }
    blocks
}

// ------------------------------------------------------------------------------- mesure

/// Une forme mesurée : la page de départ (son état, ou `None` pour une page écrite par le
/// `PageDoc` qui la récrit, « même client »), les blocs écrits.
struct Measured {
    what: &'static str,
    state: Option<Vec<u8>>,
    mmd: String,
    target: Vec<Block>,
}

/// p50, p95 et pire, au plus proche rang, en millisecondes.
fn quantiles(mut ms: Vec<f64>) -> (f64, f64, f64) {
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| ms[((q * ms.len() as f64).ceil() as usize).clamp(1, ms.len()) - 1];
    (at(0.5), at(0.95), ms[ms.len() - 1])
}

/// Mesure (Task S2, Step 3 ; release, une fois : `cargo test --release -p mementum-crdt --test
/// diff measure_diff_write_of_one_block_in_10000 -- --ignored --nocapture`) : l'écriture par
/// différence d'une page de 10 000 paragraphes, un bloc changé, et les autres formes de la Task S0
/// et de sa relecture (insérés, retirés, déplacés, la moitié, tous, l'ordre renversé, des paires
/// voisines échangées, un bloc sur deux et dix sur onze retirés, le contenu retiré écrit par le même
/// client), et des formes que S0 n'a pas mesurées (liste imbriquée profonde, colonnes, grand
/// tableau, 12 000 blocs) ; contre `rewrite_blocks`, avec l'estimation de `diff_cost`. Chaque
/// écriture mesurée est vérifiée une fois : relue comme voulu, rechargée égale.
#[test]
#[ignore]
fn measure_diff_write_of_one_block_in_10000() {
    let written_page = |mmd: &str| page_with(mmd);
    let text = |b: &mut Block, s: &str| {
        b.content = Content::Inline(vec![Inline::Text {
            text: s.into(),
            styles: Default::default(),
        }]);
    };
    let ten = (0..10_000)
        .map(|i| format!("Paragraphe numéro {i}"))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let state = written_page(&ten).encode_state();
    let mut cases = Vec::new();
    // Les blocs écrits n'ont pas d'identifiants (un MMD sans ancres) : `keep_ids` les donne, et
    // compte dans la durée, comme à la Task S0.
    let mut case =
        |what: &'static str, state: Option<Vec<u8>>, mmd: &str, edit: &dyn Fn(&mut Vec<Block>)| {
            let mut target = parse(mmd).unwrap();
            edit(&mut target);
            fn clear(blocks: &mut [Block]) {
                for b in blocks {
                    b.id.clear();
                    clear(&mut b.children);
                }
            }
            clear(&mut target);
            cases.push(Measured {
                what,
                state,
                mmd: mmd.to_string(),
                target,
            });
        };
    let s = Some(state.clone());
    case("un bloc changé", s.clone(), &ten, &|b| {
        text(&mut b[5_000], "changé")
    });
    case("un bloc inséré", s.clone(), &ten, &|b| {
        let mut new = b[0].clone();
        new.id.clear();
        text(&mut new, "inséré");
        b.insert(5_000, new);
    });
    case("un bloc retiré", s.clone(), &ten, &|b| {
        b.remove(5_000);
    });
    case("un bloc déplacé (100 → 9 000)", s.clone(), &ten, &|b| {
        let moved = b.remove(100);
        b.insert(9_000, moved);
    });
    case("la moitié changée", s.clone(), &ten, &|b| {
        b.iter_mut().step_by(2).for_each(|b| text(b, "changé"))
    });
    case("tous changés", s.clone(), &ten, &|b| {
        b.iter_mut().for_each(|b| text(b, "changé"))
    });
    case("ordre renversé", s.clone(), &ten, &|b| b.reverse());
    case("5 000 paires voisines échangées", s.clone(), &ten, &|b| {
        b.chunks_mut(2).for_each(|pair| pair.reverse())
    });
    case("un bloc sur deux retiré", s.clone(), &ten, &|b| {
        let kept: Vec<Block> = b.iter().step_by(2).cloned().collect();
        *b = kept;
    });
    case("dix sur onze retirés", s.clone(), &ten, &|b| {
        let kept: Vec<Block> = b.iter().step_by(11).cloned().collect();
        *b = kept;
    });
    case("même client, tous changés", None, &ten, &|b| {
        b.iter_mut().for_each(|b| text(b, "changé"))
    });
    case("même client, ordre renversé", None, &ten, &|b| {
        b.reverse()
    });
    let nested = (0..2_000)
        .map(|i| format!("- Élément {i}\n  - Enfant {i}\n    - Petit-enfant {i}\n      - Arrière {i}\n        - Feuille {i}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let nested_state = Some(written_page(&nested).encode_state());
    case(
        "liste imbriquée profonde (10 000 blocs), une feuille changée",
        nested_state,
        &nested,
        &|b| {
            text(
                &mut b[1_000].children[0].children[0].children[0].children[0],
                "changée",
            )
        },
    );
    let column = |side: &str| -> String {
        let lines: Vec<String> = (0..5_000).map(|i| format!("{side} {i}")).collect();
        format!("<column>\n{}\n</column>\n", lines.join("\n\n"))
    };
    let columns = format!(
        "<columns>\n{}{}</columns>\n",
        column("Gauche"),
        column("Droite")
    );
    let columns_state = Some(written_page(&columns).encode_state());
    case(
        "deux colonnes de 5 000 blocs, un changé",
        columns_state,
        &columns,
        &|b| text(&mut b[0].children[1].children[2_500], "changé"),
    );
    let table = format!(
        "| a | b | c | d | e |\n| --- | --- | --- | --- | --- |\n{}\n",
        (0..2_000)
            .map(|i| format!("| a{i} | b{i} | c{i} | d{i} | e{i} |"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let table_state = Some(written_page(&table).encode_state());
    case(
        "tableau de 2 000 lignes, une cellule changée",
        table_state,
        &table,
        &|b| {
            let Content::Table(t) = &mut b[0].content else {
                panic!("un tableau")
            };
            t.rows[1_000][2].content = vec![Inline::Text {
                text: "changée".into(),
                styles: Default::default(),
            }];
        },
    );
    let twelve = (0..12_000)
        .map(|i| format!("Paragraphe numéro {i}"))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n";
    let twelve_state = Some(written_page(&twelve).encode_state());
    case(
        "12 000 paragraphes, un changé",
        twelve_state,
        &twelve,
        &|b| text(&mut b[6_000], "changé"),
    );
    drop(case);

    for c in &cases {
        let fresh = || match &c.state {
            Some(state) => PageDoc::from_updates([state.as_slice()]).unwrap(),
            None => written_page(&c.mmd),
        };
        let current = fresh().read_blocks();
        let estimate = diff_cost(&current, &c.target).nanos as f64 / 1e6;
        // Vérifiée une fois, hors du temps.
        let page = fresh();
        let before = page.encode_state();
        let written = page.write_diff(&c.target).unwrap();
        assert!(!written.fell_back, "{}", c.what);
        assert_eq!(shown(&page.read_blocks()), shown(&c.target), "{}", c.what);
        assert_update_is_whole(&page, &before, &written, c.what);
        let bytes = written.update.len();
        let mut diff_ms = Vec::new();
        for _ in 0..5 {
            let page = fresh();
            let start = std::time::Instant::now();
            page.write_diff(&c.target).unwrap();
            diff_ms.push(start.elapsed().as_secs_f64() * 1e3);
        }
        let mut full_ms = Vec::new();
        let mut full_bytes = 0;
        for _ in 0..3 {
            let page = fresh();
            let start = std::time::Instant::now();
            full_bytes = page.rewrite_blocks(&c.target).unwrap().len();
            full_ms.push(start.elapsed().as_secs_f64() * 1e3);
        }
        let (d50, d95, dmax) = quantiles(diff_ms);
        let (f50, f95, _) = quantiles(full_ms);
        println!(
            "MESURE écriture par différence, {} : p50 {d50:.1} ms, p95 {d95:.1}, pire {dmax:.1} ; \
             {bytes} octets ; {} blocs touchés ; diff_cost {estimate:.1} ms ; rewrite_blocks p50 \
             {f50:.1} ms, p95 {f95:.1}, {full_bytes} octets",
            c.what,
            written.touched.len()
        );
    }
}

// ------------------------------------------------------------------ octets du vrai Yjs

// Les frappes concurrentes viennent du vrai Yjs (celui du client) : chaque scène part d'une page
// écrite par le serveur (`base`), que l'écriture par différence récrit (`target`) ; une frappe
// (« Z ») est faite par un onglet qui n'a que la page de départ, à une place nommée. Deux passes
// écrivent les octets :
//
// 1. `EXPORT_FIXTURES=1 cargo test -p mementum-crdt --test diff the_scenes_are_written_on_demand`
//    écrit `tests/fixtures/diff/scenes.json` : l'état de départ, l'écriture du serveur (sous un
//    identifiant de client tiré au hasard : ses octets changent à chaque passe, ne la réécrire que
//    si une scène change), et la place de chaque frappe ;
// 2. `EXPORT_FIXTURES=1 corepack pnpm -C apps/web exec vitest run src/editor/diffWrites.test.ts`
//    écrit `tests/fixtures/diff/keys.json` : les octets de chaque frappe, et, pour chacun des deux
//    onglets qui reçoivent ensuite ce qui leur manque (celui qui a tapé reçoit l'écriture ; un autre
//    a l'écriture et reçoit la frappe), le nettoyage des marques que Yjs écrit de lui-même
//    (`cleanupYTextAfterTransaction`, une transaction locale qu'un navigateur renvoie au serveur)
//    et ce que l'onglet lit.
//
// Sans la variable, chaque passe compare ses octets à ceux du dépôt. Les verdicts « gardée » et
// « perdue » se jugent sur une écriture par différence faite à neuf par ce code (pas sur celle du
// fichier) ; l'accord de Yjs et de yrs, sur l'écriture du fichier, celle que Yjs a reçue.

/// Ce qu'une frappe doit devenir.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fate {
    Kept,
    Lost,
}

/// Une frappe : son nom, sa place (le chemin des blocs depuis la racine, le rang de l'enfant du
/// nœud de contenu, la place UTF-16 dans ce texte) et ce qu'elle doit devenir.
struct Key {
    name: &'static str,
    path: Vec<usize>,
    child: usize,
    at: u32,
    fate: Fate,
}

/// Comment la scène se joue.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// La frappe arrive au serveur après l'écriture : elle est concurrente.
    Concurrent,
    /// La frappe arrive au serveur avant l'écriture, que l'auteur du MMD a calculée sans l'avoir
    /// lue (cas 1 de R1227).
    TypedBefore,
}

struct Scene {
    name: &'static str,
    base: PageDoc,
    target: String,
    keys: Vec<Key>,
    flow: Flow,
}

const SECOND: &str = "Second paragraphe intact.";

/// Une scène de contenu en ligne : un paragraphe changé (`base` puis `target`), suivi de
/// [`SECOND`] ; le milieu que l'écriture remplace est à `edit` (enfant, place, longueur) ; les
/// frappes au début, à la fin, dans le paragraphe intact, juste avant et juste après le milieu
/// remplacé sont gardées, à leur place ; dans le milieu aussi, poussée au bord du texte écrit
/// (Ruling R1283).
fn inline_scene(name: &'static str, base: &str, target: &str, edit: (usize, u32, u32)) -> Scene {
    let page = page_with(&format!("{base}\n\n{SECOND}\n"));
    let (child, at, len) = edit;
    let (last, last_len) = last_child(&page, &[0]);
    let mut keys = vec![
        key("début", &[0], 0, 1, Fate::Kept),
        key("fin", &[0], last, last_len - 1, Fate::Kept),
        key("paragraphe intact", &[1], 0, 3, Fate::Kept),
        key("juste avant", &[0], child, at, Fate::Kept),
        key("juste après", &[0], child, at + len, Fate::Kept),
    ];
    if len >= 2 {
        keys.push(key("dans le milieu", &[0], child, at + 1, Fate::Kept));
    }
    Scene {
        name,
        base: page,
        target: format!("{target}\n\n{SECOND}\n"),
        keys,
        flow: Flow::Concurrent,
    }
}

fn key(name: &'static str, path: &[usize], child: usize, at: u32, fate: Fate) -> Key {
    Key {
        name,
        path: path.to_vec(),
        child,
        at,
        fate,
    }
}

/// Le rang du dernier enfant du nœud de contenu du bloc au bout de `path`, et sa longueur.
fn last_child(page: &PageDoc, path: &[usize]) -> (usize, u32) {
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let Some(XmlOut::Element(mut list)) = fragment.get(&txn, 0) else {
        panic!("la racine");
    };
    let mut container = None;
    for &at in path {
        if let Some(c) = container.take() {
            list = group_of(&txn, &c);
        }
        let Some(XmlOut::Element(c)) = list.get(&txn, at as u32) else {
            panic!("le bloc {path:?}");
        };
        container = Some(c);
    }
    let Some(XmlOut::Element(content)) = container.unwrap().get(&txn, 0) else {
        panic!("le contenu");
    };
    let last = content.len(&txn) - 1;
    let len = match content.get(&txn, last) {
        Some(XmlOut::Text(t)) => yrs::Text::len(&t, &txn),
        _ => panic!("le dernier enfant est un texte"),
    };
    (last as usize, len)
}

fn group_of<T: ReadTxn>(txn: &T, container: &XmlElementRef) -> XmlElementRef {
    container
        .children(txn)
        .find_map(|c| match c {
            XmlOut::Element(e) if e.tag().as_ref() == "blockGroup" => Some(e),
            _ => None,
        })
        .expect("un groupe d'enfants")
}

const MENTION_A: &str = "<mention type=\"page\" id=\"pg_a\">A</mention>";

/// Les scènes. Celles de contenu en ligne reprennent les neuf de la Task S0 (R1255), et une dont
/// le milieu remplacé est long.
fn scenes() -> Vec<Scene> {
    let mut out = vec![
        inline_scene(
            "gras",
            "Du **gras** au milieu.",
            "Du **gros** au milieu.",
            (0, 5, 1),
        ),
        inline_scene(
            "frontière d'une marque",
            "Du **gras** au milieu.",
            "Du **gras**e au milieu.",
            (0, 7, 0),
        ),
        inline_scene(
            "lien",
            "Un [lien](https://a.example) et la fin.",
            "Un [lieu](https://a.example) et la fin.",
            (0, 6, 1),
        ),
        inline_scene(
            "après une mention",
            &format!("Avant {MENTION_A} la fin."),
            &format!("Avant {MENTION_A} la finale."),
            (2, 7, 0),
        ),
        inline_scene(
            "marques seules",
            "Un mot ici.",
            "Un **mot** ici.",
            (0, 3, 3),
        ),
        inline_scene(
            "à cheval sur deux marques",
            "**gras** et *ital* fin.",
            "**gra** *tal* fin.",
            (0, 3, 6),
        ),
        inline_scene(
            "en tête",
            "Début du texte.",
            "Le début du texte.",
            (0, 0, 1),
        ),
        inline_scene(
            "lien posé",
            "Un mot ici.",
            "Un [mot](https://b.example) ici.",
            (0, 3, 3),
        ),
        inline_scene(
            "lien retiré",
            "Un [mot](https://b.example) ici.",
            "Un mot ici.",
            (0, 3, 3),
        ),
        inline_scene(
            "milieu long",
            "Un texte à remplacer ici.",
            "Un mot neuf ici.",
            (0, 3, 17),
        ),
    ];
    // Une mention dans le milieu changé : tout le contenu est récrit (R1255).
    let base = page_with(&format!("Avant le texte et la fin.\n\n{SECOND}\n"));
    out.push(Scene {
        name: "mention dans le milieu",
        base,
        target: format!("Avant le {MENTION_A} et la fin.\n\n{SECOND}\n"),
        keys: vec![
            key("début", &[0], 0, 1, Fate::Lost),
            key("fin", &[0], 0, 24, Fate::Lost),
            key("paragraphe intact", &[1], 0, 3, Fate::Kept),
        ],
        flow: Flow::Concurrent,
    });
    out.push(Scene {
        name: "déplacé sous un autre parent",
        base: page_with(&format!("- Parent\n- Mobile\n\n{SECOND}\n")),
        target: format!("- Parent\n  - Mobile\n\n{SECOND}\n"),
        keys: vec![
            key("dans le bloc déplacé", &[1], 0, 2, Fate::Lost),
            key("dans le nouveau parent", &[0], 0, 2, Fate::Kept),
            key("paragraphe intact", &[2], 0, 3, Fate::Kept),
        ],
        flow: Flow::Concurrent,
    });
    out.push(Scene {
        name: "déplacé parmi ses frères",
        base: page_with("Un\n\nDeux\n\nTrois\n\nQuatre\n"),
        target: "Un\n\nTrois\n\nQuatre\n\nDeux\n".to_string(),
        keys: vec![
            key("dans le bloc déplacé", &[1], 0, 2, Fate::Lost),
            key("dans un frère resté", &[2], 0, 2, Fate::Kept),
        ],
        flow: Flow::Concurrent,
    });
    let normalized = page_built(|txn, fragment| {
        let first = fragment.insert(txn, 0, XmlElementPrelim::empty("blockGroup"));
        push_paragraph(txn, &first, "a", "Premier bloc");
        push_paragraph(txn, &first, "b", "Second bloc");
        let second = fragment.insert(txn, 1, XmlElementPrelim::empty("blockGroup"));
        push_paragraph(txn, &second, "c", "Seconde racine");
    });
    out.push(Scene {
        name: "page que la lecture normalise",
        target: mmd_of(&normalized).replace("Premier bloc", "Premier bloc changé"),
        base: normalized,
        keys: vec![
            key("dans le bloc changé", &[0], 0, 2, Fate::Lost),
            key("dans un bloc intact", &[1], 0, 2, Fate::Lost),
        ],
        flow: Flow::Concurrent,
    });
    out.push(Scene {
        name: "tapé avant l'écriture",
        base: page_with(&format!("Un texte.\n\n{SECOND}\n")),
        target: format!("Un texte modifié.\n\n{SECOND}\n"),
        keys: vec![key("tapé avant, non lu", &[0], 0, 2, Fate::Lost)],
        flow: Flow::TypedBefore,
    });
    out
}

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diff")
}

fn read_json(name: &str) -> serde_json::Value {
    let path = fixtures().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} : {e} (voir l'en-tête de cette partie)", path.display()));
    serde_json::from_str(&text).unwrap()
}

fn bytes(value: &serde_json::Value) -> Vec<u8> {
    serde_json::from_value(value.clone()).expect("des octets")
}

/// Les scènes, et ce que les fichiers en portent : l'écriture du serveur que Yjs a reçue, et,
/// par frappe, ce que Vitest a écrit (`keys.json`).
struct Played {
    scene: Scene,
    write: Vec<u8>,
    keys: Vec<(Key, serde_json::Value)>,
}

fn played() -> Vec<Played> {
    let scenes_json = read_json("scenes.json");
    let keys_json = read_json("keys.json");
    scenes()
        .into_iter()
        .map(|mut scene| {
            let stored = &scenes_json[scene.name];
            assert!(
                stored.is_object(),
                "{} : absente de scenes.json",
                scene.name
            );
            // La page de départ du fichier est celle de la scène (lue pareil), et l'écriture du
            // fichier la mène au MMD voulu.
            let base = PageDoc::from_updates([bytes(&stored["base"]).as_slice()]).unwrap();
            assert_eq!(mmd_of(&base), mmd_of(&scene.base), "{}", scene.name);
            scene.base = base;
            let write = bytes(&stored["write"]);
            if scene.flow == Flow::Concurrent {
                let written =
                    PageDoc::from_updates([scene.base.encode_state().as_slice(), write.as_slice()])
                        .unwrap();
                assert_eq!(
                    mmd_of(&written),
                    scene.target,
                    "{} : l'écriture du fichier",
                    scene.name
                );
            }
            let keys = std::mem::take(&mut scene.keys)
                .into_iter()
                .map(|key| {
                    let from_vitest = keys_json[scene.name][key.name].clone();
                    assert!(
                        from_vitest.is_object(),
                        "{} / {} : absente de keys.json",
                        scene.name,
                        key.name
                    );
                    (key, from_vitest)
                })
                .collect();
            Played { scene, write, keys }
        })
        .collect()
}

/// Ce que la place d'une frappe dit à Vitest.
fn key_json(key: &Key) -> serde_json::Value {
    serde_json::json!({ "path": key.path, "child": key.child, "at": key.at })
}

/// Passe 1 : écrit `scenes.json` sous `EXPORT_FIXTURES=1` ; sinon, vérifie que le fichier porte
/// les scènes de ce code (même page de départ, même MMD écrit, mêmes places de frappe).
#[test]
fn the_scenes_are_written_on_demand() {
    let path = fixtures().join("scenes.json");
    if std::env::var_os("EXPORT_FIXTURES").is_some() {
        let mut out = serde_json::Map::new();
        for scene in scenes() {
            let base = scene.base.encode_state();
            let write = scene
                .base
                .write_diff(&parse(&scene.target).unwrap())
                .unwrap()
                .update;
            let keys: serde_json::Map<String, serde_json::Value> = scene
                .keys
                .iter()
                .map(|k| (k.name.to_string(), key_json(k)))
                .collect();
            out.insert(
                scene.name.to_string(),
                serde_json::json!({ "base": base, "write": write, "keys": keys }),
            );
        }
        std::fs::create_dir_all(fixtures()).unwrap();
        std::fs::write(&path, format!("{}\n", serde_json::Value::Object(out))).unwrap();
        return;
    }
    let stored = read_json("scenes.json");
    for scene in scenes() {
        for k in &scene.keys {
            assert_eq!(
                stored[scene.name]["keys"][k.name],
                key_json(k),
                "{} / {} : la place de la frappe a changé (EXPORT_FIXTURES=1 réécrit le fichier)",
                scene.name,
                k.name
            );
        }
    }
    // `played` vérifie la page de départ et l'écriture de chaque scène.
    assert_eq!(played().len(), scenes().len());
}

/// Les blocs, la frappe « Z » retirée de tous leurs textes, sans identifiants, en MMD ; et le
/// nombre de « Z » retirés. Un lien qui ne portait que la frappe part avec elle (avant le
/// nettoyage de Yjs, yrs lit sous le lien une frappe tapée juste après lui : scène « lien
/// retiré »).
fn without_z(blocks: &[Block]) -> (String, usize) {
    fn strip(content: &mut Vec<Inline>, count: &mut usize) {
        for inline in content.iter_mut() {
            match inline {
                Inline::Text { text, .. } => {
                    *count += text.matches('Z').count();
                    *text = text.replace('Z', "");
                }
                Inline::Link { children, .. } => strip(children, count),
                _ => {}
            }
        }
        content.retain(|inline| match inline {
            Inline::Text { text, .. } => !text.is_empty(),
            Inline::Link { children, .. } => !children.is_empty(),
            _ => true,
        });
    }
    fn walk(blocks: &mut [Block], count: &mut usize) {
        for b in blocks {
            match &mut b.content {
                Content::Inline(content) => strip(content, count),
                Content::Table(table) => {
                    for cell in table.rows.iter_mut().flatten() {
                        strip(&mut cell.content, count);
                    }
                }
                Content::None => {}
            }
            walk(&mut b.children, count);
        }
    }
    let mut blocks = blocks.to_vec();
    let mut count = 0;
    walk(&mut blocks, &mut count);
    (shown(&mementum_md::normalize(blocks)), count)
}

/// Joue la frappe sur une écriture faite à neuf : `Ok` si la frappe est gardée, une fois, et que
/// le reste de la page est ce que l'écriture a écrit ; sinon ce qui manque.
fn fate(played: &Played, key: &Key, keystroke: &[u8]) -> Result<(), String> {
    let scene = &played.scene;
    let page = PageDoc::from_updates([scene.base.encode_state().as_slice()]).unwrap();
    let target = parse(&scene.target).unwrap();
    let expected = match scene.flow {
        Flow::Concurrent => {
            page.write_diff(&target).unwrap();
            let expected = page.read_blocks();
            page.try_client_update(keystroke).unwrap_or_else(|r| {
                panic!(
                    "{} / {} : frappe refusée : {}",
                    scene.name, key.name, r.error
                )
            });
            expected
        }
        Flow::TypedBefore => {
            page.try_client_update(keystroke).unwrap_or_else(|r| {
                panic!(
                    "{} / {} : frappe refusée : {}",
                    scene.name, key.name, r.error
                )
            });
            page.write_diff(&target).unwrap();
            let replica = PageDoc::from_updates([scene.base.encode_state().as_slice()]).unwrap();
            replica.write_diff(&target).unwrap();
            replica.read_blocks()
        }
    };
    let (now, typed) = without_z(&page.read_blocks());
    let (wanted, _) = without_z(&expected);
    match (typed, now == wanted) {
        (1, true) => Ok(()),
        (0, true) => Err(format!("{} / {} : frappe perdue", scene.name, key.name)),
        (_, true) => Err(format!("{} / {} : frappe en double", scene.name, key.name)),
        (_, false) => Err(format!("{} / {} : écriture altérée", scene.name, key.name)),
    }
}

/// Ce qu'un document lit, de la même forme que `read` de `diffWrites.test.ts` : chaque élément
/// (sa balise, ses attributs sans `null` ni `undefined`, ses enfants), chaque texte en plages
/// (texte et marques sans `null`), les plages voisines de mêmes marques fondues.
fn reading(page: &PageDoc) -> serde_json::Value {
    fn any(value: &yrs::Any) -> Option<serde_json::Value> {
        use yrs::Any;
        Some(match value {
            Any::Null | Any::Undefined => return None,
            Any::Bool(b) => serde_json::json!(b),
            Any::Number(n) => match n {
                yrs::any::Number::Int(i) => serde_json::json!(i),
                yrs::any::Number::Float(f) => serde_json::json!(f),
            },
            Any::String(s) => serde_json::json!(s.as_ref()),
            Any::Array(a) => serde_json::Value::Array(
                a.iter()
                    .map(|v| any(v).unwrap_or(serde_json::Value::Null))
                    .collect(),
            ),
            Any::Map(m) => serde_json::Value::Object(
                m.iter()
                    .filter_map(|(k, v)| any(v).map(|v| (k.clone(), v)))
                    .collect(),
            ),
            other => serde_json::json!(format!("{other:?}")),
        })
    }
    fn nodes<T: ReadTxn>(txn: &T, children: impl Iterator<Item = XmlOut>) -> serde_json::Value {
        let mut out = Vec::new();
        for child in children {
            match child {
                XmlOut::Element(e) => {
                    let attrs: serde_json::Map<String, serde_json::Value> = e
                        .attributes(txn)
                        .filter_map(|(k, v)| match v {
                            yrs::Out::Any(a) => any(&a).map(|a| (k.to_string(), a)),
                            _ => None,
                        })
                        .collect();
                    out.push(serde_json::json!({
                        "el": e.tag().as_ref(),
                        "attrs": attrs,
                        "children": nodes(txn, e.children(txn)),
                    }));
                }
                XmlOut::Text(t) => {
                    let mut runs: Vec<(String, serde_json::Value)> = Vec::new();
                    for run in yrs::Text::diff(&t, txn, yrs::types::text::YChange::identity) {
                        let yrs::Out::Any(yrs::Any::String(s)) = &run.insert else {
                            continue;
                        };
                        if s.is_empty() {
                            continue;
                        }
                        let marks: serde_json::Map<String, serde_json::Value> = run
                            .attributes
                            .iter()
                            .flat_map(|a| a.iter())
                            .filter_map(|(k, v)| any(v).map(|v| (k.to_string(), v)))
                            .collect();
                        let marks = serde_json::Value::Object(marks);
                        match runs.last_mut() {
                            Some((text, m)) if *m == marks => text.push_str(s.as_ref()),
                            _ => runs.push((s.to_string(), marks)),
                        }
                    }
                    out.push(serde_json::json!({ "text": runs }));
                }
                XmlOut::Fragment(_) => {}
            }
        }
        serde_json::Value::Array(out)
    }
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    nodes(&txn, fragment.children(&txn))
}

/// La page du serveur, à qui l'onglet `tab` (« typist » : celui qui a tapé ; « observer » : un
/// autre) a renvoyé ce qu'il écrit : l'écriture du fichier, la frappe, puis le nettoyage de cet
/// onglet, chacun par la garde d'une mise à jour de client ; et ce que cet onglet lit.
fn server_after(
    played: &Played,
    from_vitest: &serde_json::Value,
    tab: &str,
) -> (PageDoc, serde_json::Value) {
    let what = format!("{} / {tab}", played.scene.name);
    let page = PageDoc::from_updates([
        played.scene.base.encode_state().as_slice(),
        played.write.as_slice(),
    ])
    .unwrap();
    page.try_client_update(&bytes(&from_vitest["update"]))
        .unwrap_or_else(|r| panic!("{what} : frappe refusée : {}", r.error));
    if let Some(cleanup) = from_vitest[tab]["cleanup"].as_array() {
        let cleanup: Vec<u8> = cleanup.iter().map(|b| b.as_u64().unwrap() as u8).collect();
        page.try_client_update(&cleanup)
            .unwrap_or_else(|r| panic!("{what} : nettoyage refusé : {}", r.error));
    }
    (page, from_vitest[tab]["read"].clone())
}

/// Pour chaque scène, chaque frappe et chaque onglet : le serveur, qui a reçu l'écriture, la
/// frappe et le nettoyage de l'onglet, lit ce que lit l'onglet (le vrai Yjs).
fn assert_yjs_reads_as_yrs(played: &Played, key: &Key, from_vitest: &serde_json::Value) {
    if played.scene.flow != Flow::Concurrent {
        return;
    }
    for tab in ["typist", "observer"] {
        let (page, yjs) = server_after(played, from_vitest, tab);
        assert_eq!(
            reading(&page),
            yjs,
            "{} / {} / {tab} : Yjs ne lit pas comme yrs",
            played.scene.name,
            key.name
        );
    }
}

/// Le sort de chaque frappe d'un sort donné, et l'accord de Yjs et de yrs sur chacune.
fn fates(wanted: Fate, only: impl Fn(&Scene, &Key) -> bool) -> (usize, Vec<String>) {
    let mut seen = 0;
    let mut unmet = Vec::new();
    for played in played() {
        for (key, from_vitest) in &played.keys {
            if key.fate != wanted || !only(&played.scene, key) {
                continue;
            }
            seen += 1;
            assert_yjs_reads_as_yrs(&played, key, from_vitest);
            if let Err(why) = fate(&played, key, &bytes(&from_vitest["update"])) {
                unmet.push(why);
            }
        }
    }
    (seen, unmet)
}

/// Une frappe concurrente dans un bloc que l'écriture ne touche pas est gardée, quoi que
/// l'écriture fasse ailleurs (un texte remplacé, un bloc déplacé, un bloc qui change de parent).
#[test]
fn typing_in_an_untouched_block_survives_a_diff_write() {
    let (seen, unmet) = fates(Fate::Kept, |_, key| {
        matches!(
            key.name,
            "paragraphe intact" | "dans un frère resté" | "dans le nouveau parent"
        )
    });
    assert!(seen >= 13, "{seen} frappes");
    assert!(unmet.is_empty(), "{unmet:#?}");
}

/// Une frappe concurrente dans un bloc changé, hors du texte que l'écriture y remplace, est
/// gardée (R1255) : au début, à la fin, après une mention, contre les deux bornes du milieu
/// remplacé, dans les deux ordres (l'onglet qui a tapé reçoit l'écriture ; un autre, qui a
/// l'écriture, reçoit la frappe).
#[test]
fn typing_outside_the_replaced_text_of_a_changed_block_survives() {
    let (seen, unmet) = fates(Fate::Kept, |_, key| {
        matches!(key.name, "début" | "fin" | "juste avant" | "juste après")
    });
    assert_eq!(seen, 40, "dix scènes, quatre frappes chacune");
    assert!(unmet.is_empty(), "{unmet:#?}");
}

/// Une frappe concurrente dans le texte que l'écriture remplace par préfixe et suffixe est
/// gardée : Yjs ne supprime pas ce qu'un autre a inséré entre deux unités qu'il supprime ; elle
/// paraît au bord du texte écrit (Ruling R1283). Seul un contenu récrit en entier la perd
/// (`known_limits`).
#[test]
fn typing_in_the_replaced_text_survives_beside_the_new_text() {
    let (seen, unmet) = fates(Fate::Kept, |_, key| key.name == "dans le milieu");
    assert_eq!(seen, 5);
    assert!(unmet.is_empty(), "{unmet:#?}");
}

/// Un lien retiré, une frappe juste après : Yjs nettoie de lui-même les marques devenues inutiles
/// (une transaction locale que l'onglet renvoie) ; le serveur reçoit ce nettoyage par la garde
/// d'une mise à jour de client, après la frappe, et lit alors ce que lit l'onglet (R1255).
#[test]
fn a_marks_change_next_to_a_keystroke_converges_once_yjs_cleanup_is_received() {
    let played = played()
        .into_iter()
        .find(|p| p.scene.name == "lien retiré")
        .unwrap();
    let (key, from_vitest) = played
        .keys
        .iter()
        .find(|(k, _)| k.name == "juste après")
        .unwrap();
    let cleaned: Vec<&str> = ["typist", "observer"]
        .into_iter()
        .filter(|tab| from_vitest[*tab]["cleanup"].is_array())
        .collect();
    assert!(!cleaned.is_empty(), "aucun onglet n'a nettoyé de marques");
    for tab in cleaned {
        let (page, yjs) = server_after(&played, from_vitest, tab);
        assert_eq!(reading(&page), yjs, "{tab} : nettoyage reçu");
        let (_, typed) = without_z(&page.read_blocks());
        assert_eq!(typed, 1, "{tab} : la frappe est gardée");
    }
    assert_eq!(fate(&played, key, &bytes(&from_vitest["update"])), Ok(()));
}

/// L'affirmation d'une limite connue, partagée avec les tests de `mementum-sync` (déclarée au
/// niveau du fichier : voir `reload.rs`).
#[path = "../../mementum-sync/tests/common/limit.rs"]
mod limit;

/// Les limites connues de l'écriture par différence (limite LA-a du plan du lot 5, R1227), mot
/// pour mot : chaque test rougit le jour où la perte est levée.
mod known_limits {
    use super::limit::known_limit;
    use super::*;

    /// Une frappe concurrente dans un contenu récrit en entier (ici, une mention dans le milieu
    /// changé) est perdue : son texte est supprimé avec elle, où qu'elle soit dans le bloc.
    #[test]
    fn typing_in_a_content_rewritten_whole_is_lost() {
        let (seen, unmet) = fates(Fate::Lost, |scene, _| {
            scene.name == "mention dans le milieu"
        });
        assert_eq!(seen, 2);
        known_limit(
            "LA-a : un contenu récrit en entier (un élément en ligne dans le milieu changé, un \
             tableau, un type changé) supprime ses textes, et une frappe faite dedans, pas encore \
             reçue, part avec eux",
            &unmet,
            &[
                "mention dans le milieu / début : frappe perdue".to_string(),
                "mention dans le milieu / fin : frappe perdue".to_string(),
            ],
        );
    }

    /// Une frappe concurrente dans un bloc qui change de parent est perdue : `keep_ids` ne le
    /// suit pas, il est retiré puis écrit à neuf.
    #[test]
    fn typing_in_a_block_moved_under_another_parent_is_lost() {
        let (_, unmet) = fates(Fate::Lost, |scene, _| {
            scene.name == "déplacé sous un autre parent"
        });
        known_limit(
            "LA-a, cas 3 de R1227 : un bloc qui change de parent est retiré et écrit à neuf",
            &unmet,
            &["déplacé sous un autre parent / dans le bloc déplacé : frappe perdue".to_string()],
        );
    }

    /// Une frappe concurrente dans un bloc recréé parce qu'il change de place parmi ses frères est
    /// perdue : Yjs n'a pas de déplacement, et le report de la Task 13b ne vaut que pour une mise
    /// à jour de client (R222). Pour deux blocs échangés, lequel est recréé est arbitraire.
    #[test]
    fn typing_in_a_block_moved_among_its_siblings_is_lost() {
        let (_, unmet) = fates(Fate::Lost, |scene, _| {
            scene.name == "déplacé parmi ses frères"
        });
        known_limit(
            "LA-a : un bloc déplacé parmi ses frères est supprimé et recréé au même identifiant",
            &unmet,
            &["déplacé parmi ses frères / dans le bloc déplacé : frappe perdue".to_string()],
        );
    }

    /// Ce qu'un humain a tapé avant l'écriture, que l'auteur du MMD n'avait pas lu, est ramené au
    /// texte écrit (cas 1 de R1227 : seule la précondition de R1231 l'empêche).
    #[test]
    fn typing_typed_before_the_write_and_unread_by_its_author_is_overwritten() {
        let (_, unmet) = fates(Fate::Lost, |scene, _| scene.name == "tapé avant l'écriture");
        known_limit(
            "LA-a, cas 1 de R1227 : la différence se calcule contre la page courante",
            &unmet,
            &["tapé avant l'écriture / tapé avant, non lu : frappe perdue".to_string()],
        );
    }

    /// Sur une page que `read_blocks` normalise, l'écriture retombe sur la récriture complète, et
    /// toute frappe concurrente est perdue, même dans un bloc que l'écriture ne change pas.
    #[test]
    fn every_concurrent_keystroke_is_lost_on_a_page_that_falls_back() {
        let (_, unmet) = fates(Fate::Lost, |scene, _| {
            scene.name == "page que la lecture normalise"
        });
        known_limit(
            "LA-a, cas 4 de R1227 : la récriture complète remplace tout le contenu",
            &unmet,
            &[
                "page que la lecture normalise / dans le bloc changé : frappe perdue".to_string(),
                "page que la lecture normalise / dans un bloc intact : frappe perdue".to_string(),
            ],
        );
    }
}
