//! Ce que yrs demande de mémoire pour fondre des éléments voisins (Rulings R874, R876).
//!
//! yrs fond à la validation d'une transaction les blocs qui se suivent. Celui de crates.io
//! (0.28.1) les fondait du dernier au premier, en recopiant à chaque pas tout ce qui était déjà
//! fondu, chaque bloc gardant sa copie jusqu'à la fin de la suite : `n` lettres envoyées une à
//! une lui demandaient une mémoire en `n²` (40 000 lettres, 263 Ko : 1 142 Mio ; 10 000 valeurs,
//! 60 Ko : 1 147 Mio). Le serveur compile un yrs corrigé (`vendor/yrs`), qui recopie chaque
//! contenu une fois.
//!
//! Chaque forme par laquelle un compte obtenait ce carré est ici, à deux tailles, par le chemin
//! d'une vraie page (un message d'un client, une entrée du journal, un état relu), avec un
//! plafond en octets de pic par octet reçu. Aucune garde ne les refuse plus pour cela : c'est le
//! yrs corrigé qui tient le plafond, et chaque test rougit sur le yrs d'origine (aux deux
//! tailles : de 2 à 70 fois le plafond). Les pics ne dépendent pas du poste : ils sont comptés
//! par l'allocateur du binaire, pour le fil du test.
//!
//! Deux formes de plus, que le yrs d'origine fondait déjà sans rien recopier, tiennent un plafond
//! à elles (Ruling R884) : des plages ramassées d'une unité, et des clients d'une plage chacun.
//! Deux octets reçus y font un bloc de yrs : ce sont les formes qui pèsent le plus par octet reçu
//! (jusqu'à 258), au-delà du plafond des autres.
//!
//! **À une montée de yrs**, ces tests disent si le correctif est encore là et suffit.

#[allow(dead_code)]
#[path = "../../mementum-md/tests/common/linear.rs"]
mod linear;

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;
use yrs::{
    ClientID, Doc, GetString, OffsetKind, Options, ReadTxn, Transact, Update, XmlFragment, XmlOut,
    XmlTextPrelim,
};

/// Les éléments d'une forme : dix mille, puis quatre fois plus.
const SIZES: [u32; 2] = [10_000, 40_000];

/// Octets de pic par octet reçu. Un élément d'une lettre pèse six ou sept octets dans une mise à
/// jour et, une fois intégré, un bloc de yrs (un peu moins de deux cents octets) : les formes
/// d'ici mesurent de 60 à 150. Le yrs d'origine en demandait de 500 à 19 000.
const PEAK_PER_BYTE: usize = 256;

const AUTHOR: u64 = 10;
const LETTERS: u64 = 90;

type Id = (u64, u32);

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

fn id(out: &mut Vec<u8>, id: Id) {
    var(out, id.0);
    var(out, u64::from(id.1));
}

/// Une page dont le premier paragraphe porte un texte vide, l'élément `(AUTHOR, 0)`.
fn page() -> PageDoc {
    let (page, created) = PageDoc::new_page();
    let tab = Doc::with_options(Options {
        client_id: ClientID::new(AUTHOR),
        offset_kind: OffsetKind::Utf16,
        ..Options::default()
    });
    let fragment = tab.get_or_insert_xml_fragment(FRAGMENT);
    tab.transact_mut()
        .apply_update(Update::decode_v1(&created).unwrap())
        .unwrap();
    let update = {
        let mut txn = tab.transact_mut();
        let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
            panic!("le groupe");
        };
        let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
            panic!("le bloc");
        };
        let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
            panic!("le paragraphe");
        };
        paragraph.insert(&mut txn, 0, XmlTextPrelim::new(""));
        txn.encode_update_v1()
    };
    page.try_client_update(&update).expect("le texte");
    page
}

/// La longueur du texte du premier paragraphe.
fn text(page: &PageDoc) -> usize {
    let txn = page.yrs_doc().transact();
    let fragment = txn.get_xml_fragment(FRAGMENT).unwrap();
    let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
        panic!("le groupe");
    };
    let Some(XmlOut::Element(container)) = group.get(&txn, 0) else {
        panic!("le bloc");
    };
    let Some(XmlOut::Element(paragraph)) = container.get(&txn, 0) else {
        panic!("le paragraphe");
    };
    let Some(XmlOut::Text(text)) = paragraph.get(&txn, 0) else {
        panic!("le texte");
    };
    text.get_string(&txn).len()
}

/// Un élément de `letters` lettres dans le texte : en tête sans `origin`, sinon après elle.
fn letters(origin: Option<Id>, right: Option<Id>, letters: u32) -> Vec<u8> {
    let mut out = Vec::new();
    match origin {
        None => {
            out.extend_from_slice(&[0x04, 0]);
            id(&mut out, (AUTHOR, 0));
        }
        Some(origin) => {
            out.push(if right.is_some() { 0xC4 } else { 0x84 });
            id(&mut out, origin);
            if let Some(right) = right {
                id(&mut out, right);
            }
        }
    }
    var(&mut out, u64::from(letters));
    out.extend(std::iter::repeat_n(b'x', letters as usize));
    out
}

/// `n` lettres de `client` à partir de son horloge 0, chacune un élément, à la suite de la
/// précédente.
fn chain(client: u64, n: u32) -> Vec<Vec<u8>> {
    (0..n)
        .map(|k| letters((k > 0).then(|| (client, k - 1)), None, 1))
        .collect()
}

/// Une mise à jour : par client, sa première horloge et ses éléments.
fn update(clients: &[(u64, u32, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut out = Vec::new();
    var(&mut out, clients.len() as u64);
    for (client, clock, items) in clients {
        var(&mut out, items.len() as u64);
        var(&mut out, *client);
        var(&mut out, u64::from(*clock));
        out.extend(items.iter().flatten());
    }
    out.push(0);
    out
}

/// Une lettre d'un autre client après chacune des `n` premières lettres de `LETTERS`, sauf la
/// dernière : sans origine droite, comme la frappe concurrente d'un client qui n'aurait rien vu
/// de la suite. D'un client plus grand, yrs la range après tout ce qui suit : l'élément de
/// `LETTERS` est coupé en `n` morceaux, qui restent voisins.
fn saw(n: u32) -> Vec<Vec<u8>> {
    (0..n - 1)
        .map(|k| letters(Some((LETTERS, k)), None, 1))
        .collect()
}

fn whole(n: u32) -> Vec<u8> {
    update(&[(LETTERS, 0, vec![letters(None, None, n)])])
}

/// Octets de pic par octet reçu pour une plage ramassée d'une unité (Ruling R884) : deux octets dans
/// une mise à jour, un bloc entier une fois lue. Mesurés : 259 pour des plages d'un client, 255
/// pour des clients d'une plage chacun.
const PEAK_PER_BYTE_OF_RANGES: usize = 320;

/// Le pic de mémoire de `read`, tenu sous le plafond pour `received` octets reçus.
fn within<R>(what: &str, n: u32, received: usize, read: impl FnOnce() -> R) -> R {
    within_bound(what, n, received, PEAK_PER_BYTE, read)
}

/// [`within`], sous `per_byte` octets de pic par octet reçu.
fn within_bound<R>(
    what: &str,
    n: u32,
    received: usize,
    per_byte: usize,
    read: impl FnOnce() -> R,
) -> R {
    let mut answer = None;
    let cost = linear::once(|| answer = Some(read()));
    let bound = per_byte * received;
    println!(
        "MESURE {what}, {n} éléments : {received} octets reçus, pic de {} octets ({} par octet)",
        cost.peak,
        cost.peak / received.max(1)
    );
    assert!(
        cost.peak <= bound,
        "{what}, {n} éléments : pic de {} octets pour {received} octets reçus, plus de {per_byte} par octet",
        cost.peak
    );
    answer.unwrap()
}

/// Des lettres envoyées une à une dans un seul message : rien ne les refuse plus, et yrs les
/// fond en un élément.
#[test]
fn letters_chained_in_one_message() {
    for n in SIZES {
        let page = page();
        let sent = update(&[(LETTERS, 0, chain(LETTERS, n))]);
        let applied = within("des lettres en un message", n, sent.len(), || {
            page.try_client_update(&sent)
        })
        .expect("acceptée");
        assert_eq!(text(&page), n as usize);
        // Le journal garde la transaction telle qu'elle était avant d'être fondue (Ruling R491) :
        // la relire coûte ce qu'a coûté le message.
        assert!(applied.read.len() > 5 * n as usize);
    }
}

/// Les mêmes lettres dans une entrée du journal, ou dans un état gardé (un instantané, une
/// version) : relues par le serveur, et par un yrs sans garde.
#[test]
fn letters_chained_in_the_journal_or_in_a_state() {
    for n in SIZES {
        let before = page().encode_state();
        let entry = update(&[(LETTERS, 0, chain(LETTERS, n))]);
        let received = before.len() + entry.len();
        let page = within("des lettres au journal", n, received, || {
            PageDoc::from_updates([before.as_slice(), entry.as_slice()])
        })
        .expect("la page se charge");
        assert_eq!(text(&page), n as usize);
        let bare = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        within(
            "des lettres lues par un yrs sans garde",
            n,
            received,
            || {
                for bytes in [&before, &entry] {
                    bare.transact_mut()
                        .apply_update(Update::decode_v1(bytes).unwrap())
                        .unwrap();
                }
            },
        );
    }
}

/// La scie (constat C1 de la re-relecture 11) : un élément de `n` lettres et, dans la même mise
/// à jour, une lettre d'un client plus grand après chacune. yrs coupe l'élément à chaque lettre,
/// puis refond ses morceaux. Acceptée : c'est ce qu'écrivent, en petit, deux personnes qui tapent
/// au même endroit. Ce que le journal en garde se relit au même prix.
#[test]
fn the_saw_in_one_update() {
    for n in SIZES {
        let page = page();
        let before = page.encode_state();
        let sent = update(&[(LETTERS, 0, vec![letters(None, None, n)]), (95, 0, saw(n))]);
        let applied =
            within("la scie", n, sent.len(), || page.try_client_update(&sent)).expect("acceptée");
        assert_eq!(text(&page), 2 * n as usize - 1);
        let received = before.len() + applied.read.len();
        let again = within("la scie, relue du journal", n, received, || {
            PageDoc::from_updates([before.as_slice(), applied.read.as_slice()])
        })
        .expect("la page se charge");
        assert_eq!(text(&again), 2 * n as usize - 1);
    }
}

/// La scie sur un élément que la page a déjà : la mise à jour ne porte que les lettres du client
/// plus grand, et une lettre de plus du client de l'élément, qui fait refondre ses morceaux.
#[test]
fn the_saw_on_an_item_of_the_page() {
    for n in SIZES {
        let page = page();
        page.try_client_update(&whole(n)).expect("l'élément");
        let before = page.encode_state();
        let sent = update(&[
            (LETTERS, n, vec![letters(Some((LETTERS, n - 1)), None, 1)]),
            (95, 0, saw(n)),
        ]);
        let applied = within("la scie sur la page", n, sent.len(), || {
            page.try_client_update(&sent)
        })
        .expect("acceptée");
        assert_eq!(text(&page), 2 * n as usize);
        let received = before.len() + applied.read.len();
        within("la scie sur la page, relue du journal", n, received, || {
            PageDoc::from_updates([before.as_slice(), applied.read.as_slice()])
        })
        .expect("la page se charge");
    }
}

/// La scie différée : sans lettre de plus du client de l'élément, rien n'est refondu à
/// l'arrivée, et la page garde `n` morceaux d'une lettre. **L'état qu'elle encode** (l'instantané
/// d'une compaction, une version de l'historique, ce que reçoit un onglet) est alors une suite de
/// `n` lettres, que tout lecteur fond d'un coup : le serveur qui recharge la page, celui qui sert
/// une version, un yrs sans garde.
#[test]
fn the_state_of_a_sawn_page_read_again() {
    for n in SIZES {
        let page = page();
        page.try_client_update(&whole(n)).expect("l'élément");
        let sent = update(&[(95, 0, saw(n))]);
        within("la scie différée, à l'arrivée", n, sent.len(), || {
            page.try_client_update(&sent)
        })
        .expect("acceptée");
        let state = page.encode_state();
        let again = within(
            "l'état d'une page sciée, rechargé",
            n,
            state.len(),
            || PageDoc::from_updates([state.as_slice()]),
        )
        .expect("la page se charge");
        assert_eq!(text(&again), 2 * n as usize - 1);
        let served = within("l'état d'une page sciée, servi", n, state.len(), || {
            PageDoc::served_state(&state)
        });
        assert!(served.is_some());
        let bare = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        within(
            "l'état d'une page sciée, lu par un yrs sans garde",
            n,
            state.len(),
            || {
                bare.transact_mut()
                    .apply_update(Update::decode_v1(&state).unwrap())
                    .unwrap();
            },
        );
    }
}

/// Des valeurs enchaînées (constat C2) : marquées « entrée de map », la première à la suite d'un
/// enfant de liste. yrs en fait des enfants de liste et les fond ; la garde ne peut le refuser
/// qu'après l'application. Refusée, mais payée : au prix de ce qu'elle pèse.
#[test]
fn values_chained_after_a_list_child() {
    for n in SIZES {
        for after_a_letter in [false, true] {
            let mut items = Vec::new();
            if after_a_letter {
                items.push(letters(None, None, 1));
            }
            let first = items.len() as u32;
            for k in first..n + first {
                let mut item = vec![0xA8];
                id(
                    &mut item,
                    if k == 0 {
                        (AUTHOR, 0)
                    } else {
                        (LETTERS, k - 1)
                    },
                );
                // Une valeur : `true`.
                item.extend_from_slice(&[1, 120]);
                items.push(item);
            }
            let sent = update(&[(LETTERS, 0, items)]);
            let page = page();
            let refused = within("des valeurs enchaînées", n, sent.len(), || {
                page.try_client_update(&sent)
            })
            .expect_err("aucun client n'écrit cela");
            assert!(!refused.intact, "{}", refused.error);
        }
    }
}

/// yrs s'arrête en erreur au milieu d'une mise à jour (constat C3) : un élément dont le parent,
/// nommé par son identifiant, est une lettre. Ce qu'il avait intégré avant est validé, donc
/// fondu : les lettres, sans les éléments qui les auraient séparées.
#[test]
fn letters_before_an_item_yrs_stops_on() {
    for n in SIZES {
        for apart in [false, true] {
            let mut bad = vec![0x04, 0];
            id(&mut bad, (LETTERS, 0));
            bad.extend_from_slice(&[1, b'p']);
            let separators = (1..n).map(|k| letters(Some((LETTERS, k - 1)), Some((LETTERS, k)), 1));
            let sent = if apart {
                update(&[
                    (LETTERS, 0, chain(LETTERS, n)),
                    (70, 0, vec![bad]),
                    (50, 0, separators.collect()),
                ])
            } else {
                update(&[
                    (LETTERS, 0, chain(LETTERS, n)),
                    (50, 0, std::iter::once(bad).chain(separators).collect()),
                ])
            };
            let page = page();
            let refused = within("des lettres avant un arrêt de yrs", n, sent.len(), || {
                page.try_client_update(&sent)
            })
            .expect_err("yrs refuse ce parent");
            assert!(!refused.intact, "{}", refused.error);
        }
    }
}

/// Des éléments que la page connaît déjà, entre les lettres (constat C1 de la re-relecture 10) :
/// yrs les ignore, ils ne séparent rien, et les lettres se fondent.
#[test]
fn letters_between_items_the_page_already_has() {
    for n in SIZES {
        let page = page();
        page.try_client_update(&update(&[(30, 0, vec![letters(None, None, n)])]))
            .expect("le client 30 écrit");
        let known = (1..n)
            .map(|k| letters(Some((LETTERS, k - 1)), Some((LETTERS, k)), 1))
            .collect();
        let before = page.encode_state();
        let sent = update(&[(30, 0, known), (LETTERS, 0, chain(LETTERS, n))]);
        let applied = within(
            "des lettres entre des éléments connus",
            n,
            sent.len(),
            || page.try_client_update(&sent),
        )
        .expect("acceptée");
        assert_eq!(text(&page), 2 * n as usize);
        let received = before.len() + applied.read.len();
        within(
            "des lettres entre des éléments connus, relues du journal",
            n,
            received,
            || PageDoc::from_updates([before.as_slice(), applied.read.as_slice()]),
        )
        .expect("la page se charge");
    }
}

/// Des contenus JSON enchaînés (l'ancien format de Yjs), lus par yrs seul : la garde les refuse
/// d'un client et les supprime d'un journal, mais yrs les fond comme des lettres. Le yrs d'origine
/// y demandait 1,1 Gio pour 10 000 éléments.
#[test]
fn json_contents_chained_read_by_yrs_alone() {
    for n in SIZES {
        let items: Vec<Vec<u8>> = (0..n)
            .map(|k| {
                let mut item = match k {
                    0 => vec![0x02, 1, 1, b't'],
                    _ => {
                        let mut item = vec![0x82];
                        id(&mut item, (LETTERS, k - 1));
                        item
                    }
                };
                // yrs lit une chaîne de plus que le compte écrit.
                item.extend_from_slice(&[0, 4, b't', b'r', b'u', b'e']);
                item
            })
            .collect();
        let sent = update(&[(LETTERS, 0, items)]);
        let bare = Doc::with_options(Options {
            offset_kind: OffsetKind::Utf16,
            ..Options::default()
        });
        within("des contenus JSON enchaînés", n, sent.len(), || {
            bare.transact_mut()
                .apply_update(Update::decode_v1(&sent).unwrap())
                .unwrap();
        });
    }
}

/// Des plages ramassées d'une unité, à la suite (constat M2 de la re-relecture 12) : yrs les fond
/// en une plage, sans rien recopier, mais chacune pèse deux octets dans la mise à jour et un bloc
/// une fois lue. C'est la forme qui pèse le plus par octet reçu ; d'où son plafond à elle.
#[test]
fn ranges_of_one_unit() {
    for n in SIZES {
        let page = page();
        let before = page.encode_state();
        let sent = update(&[(LETTERS, 0, (0..n).map(|_| vec![0, 1]).collect())]);
        let applied = within_bound(
            "des plages ramassées d'une unité",
            n,
            sent.len(),
            PEAK_PER_BYTE_OF_RANGES,
            || page.try_client_update(&sent),
        )
        .expect("acceptée");
        let received = before.len() + applied.read.len();
        within_bound(
            "des plages ramassées d'une unité, relues du journal",
            n,
            received,
            PEAK_PER_BYTE_OF_RANGES,
            || PageDoc::from_updates([before.as_slice(), applied.read.as_slice()]),
        )
        .expect("la page se charge");
    }
}

/// Autant de clients que de plages ramassées d'une unité : rien ne se fond, et chaque client pèse
/// sa liste de blocs.
#[test]
fn clients_of_one_range_each() {
    for n in SIZES {
        let page = page();
        let before = page.encode_state();
        let clients: Vec<(u64, u32, Vec<Vec<u8>>)> = (0..u64::from(n))
            .map(|k| (1_000 + k, 0, vec![vec![0, 1]]))
            .collect();
        let sent = update(&clients);
        let applied = within_bound(
            "des clients d'une plage ramassée chacun",
            n,
            sent.len(),
            PEAK_PER_BYTE_OF_RANGES,
            || page.try_client_update(&sent),
        )
        .expect("acceptée");
        let received = before.len() + applied.read.len();
        within_bound(
            "des clients d'une plage ramassée chacun, relus du journal",
            n,
            received,
            PEAK_PER_BYTE_OF_RANGES,
            || PageDoc::from_updates([before.as_slice(), applied.read.as_slice()]),
        )
        .expect("la page se charge");
    }
}

/// Les fichiers `.rs` sous `dir`, à toute profondeur.
fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Aucun document du serveur n'est créé sans ramasse-miettes (Ruling R882). yrs fond alors aussi
/// les contenus supprimés que nomme un jeu de suppressions (`squash_left_range_compaction`, que le
/// correctif ne touche pas) du dernier au premier, en recopiant à chaque pas ce qui est déjà
/// fondu : 20 000 lettres supprimées par leur propre mise à jour demandent 288 Mio, avec le yrs
/// d'origine comme avec le corrigé. Le ramasse-miettes en retire le texte avant, et rien n'est
/// recopié. Le mot ne paraît donc dans aucune source des crates de l'atelier (`src/`, tests
/// exclus).
#[test]
fn no_document_of_the_server_skips_the_garbage_collector() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).unwrap() {
        let src = entry.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(files.len() > 60, "{} sources lues", files.len());
    let found: Vec<_> = files
        .iter()
        .filter(|path| std::fs::read_to_string(path).unwrap().contains("skip_gc"))
        .collect();
    assert!(found.is_empty(), "`skip_gc` dans {found:?}");
}

/// Ce que `vendor/yrs` corrige, tenu sur le yrs de crates.io.
mod known_limits {
    use super::*;
    use yrs_upstream::updates::decoder::Decode as _;
    use yrs_upstream::{Doc as TheirDoc, Transact as _, Update as TheirUpdate};

    /// Le yrs de crates.io (0.28.1, d'origine) fond une suite du dernier élément au premier, et
    /// chaque élément garde sa copie de ce qui est déjà fondu : dix mille lettres d'un message de
    /// 60 Ko lui demandent 73 Mio, plus de mille octets par octet reçu. **Rouge le jour où une
    /// version publiée de yrs ne le fait plus** : le serveur peut alors la prendre telle quelle,
    /// et `vendor/yrs` partir (Ruling R874).
    #[test]
    fn the_yrs_of_crates_io_copies_what_it_has_squashed_for_each_letter() {
        let n = 10_000u32;
        let items: Vec<Vec<u8>> = (0..n)
            .map(|k| match k {
                // Dans le texte racine `t`.
                0 => vec![0x04, 1, 1, b't', 1, b'x'],
                _ => letters(Some((LETTERS, k - 1)), None, 1),
            })
            .collect();
        let sent = update(&[(LETTERS, 0, items)]);
        let doc = TheirDoc::new();
        let cost = linear::once(|| {
            doc.transact_mut()
                .apply_update(TheirUpdate::decode_v1(&sent).unwrap())
                .unwrap();
        });
        println!(
            "MESURE le yrs de crates.io, {n} lettres : {} octets reçus, pic de {} octets",
            sent.len(),
            cost.peak
        );
        assert!(
            cost.peak > 4 * PEAK_PER_BYTE * sent.len(),
            "pic de {} octets pour {} octets reçus : la limite est levée en amont",
            cost.peak,
            sent.len()
        );
    }
}
