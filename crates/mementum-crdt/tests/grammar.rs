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
#[test]
fn a_keyed_content_that_is_not_a_value_is_refused_before_it_is_applied() {
    let served = Served::new();
    let before = served.page.encode_state();
    let mut u = vec![1, 1];
    var(&mut u, 7_000);
    u.extend_from_slice(&[0, 0x24, 0]);
    var(&mut u, served.server);
    var(&mut u, PARAGRAPH);
    string(&mut u, "backgroundColor");
    string(&mut u, "red");
    u.push(0);
    let refused = served.page.try_client_update(&u).expect_err("accepté");
    assert!(refused.intact, "{refused:?}");
    assert!(served.page.encode_state() == before);
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
