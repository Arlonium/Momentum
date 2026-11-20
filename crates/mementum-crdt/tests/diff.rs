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
/// un sur deux, par leur identifiant, ne la parcourent pas : quatre fois plus de blocs, au plus
/// cinq fois plus (Ruling R1281) ; des paires voisines échangées recréent un bloc sur deux, et
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
    assert!(large <= 5 * small, "retirés : {small} puis {large} ns");

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
