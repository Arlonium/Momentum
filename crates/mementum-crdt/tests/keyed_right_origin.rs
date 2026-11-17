//! Une entrée de map à origine droite (Ruling R723, la cause du constat R722).
//!
//! yrs 0.28.1 donne à un élément la clé de map (`parentSub`) de son voisin de gauche et, s'il n'en
//! a pas, celle de son voisin de **droite** (`integrate_item`, `block.rs`) ; Yjs ne prend que
//! celle de gauche. Un élément dont l'origine est un enfant d'une liste et l'origine droite une
//! entrée de map devient donc, pour yrs seul, l'entrée courante de la clé : yrs supprime son
//! voisin de gauche (un enfant de la liste), et l'ancienne entrée n'est plus tenue par rien. Le
//! jour où le parent est supprimé puis ramassé, elle lui survit avec un pointeur vers un parent
//! libéré : l'état que la page encode ensuite est fait d'octets au hasard (`InvalidParent` chez
//! qui le relit), ou le processus s'arrête sur une violation d'accès.
//!
//! Aucun client n'écrit une entrée de map à origine droite. La page la refuse : avant d'appliquer
//! quand les octets le disent, sinon d'après ce que yrs en a fait (l'encodage de la transaction,
//! où chaque élément porte la marque `parentSub` que yrs lui a donnée).

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::{
    ClientID, Doc, GetString, OffsetKind, Options, ReadTxn, StateVector, Text, Transact, Update,
    XmlFragment, XmlOut, XmlTextPrelim,
};

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Une page servie et son journal, comme les tient l'acteur.
struct Served {
    page: PageDoc,
    journal: Vec<Vec<u8>>,
    /// Le client du serveur, qui a écrit la page : `blockGroup` à l'horloge 0, `blockContainer`
    /// à 1, son `id` à 2, `paragraph` à 3, `backgroundColor` à 4, `textColor` à 5,
    /// `textAlignment` à 6.
    server: u64,
}

const CONTAINER: u32 = 1;
const BACKGROUND: u32 = 4;

impl Served {
    /// Une page neuve, où le client 10 a tapé « abc » : le texte est son élément 0, les lettres
    /// ses horloges 1 à 3.
    fn new() -> Served {
        let (page, created) = PageDoc::new_page();
        let state = StateVector::decode_v1(&page.state_vector()).unwrap();
        let server = state.iter().next().unwrap().0.get();
        let mut served = Served {
            page,
            journal: vec![created],
            server,
        };
        let doc = served.client(10);
        let typed = {
            let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
            let mut txn = doc.transact_mut();
            let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
                panic!("pas de groupe");
            };
            let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
                panic!("pas de bloc");
            };
            let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
                panic!("pas de paragraphe");
            };
            let text = paragraph.insert(&mut txn, 0, XmlTextPrelim::new(""));
            text.insert(&mut txn, 0, "abc");
            txn.encode_update_v1()
        };
        served.receive(&typed).expect("la frappe de départ");
        served
    }

    fn client(&self, id: u64) -> Doc {
        let doc = Doc::with_options(Options {
            client_id: ClientID::new(id),
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        doc.get_or_insert_xml_fragment(FRAGMENT);
        doc.transact_mut()
            .apply_update(Update::decode_v1(&self.page.encode_state()).unwrap())
            .unwrap();
        doc
    }

    /// Comme l'acteur : acceptée, la mise à jour va au journal telle que la page la rend ;
    /// refusée, la page est rechargée de son journal.
    fn receive(&mut self, update: &[u8]) -> Result<(), String> {
        match self.page.apply_client_update(update) {
            Ok(applied) => {
                self.journal.push(applied.read);
                self.journal.extend(applied.carried);
                self.journal.extend(self.page.dedupe_blocks());
                Ok(())
            }
            Err(error) => {
                self.page = self.reloaded();
                Err(error.to_string())
            }
        }
    }

    fn reloaded(&self) -> PageDoc {
        PageDoc::from_updates(self.journal.iter().map(Vec::as_slice)).expect("le journal se relit")
    }

    /// Une chaîne « bc » du client `client`, à l'horloge `clock`, d'origine `origin` et d'origine
    /// droite `right` : pas de parent nommé, pas de clé. Sans son jeu de suppressions.
    fn string_between(client: u64, clock: u32, origin: (u64, u32), right: (u64, u32)) -> Vec<u8> {
        let mut u = vec![1, 1];
        var(&mut u, client);
        var(&mut u, u64::from(clock));
        u.push(0xC4);
        var(&mut u, origin.0);
        var(&mut u, u64::from(origin.1));
        var(&mut u, right.0);
        var(&mut u, u64::from(right.1));
        u.extend_from_slice(&[2, b'b', b'c']);
        u
    }

    /// La suppression du bloc de la page (son `blockContainer`).
    fn block_deleted(&self) -> Vec<u8> {
        let mut u = vec![0, 1];
        var(&mut u, self.server);
        u.extend_from_slice(&[1, CONTAINER as u8, 1]);
        u
    }
}

/// Ce qu'un client neuf (yrs) lit d'un état : `Err` s'il ne le relit pas.
fn reread(state: &[u8]) -> Result<String, String> {
    let doc = Doc::with_options(Options {
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = Update::decode_v1(state).map_err(|e| format!("illisible : {e:?}"))?;
    doc.transact_mut()
        .apply_update(update)
        .map_err(|e| format!("refusé : {e:?}"))?;
    let txn = doc.transact();
    if txn.store().pending_update().is_some() {
        return Err("des éléments restent en attente".to_string());
    }
    Ok(fragment.get_string(&txn))
}

/// La page et son journal se relisent par un client neuf, et disent la même chose.
fn assert_sound(served: &Served, what: &str) {
    let live = reread(&served.page.encode_state());
    assert!(live.is_ok(), "{what} : l'état de la page vivante, {live:?}");
    let reloaded = served.reloaded();
    let again = reread(&reloaded.encode_state());
    assert!(
        again.is_ok(),
        "{what} : l'état de la page rechargée, {again:?}"
    );
    assert_eq!(
        reloaded.read_blocks(),
        served.page.read_blocks(),
        "{what} : les blocs de la page rechargée"
    );
    assert_eq!(
        reloaded.revision(),
        served.page.revision(),
        "{what} : la révision de la page rechargée"
    );
}

/// Le constat R722, réduit : une chaîne d'origine le texte du paragraphe (son dernier enfant) et
/// d'origine droite l'entrée courante de `backgroundColor`. yrs en fait l'entrée courante de la
/// couleur, et l'ancienne entrée n'est plus tenue par rien. Refusée ; et, le bloc supprimé par
/// la suite, l'état de la page se relit toujours.
#[test]
fn a_string_between_a_child_and_a_map_entry_is_refused() {
    let mut served = Served::new();
    let mut hybrid = Served::string_between(7_000, 0, (10, 0), (served.server, BACKGROUND));
    hybrid.push(0);
    let refused = served.receive(&hybrid);
    assert!(refused.is_err(), "la chaîne est acceptée");
    let deleted = served.block_deleted();
    let _ = served.receive(&deleted);
    assert_sound(&served, "le bloc supprimé");
}

/// La même, avec la suppression du bloc dans la même mise à jour : le parent est ramassé à la
/// validation de la transaction qui pose la chaîne.
#[test]
fn with_its_block_deleted_in_the_same_update_it_is_refused() {
    let mut served = Served::new();
    let mut hybrid = Served::string_between(7_000, 0, (10, 0), (served.server, BACKGROUND));
    hybrid.extend_from_slice(&served.block_deleted()[1..]);
    assert!(served.receive(&hybrid).is_err(), "la chaîne est acceptée");
    assert_sound(&served, "la chaîne et la suppression refusées");
}

/// D'un parent à l'autre : l'origine est la dernière lettre du texte, l'origine droite l'entrée
/// de couleur du paragraphe. yrs en fait une entrée de map **du texte** (et supprime la lettre),
/// Yjs une lettre de plus : refusée.
#[test]
fn a_string_between_a_letter_and_a_map_entry_of_another_parent_is_refused() {
    let mut served = Served::new();
    let mut hybrid = Served::string_between(7_000, 0, (10, 3), (served.server, BACKGROUND));
    hybrid.push(0);
    assert!(served.receive(&hybrid).is_err(), "la chaîne est acceptée");
    assert_sound(&served, "la chaîne refusée");
}

/// Sans origine gauche : l'origine droite seule est une entrée de map. yrs et Yjs y lisent tous
/// deux une ancienne entrée, aussitôt supprimée ; aucun client ne l'écrit, refusée aussi.
#[test]
fn a_string_before_a_map_entry_is_refused() {
    let mut served = Served::new();
    let mut u = vec![1, 1];
    var(&mut u, 7_000);
    u.extend_from_slice(&[0, 0x44]);
    var(&mut u, served.server);
    u.extend_from_slice(&[BACKGROUND as u8, 2, b'b', b'c', 0]);
    assert!(served.receive(&u).is_err(), "la chaîne est acceptée");
    assert_sound(&served, "la chaîne refusée");
}

/// Une frappe au milieu d'un texte porte une origine droite, et passe : elle n'est pas une
/// entrée de map.
#[test]
fn typing_in_the_middle_of_a_text_is_accepted() {
    let mut served = Served::new();
    let mut typed = Served::string_between(7_000, 0, (10, 1), (10, 2));
    typed.push(0);
    served.receive(&typed).expect("une frappe au milieu");
    assert_sound(&served, "la frappe");
    assert!(
        reread(&served.page.encode_state())
            .unwrap()
            .contains("abcbc"),
        "{:?}",
        reread(&served.page.encode_state())
    );
}

/// Déjà au journal (écrite par un serveur d'avant cette garde, telle que yrs l'a encodée : la
/// marque `parentSub` y est) : l'entrée est refusée avant d'être appliquée, et la page ne se
/// charge pas ; elle ne rend jamais un état fait d'octets au hasard.
#[test]
fn already_in_the_journal_as_yrs_wrote_it_the_page_is_not_loaded() {
    let served = Served::new();
    let mut stored = Served::string_between(7_000, 0, (10, 0), (served.server, BACKGROUND));
    // Telle que yrs l'encode après l'avoir intégrée : la marque `parentSub`, et la suppression
    // du voisin de gauche.
    assert_eq!(stored[5], 0xC4);
    stored[5] = 0xE4;
    stored.extend_from_slice(&[1, 10, 1, 0, 4]);
    let mut journal = served.journal.clone();
    journal.push(stored);
    assert!(
        PageDoc::from_updates(journal.iter().map(Vec::as_slice)).is_err(),
        "la page se charge avec l'entrée"
    );
}

/// Déjà au journal, octets d'un client gardés tels quels (une base d'avant R130) : rien ne dit
/// dans les octets que yrs en fera une entrée de map. La page le voit après l'avoir appliquée,
/// et ne se charge pas.
#[test]
fn already_in_the_journal_as_a_client_sent_it_the_page_is_not_loaded() {
    let served = Served::new();
    let mut stored = Served::string_between(7_000, 0, (10, 0), (served.server, BACKGROUND));
    stored.push(0);
    let mut journal = served.journal.clone();
    journal.push(stored);
    assert!(
        PageDoc::from_updates(journal.iter().map(Vec::as_slice)).is_err(),
        "la page se charge avec l'entrée"
    );
}

/// Ce que le serveur écrit lui-même se relit : l'état complet d'une page où l'on a tapé au
/// milieu d'un texte et écrasé des attributs (des origines droites, des entrées de map) passe
/// comme une entrée du journal.
#[test]
fn the_state_of_an_honest_page_is_reloaded() {
    let mut served = Served::new();
    let mut typed = Served::string_between(7_000, 0, (10, 1), (10, 2));
    typed.push(0);
    served.receive(&typed).unwrap();
    let doc = served.client(30);
    let colored = {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("pas de groupe");
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            panic!("pas de bloc");
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            panic!("pas de paragraphe");
        };
        use yrs::Xml;
        paragraph.insert_attribute(&mut txn, "backgroundColor", "red");
        paragraph.insert_attribute(&mut txn, "backgroundColor", "blue");
        txn.encode_update_v1()
    };
    served.receive(&colored).unwrap();
    assert_sound(&served, "la page honnête");
    let state = served.page.encode_state();
    assert!(PageDoc::from_updates([state.as_slice()]).is_ok());
}

/// Le texte vide qu'un client de passage ajoute à la fin du paragraphe : son élément 0.
fn empty_text(served: &mut Served, client: u64) {
    let doc = served.client(client);
    let text = {
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let mut txn = doc.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("pas de groupe");
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            panic!("pas de bloc");
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            panic!("pas de paragraphe");
        };
        paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
        txn.encode_update_v1()
    };
    served.receive(&text).expect("le texte vide");
}

/// Le constat N1 de la re-relecture, et ses voisines : une chaîne à cheval sur ce que la page
/// connaît (le texte vide du client, son horloge 0), d'origine droite l'entrée courante de la
/// couleur, puis un ou deux contenus qui se suivent par leur origine. Le serveur lisait
/// `backgroundColor = "ok"` et un paragraphe sans enfant ; Yjs y lit deux enfants sans type et la
/// couleur d'avant (`apps/web/src/editor/attrContents.test.ts`). Le premier morceau est, pour
/// yrs, une entrée de map à origine droite : tout est refusé.
#[test]
fn contents_that_follow_a_string_astride_with_a_right_origin_are_refused() {
    let ok: &[u8] = &[0x88, 0xD8, 0x36, 2, 1, 119, 2, b'o', b'k'];
    let object: &[u8] = &[
        0x88, 0xD8, 0x36, 2, 1, 118, 1, 8, b't', b'o', b'S', b't', b'r', b'i', b'n', b'g', 125, 1,
    ];
    let embed: &[u8] = &[0x85, 0xD8, 0x36, 2, 2, b'{', b'}'];
    let bytes: &[u8] = &[0x83, 0xD8, 0x36, 2, 2, 1, 2];
    let mark: &[u8] = &[
        0x86, 0xD8, 0x36, 2, 4, b'b', b'o', b'l', b'd', 2, b'{', b'}',
    ];
    // Un troisième morceau, d'origine le deuxième (l'horloge 3).
    let third: &[u8] = &[0x88, 0xD8, 0x36, 3, 1, 119, 2, b'o', b'k'];
    let followers: [(&str, Vec<&[u8]>); 7] = [
        ("rien", vec![]),
        ("une valeur", vec![ok]),
        ("un objet", vec![object]),
        ("un objet incorporé", vec![embed]),
        ("des octets", vec![bytes]),
        ("une marque", vec![mark]),
        ("un objet incorporé puis une valeur", vec![embed, third]),
    ];
    for (what, pieces) in followers {
        let mut served = Served::new();
        empty_text(&mut served, 7_000);
        let before = served.page.read_blocks();
        // La chaîne « abc » à l'horloge 0 : la page connaît l'horloge 0, yrs intègre « bc ».
        let mut update = vec![1, 1 + pieces.len() as u8];
        var(&mut update, 7_000);
        update.extend_from_slice(&[0, 0xC4]);
        var(&mut update, 7_000);
        update.push(0);
        var(&mut update, served.server);
        update.extend_from_slice(&[BACKGROUND as u8, 3, b'a', b'b', b'c']);
        for piece in pieces {
            update.extend_from_slice(piece);
        }
        update.push(0);
        let result = served.receive(&update);
        assert!(result.is_err(), "{what} : acceptée");
        assert_eq!(
            served.page.read_blocks(),
            before,
            "{what} : la page a changé"
        );
        assert_sound(&served, what);
    }
}

/// Ruling R724 : un contenu qui n'est pas une valeur, posé par l'origine de l'entrée courante
/// d'un attribut. yrs ne montre rien d'une marque ou d'un type parmi les attributs d'un élément,
/// et la page ne voyait donc pas qu'ils avaient pris la place de la valeur ; la garde, relisant
/// l'état servi, y lisait un attribut qu'elle refuse. Refusés d'après ce que yrs en a fait ; un
/// contenu supprimé, lui, passe : c'est retirer l'attribut.
#[test]
fn a_content_that_is_not_a_value_placed_over_an_attribute_is_refused() {
    let mark: &[u8] = &[0x86, 0, 0, 4, b'b', b'o', b'l', b'd', 2, b'{', b'}'];
    let string: &[u8] = &[0x84, 0, 0, 3, b'r', b'e', b'd'];
    let element: &[u8] = &[0x87, 0, 0, 3, 1, b'p'];
    let text: &[u8] = &[0x87, 0, 0, 6];
    let deleted: &[u8] = &[0x81, 0, 0, 1];
    for (what, piece, refused) in [
        ("une marque", mark, true),
        ("une chaîne", string, true),
        ("un élément", element, true),
        ("un texte", text, true),
        ("un contenu supprimé", deleted, false),
    ] {
        let mut served = Served::new();
        let mut update = vec![1, 1];
        var(&mut update, 7_000);
        update.extend_from_slice(&[0, piece[0]]);
        var(&mut update, served.server);
        update.push(BACKGROUND as u8);
        update.extend_from_slice(&piece[3..]);
        update.push(0);
        let result = served.receive(&update);
        assert_eq!(result.is_err(), refused, "{what} : {result:?}");
        assert_sound(&served, what);
        // Ce que la garde lit de l'état servi, dans ses octets : rien qu'elle refuse.
        let empty = PageDoc::from_updates(std::iter::empty::<&[u8]>()).unwrap();
        assert_eq!(
            empty
                .attribute_refusal(&served.page.encode_state())
                .unwrap(),
            None,
            "{what}"
        );
    }
}

/// Un contenu supprimé entre un enfant et une entrée de map : rien à lire, mais yrs en fait
/// quand même l'entrée courante de la clé, et défait la liste comme pour une chaîne. Refusé.
#[test]
fn a_deleted_content_between_a_child_and_a_map_entry_is_refused() {
    let mut served = Served::new();
    let mut update = vec![1, 1];
    var(&mut update, 7_000);
    update.extend_from_slice(&[0, 0xC1, 10, 0]);
    var(&mut update, served.server);
    update.extend_from_slice(&[BACKGROUND as u8, 1, 0]);
    let before = served.page.read_blocks();
    assert!(served.receive(&update).is_err(), "le contenu est accepté");
    assert_eq!(served.page.read_blocks(), before);
    let deleted = served.block_deleted();
    let _ = served.receive(&deleted);
    assert_sound(&served, "le bloc supprimé");
}

/// L'état complet d'une page qui porte déjà une telle entrée (un instantané, une version de
/// l'historique, écrits par un serveur d'avant cette garde) : yrs l'a encodé, la marque « entrée
/// de map » y est, et la page vide qui le relit le refuse avant de l'appliquer.
#[test]
fn a_state_that_holds_one_is_refused_by_an_empty_page() {
    let served = Served::new();
    let state = served.page.encode_state();
    assert!(PageDoc::from_updates([state.as_slice()]).is_ok());
    // Les clients d'un état vont du plus grand identifiant au plus petit : le serveur, puis le
    // client 7 000, puis le client 10 (deux éléments à partir de l'horloge 0 : son texte, un
    // type, puis ses lettres).
    assert_eq!(state[0], 2, "deux clients");
    let ten = state
        .windows(4)
        .rposition(|w| w == [2, 10, 0, 7])
        .expect("le client 10");
    let mut poisoned = vec![3];
    poisoned.extend_from_slice(&state[1..ten]);
    poisoned.push(1);
    var(&mut poisoned, 7_000);
    poisoned.extend_from_slice(&[0, 0xE4, 10, 0]);
    var(&mut poisoned, served.server);
    poisoned.extend_from_slice(&[BACKGROUND as u8, 2, b'b', b'c']);
    poisoned.extend_from_slice(&state[ten..]);
    assert!(PageDoc::has_keyed_right_origin(&poisoned));
    let result = PageDoc::from_updates([poisoned.as_slice()]);
    assert!(
        matches!(&result, Err(e) if e.to_string().contains("origine droite")),
        "l'état est relu"
    );
    assert!(PageDoc::served_state(&poisoned).is_none());
}

// ------------------------------------------------------------- à cheval (Rulings R784 à R786)

/// Le client 7 000 ajoute un second bloc : son `blockContainer` est son élément 0, l'`id` le 1,
/// le paragraphe le 2, et sa couleur de fond « red » le 3.
fn second_block(served: &mut Served) {
    use yrs::{Xml, XmlElementPrelim};
    let doc = served.client(7_000);
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let update = {
        let mut txn = doc.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("pas de groupe");
        };
        let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
        container.insert_attribute(&mut txn, "id", "0196f0a0-0000-7000-8000-000000000001");
        let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
        paragraph.insert_attribute(&mut txn, "backgroundColor", "red");
        txn.encode_update_v1()
    };
    served.receive(&update).expect("le second bloc");
}

/// Un élément du client 7 000 à l'horloge 3, que la page connaît (la couleur du second bloc), de
/// deux unités : parent nommé, le paragraphe du **premier** bloc, clé `backgroundColor`.
fn keyed_astride(served: &Served, info: u8, content: &[u8]) -> Vec<u8> {
    let mut u = vec![1, 1];
    var(&mut u, 7_000);
    u.extend_from_slice(&[3, info, 0]);
    var(&mut u, served.server);
    u.push(3);
    let key = b"backgroundColor";
    u.push(key.len() as u8);
    u.extend_from_slice(key);
    u.extend_from_slice(content);
    u.push(0);
    u
}

/// Ce qui bâtit une mise à jour pour une page servie.
type Form = Box<dyn Fn(&Served) -> Vec<u8>>;

/// Les formes du constat C1 : ce que le client 7 000 envoie à son horloge 3, déjà connue.
fn astride_forms() -> Vec<(&'static str, Form)> {
    let two_values: &[u8] = &[2, 119, 3, b'r', b'e', b'd', 119, 4, b'b', b'l', b'u', b'e'];
    vec![
        (
            "un contenu supprimé, parent nommé ailleurs",
            Box::new(|s| keyed_astride(s, 0x21, &[2])),
        ),
        (
            "deux valeurs, parent nommé ailleurs",
            Box::new(move |s| keyed_astride(s, 0x28, two_values)),
        ),
        (
            "un contenu supprimé, d'origine une lettre du premier bloc",
            Box::new(|_| vec![1, 1, 216, 54, 3, 0x81, 10, 3, 2, 0]),
        ),
        (
            "une chaîne, parent nommé le texte du premier bloc",
            Box::new(|_| vec![1, 1, 216, 54, 3, 0x04, 0, 10, 0, 2, b'a', b'b', 0]),
        ),
    ]
}

/// Après une scène : le premier bloc supprimé, des clients honnêtes écrivent, et la page est
/// encodée en boucle (son état complet, et ce qui manque à chaque état passé `stages`), relue
/// chaque fois par un yrs neuf ; son journal relu donne la même page. C'est la boucle où le
/// constat C1 de la re-relecture 3 arrêtait le serveur.
fn aftermath(served: &mut Served, stages: &[Vec<u8>], what: &str) {
    use yrs::updates::encoder::Encode;
    use yrs::{Xml, XmlElementPrelim};
    let deleted = served.block_deleted();
    let _ = served.receive(&deleted);
    for n in 0..10u64 {
        let doc = served.client(100 + n);
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        let update = {
            let mut txn = doc.transact_mut();
            let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
                panic!("pas de groupe");
            };
            let container = group.push_back(&mut txn, XmlElementPrelim::empty("blockContainer"));
            container.insert_attribute(
                &mut txn,
                "id",
                format!("0196f0a0-0000-7000-8000-0000000001{n:02}"),
            );
            let paragraph = container.push_back(&mut txn, XmlElementPrelim::empty("paragraph"));
            let text = paragraph.push_back(&mut txn, XmlTextPrelim::new(""));
            text.insert(&mut txn, 0, "bonjour tout le monde");
            txn.encode_update_v1()
        };
        let written = served.receive(&update);
        assert!(
            written.is_ok(),
            "{what} : un client honnête refusé ensuite, {written:?}"
        );
        for _ in 0..10 {
            let read = reread(&served.page.encode_state());
            assert!(read.is_ok(), "{what} : {read:?}");
        }
    }
    assert_sound(served, what);
    let whole = reread(&served.page.encode_state()).unwrap();
    for (n, stage) in stages.iter().enumerate() {
        let doc = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
        doc.transact_mut()
            .apply_update(Update::decode_v1(stage).unwrap())
            .unwrap();
        let sv = doc.transact().state_vector().encode_v1();
        let diff = served.page.encode_diff(&sv).unwrap();
        let decoded = Update::decode_v1(&diff)
            .unwrap_or_else(|e| panic!("{what} : étape 2 depuis l'état {n} illisible : {e:?}"));
        doc.transact_mut().apply_update(decoded).unwrap();
        let txn = doc.transact();
        assert!(
            txn.store().pending_update().is_none(),
            "{what} : l'étape 2 depuis l'état {n} laisse des éléments en attente"
        );
        assert_eq!(
            fragment.get_string(&txn),
            whole,
            "{what} : l'étape 2 depuis l'état {n}"
        );
    }
}

/// La couleur de fond du bloc `n` de la page, telle que le serveur la lit.
fn background(page: &PageDoc, n: usize) -> Option<String> {
    page.read_blocks()[n]
        .props
        .get("backgroundColor")
        .map(|value| format!("{value:?}"))
}

/// Ce que devient chaque forme de [`astride_forms`], coupée (Ruling R784) : la part neuve est un
/// élément ordinaire, d'origine la couleur du **second** bloc (l'horloge 3 du client 7 000), où
/// qu'aient dit les octets. `Some(couleur lue du second bloc)` si elle passe, `None` si elle est
/// refusée.
fn fates() -> [Option<Option<&'static str>>; 4] {
    [
        // Un contenu supprimé posé sur la couleur : c'est la retirer.
        Some(None),
        // La seconde valeur, posée sur la couleur.
        Some(Some("blue")),
        Some(None),
        // Une chaîne n'est pas une valeur d'attribut (Ruling R724) : refusée, comme d'un client
        // qui l'enverrait coupée.
        None,
    ]
}

/// Constat C1 de la re-relecture 3 : un élément à cheval sur ce que la page connaît. yrs n'en
/// intégrait que la part au-dessus du plancher, lui donnait pour voisin de gauche l'élément de la
/// page à l'horloge d'avant sans regarder son parent, gardait le parent des octets et prenait la
/// clé du voisin : la part intégrée devenait l'entrée courante de la couleur du premier bloc,
/// avec un voisin de gauche d'un autre parent, et l'ancienne entrée n'était plus tenue par rien
/// (28 octets ; le premier bloc supprimé, le serveur s'arrêtait).
///
/// Ruling R784 : l'élément est coupé avant que yrs le lise. La part neuve ne nomme plus ni parent
/// ni clé : elle est rangée où son origine la met (le second bloc), le premier bloc n'est jamais
/// touché, et ce qu'elle y pose est jugé comme ce qu'un client enverrait tel quel. La page reste
/// saine sous toutes ses formes.
#[test]
fn an_item_astride_what_the_page_knows_is_cut_and_placed_by_its_origin() {
    for ((what, build), fate) in astride_forms().into_iter().zip(fates()) {
        let mut served = Served::new();
        second_block(&mut served);
        let (blocks, state) = (served.page.read_blocks(), served.page.encode_state());
        assert!(background(&served.page, 1).is_some_and(|color| color.contains("red")));
        let hostile = build(&served);
        let result = served.receive(&hostile);
        assert_eq!(result.is_ok(), fate.is_some(), "{what} : {result:?}");
        // Le parent que disent les octets (le premier bloc) n'est jamais cru.
        assert_eq!(
            served.page.read_blocks()[0],
            blocks[0],
            "{what} : le premier bloc a changé"
        );
        match fate {
            Some(color) => {
                let read = background(&served.page, 1);
                assert_eq!(
                    read.is_some(),
                    color.is_some(),
                    "{what} : la couleur du second bloc, {read:?}"
                );
                if let Some(color) = color {
                    assert!(read.unwrap().contains(color), "{what}");
                }
            }
            None => assert_eq!(
                served.page.read_blocks(),
                blocks,
                "{what} : la page a changé"
            ),
        }
        // Ce qu'un vrai client reçoit du serveur (un yrs neuf, qui a l'état d'avant) le mène au
        // même état : ce qui est relayé est ce que la page a intégré.
        assert_sound(&served, what);
        aftermath(&mut served, &[state], what);
    }
}

/// À cheval sur un enfant d'une liste : le texte vide du client 7 000 (son horloge 0), renvoyé
/// comme une chaîne de deux lettres dont le parent nommé est le texte du client 10. Coupée, la
/// lettre neuve a pour origine ce texte vide : une lettre parmi les enfants du paragraphe, ce que
/// la garde refuse (Ruling R497).
#[test]
fn a_string_astride_a_list_child_of_another_parent_is_placed_by_its_origin_and_refused() {
    let mut served = Served::new();
    empty_text(&mut served, 7_000);
    let before = served.page.read_blocks();
    let state = served.page.encode_state();
    let hostile = vec![1, 1, 216, 54, 0, 0x04, 0, 10, 0, 2, b'a', b'b', 0];
    let result = served.receive(&hostile);
    assert!(result.is_err(), "la chaîne est acceptée");
    assert_eq!(served.page.read_blocks(), before);
    assert_sound(&served, "la chaîne refusée");
    aftermath(&mut served, &[state], "la chaîne refusée");
}

/// Ce que la page connaît déjà en entier n'est pas à cheval : yrs l'ignore, la mise à jour passe
/// (un onglet qui renvoie ce que le serveur a déjà). Et ce qui part du plancher passe.
#[test]
fn what_the_page_already_knows_and_what_starts_at_its_floor_pass() {
    let mut served = Served::new();
    // Les lettres « abc » du client 10 (horloges 1 à 3), renvoyées telles quelles.
    let known = vec![1, 1, 10, 1, 0x84, 10, 0, 3, b'a', b'b', b'c', 0];
    served.receive(&known).expect("déjà connu");
    // La suite, à l'horloge 4.
    let next = vec![1, 1, 10, 4, 0x84, 10, 3, 1, b'd', 0];
    served.receive(&next).expect("à partir du plancher");
    assert_sound(&served, "la suite");
    assert!(
        reread(&served.page.encode_state())
            .unwrap()
            .contains("abcd")
    );
}

/// Une chaîne à cheval coupée au milieu d'une paire de substitution : la moitié gardée devient
/// U+FFFD, comme dans Yjs (`ContentString.splice`). Le client 10 a tapé « abc » (horloges 1 à
/// 3) ; il renvoie « c😀 » à l'horloge 3, puis « 😀 » à l'horloge 4, quand la page en connaît 5.
#[test]
fn a_string_cut_in_the_middle_of_a_surrogate_pair_keeps_a_replacement_character() {
    let mut served = Served::new();
    let emoji = "😀".as_bytes();
    let mut first = vec![1, 1, 10, 3, 0x84, 10, 2, 1 + emoji.len() as u8, b'c'];
    first.extend_from_slice(emoji);
    first.push(0);
    served.receive(&first).expect("à cheval sur « c »");
    assert!(
        reread(&served.page.encode_state())
            .unwrap()
            .contains("abc😀")
    );
    // Une lettre de plus : « d », à l'horloge 6.
    let next = vec![1, 1, 10, 6, 0x84, 10, 5, 1, b'd', 0];
    served.receive(&next).expect("la suite");
    // « 😀x » renvoyé à l'horloge 6, quand la page en connaît une unité (« d ») : la paire est
    // coupée en deux, sa seconde moitié seule est neuve.
    let mut astride = vec![1, 1, 10, 6, 0x84, 10, 5, 1 + emoji.len() as u8];
    astride.extend_from_slice(emoji);
    astride.extend_from_slice(&[b'x', 0]);
    let state = served.page.encode_state();
    served.receive(&astride).expect("coupée dans la paire");
    let text = reread(&served.page.encode_state()).unwrap();
    assert!(text.contains("abc😀d\u{FFFD}x"), "{text}");
    assert_sound(&served, "la paire coupée");
    aftermath(&mut served, &[state], "la paire coupée");
}

/// Au journal (une base qui gardait les octets d'un client, avant R130) : l'entrée est coupée de
/// même, la page se charge, le premier bloc n'est pas touché, et la page reste saine. Une chaîne
/// rangée comme valeur d'attribut y est retirée (Ruling R690), au lieu d'être refusée.
#[test]
fn an_item_astride_already_in_the_journal_is_cut_and_the_page_is_loaded() {
    for (what, build) in astride_forms() {
        let mut served = Served::new();
        second_block(&mut served);
        let blocks = served.page.read_blocks();
        let state = served.page.encode_state();
        served.journal.push(build(&served));
        served.page = PageDoc::from_updates(served.journal.iter().map(Vec::as_slice))
            .unwrap_or_else(|e| panic!("{what} : la page ne se charge pas : {e}"));
        assert_eq!(
            served.page.read_blocks()[0],
            blocks[0],
            "{what} : le premier bloc a changé"
        );
        assert_sound(&served, what);
        aftermath(&mut served, &[state], what);
    }
}

/// Du JSON à plusieurs valeurs, à cheval : yrs et Yjs n'en lisent pas le même nombre, aucun
/// client n'en écrit ; refusé avant d'être appliqué, d'un client comme au journal, où qui relit
/// peut écarter l'entrée (Ruling R784).
#[test]
fn json_astride_what_the_page_knows_is_still_refused_before_it_is_applied() {
    let mut served = Served::new();
    second_block(&mut served);
    let state = served.page.encode_state();
    // Deux chaînes JSON du client 7 000 à l'horloge 3, d'origine son horloge 2 (yrs lit une
    // chaîne de plus que le nombre écrit).
    let json = vec![1, 1, 216, 54, 3, 0x82, 216, 54, 2, 1, 1, b'1', 1, b'2', 0];
    let result = served.page.try_client_update(&json);
    assert!(
        matches!(&result, Err(refused) if refused.intact),
        "{:?}",
        result.map(|_| ())
    );
    assert!(served.page.encode_state() == state);
    let page = served.reloaded();
    let error = page.apply_update(&json).expect_err("l'entrée est relue");
    assert!(PageDoc::refused_intact(&error), "{error}");
    assert!(page.encode_state() == state, "la page a changé");
}

/// Constat m1 de la re-relecture 4 (Ruling R786) : au niveau de la page seule, sans la retenue de
/// l'acteur (R131), un trou de la mise à jour laissait un trou chez le client 7 000 ([4, 6)),
/// puis un élément partait de l'horloge 6 (connue, au-delà du trou) et finissait à 8 : à cheval
/// sur la fin de ce que la page connaît, mais au-dessus de son plancher, le début du premier trou
/// (`STATUS_ACCESS_VIOLATION`). Une mise à jour de client qui laisserait un trou est refusée par
/// la page elle-même, avant d'être appliquée.
#[test]
fn a_client_update_that_would_leave_a_hole_is_refused_by_the_page_itself() {
    let mut served = Served::new();
    second_block(&mut served);
    let state = served.page.encode_state();
    let mut hole = vec![1, 2];
    var(&mut hole, 7_000);
    hole.extend([4, 10, 2, 0x84, 10, 3, 1, b'z', 0]);
    let result = served.page.try_client_update(&hole);
    assert!(
        matches!(&result, Err(refused) if refused.intact && refused.error.to_string().contains("trou")),
        "{:?}",
        result.map(|_| ())
    );
    assert!(served.page.encode_state() == state, "la page a changé");
    assert_eq!(served.page.leaves_gap(&hole), Ok(true));
    // La suite de la sonde : l'élément à cheval au-delà du trou, qui n'existe pas.
    let mut astride = vec![1, 1];
    var(&mut astride, 7_000);
    astride.extend([6, 0x21, 0]);
    var(&mut astride, served.server);
    astride.push(3);
    astride.push(15);
    astride.extend_from_slice(b"backgroundColor");
    astride.extend([2, 0]);
    let result = served.receive(&astride);
    assert!(result.is_err(), "au-delà de ce que la page connaît");
    aftermath(&mut served, &[state], "le trou refusé");
}

/// Une page relue d'un journal d'avant R131, qui a gardé un trou chez le client 7 000 (elle
/// connaît [0, 4) et [6, 7)), puis reçoit, d'un client ou du journal, ce qui comble le trou d'un
/// tenant avec ce qui suit : un élément à cheval sur ce que la page connaît **au-delà** de son
/// plancher. yrs le coupait lui-même (la part d'après gardait le parent des octets, la part
/// d'avant un pointeur vers la part jetée). Ruling R785 : la page le coupe avant lui, d'après ce
/// qu'elle connaît au-delà du trou.
#[test]
fn in_a_page_with_a_hole_an_item_astride_what_lies_beyond_is_cut_too() {
    for from_client in [true, false] {
        for named in [false, true] {
            let what = format!("d'un client : {from_client}, parent nommé : {named}");
            let mut served = Served::new();
            second_block(&mut served);
            empty_text(&mut served, 30);
            // Au journal seulement : « z » à l'horloge 6 du client 7 000, d'origine une lettre du
            // premier bloc, derrière un trou de deux horloges.
            let mut hole = vec![1, 2];
            var(&mut hole, 7_000);
            hole.extend([4, 10, 2, 0x84, 10, 3, 1, b'z', 0]);
            served.journal.push(hole);
            served.page = served.reloaded();
            let state = served.page.encode_state();
            let before = served.page.read_blocks();
            // « vwxy » de l'horloge 4 à 8 : « vw » comble le trou, « x » est connu (c'est « z »),
            // « y » est neuf. Sans origine, parent nommé (le texte vide du client 30), ou
            // d'origine ce texte.
            let mut whole = vec![1, 1];
            var(&mut whole, 7_000);
            if named {
                whole.extend([4, 0x04, 0, 30, 0]);
            } else {
                whole.extend([4, 0x84, 10, 3]);
            }
            whole.extend([4, b'v', b'w', b'x', b'y', 0]);
            let result = if from_client {
                served.receive(&whole)
            } else {
                served.journal.push(whole);
                PageDoc::from_updates(served.journal.iter().map(Vec::as_slice))
                    .map(|page| served.page = page)
                    .map_err(|e| e.to_string())
            };
            let text = reread(&served.page.encode_state());
            // Dans le texte vide du client 30, « vw » : une chaîne parmi les lettres d'un texte.
            // Ou, d'origine la lettre « c » comme « z » : « vw » derrière « z » et sa suite « y ».
            assert!(result.is_ok(), "{what} : {result:?}");
            let text = text.unwrap();
            if named {
                assert!(
                    text.contains("abczy") && text.contains("vw"),
                    "{what} : {text}"
                );
            } else {
                assert!(text.contains("abczyvw"), "{what} : {text}");
            }
            assert_eq!(served.page.read_blocks()[1], before[1], "{what}");
            assert_sound(&served, &what);
            // Sans repartir de l'état d'avant : un client qui aurait lui-même un trou n'existe
            // pas (Yjs garde en attente ce qui en laisserait un), et yrs, à sa place, coupe mal
            // une plage ramassée qui dépasse son trou.
            let _ = state;
            aftermath(&mut served, &[], &what);
        }
    }
}
