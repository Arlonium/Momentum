//! Ce que le vrai Yjs écrit passe la garde (Rulings R723, R784) : les mises à jour de
//! `apps/web/src/editor/yjsUpdates.test.ts` (deux clients, dans les deux ordres d'identifiants :
//! écritures concurrentes d'un attribut, annuler et rétablir, frappe au milieu d'un texte,
//! session hors ligne ; dans l'ordre, l'autre d'abord, en étapes 2 depuis l'état de base),
//! gardées dans `tests/fixtures/yjs_updates/scenarios.json`, sont données à une vraie page comme
//! celles d'un client. Aucune n'est refusée : Yjs n'envoie pas d'entrée de map à origine droite.
//! (Un élément à cheval sur ce que la page connaît, qu'une frappe arrivée en retard fait écrire
//! à Yjs, est coupé : `tests/astride.rs`.)
//!
//! `EXPORT_FIXTURES=1 cargo test -p mementum-crdt --test yjs_updates` réécrit `base.json` (l'état
//! de la page de départ ; l'identifiant du client du serveur y est tiré au hasard), puis Vitest
//! réécrit les scénarios (voir son en-tête).
#![allow(dead_code)]

use mementum_crdt::{FRAGMENT, PageDoc};
use yrs::updates::decoder::Decode;

#[path = "common/live.rs"]
mod live;
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

/// La page et son journal se relisent par un client neuf, et disent la même chose ; et la page
/// relue de son propre état est la page vivante (Ruling R825).
fn assert_sound(served: &Served, what: &str) {
    if let Some(why) = live::reloaded_from_state_differs(&served.page) {
        panic!("{what} : {why}");
    }
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

fn dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/yjs_updates")
}

/// Écrit, sur demande, l'état d'une page servie (un paragraphe « abc »), pour Vitest.
#[test]
fn the_base_is_written_on_demand() {
    if std::env::var_os("EXPORT_FIXTURES").is_none() {
        return;
    }
    let served = Served::new();
    let state = served.page.encode_state();
    std::fs::create_dir_all(dir()).unwrap();
    let mut json = serde_json::to_string(&state).unwrap();
    json.push('\n');
    std::fs::write(dir().join("base.json"), json).unwrap();
}

/// Rejoue ce que le vrai Yjs a écrit (scénarios de `yjsUpdates.test.ts`) : chaque mise à jour
/// est donnée à la page comme celle d'un client, et doit passer.
#[test]
fn what_yjs_writes_passes_the_guard() {
    let base: Vec<u8> =
        serde_json::from_str(&std::fs::read_to_string(dir().join("base.json")).unwrap()).unwrap();
    let scenarios: Vec<(String, Vec<Vec<u8>>)> =
        serde_json::from_str(&std::fs::read_to_string(dir().join("scenarios.json")).unwrap())
            .unwrap();
    assert!(
        scenarios.len() >= 8,
        "peu de scénarios : {}",
        scenarios.len()
    );
    let mut refused = Vec::new();
    let mut steps = 0;
    for (name, updates) in scenarios {
        let page = PageDoc::from_updates([base.as_slice()]).expect("la base");
        let mut journal = vec![base.clone()];
        for (n, update) in updates.iter().enumerate() {
            steps += 1;
            match page.apply_client_update(update) {
                Ok(applied) => {
                    journal.push(applied.read);
                    journal.extend(applied.carried);
                    if let Some(why) = live::reloaded_from_state_differs(&page) {
                        refused.push(format!("{name}, étape {n} : {why}"));
                    }
                }
                Err(error) => {
                    refused.push(format!("{name}, étape {n} : {error} ; {update:?}"));
                    break;
                }
            }
        }
        if let Ok(reloaded) = PageDoc::from_updates(journal.iter().map(Vec::as_slice)) {
            if reloaded.read_blocks() != page.read_blocks() {
                refused.push(format!(
                    "{name} : la page rechargée n'est pas la page vivante"
                ));
            }
        } else {
            refused.push(format!("{name} : le journal ne se relit pas"));
        }
    }
    assert!(steps >= 150, "peu de mises à jour rejouées : {steps}");
    assert!(refused.is_empty(), "{refused:#?}");
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|n| u8::from_str_radix(&text[2 * n..2 * n + 2], 16).expect("de l'hexadécimal"))
        .collect()
}

/// La grammaire d'un client (tour 4, Rulings R801 à R806) contre le vrai Yjs : les scènes tirées
/// au hasard par `apps/web/src/editor/yjsAtRandom.test.ts` (deux ou trois onglets, annulations,
/// hors ligne, étapes 2 depuis un état ancien, mises à jour fusionnées ou redites), données à une
/// vraie page comme celles d'un client. **Aucune n'est refusée**, la page vivante se relit, et
/// son journal relu la redonne. Les mêmes octets, relus comme un vieux journal (une base d'avant
/// R130 gardait les octets reçus), donnent la même page.
///
/// `random-large.json` (hors du dépôt, écrit par `MEMENTUM_FUZZ_SEEDS=6000` côté Vitest) est lu
/// à la place de `random.json` s'il est là.
#[test]
fn what_yjs_writes_at_random_follows_the_grammar() {
    let base: Vec<u8> =
        serde_json::from_str(&std::fs::read_to_string(dir().join("base.json")).unwrap()).unwrap();
    let large = dir().join("random-large.json");
    let file = if large.exists() {
        large
    } else {
        dir().join("random.json")
    };
    let scenes: Vec<Vec<String>> =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(scenes.len() >= 100, "peu de scènes : {}", scenes.len());
    let mut refused = Vec::new();
    // Ce que la page relaie de chaque scène (son journal, la base exceptée), pour le rejouer
    // dans un vrai Yjs (`yjsAtRandom.test.ts`) ; `None` quand le serveur y a écrit lui-même (un
    // report, un dédoublonnage : sous un identifiant de client tiré au hasard).
    let mut relayed: Vec<Option<Vec<String>>> = Vec::new();
    let (mut updates, mut wrote, mut compared) = (0, 0, 0);
    for (n, scene) in scenes.iter().enumerate() {
        let what = format!("scène {}", n + 1);
        let mut served = Served {
            page: PageDoc::from_updates([base.as_slice()]).expect("la base"),
            journal: vec![base.clone()],
            server: 0,
        };
        let stored = PageDoc::from_updates([base.as_slice()]).expect("la base");
        let mut plain = true;
        for (step, update) in scene.iter().enumerate() {
            let update = unhex(update);
            updates += 1;
            let before = served.journal.len();
            match served.receive(&update) {
                Ok(()) => {
                    // Ruling R825 : après chaque mise à jour acceptée, la page relue de son propre
                    // état est la page vivante (ce que Yjs écrit n'est jamais rangé selon le
                    // découpage des blocs).
                    if let Some(why) = live::reloaded_from_state_differs(&served.page) {
                        refused.push(format!("{what}, mise à jour {step} : {why}"));
                    }
                    wrote += usize::from(served.journal[before] != [0, 0]);
                    // Un report (un bloc recréé par une annulation) ou un dédoublonnage : une
                    // écriture du serveur, que le vieux journal n'a pas.
                    plain &= served.journal.len() == before + 1;
                }
                Err(error) => refused.push(format!("{what}, mise à jour {step} : {error}")),
            }
            if let Err(error) = stored.apply_update(&update) {
                refused.push(format!("{what}, entrée {step} du vieux journal : {error}"));
            }
        }
        if refused.is_empty() {
            assert_sound(&served, &what);
            compared += usize::from(plain);
        }
        relayed.push(plain.then(|| {
            served.journal[1..]
                .iter()
                .map(|entry| entry.iter().map(|byte| format!("{byte:02x}")).collect())
                .collect()
        }));
        if refused.is_empty() && plain {
            assert_eq!(
                stored.read_blocks(),
                served.page.read_blocks(),
                "{what} : les mêmes octets relus comme un vieux journal"
            );
        }
    }
    println!(
        "ESSAI Yjs : {} scènes, {updates} mises à jour dont {wrote} ont écrit, {} refus",
        scenes.len(),
        refused.len()
    );
    assert!(
        refused.is_empty(),
        "{} refus de ce que Yjs écrit ; les premiers : {:#?}",
        refused.len(),
        refused.iter().take(5).collect::<Vec<_>>()
    );
    assert!(wrote > updates / 2, "peu de mises à jour ont écrit");
    {
        let large = file.ends_with("random-large.json");
        let path = dir().join(if large {
            "random-relayed-large.json"
        } else {
            "random-relayed.json"
        });
        let mut written = serde_json::to_string(&relayed).unwrap();
        written.push(char::from(10));
        // L'essai élargi est hors du dépôt : toujours récrit.
        if large || std::env::var_os("EXPORT_FIXTURES").is_some() {
            std::fs::write(&path, &written).unwrap();
        } else {
            assert!(
                std::fs::read_to_string(&path).is_ok_and(|kept| kept == written),
                "{} n'est plus ce que la page relaie (EXPORT_FIXTURES=1 le réécrit)",
                path.display()
            );
        }
    }
    assert!(
        compared > scenes.len() / 2,
        "peu de scènes comparées au vieux journal : {compared}"
    );
}

/// Les scènes d'un rétablissement (`redo.json`, `redo-blocks.json`, écrits par
/// `yjsAtRandom.test.ts` : la scène, ses mises à jour, les rangs de celles qui portent la forme),
/// données à une vraie page comme celles d'un client : **toutes acceptées**, la page vivante se
/// relit, son journal la redonne, et la page relue de son état est la page vivante. Ce qu'elle
/// relaie est écrit dans `relayed_name`, que le second test de `yjsAtRandom.test.ts` rejoue dans un
/// vrai Yjs.
fn redo_scenes_are_accepted(name: &str, relayed_name: &str, form: &str) {
    let base: Vec<u8> =
        serde_json::from_str(&std::fs::read_to_string(dir().join("base.json")).unwrap()).unwrap();
    let scenes: Vec<(u64, Vec<String>, Vec<usize>)> =
        serde_json::from_str(&std::fs::read_to_string(dir().join(name)).unwrap()).unwrap();
    assert!(!scenes.is_empty());
    let mut relayed: Vec<Option<Vec<String>>> = Vec::new();
    for (seed, updates, ranks) in &scenes {
        let what = format!("scène {seed}");
        assert!(!ranks.is_empty(), "{what} : aucune forme {form}");
        let mut served = Served {
            page: PageDoc::from_updates([base.as_slice()]).expect("la base"),
            journal: vec![base.clone()],
            server: 0,
        };
        let mut plain = true;
        for (step, update) in updates.iter().enumerate() {
            let before = served.journal.len();
            let received = served.receive(&unhex(update));
            assert!(
                received.is_ok(),
                "{what}, mise à jour {step}{} : {received:?}",
                if ranks.contains(&step) {
                    format!(" ({form})")
                } else {
                    String::new()
                }
            );
            if let Some(why) = live::reloaded_from_state_differs(&served.page) {
                panic!("{what}, mise à jour {step} : {why}");
            }
            plain &= served.journal.len() == before + 1;
        }
        assert_sound(&served, &what);
        relayed.push(plain.then(|| {
            served.journal[1..]
                .iter()
                .map(|entry| entry.iter().map(|byte| format!("{byte:02x}")).collect())
                .collect()
        }));
    }
    let path = dir().join(relayed_name);
    let mut written = serde_json::to_string(&relayed).unwrap();
    written.push(char::from(10));
    if std::env::var_os("EXPORT_FIXTURES").is_some() {
        std::fs::write(&path, &written).unwrap();
    } else {
        assert!(
            std::fs::read_to_string(&path).is_ok_and(|kept| kept == written),
            "{} n'est plus ce que la page relaie (EXPORT_FIXTURES=1 le réécrit)",
            path.display()
        );
    }
}

/// N2 (Ruling R802) contre le vrai Yjs (constat I-6.4 de la re-relecture 6) : les scènes où un
/// onglet envoie un élément dont l'origine droite est son origine, que `redoItem` écrit quand il
/// retrouve le même voisin des deux côtés (`redo.json`). Elles sont **acceptées** (la page récrit
/// l'élément, sans origine droite ; le filet L1 ne refuse rien), la page vivante se relit et son
/// journal la redonne ; ce qu'elle relaie (`redo-relayed.json`) ramène un vrai Yjs à la page du
/// serveur Yjs (second test de `yjsAtRandom.test.ts`).
#[test]
fn what_yjs_writes_on_a_redo_with_the_same_neighbour_on_both_sides_is_accepted() {
    redo_scenes_are_accepted("redo.json", "redo-relayed.json", "N2");
}

/// N3 (Ruling R825) contre le vrai Yjs : les scènes où `redoItem` suit les deux voisins d'un
/// élément jusqu'au même élément X de plusieurs unités, dont la première a pour origine l'unité
/// d'avant (`redo-blocks.json`) : il écrit l'origine `X.lastId` et l'origine droite `X.id`, que Yjs
/// comme yrs rangent selon le découpage de leurs blocs. Elles sont **acceptées** (la page récrit
/// l'élément sans son origine droite), et la page relue de son état est la page vivante après
/// chaque mise à jour.
#[test]
fn what_yjs_writes_on_a_redo_into_one_block_is_accepted() {
    redo_scenes_are_accepted("redo-blocks.json", "redo-blocks-relayed.json", "N3");
}
