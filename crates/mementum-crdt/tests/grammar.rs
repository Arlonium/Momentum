//! La grammaire d'une mise à jour (garde des mises à jour, tour 4 ; Rulings R801 à R808), règle
//! par règle. Les cas trouvés par l'essai au hasard sont dans `hostile_at_random.rs` ; ce que le
//! vrai Yjs écrit, dans `yjs_updates.rs`.

use mementum_crdt::{CrdtError, FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::{Doc, GetString, OffsetKind, Options, ReadTxn, StateVector, Transact, Update};

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

fn string(out: &mut Vec<u8>, s: &str) {
    var(out, s.len() as u64);
    out.extend(s.as_bytes());
}

/// Les horloges du client du serveur dans une page neuve : le bloc, son paragraphe, et la
/// couleur de fond du paragraphe.
const CONTAINER: u64 = 1;
const PARAGRAPH: u64 = 3;
const BACKGROUND: u64 = 4;

struct Served {
    page: PageDoc,
    journal: Vec<Vec<u8>>,
    server: u64,
}

impl Served {
    fn new() -> Served {
        let (page, created) = PageDoc::new_page();
        let state = StateVector::decode_v1(&page.state_vector()).unwrap();
        let server = state.iter().next().unwrap().0.get();
        Served {
            page,
            journal: vec![created],
            server,
        }
    }

    fn receive(&mut self, update: &[u8]) -> Result<Vec<u8>, CrdtError> {
        match self.page.try_client_update(update) {
            Ok(applied) => {
                self.journal.push(applied.read.clone());
                Ok(applied.read)
            }
            Err(refused) => {
                if !refused.intact {
                    self.page = self.reloaded();
                }
                Err(refused.error)
            }
        }
    }

    fn reloaded(&self) -> PageDoc {
        PageDoc::from_updates(self.journal.iter().map(Vec::as_slice)).expect("le journal se relit")
    }

    fn background(&self) -> Option<String> {
        self.page.read_blocks()[0]
            .props
            .get("backgroundColor")
            .map(|value| format!("{value:?}"))
    }

    /// Une entrée qui récrit la couleur de fond du paragraphe, comme Yjs l'écrit : d'origine
    /// l'entrée courante, avec la marque d'une entrée de map ; `values` valeurs.
    fn colors(&self, client: u64, values: &[&str]) -> Vec<u8> {
        let mut u = vec![1, 1];
        var(&mut u, client);
        u.extend_from_slice(&[0, 0xA8]);
        var(&mut u, self.server);
        var(&mut u, BACKGROUND);
        var(&mut u, values.len() as u64);
        for value in values {
            u.push(119);
            string(&mut u, value);
        }
        u.push(0);
        u
    }

    fn deleted(client: u64, clock: u64, len: u64) -> Vec<u8> {
        let mut u = vec![0, 1];
        var(&mut u, client);
        u.push(1);
        var(&mut u, clock);
        var(&mut u, len);
        u
    }

    /// L'état se relit par un yrs neuf, plusieurs fois, et le journal relu donne la page.
    fn assert_sound(&self, what: &str) {
        for _ in 0..20 {
            let doc = Doc::with_options(Options {
                offset_kind: OffsetKind::Utf16,
                ..Options::default()
            });
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            let update = Update::decode_v1(&self.page.encode_state())
                .unwrap_or_else(|e| panic!("{what} : état illisible : {e:?}"));
            doc.transact_mut()
                .apply_update(update)
                .unwrap_or_else(|e| panic!("{what} : état refusé : {e:?}"));
            let txn = doc.transact();
            assert!(
                txn.store().pending_update().is_none(),
                "{what} : des éléments restent en attente"
            );
            let _ = fragment.get_string(&txn);
        }
        let reloaded = self.reloaded();
        assert_eq!(reloaded.read_blocks(), self.page.read_blocks(), "{what}");
        assert_eq!(reloaded.revision(), self.page.revision(), "{what}");
    }
}

fn is_refused_with(result: &Result<Vec<u8>, CrdtError>, part: &str) -> bool {
    matches!(result, Err(CrdtError::Structure(why)) if why.contains(part))
}

/// N1 (Ruling R801), le constat C1 de la re-relecture 5 : une entrée de map de plusieurs
/// valeurs, vivante (Yjs en écrit après une annulation : `redoItem` recopie une entrée fondue).
/// Elle n'est pas refusée ; la page la récrit en entrées d'une valeur, chacune d'origine la
/// précédente, et lit la dernière. Une valeur du milieu supprimée, puis le bloc : l'état se
/// relit (yrs y lisait de la mémoire libérée).
#[test]
fn a_many_valued_entry_is_rewritten_into_entries_of_one_value() {
    let mut served = Served::new();
    let relayed = served
        .receive(&served.colors(7_000, &["red", "blue", "green"]))
        .expect("ce que Yjs écrit après une annulation passe");
    assert!(served.background().is_some_and(|c| c.contains("green")));
    // Ce qui est relayé et écrit au journal : trois éléments, plus aucun de plusieurs valeurs,
    // et la suppression des deux premières valeurs, que les autres clients appliquent.
    let mut three = vec![1, 3];
    var(&mut three, 7_000);
    assert!(
        relayed.starts_with(&three),
        "trois éléments attendus : {relayed:?}"
    );
    served.assert_sound("trois valeurs");
    let middle = served.receive(&Served::deleted(7_000, 1, 1));
    assert!(middle.is_ok(), "{middle:?}");
    served.assert_sound("la valeur du milieu supprimée");
    let block = served.receive(&Served::deleted(served.server, CONTAINER, 1));
    assert!(block.is_ok(), "{block:?}");
    served.assert_sound("le bloc supprimé");
}

/// E1 et E3 (Ruling R806) : sous une clé, un contenu qui n'est pas une valeur est refusé avant
/// d'être appliqué, quand ses octets le disent entrée de map ; une chaîne de plusieurs unités
/// s'y couperait comme une valeur.
///
/// La chaîne d'une seule unité, attribut d'un texte posé par la même mise à jour, n'est refusée
/// que par E1 (constat M-6.5 de la re-relecture 6) : ni E3 (une unité), ni la garde des attributs
/// (un texte n'est pas jugé), ni rien d'après l'application (son parent est nommé).
#[test]
fn a_keyed_content_that_is_not_a_value_is_refused_before_it_is_applied() {
    let served = Served::new();
    let before = served.page.encode_state();
    // Sur le paragraphe, une chaîne de trois unités.
    let mut on_paragraph = vec![1, 1];
    var(&mut on_paragraph, 7_000);
    on_paragraph.extend_from_slice(&[0, 0x24, 0]);
    var(&mut on_paragraph, served.server);
    var(&mut on_paragraph, PARAGRAPH);
    string(&mut on_paragraph, "backgroundColor");
    string(&mut on_paragraph, "red");
    on_paragraph.push(0);
    // Un texte dans le paragraphe, puis, comme attribut du texte, une chaîne d'une unité.
    let mut on_text = vec![1, 2];
    var(&mut on_text, 7_000);
    on_text.extend_from_slice(&[0, 7, 0]);
    var(&mut on_text, served.server);
    var(&mut on_text, PARAGRAPH);
    on_text.extend_from_slice(&[6, 0x24, 0]);
    var(&mut on_text, 7_000);
    on_text.push(0);
    string(&mut on_text, "k");
    string(&mut on_text, "r");
    on_text.push(0);
    for (what, u) in [("le texte", on_text), ("le paragraphe", on_paragraph)] {
        let refused = served.page.try_client_update(&u).expect_err(what);
        assert!(refused.intact, "{what} : {refused:?}");
        assert!(
            refused
                .error
                .to_string()
                .contains("contenu qui n'en est pas une"),
            "{what} : {refused:?}"
        );
        assert!(served.page.encode_state() == before, "{what}");
    }
}

/// E4 (Ruling R803) : une valeur `Any` qui ne porte pas la marque d'une entrée de map, placée
/// par l'origine d'une entrée. yrs en ferait une entrée, de la forme que l'auteur veut. Refusée
/// avant d'être appliquée.
#[test]
fn a_value_that_does_not_say_it_is_a_map_entry_is_refused() {
    let served = Served::new();
    let before = served.page.encode_state();
    let mut u = vec![1, 1];
    var(&mut u, 7_000);
    u.extend_from_slice(&[0, 0x88]);
    var(&mut u, served.server);
    var(&mut u, BACKGROUND);
    u.extend_from_slice(&[1, 119, 3, b'r', b'e', b'd', 0]);
    let refused = served.page.try_client_update(&u).expect_err("accepté");
    assert!(refused.intact, "{refused:?}");
    assert!(
        refused
            .error
            .to_string()
            .contains("ne se dit pas entrée de map"),
        "{refused:?}"
    );
    assert!(served.page.encode_state() == before);
}

/// P1 (Ruling R803) : un contenu supprimé sans la marque d'une entrée de map, placé par
/// l'origine d'une entrée : yrs en fait une entrée de map, ses octets disent un enfant de
/// liste. Refusé d'après ce que yrs en a fait ; la page rechargée est celle d'avant.
#[test]
fn an_item_filed_otherwise_than_its_bytes_say_is_refused() {
    let mut served = Served::new();
    let blocks = served.page.read_blocks();
    let mut u = vec![1, 1];
    var(&mut u, 7_000);
    u.extend_from_slice(&[0, 0x81]);
    var(&mut u, served.server);
    var(&mut u, BACKGROUND);
    u.extend_from_slice(&[1, 0]);
    let result = served.receive(&u);
    assert!(
        is_refused_with(&result, "autrement que ses octets ne le disent"),
        "{result:?}"
    );
    assert_eq!(served.page.read_blocks(), blocks);
    // Avec la marque, comme Yjs l'écrit : c'est retirer l'attribut.
    let info = u.iter().position(|byte| *byte == 0x81).unwrap();
    u[info] = 0xA1;
    let result = served.receive(&u);
    assert!(result.is_ok(), "{result:?}");
    served.assert_sound("un contenu supprimé posé sur la couleur");
}

/// Ruling R808 : une horloge au-delà de 2^31. yrs compare les horloges en `i32` : un bloc qui
/// laisse un trou de deux milliards d'unités passait pour contigu, sans le `Skip` qui dit le
/// trou, et une origine dans le trou le faisait paniquer. Refusée avant d'être lue par yrs, d'un
/// client comme au journal, où l'entrée est écartée.
#[test]
fn a_clock_beyond_two_to_the_31_is_refused() {
    let served = Served::new();
    let before = served.page.encode_state();
    let mut u = vec![1, 1];
    var(&mut u, 7_000);
    var(&mut u, (1 << 31) - 1);
    u.extend_from_slice(&[0x01, 0]);
    var(&mut u, served.server);
    var(&mut u, 0);
    u.extend_from_slice(&[1, 0]);
    let refused = served.page.try_client_update(&u).expect_err("accepté");
    assert!(refused.intact, "{refused:?}");
    let stored = served.page.apply_update(&u).expect_err("relue");
    assert!(PageDoc::refused_intact(&stored), "{stored:?}");
    assert!(served.page.encode_state() == before);
}

/// Ruling R807 : dans une page qui garde un trou (un journal d'avant R131), une entrée qui
/// dépend d'une horloge du trou est écartée avant d'être appliquée.
#[test]
fn a_stored_entry_that_depends_on_a_hole_is_set_aside() {
    let served = Served::new();
    // Le client 9 000 : un trou de cinq unités, puis un contenu supprimé dans le groupe.
    let mut holed = vec![1, 2];
    var(&mut holed, 9_000);
    holed.extend_from_slice(&[0, 10, 5, 0x01, 0]);
    var(&mut holed, served.server);
    holed.extend_from_slice(&[0, 1, 0]);
    served.page.apply_update(&holed).expect("un trou se relit");
    let before = served.page.encode_state();
    // Un élément dont l'origine est dans le trou.
    let mut dependent = vec![1, 1];
    var(&mut dependent, 9_001);
    dependent.extend_from_slice(&[0, 0x81]);
    var(&mut dependent, 9_000);
    dependent.extend_from_slice(&[2, 1, 0]);
    let error = served.page.apply_update(&dependent).expect_err("relue");
    assert!(PageDoc::refused_intact(&error), "{error:?}");
    assert!(served.page.encode_state() == before);
}

/// Un client (yrs) de la page, à l'identifiant donné.
fn client(served: &Served, id: u64) -> Doc {
    let doc = Doc::with_options(Options {
        client_id: yrs::ClientID::new(id),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(&served.page.encode_state()).unwrap())
        .unwrap();
    doc
}

/// Le paragraphe de la page et son texte, s'il en a un. `fragment` est pris avant d'ouvrir `txn`
/// (piège du `CLAUDE.md` : `get_or_insert_*` attend le verrou de la transaction ouverte).
fn paragraph(
    fragment: &yrs::XmlFragmentRef,
    txn: &impl ReadTxn,
) -> Option<(yrs::XmlElementRef, Option<yrs::XmlTextRef>)> {
    use yrs::{XmlFragment, XmlOut};
    let Some(XmlOut::Element(group)) = fragment.get(txn, 0) else {
        return None;
    };
    let Some(XmlOut::Element(container)) = group.get(txn, 0) else {
        return None;
    };
    let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
        return None;
    };
    let text = match paragraph.get(txn, 0) {
        Some(XmlOut::Text(text)) => Some(text),
        _ => None,
    };
    Some((paragraph, text))
}

/// Le texte du paragraphe de la page, tel que son document le porte (sans le réencoder).
fn paragraph_text(doc: &Doc) -> String {
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    paragraph(&fragment, &txn)
        .and_then(|(_, text)| text)
        .map(|text| text.get_string(&txn))
        .unwrap_or_default()
}

/// Le client tape `what` à la place `at` du texte du paragraphe (qu'il crée s'il n'y en a pas).
fn typed(doc: &Doc, at: u32, what: &str) -> Vec<u8> {
    use yrs::{Text, XmlFragment, XmlTextPrelim};
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let found = {
        let txn = doc.transact();
        paragraph(&fragment, &txn).expect("un paragraphe")
    };
    let mut txn = doc.transact_mut();
    let text = match found {
        (_, Some(text)) => text,
        (paragraph, None) => paragraph.insert(&mut txn, 0, XmlTextPrelim::new("")),
    };
    text.insert(&mut txn, at, what);
    txn.encode_update_v1()
}

/// L'octet d'en-tête du premier élément d'une mise à jour (v1) d'un seul client.
fn first_info(update: &[u8]) -> u8 {
    let mut at = 2;
    for _ in 0..2 {
        while update[at] & 0x80 != 0 {
            at += 1;
        }
        at += 1;
    }
    update[at]
}

/// Un élément de `client`, à `clock`, d'origine `origin` et d'origine droite `right`, qui porte
/// la chaîne `what` (sans jeu de suppressions).
fn string_between(
    client: u64,
    clock: u64,
    origin: (u64, u64),
    right: (u64, u64),
    what: &str,
) -> Vec<u8> {
    let mut u = vec![1, 1];
    var(&mut u, client);
    var(&mut u, clock);
    u.push(0xC4);
    var(&mut u, origin.0);
    var(&mut u, origin.1);
    var(&mut u, right.0);
    var(&mut u, right.1);
    string(&mut u, what);
    u.push(0);
    u
}

/// Règle N3 (Ruling R825), le constat I-7.1 de la re-relecture 7 : un élément d'origine « c » et
/// d'origine droite « b », dans « abc » écrit d'un tenant (un seul bloc de yrs). yrs comme Yjs
/// prennent le bloc « abc » pour voisin de gauche, puis le coupent pour trouver « b » : le voisin
/// de gauche reste « a », et l'élément se range entre « a » et « b » chez qui a « abc » d'un bloc,
/// après « c » chez qui l'a en morceaux (la page relue de son état). La page le récrit sans son
/// origine droite : il est rangé après « c », par son origine seule, partout pareil.
#[test]
fn a_right_origin_left_of_the_origin_in_one_block_is_removed() {
    let mut served = Served::new();
    let writer = client(&served, 10);
    served
        .receive(&typed(&writer, 0, "abc"))
        .expect("la frappe de départ");
    // Le texte est l'élément 0 du client 10, « abc » ses horloges 1 à 3.
    let relayed = served
        .receive(&string_between(20, 0, (10, 3), (10, 2), "zy"))
        .expect("accepté, récrit");
    assert_eq!(
        first_info(&relayed) & 0x40,
        0,
        "l'origine droite reste : {relayed:?}"
    );
    assert_eq!(paragraph_text(served.page.yrs_doc()), "abczy");
    let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
    assert_eq!(paragraph_text(again.yrs_doc()), "abczy");
    served.assert_sound("l'élément récrit");
    // La même chose quand « abc » et l'élément arrivent dans une même mise à jour.
    let mut served = Served::new();
    let mut both = vec![2];
    // Le client 30 : le texte, puis « abc », dans le paragraphe du serveur.
    both.extend_from_slice(&[2, 30, 0, 7, 0]);
    var(&mut both, served.server);
    var(&mut both, PARAGRAPH);
    both.extend_from_slice(&[6, 4, 0, 30, 0, 3, b'a', b'b', b'c']);
    // Le client 31 : « zy », d'origine « c » et d'origine droite « b ».
    both.extend_from_slice(&[1, 31, 0, 0xC4, 30, 3, 30, 2, 2, b'z', b'y', 0]);
    served.receive(&both).expect("accepté, récrit");
    assert_eq!(paragraph_text(served.page.yrs_doc()), "abczy");
    let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
    assert_eq!(paragraph_text(again.yrs_doc()), "abczy");
    served.assert_sound("dans la même mise à jour");
}

/// Règle N3 (Ruling R825) : ce qu'un client honnête écrit n'est pas récrit. L'onglet 10 tape
/// « xb » d'un tenant, puis « a » entre « x » et « b » ; l'onglet 20 tape « Z » entre « a » et
/// « b » : son origine droite (« b », horloge 2) est du client de son origine (« a », horloge 3),
/// à une horloge plus basse, mais « b » est à droite de « a » ; les unités 1 à 3 ne tiennent pas en
/// un bloc (« a » n'a pas « b » pour origine). Et un élément dont l'origine droite commence une
/// suite (rien ne tient avec l'unité d'avant elle) est rangé pareil partout : il garde la sienne.
#[test]
fn an_honest_right_origin_of_the_client_of_the_origin_is_kept() {
    let mut served = Served::new();
    let writer = client(&served, 10);
    served.receive(&typed(&writer, 0, "xb")).unwrap();
    served.receive(&typed(&writer, 1, "a")).unwrap();
    let other = client(&served, 20);
    let update = typed(&other, 2, "Z");
    assert_ne!(first_info(&update) & 0x40, 0, "yrs écrit l'origine droite");
    let relayed = served.receive(&update).expect("honnête");
    assert_ne!(
        first_info(&relayed) & 0x40,
        0,
        "l'origine droite est retirée"
    );
    assert_eq!(paragraph_text(served.page.yrs_doc()), "xaZb");
    let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
    assert_eq!(paragraph_text(again.yrs_doc()), "xaZb");
    served.assert_sound("honnête");
    // « a » seul, puis « bc » écrit devant lui (son origine droite est « a ») : « bca ». Un
    // élément d'origine « c » et d'origine droite « b » : « b » commence une suite, il est rangé
    // après « c » partout, et garde son origine droite.
    let mut served = Served::new();
    let writer = client(&served, 10);
    served.receive(&typed(&writer, 0, "a")).unwrap();
    served.receive(&typed(&writer, 0, "bc")).unwrap();
    assert_eq!(paragraph_text(served.page.yrs_doc()), "bca");
    let relayed = served
        .receive(&string_between(20, 0, (10, 3), (10, 2), "zy"))
        .expect("accepté");
    assert_ne!(
        first_info(&relayed) & 0x40,
        0,
        "l'origine droite est retirée"
    );
    assert_eq!(paragraph_text(served.page.yrs_doc()), "bczya");
    let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
    assert_eq!(paragraph_text(again.yrs_doc()), "bczya");
    served.assert_sound("une suite qui commence à l'origine droite");
}

/// Règle N3 (Ruling R825), au plus juste : deux unités ne tiennent en un bloc que si la seconde a
/// pour origine la première **et** la même origine droite (`try_squash` de yrs, `Item.mergeWith`
/// de Yjs). Un élément dont l'origine droite ne peut pas tenir en un bloc avec l'unité d'avant
/// elle est rangé pareil partout : il garde son origine droite.
#[test]
fn a_right_origin_that_cannot_share_a_block_with_the_unit_before_it_is_kept() {
    // L'onglet 10 tape « a », l'onglet 20 « z » après lui, puis l'onglet 10 « bc » entre « a » et
    // « z » : « bc » a pour origine « a », mais pour origine droite « z », et « a » n'en a pas.
    let mut served = Served::new();
    let writer = client(&served, 10);
    served.receive(&typed(&writer, 0, "a")).unwrap();
    let other = client(&served, 20);
    served.receive(&typed(&other, 1, "z")).unwrap();
    let writer = client(&served, 10);
    served.receive(&typed(&writer, 1, "bc")).unwrap();
    assert_eq!(paragraph_text(served.page.yrs_doc()), "abcz");
    // L'élément 0 du client 10 est le texte : « a » à 1, « bc » à 2 et 3. Un élément d'origine
    // « c » et d'origine droite « b ».
    let relayed = served
        .receive(&string_between(30, 0, (10, 3), (10, 2), "y"))
        .expect("accepté");
    assert_ne!(
        first_info(&relayed) & 0x40,
        0,
        "l'origine droite est retirée"
    );
    assert_eq!(paragraph_text(served.page.yrs_doc()), "abcyz");
    let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
    assert_eq!(paragraph_text(again.yrs_doc()), "abcyz");
    served.assert_sound("une autre origine droite");
    // L'onglet 10 tape « a », l'onglet 20 « z » après lui, puis l'onglet 10 « bc » après « z » :
    // « bc » n'a pas « a » pour origine, bien qu'ils aient la même origine droite (aucune). Un
    // élément d'origine « c » et d'origine droite « b ».
    let mut served = Served::new();
    let writer = client(&served, 10);
    served.receive(&typed(&writer, 0, "a")).unwrap();
    let other = client(&served, 20);
    served.receive(&typed(&other, 1, "z")).unwrap();
    let writer = client(&served, 10);
    served.receive(&typed(&writer, 2, "bc")).unwrap();
    assert_eq!(paragraph_text(served.page.yrs_doc()), "azbc");
    let relayed = served
        .receive(&string_between(30, 0, (10, 3), (10, 2), "y"))
        .expect("accepté");
    assert_ne!(
        first_info(&relayed) & 0x40,
        0,
        "l'origine droite est retirée"
    );
    assert_eq!(paragraph_text(served.page.yrs_doc()), "azbcy");
    let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
    assert_eq!(paragraph_text(again.yrs_doc()), "azbcy");
    served.assert_sound("une autre origine");
}

/// Règle N3 (Ruling R825) quand les unités qui tiendraient en un bloc sont pour partie dans la page,
/// pour partie dans la mise à jour : la page tient « a » de l'onglet 10 ; une même mise à jour
/// porte « bc » (de l'onglet 10, origine « a ») et un élément d'origine « c » et d'origine droite
/// « b ». Avec la même origine droite que « a » (aucune), « abc » tient en un bloc : récrit. Avec
/// une autre (« z », tapé entre-temps après « a »), non : gardé.
#[test]
fn a_block_across_the_page_and_the_update_is_judged_whole() {
    for (with_z, expected, kept) in [(false, "abczy", false), (true, "abczyz", true)] {
        let mut served = Served::new();
        let writer = client(&served, 10);
        served.receive(&typed(&writer, 0, "a")).unwrap();
        if with_z {
            let other = client(&served, 20);
            served.receive(&typed(&other, 1, "z")).unwrap();
        }
        // « bc » à 2 et 3, d'origine « a » (10, 1), d'origine droite « z » (20, 0) ou aucune ;
        // puis le client 31 : « zy », d'origine « c », d'origine droite « b ».
        let mut both = vec![2, 1, 10, 2];
        if with_z {
            both.extend_from_slice(&[0xC4, 10, 1, 20, 0]);
        } else {
            both.extend_from_slice(&[0x84, 10, 1]);
        }
        both.extend_from_slice(&[2, b'b', b'c']);
        both.extend_from_slice(&[1, 31, 0, 0xC4, 10, 3, 10, 2, 2, b'z', b'y', 0]);
        served.receive(&both).expect("accepté");
        let text = paragraph_text(served.page.yrs_doc());
        assert_eq!(text, expected, "avec « z » : {with_z}");
        let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
        assert_eq!(paragraph_text(again.yrs_doc()), expected);
        served.assert_sound(&format!("avec « z » : {with_z}"));
        // Ce que la page relaie : l'élément du client 31 garde son origine droite, ou la perd.
        let relayed = served.journal.last().unwrap();
        let at = relayed
            .windows(3)
            .position(|w| w == [1, 31, 0])
            .expect("le client 31");
        assert_eq!(relayed[at + 3] & 0x40 != 0, kept, "avec « z » : {with_z}");
    }
}

/// Règle N3 (Ruling R825) sur ce que le serveur a écrit lui-même (une écriture du serveur ne
/// passe pas par la lecture des mises à jour) : la page lit ce que yrs en garde. « abc » écrit
/// d'un tenant par le client du serveur, puis un élément d'origine « c » et d'origine droite
/// « b » : récrit, comme sur une frappe d'un client.
#[test]
fn a_right_origin_left_of_the_origin_in_a_block_the_server_wrote_is_removed() {
    use yrs::{XmlFragment, XmlOut, XmlTextPrelim};
    let mut served = Served::new();
    let doc = served.page.yrs_doc();
    let me = doc.client_id().get();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let (clock, written) = {
        let mut txn = doc.transact_mut();
        let clock = txn.state_vector().get(&doc.client_id());
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("pas de groupe");
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            panic!("pas de bloc");
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            panic!("pas de paragraphe");
        };
        paragraph.insert(&mut txn, 0, XmlTextPrelim::new("abc"));
        (u64::from(clock), txn.encode_update_v1())
    };
    served.journal.push(written);
    // Le texte est l'élément `clock` du client du serveur, « abc » ses horloges suivantes.
    let relayed = served
        .receive(&string_between(
            40,
            0,
            (me, clock + 3),
            (me, clock + 2),
            "zy",
        ))
        .expect("accepté, récrit");
    assert_eq!(
        first_info(&relayed) & 0x40,
        0,
        "l'origine droite reste : {relayed:?}"
    );
    assert_eq!(paragraph_text(served.page.yrs_doc()), "abczy");
    let again = PageDoc::from_updates([served.page.encode_state().as_slice()]).unwrap();
    assert_eq!(paragraph_text(again.yrs_doc()), "abczy");
    served.assert_sound("une écriture du serveur");
}
