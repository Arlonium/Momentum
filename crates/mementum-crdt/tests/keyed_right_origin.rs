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
