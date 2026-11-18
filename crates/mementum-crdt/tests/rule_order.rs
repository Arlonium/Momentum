//! Ce qui passe avant les règles qui récrivent une mise à jour (N3, G, I1, I3, P2 ; Rulings R825,
//! R827 à R829, R834 ; Ruling R839). Une règle récrit un élément qu'aucune garde ne refuse : ce que
//! les gardes refusaient avant elle reste refusé, avec le même motif, et un élément posé à côté
//! d'une entrée de map reste jugé d'après ce que yrs en fait (Ruling R723).

use mementum_crdt::{CrdtError, FRAGMENT, PageDoc, Refused};
use yrs::updates::decoder::Decode;
use yrs::{
    Doc, GetString, OffsetKind, Options, ReadTxn, Text, Transact, Update, Xml, XmlElementPrelim,
    XmlElementRef, XmlFragment, XmlOut, XmlTextPrelim, XmlTextRef,
};

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

type Id = (u64, u32);

/// Ce que porte un élément fait à la main.
#[derive(Clone, Copy)]
enum Content {
    /// Une valeur `Any` : une valeur d'attribut, qu'aucune origine droite n'accompagne (R703).
    Value,
    /// Une chaîne.
    Letters,
}

/// Un élément de `client`, à `clock`, posé entre `origin` et `right`.
fn item(client: u64, clock: u32, origin: Option<Id>, right: Option<Id>, what: Content) -> Vec<u8> {
    let mut u = vec![1, 1];
    var(&mut u, client);
    var(&mut u, u64::from(clock));
    let kind = match what {
        Content::Value => 8,
        Content::Letters => 4,
    };
    u.push(kind | (u8::from(origin.is_some()) * 0x80) | (u8::from(right.is_some()) * 0x40));
    for id in [origin, right].into_iter().flatten() {
        var(&mut u, id.0);
        var(&mut u, u64::from(id.1));
    }
    match what {
        Content::Value => u.extend_from_slice(&[1, 119, 1, b'z']),
        Content::Letters => u.extend_from_slice(&[1, b'z']),
    }
    u.push(0);
    u
}

/// Une page et l'onglet qui l'écrit.
struct Scene {
    page: PageDoc,
    tab: Doc,
}

const TAB: u64 = 50;

impl Scene {
    fn new() -> Scene {
        let (page, created) = PageDoc::new_page();
        let tab = replica(TAB, &created);
        Scene { page, tab }
    }

    /// L'onglet envoie ce que `write` écrit, et rend la première horloge de cette écriture.
    fn write(&self, write: impl FnOnce(&Doc)) -> u32 {
        let before = self.tab.transact().state_vector();
        let clock = before.get(&yrs::ClientID::new(TAB));
        write(&self.tab);
        let update = self.tab.transact().encode_diff_v1(&before);
        self.page.try_client_update(&update).expect("accepté");
        clock
    }

    /// L'onglet tape `what` à la fin du paragraphe `at` : l'horloge de sa première lettre.
    fn typed(&self, at: u32, what: &str) -> u32 {
        self.write(|tab| {
            let (text, len) = {
                let txn = tab.transact();
                let text = text_of(tab, &txn, at);
                let len = text.get_string(&txn).len() as u32;
                (text, len)
            };
            text.insert(&mut tab.transact_mut(), len, what);
        })
    }

    /// L'onglet ajoute un bloc, dont le paragraphe porte `what` : l'horloge de sa première lettre.
    fn block(&self, what: &str) -> u32 {
        let start = self.write(|tab| {
            let group = group_of(tab);
            let mut txn = tab.transact_mut();
            let at = group.len(&txn);
            let container = group.insert(&mut txn, at, XmlElementPrelim::empty("blockContainer"));
            let paragraph = container.insert(&mut txn, 0, XmlElementPrelim::empty("paragraph"));
            paragraph.insert(&mut txn, 0, XmlTextPrelim::new(what));
        });
        // Le bloc, le paragraphe, le texte, puis ses lettres.
        start + 3
    }

    /// L'onglet supprime le dernier bloc : le ramasse-miettes en ramasse tout ce qu'il portait.
    fn delete_last_block(&self) {
        self.write(|tab| {
            let group = group_of(tab);
            let mut txn = tab.transact_mut();
            let at = group.len(&txn) - 1;
            group.remove_range(&mut txn, at, 1);
        });
    }

    /// L'onglet pose l'attribut `key` du paragraphe : l'horloge de cette entrée.
    fn attribute(&self, key: &str, value: &str) -> u32 {
        self.write(|tab| {
            let paragraph = paragraph_of(tab, &tab.transact(), 0);
            paragraph.insert_attribute(&mut tab.transact_mut(), key, value);
        })
    }

    fn state(&self) -> Vec<u8> {
        self.page.encode_state()
    }
}

fn replica(id: u64, state: &[u8]) -> Doc {
    let doc = Doc::with_options(Options {
        client_id: yrs::ClientID::new(id),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    doc.get_or_insert_xml_fragment(FRAGMENT);
    doc.transact_mut()
        .apply_update(Update::decode_v1(state).unwrap())
        .unwrap();
    doc
}

fn group_of(doc: &Doc) -> XmlElementRef {
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    match fragment.get(&txn, 0) {
        Some(XmlOut::Element(group)) => group,
        _ => panic!("pas de groupe de blocs"),
    }
}

fn paragraph_of(doc: &Doc, txn: &impl ReadTxn, at: u32) -> XmlElementRef {
    let Some(XmlOut::Element(container)) = group_of_in(doc, txn).get(txn, at) else {
        panic!("pas de bloc");
    };
    let Some(XmlOut::Element(paragraph)) = container.get(txn, 0) else {
        panic!("pas de paragraphe");
    };
    paragraph
}

fn group_of_in(doc: &Doc, txn: &impl ReadTxn) -> XmlElementRef {
    let Some(fragment) = txn.get_xml_fragment(FRAGMENT) else {
        panic!("pas de fragment dans {:?}", doc.client_id());
    };
    match fragment.get(txn, 0) {
        Some(XmlOut::Element(group)) => group,
        _ => panic!("pas de groupe de blocs"),
    }
}

fn text_of(doc: &Doc, txn: &impl ReadTxn, at: u32) -> XmlTextRef {
    match paragraph_of(doc, txn, at).get(txn, 0) {
        Some(XmlOut::Text(text)) => text,
        _ => panic!("pas de texte"),
    }
}

/// Le texte du paragraphe `at`, tel que la page le porte.
fn page_text(page: &PageDoc, at: u32) -> String {
    let doc = page.yrs_doc();
    let txn = doc.transact();
    text_of(doc, &txn, at).get_string(&txn)
}

/// Une page qui porte un premier paragraphe « abc ».
fn with_abc() -> (Scene, u32) {
    let scene = Scene::new();
    scene.write(|tab| {
        let paragraph = paragraph_of(tab, &tab.transact(), 0);
        paragraph.insert(&mut tab.transact_mut(), 0, XmlTextPrelim::new(""));
    });
    let a = scene.typed(0, "abc");
    (scene, a)
}

/// `form` est refusé avant d'être appliqué pour ce que disent ses octets (R703), et sa variante en
/// lettres, qu'aucune garde ne refuse, passe : la règle s'y applique, la page reste saine.
fn value_is_refused_and_letters_pass(scene: &Scene, form: impl Fn(Content) -> Vec<u8>, rule: &str) {
    let before = scene.state();
    match scene.page.try_client_update(&form(Content::Value)) {
        Err(Refused {
            error: CrdtError::Structure(why),
            intact: true,
        }) => assert!(why.contains("origine droite"), "{rule} : {why}"),
        other => panic!(
            "{rule} : la valeur n'est pas refusée d'après ses octets : {:?}",
            other.map(|_| ()).map_err(|refused| refused.error)
        ),
    }
    assert!(scene.state() == before, "{rule} : la page a changé");
    let letters = scene.page.try_client_update(&form(Content::Letters));
    assert!(
        letters.is_ok(),
        "{rule} : {:?}",
        letters.err().map(|r| r.error)
    );
    assert_eq!(scene.page.misjudged_units(), None, "{rule}");
}

/// G : une valeur dont l'origine est ramassée et l'origine droite vivante.
#[test]
fn the_guards_pass_before_rule_g() {
    let (scene, a) = with_abc();
    let gone = scene.block("bonjour");
    scene.delete_last_block();
    value_is_refused_and_letters_pass(
        &scene,
        |what| item(900, 0, Some((TAB, gone)), Some((TAB, a + 1)), what),
        "G",
    );
    assert_eq!(
        page_text(&scene.page, 0),
        "abc",
        "G : les lettres sont ramassées"
    );
}

/// I1 : une valeur sans origine, posée devant une lettre qui en a une.
#[test]
fn the_guards_pass_before_rule_i1() {
    let (scene, a) = with_abc();
    value_is_refused_and_letters_pass(
        &scene,
        |what| item(900, 0, None, Some((TAB, a + 1)), what),
        "I1",
    );
    assert_eq!(
        page_text(&scene.page, 0),
        "abc",
        "I1 : les lettres sont ramassées"
    );
}

/// I3 : une valeur entre « a » et une lettre posée après « c ».
#[test]
fn the_guards_pass_before_rule_i3() {
    let (scene, a) = with_abc();
    let after = scene.typed(0, "d");
    // « d » a pour origine « c », deux unités plus loin que « a » dans sa chaîne.
    value_is_refused_and_letters_pass(
        &scene,
        |what| item(900, 0, Some((TAB, a)), Some((TAB, after)), what),
        "I3",
    );
    assert_eq!(
        page_text(&scene.page, 0),
        "abcd",
        "I3 : les lettres sont ramassées"
    );
}

/// N3 : une valeur d'origine « c » et d'origine droite « b », dans « abc » d'un tenant.
#[test]
fn the_guards_pass_before_rule_n3() {
    let (scene, a) = with_abc();
    value_is_refused_and_letters_pass(
        &scene,
        |what| item(900, 0, Some((TAB, a + 2)), Some((TAB, a + 1)), what),
        "N3",
    );
    assert_eq!(
        page_text(&scene.page, 0),
        "abcz",
        "N3 : les lettres gardent leur origine"
    );
}

/// P2 : une valeur entre deux lettres de deux paragraphes.
#[test]
fn the_guards_pass_before_rule_p2() {
    let (scene, a) = with_abc();
    let other = scene.block("xy");
    value_is_refused_and_letters_pass(
        &scene,
        |what| item(900, 0, Some((TAB, a)), Some((TAB, other)), what),
        "P2",
    );
    assert_eq!(
        page_text(&scene.page, 0),
        "abc",
        "P2 : les lettres sont ramassées"
    );
}

/// `update`, posé à côté d'une entrée de map, n'est pas récrit : yrs en fait une entrée de map à
/// origine droite, et c'est ce refus-là qui répond (Ruling R723).
fn beside_a_map_entry_is_refused_for_what_yrs_makes(scene: &Scene, update: &[u8], rule: &str) {
    match scene.page.try_client_update(update) {
        Err(Refused {
            error: CrdtError::Structure(why),
            ..
        }) if why.contains("origine droite") => {}
        other => panic!(
            "{rule} : {:?}",
            other.map(|_| ()).map_err(|refused| refused.error)
        ),
    }
}

/// G : origine ramassée, origine droite une entrée de map vivante.
#[test]
fn rule_g_leaves_an_item_beside_a_map_entry_to_r723() {
    let (scene, _) = with_abc();
    let gone = scene.block("bonjour");
    scene.delete_last_block();
    let entry = scene.attribute("textColor", "red");
    let update = item(
        900,
        0,
        Some((TAB, gone)),
        Some((TAB, entry)),
        Content::Letters,
    );
    beside_a_map_entry_is_refused_for_what_yrs_makes(&scene, &update, "G");
}

/// I1 : sans origine, devant une entrée de map qui en écrase une autre.
#[test]
fn rule_i1_leaves_an_item_beside_a_map_entry_to_r723() {
    let (scene, _) = with_abc();
    let entry = scene.attribute("textColor", "red");
    let update = item(900, 0, None, Some((TAB, entry)), Content::Letters);
    beside_a_map_entry_is_refused_for_what_yrs_makes(&scene, &update, "I1");
}

/// I3 : entre deux entrées d'une même clé, dont la seconde suit la troisième d'un autre.
#[test]
fn rule_i3_leaves_an_item_beside_a_map_entry_to_r723() {
    let (scene, _) = with_abc();
    let first = scene.attribute("textColor", "red");
    scene.attribute("textColor", "blue");
    let third = scene.attribute("textColor", "green");
    // L'entrée d'un autre client, qui écrase la troisième : son origine est plus loin que la
    // première dans la chaîne de la première.
    let other = replica(60, &scene.state());
    let before = other.transact().state_vector();
    {
        let paragraph = paragraph_of(&other, &other.transact(), 0);
        paragraph.insert_attribute(&mut other.transact_mut(), "textColor", "black");
    }
    let written = other.transact().encode_diff_v1(&before);
    scene.page.try_client_update(&written).expect("accepté");
    assert_eq!(third, first + 2);
    let update = item(900, 0, Some((TAB, first)), Some((60, 0)), Content::Letters);
    beside_a_map_entry_is_refused_for_what_yrs_makes(&scene, &update, "I3");
}

/// N3 : l'origine et l'origine droite sont deux entrées d'une même clé, d'un même client.
#[test]
fn rule_n3_leaves_an_item_beside_a_map_entry_to_r723() {
    let (scene, _) = with_abc();
    scene.attribute("textColor", "red");
    let second = scene.attribute("textColor", "blue");
    let third = scene.attribute("textColor", "green");
    let update = item(
        900,
        0,
        Some((TAB, third)),
        Some((TAB, second)),
        Content::Letters,
    );
    beside_a_map_entry_is_refused_for_what_yrs_makes(&scene, &update, "N3");
}
