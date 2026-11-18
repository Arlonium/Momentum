//! Ce qui passe avant les règles qui récrivent une mise à jour (N3, G, I1, V, P2 ; Rulings R825,
//! R827 à R829, R856 ; Ruling R839). Une règle récrit un élément qu'aucune garde ne refuse : ce que
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

/// V : une valeur entre « a » et une lettre posée après « c ».
#[test]
fn the_guards_pass_before_rule_v() {
    let (scene, a) = with_abc();
    let after = scene.typed(0, "d");
    // « d » a pour origine « c », deux unités plus loin que « a » dans sa chaîne.
    value_is_refused_and_letters_pass(
        &scene,
        |what| item(900, 0, Some((TAB, a)), Some((TAB, after)), what),
        "V",
    );
    assert_eq!(
        page_text(&scene.page, 0),
        "abcdz",
        "V : les lettres perdent leur origine droite, et restent"
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

/// V : entre deux entrées d'une même clé, dont la seconde suit la troisième d'un autre.
#[test]
fn rule_v_leaves_an_item_beside_a_map_entry_to_r723() {
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
    beside_a_map_entry_is_refused_for_what_yrs_makes(&scene, &update, "V");
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

/// Une mise à jour faite à la main, d'octets écrits en hexadécimal.
fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|n| u8::from_str_radix(&text[2 * n..2 * n + 2], 16).unwrap())
        .collect()
}

/// La page et le premier texte, « abcdef », écrit par le client 10 : le texte à l'horloge 0, les
/// lettres de 1 à 6.
fn with_abcdef() -> PageDoc {
    let (page, created) = PageDoc::new_page();
    let tab = replica(10, &created);
    let typed = {
        let mut txn = tab.transact_mut();
        let paragraph = paragraph_of(&tab, &txn, 0);
        let text = paragraph.insert(&mut txn, 0, XmlTextPrelim::new(""));
        text.insert(&mut txn, 0, "abcdef");
        txn.encode_update_v1()
    };
    page.try_client_update(&typed).expect("la frappe");
    page
}

/// La règle V (Ruling R856) : ce qui suit une origine a pour origine cette origine ou un de ses
/// ancêtres. L'élément (23,0) se dit posé entre « c » et (20,0), dont l'origine (10,8) descend de
/// « c » par une chaîne qui passe par trois suites : son auteur ne voyait pas ses voisins côte à
/// côte. La page vivante lisait « abcdeppppfp », la page relue de son état « abcdepppppf », sous la
/// même révision (constat C3 de la re-relecture 8).
#[test]
fn a_right_origin_that_cannot_follow_the_origin_is_dropped() {
    let page = with_abcdef();
    for update in [
        // (22,0) : origine « e », origine droite « b ».
        "01011600c40a050a02017000",
        // (10,7) : mêmes voisins ; (10,8) : origine (10,7).
        "01020a07c40a050a020170840a07017000",
        // (20,0) : origine (10,8), origine droite (10,7).
        "01011400c40a080a07017000",
        // (23,0) : origine « c », origine droite (20,0).
        "01011700c40a031400017000",
    ] {
        page.try_client_update(&unhex(update)).expect("acceptée");
    }
    let again = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    assert_eq!(page_text(&page, 0), page_text(&again, 0));
    assert_eq!(page.revision(), again.revision());
}

/// La règle V nomme une unité, pas son client : (30,0) et (30,1) sont deux enfants de « c », écrits
/// par un même client, à la même profondeur. Ce qui descend de (30,1) peut être suivi d'un autre
/// enfant de (30,1), pas d'un enfant de (30,0), qui n'est pas son ancêtre.
#[test]
fn a_right_of_a_sibling_of_the_same_client_is_dropped() {
    let page = with_abcdef();
    let c = (10, 2);
    let letter = |id: Id, origin: Id, right: Id| {
        item(id.0, id.1, Some(origin), Some(right), Content::Letters)
    };
    for update in [
        letter((30, 0), c, (10, 3)),
        letter((30, 1), c, (30, 0)),
        letter((31, 0), (30, 1), (30, 0)),
        letter((37, 0), (30, 1), (30, 0)),
        letter((32, 0), (30, 0), (10, 3)),
    ] {
        let read = page.try_client_update(&update).expect("honnête").read;
        assert_eq!(read, update, "un élément honnête est récrit");
    }
    let beside_the_sibling = letter((33, 0), (31, 0), (32, 0));
    let read = page
        .try_client_update(&beside_the_sibling)
        .expect("acceptée")
        .read;
    assert_eq!(
        read,
        item(33, 0, Some((31, 0)), None, Content::Letters),
        "(32,0) a pour origine (30,0), qui n'est pas un ancêtre de (31,0)"
    );
    let beside_its_own = letter((34, 0), (31, 0), (37, 0));
    let read = page
        .try_client_update(&beside_its_own)
        .expect("acceptée")
        .read;
    assert_eq!(
        read, beside_its_own,
        "(37,0) a pour origine (30,1), un ancêtre de (31,0)"
    );
    let again = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    assert_eq!(page_text(&page, 0), page_text(&again, 0));
    assert_eq!(page.revision(), again.revision());
}

/// Ruling R864 : des unités que la page connaît déjà, et que yrs ignore, peuvent boucler dans les
/// octets d'une mise à jour sans que rien reste en attente : (10,1) se dit posée après (10,2), et
/// (10,2) après (10,1). Le parcours qui cherche si des valeurs sont dans un texte
/// (`Shape::untyped_in_texts`) s'arrête au premier élément revu.
#[test]
fn known_units_whose_bytes_loop_stop_the_walk_of_the_texts() {
    let (done, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let page = with_abcdef();
        let read = page.try_client_update(&unhex("01030a01840a020161840a010162840a02016300"));
        done.send((read.is_ok(), page_text(&page, 0))).unwrap();
    });
    let (accepted, text) = answer
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("le parcours ne finit pas");
    assert!(accepted, "rien de neuf n'est refusé");
    assert_eq!(text, "abcdef");
}

/// Le texte que lit un lecteur sans garde : un yrs neuf, à qui la page sert son état. Relue par une
/// autre page, la mise à jour repasserait par les règles, et l'écart ne se verrait pas.
fn read_without_the_guard(page: &PageDoc, at: u32) -> String {
    let bare = replica(99, &page.encode_state());
    let txn = bare.transact();
    text_of(&bare, &txn, at).get_string(&txn)
}

/// Le corps d'un élément fait par [`item`], sans ses en-têtes de mise à jour.
fn body(update: &[u8], client: u64, clock: u32) -> Vec<u8> {
    let mut head = vec![1, 1];
    var(&mut head, client);
    var(&mut head, u64::from(clock));
    update[head.len()..update.len() - 1].to_vec()
}

/// Ruling R861 : N3 et V se jugent dans l'ordre où les éléments s'intègrent, sur ce qui a déjà été
/// récrit. (10,7) a la forme N3 et perd son origine droite ; (10,6) et (10,7) tiennent alors en une
/// suite, et (10,8) a la forme N3 à son tour. Jugée sur la mise à jour reçue, (10,8) gardait son
/// origine droite : la page lisait « abcdefpq », un vrai Yjs qui ouvre l'état servi « abqcdefp ».
#[test]
fn rule_n3_after_rule_n3_in_the_same_update() {
    let page = with_abcdef();
    let read = page
        .try_client_update(&unhex("01020a07c40a060a050170c40a070a03017100"))
        .expect("acceptée")
        .read;
    assert_eq!(
        read,
        unhex("01020a07840a060170840a07017100"),
        "les deux éléments perdent leur origine droite"
    );
    assert_eq!(page_text(&page, 0), "abcdefpq");
    assert_eq!(read_without_the_guard(&page, 0), "abcdefpq");
}

/// Ruling R861 : de même après la règle V. (20,1) perd son origine droite par V ; (20,0) à (20,2)
/// tiennent alors en une suite, où (20,3) a la forme N3.
#[test]
fn rule_n3_after_rule_v_in_the_same_update() {
    let page = with_abcdef();
    let mut update = vec![1, 4, 20, 0];
    for (clock, origin, right) in [
        (0, (10, 2), None),
        (1, (20, 0), Some((10, 5))),
        (2, (20, 1), None),
        (3, (20, 2), Some((20, 1))),
    ] {
        let one = item(20, clock, Some(origin), right, Content::Letters);
        update.extend(body(&one, 20, clock));
    }
    update.push(0);
    let read = page.try_client_update(&update).expect("acceptée").read;
    assert_eq!(read_without_the_guard(&page, 0), page_text(&page, 0));
    let rewritten = with_abcdef();
    let mut expected = vec![1, 4, 20, 0];
    for (clock, origin) in [(0, (10, 2)), (1, (20, 0)), (2, (20, 1)), (3, (20, 2))] {
        let one = item(20, clock, Some(origin), None, Content::Letters);
        expected.extend(body(&one, 20, clock));
    }
    expected.push(0);
    assert_eq!(
        read,
        rewritten
            .try_client_update(&expected)
            .expect("acceptée")
            .read,
        "(20,1) et (20,3) perdent leur origine droite"
    );
}

/// Ruling R861 : ce que nomme un élément d'un autre client est jugé avant lui, quel que soit l'ordre
/// des clients dans la mise à jour. Le client 30 vient d'abord dans les octets ; son élément nomme
/// l'unité (10,7), que N3 récrit dans la même mise à jour.
#[test]
fn rule_n3_waits_for_the_units_of_another_client_it_names() {
    let page = with_abcdef();
    let mut update = vec![2, 1, 30, 0];
    let other = item(30, 0, Some((10, 7)), Some((10, 3)), Content::Letters);
    update.extend(body(&other, 30, 0));
    update.extend_from_slice(&[1, 10, 7]);
    let first = item(10, 7, Some((10, 6)), Some((10, 5)), Content::Letters);
    update.extend(body(&first, 10, 7));
    update.push(0);
    let read = page.try_client_update(&update).expect("acceptée").read;
    assert_eq!(read_without_the_guard(&page, 0), page_text(&page, 0));
    assert!(
        !read.windows(2).any(|w| w == [10, 3]),
        "(30,0) perd son origine droite : {read:?}"
    );
}

/// Ruling R861 : tout ce qui précède, chez le client d'une unité nommée, est jugé avant elle. Le
/// client 30 nomme (10,8) ; c'est (10,7), juste avant elle, qui perd son origine droite, et qui fait
/// tenir (10,6) à (10,8) en une suite.
#[test]
fn rule_n3_judges_what_comes_before_the_unit_it_names() {
    let page = with_abcdef();
    let mut update = vec![2, 1, 30, 0];
    let other = item(30, 0, Some((10, 8)), Some((10, 3)), Content::Letters);
    update.extend(body(&other, 30, 0));
    update.extend_from_slice(&[2, 10, 7]);
    for (clock, origin, right) in [(7, (10, 6), Some((10, 5))), (8, (10, 7), None)] {
        let one = item(10, clock, Some(origin), right, Content::Letters);
        update.extend(body(&one, 10, clock));
    }
    update.push(0);
    let read = page.try_client_update(&update).expect("acceptée").read;
    assert_eq!(read_without_the_guard(&page, 0), page_text(&page, 0));
    assert!(
        !read.windows(2).any(|w| w == [10, 3]),
        "(30,0) perd son origine droite : {read:?}"
    );
}

/// Ruling R861 : tout ce dont dépend un élément est jugé avant lui, jusqu'au bout de ses origines,
/// quel que soit l'ordre des clients dans les octets. (30,0) est posé entre (40,0) et (60,0), qui ont
/// la même origine (50,0) : il est honnête, et garde son origine droite. Jugé avant que (50,0) soit
/// connu, il se disait sans ancêtre commun avec son voisin de droite, et la règle V la lui retirait.
#[test]
fn a_right_origin_is_kept_when_the_origins_it_depends_on_come_later_in_the_update() {
    let page = with_abcdef();
    let mut update = vec![4];
    for (client, origin, right) in [
        (60, (50, 0), None),
        (30, (40, 0), Some((60, 0))),
        (40, (50, 0), None),
        (50, (10, 6), None),
    ] {
        update.extend_from_slice(&[1, client, 0]);
        let one = item(u64::from(client), 0, Some(origin), right, Content::Letters);
        update.extend(body(&one, u64::from(client), 0));
    }
    update.push(0);
    let read = page.try_client_update(&update).expect("acceptée").read;
    assert!(
        read.windows(5).any(|w| w == [0xC4, 40, 0, 60, 0]),
        "(30,0) garde son origine droite : {read:?}"
    );
    assert_eq!(read_without_the_guard(&page, 0), page_text(&page, 0));
}

/// Ruling R861 : de même pour l'unité d'avant d'un client : (30,0) nomme (10,8), dont l'origine est
/// (10,7), écrite juste avant par le même client.
#[test]
fn a_right_origin_is_kept_when_the_unit_before_the_one_it_names_comes_later() {
    let page = with_abcdef();
    let mut update = vec![3];
    let right = item(20, 0, Some((10, 7)), None, Content::Letters);
    let other = item(30, 0, Some((10, 8)), Some((20, 0)), Content::Letters);
    update.extend_from_slice(&[1, 30, 0]);
    update.extend(body(&other, 30, 0));
    update.extend_from_slice(&[1, 20, 0]);
    update.extend(body(&right, 20, 0));
    update.extend_from_slice(&[2, 10, 7]);
    for (clock, origin) in [(7, (10, 6)), (8, (10, 7))] {
        let one = item(10, clock, Some(origin), None, Content::Letters);
        update.extend(body(&one, 10, clock));
    }
    update.push(0);
    let read = page.try_client_update(&update).expect("acceptée").read;
    assert!(
        read.windows(5).any(|w| w == [0xC4, 10, 8, 20, 0]),
        "(30,0) garde son origine droite : {read:?}"
    );
    assert_eq!(read_without_the_guard(&page, 0), page_text(&page, 0));
}

/// Ruling R861 : les unités d'un client sont jugées dans l'ordre de ses horloges, même quand un
/// élément nomme d'abord la plus haute : (30,0) nomme (10,8), dont rien ne dépend de (10,7). Jugée
/// après elle, (10,7) n'aurait plus sa place parmi les suites, et (20,0), posée juste après (10,7),
/// se dirait sans ancêtre commun avec son voisin de droite.
#[test]
fn the_units_of_a_client_are_judged_in_the_order_of_their_clocks() {
    let page = with_abcdef();
    let mut update = vec![4];
    for (client, origin, right) in [
        (30, (10, 8), None),
        (20, (10, 7), Some((40, 0))),
        (40, (10, 6), None),
    ] {
        update.extend_from_slice(&[1, client, 0]);
        let one = item(u64::from(client), 0, Some(origin), right, Content::Letters);
        update.extend(body(&one, u64::from(client), 0));
    }
    update.extend_from_slice(&[2, 10, 7]);
    for clock in [7, 8] {
        let one = item(10, clock, Some((10, 6)), None, Content::Letters);
        update.extend(body(&one, 10, clock));
    }
    update.push(0);
    let read = page.try_client_update(&update).expect("acceptée").read;
    assert!(
        read.windows(5).any(|w| w == [0xC4, 10, 7, 40, 0]),
        "(20,0) garde son origine droite : {read:?}"
    );
}

/// Une frappe honnête au milieu d'un texte n'est pas touchée : l'origine de ce qui suit est
/// l'origine de la frappe, ou un de ses ancêtres.
#[test]
fn a_right_origin_that_follows_the_origin_is_kept() {
    let page = with_abcdef();
    for (at, what) in [(3, "x"), (4, "y"), (3, "z"), (6, "w")] {
        let tab = replica(77, &page.encode_state());
        let keystroke = {
            let mut txn = tab.transact_mut();
            let text = text_of(&tab, &txn, 0);
            text.insert(&mut txn, at, what);
            txn.encode_update_v1()
        };
        let read = page.try_client_update(&keystroke).expect("la frappe").read;
        assert_eq!(read, keystroke, "la frappe {what} est récrite");
        let seen = {
            let txn = tab.transact();
            text_of(&tab, &txn, 0).get_string(&txn)
        };
        assert_eq!(page_text(&page, 0), seen);
    }
}

/// Une chaîne d'origines de `length` suites, qui se casse à sa quatrième unité, et un élément entre
/// son début et ce qui suit sa fin : rien de ce que la règle décide ne dépend de la longueur.
fn beside_a_chain(length: u32) -> (bool, bool) {
    let page = with_abcdef();
    let letter = |u: &mut Vec<u8>, origin: Id, right: Id, content: u8| {
        u.push(0xC4);
        for id in [origin, right] {
            var(u, id.0);
            var(u, u64::from(id.1));
        }
        u.extend_from_slice(&[1, content]);
    };
    // Une suite par unité : l'origine droite change à chaque lettre.
    let mut chain = vec![1];
    var(&mut chain, u64::from(length));
    var(&mut chain, 30);
    chain.push(0);
    for i in 0..length {
        let origin = match i {
            0 => (10, 6),
            3 => (10, 5),
            _ => (30, i - 1),
        };
        letter(&mut chain, origin, (10, 2 + (i % 2)), b'u');
    }
    chain.push(0);
    page.try_client_update(&chain).expect("la chaîne");
    let mut right = vec![1, 1];
    var(&mut right, 31);
    right.push(0);
    right.push(0x84);
    var(&mut right, 30);
    var(&mut right, u64::from(length - 1));
    right.extend_from_slice(&[1, b'r']);
    right.push(0);
    page.try_client_update(&right)
        .expect("ce qui suit la chaîne");
    let mut x = vec![1, 1];
    var(&mut x, 32);
    x.push(0);
    letter(&mut x, (30, 0), (31, 0), b'x');
    x.push(0);
    page.try_client_update(&x).expect("l'élément");
    let again = PageDoc::from_updates([page.encode_state().as_slice()]).unwrap();
    (
        page_text(&page, 0).contains('x'),
        page_text(&page, 0) == page_text(&again, 0),
    )
}

/// La règle V ne s'arrête à aucune longueur : à 40, 90 ou 400 suites, l'élément garde sa place et
/// la page relue de son état est la page vivante. La règle I3 qu'elle remplace tenait une chaîne de
/// plus de 64 suites pour enchaînée, et ramassait l'élément (Ruling R857).
#[test]
fn the_length_of_a_chain_decides_nothing() {
    for length in [40, 90, 400] {
        assert_eq!(beside_a_chain(length), (true, true), "{length} suites");
    }
}
