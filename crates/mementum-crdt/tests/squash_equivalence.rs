//! Le yrs du serveur est celui de crates.io, à la mémoire près (Rulings R874, R875).
//!
//! `vendor/yrs` porte un correctif : une suite de blocs voisins se fond du premier au dernier, en
//! recopiant chaque contenu une fois, au lieu du dernier au premier en recopiant à chaque pas tout
//! ce qui est déjà fondu (`ClientBlockList::squash_left`). **Rien de ce que yrs produit ne doit en
//! changer** : ni les blocs gardés, ni leurs voisins, ni ce qu'il encode. Ce binaire mène le yrs
//! corrigé et celui d'origine (la crate `yrs` de crates.io, dépendance de test sous le nom
//! `yrs-upstream`) par le même code, et compare tout ce qu'on voit d'eux (`common/one_yrs.rs`) :
//!
//! - des scènes tirées au hasard par l'API de yrs (frappe, listes, maps, liens, annulations,
//!   transactions reçues avant d'être fondues, échanges d'états, avec et sans ramasse-miettes) ;
//! - des mises à jour faites à la main, une par condition que yrs pose à une fusion, de un à
//!   trente-trois éléments : chacune a sa forme qui se fond et sa forme qui ne se fond pas.
//!
//! Ce que les pages donnent à yrs (essais hostiles, vrai Yjs, BlockNote, déplacements) est
//! comparé dans les suites qui le produisent, par `common/twins.rs`.
//!
//! `MEMENTUM_FUZZ_SEEDS` règle le nombre de scènes tirées, `MEMENTUM_FUZZ_FROM` la première.
//!
//! **À une montée de yrs**, c'est ce binaire qui dit si le correctif porté est juste, avec les
//! pics de `squash_memory.rs` ; le témoin est alors la nouvelle version de crates.io.

#[path = "common/twins.rs"]
mod twins;

use std::path::{Path, PathBuf};

use mementum_crdt::PageDoc;
use twins::{Compared, difference, fixed, replayed_alike, upstream, watching};

fn number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

const STEPS: usize = 70;

/// Le premier pas où deux passages d'une scène ne se ressemblent plus.
fn parting(
    one: &[(String, Vec<fixed::Seen>)],
    other: &[(String, Vec<fixed::Seen>)],
) -> Option<(usize, String)> {
    for (step, ((what, docs), (their_what, their_docs))) in one.iter().zip(other).enumerate() {
        if what != their_what {
            return Some((step, format!("« {what} » au lieu de « {their_what} »")));
        }
        for (doc, (seen, their_seen)) in docs.iter().zip(their_docs).enumerate() {
            if let Some(why) = difference(seen, their_seen) {
                return Some((step, format!("({what}), document {doc} : {why}")));
            }
        }
    }
    None
}

/// Les passages du témoin, pour savoir jusqu'où il refait la même scène.
const WITNESSES: usize = 4;
/// Ce qu'on redonne au yrs corrigé avant de le dire différent du témoin.
const TRIES: usize = 48;

/// Aucun des deux yrs ne refait toujours deux fois la même scène : en annulant, yrs rétablit les
/// éléments d'un pas dans l'ordre d'une table de hachage (`UndoManager::try_process`), et il fond
/// les plages ramassées voisines dans l'ordre d'une autre (`TransactionMut::delete`). Le témoin
/// est donc mené [`WITNESSES`] fois, et une scène n'est comparée que jusqu'au pas où il se sépare
/// de lui-même ; s'il a tiré quatre fois le même ordre, le yrs corrigé, qui tire le sien, est
/// mené jusqu'à [`TRIES`] fois : ce que le correctif changerait différerait à chaque passage.
#[test]
fn scenes_at_random_by_the_api_are_alike() {
    let first = number("MEMENTUM_FUZZ_FROM", 1);
    let seeds = number("MEMENTUM_FUZZ_SEEDS", 250);
    let (mut received, mut undone, mut redone, mut linked) = (0u64, 0u64, 0u64, 0u64);
    let (mut compared, mut unsteady, mut retried) = (0u64, 0u64, 0u64);
    for seed in first..first + seeds {
        let theirs = upstream::scene(seed, STEPS);
        let steady = (1..WITNESSES)
            .filter_map(|_| parting(&theirs, &upstream::scene(seed, STEPS)))
            .map(|(step, _)| step)
            .min()
            .unwrap_or(STEPS);
        unsteady += u64::from(steady < STEPS);
        let mut ours = fixed::scene(seed, STEPS);
        let mut tries = 1;
        while let Some((step, why)) = parting(&ours[..steady], &theirs[..steady]) {
            assert!(tries < TRIES, "graine {seed}, pas {step} {why}");
            tries += 1;
            ours = fixed::scene(seed, STEPS);
        }
        retried += u64::from(tries > 1);
        compared += steady as u64;
        for (what, docs) in &ours[..steady] {
            received += u64::from(what == "reçu : appliquée");
            undone += u64::from(what == "annuler : true");
            redone += u64::from(what == "rétablir : true");
            linked += docs.iter().filter(|seen| seen.3.contains("lien[")).count() as u64;
        }
    }
    println!(
        "ESSAI des deux yrs : {seeds} scènes, {compared} pas comparés ({unsteady} scènes où le \
         témoin se sépare de lui-même, {retried} où le yrs corrigé a été mené plus d'une fois) ; \
         {received} transactions reçues, {undone} annulations, {redone} rétablissements, {linked} \
         vues d'un document à lien"
    );
    // Le test ne passe pas à vide.
    assert!(compared > seeds * STEPS as u64 * 9 / 10, "{compared} pas");
    assert!(received > seeds && undone > seeds && redone > seeds / 4 && linked > seeds);
}

// Des mises à jour faites à la main.

const WRITER: u64 = 90;

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

type Id = (u64, u32);

/// Un élément. `kind` : 1 supprimé, 2 JSON, 4 lettres, 5 objet posé dans le texte, 6 marque, 8
/// valeurs. Sans origine ni origine droite, il est posé dans le texte racine `t` (sous la clé `k`
/// avec `keyed`).
fn item(kind: u8, origin: Option<Id>, right: Option<Id>, keyed: bool, units: u32) -> Vec<u8> {
    let mut out = vec![
        kind | if origin.is_some() { 0x80 } else { 0 }
            | if right.is_some() { 0x40 } else { 0 }
            | if keyed { 0x20 } else { 0 },
    ];
    for id in origin.into_iter().chain(right) {
        var(&mut out, id.0);
        var(&mut out, u64::from(id.1));
    }
    if origin.is_none() && right.is_none() {
        out.extend_from_slice(&[1, 1, b't']);
        if keyed {
            out.extend_from_slice(&[1, b'k']);
        }
    }
    match kind {
        1 => var(&mut out, u64::from(units)),
        // yrs lit une chaîne de plus que le compte écrit.
        2 => {
            var(&mut out, u64::from(units - 1));
            for _ in 0..units {
                out.extend_from_slice(&[4, b't', b'r', b'u', b'e']);
            }
        }
        4 => {
            var(&mut out, u64::from(units));
            out.extend(std::iter::repeat_n(b'x', units as usize));
        }
        5 => out.extend_from_slice(&[1, b'1']),
        6 => out.extend_from_slice(&[4, b'b', b'o', b'l', b'd', 4, b't', b'r', b'u', b'e']),
        8 => {
            var(&mut out, u64::from(units));
            out.extend(std::iter::repeat_n(120, units as usize));
        }
        other => panic!("genre {other}"),
    }
    out
}

/// Une mise à jour : par client, sa première horloge et ses éléments ; puis ce qu'elle supprime.
fn update(clients: &[(u64, u32, Vec<Vec<u8>>)], deleted: &[(u64, Vec<(u32, u32)>)]) -> Vec<u8> {
    let mut out = Vec::new();
    var(&mut out, clients.len() as u64);
    for (client, clock, items) in clients {
        var(&mut out, items.len() as u64);
        var(&mut out, *client);
        var(&mut out, u64::from(*clock));
        out.extend(items.iter().flatten());
    }
    var(&mut out, deleted.len() as u64);
    for (client, ranges) in deleted {
        var(&mut out, *client);
        var(&mut out, ranges.len() as u64);
        for (clock, len) in ranges {
            var(&mut out, u64::from(*clock));
            var(&mut out, u64::from(*len));
        }
    }
    out
}

/// `n` éléments de `WRITER`, de `units` unités, chacun à la suite du précédent ; `right` donne
/// l'origine droite de celui de rang `k`.
fn chain(
    kind: u8,
    n: u32,
    units: u32,
    keyed: bool,
    right: impl Fn(u32) -> Option<Id>,
) -> Vec<Vec<u8>> {
    (0..n)
        .map(|k| {
            let origin = (k > 0).then(|| (WRITER, k * units - 1));
            item(kind, origin, right(k), keyed, units)
        })
        .collect()
}

fn none(_: u32) -> Option<Id> {
    None
}

/// Une forme : son nom, sans ramasse-miettes ou avec, ses mises à jour, et le nombre de blocs de
/// `WRITER` qu'elle laisse quand `n` vaut 33 (ce qui se fond, ce qui ne se fond pas).
type Form = (&'static str, bool, Vec<Vec<u8>>, usize);

fn forms(n: u32) -> Vec<Form> {
    let every_other: Vec<(u32, u32)> = (0..n).step_by(2).map(|k| (k, 1)).collect();
    // Une lettre après chacune de celles de `WRITER`, sans origine droite.
    let saw = |backwards: bool| -> Vec<Vec<u8>> {
        (0..n.saturating_sub(1))
            .map(|k| if backwards { n - 2 - k } else { k })
            .map(|k| item(4, Some((WRITER, k)), None, false, 1))
            .collect()
    };
    let whole = || update(&[(WRITER, 0, vec![item(4, None, None, false, n)])], &[]);
    let letters = || chain(4, n, 1, false, none);
    let one = |items: Vec<Vec<u8>>| vec![update(&[(WRITER, 0, items)], &[])];
    // La jointure du milieu.
    let half = n / 2;
    vec![
        // Ce qui se fond : toutes les sortes de contenu que yrs fond.
        ("lettres", false, one(letters()), 1),
        (
            "lettres de trois",
            false,
            one(chain(4, n, 3, false, none)),
            1,
        ),
        ("valeurs", false, one(chain(8, n, 1, false, none)), 1),
        (
            "valeurs de deux",
            false,
            one(chain(8, n, 2, false, none)),
            1,
        ),
        ("json", false, one(chain(2, n, 1, false, none)), 1),
        (
            "contenus supprimés",
            false,
            one(chain(1, n, 2, false, none)),
            1,
        ),
        (
            "plages ramassées",
            false,
            one((0..n).map(|_| vec![0, 2]).collect()),
            1,
        ),
        (
            "lettres supprimées par leur mise à jour",
            false,
            vec![update(&[(WRITER, 0, letters())], &[(WRITER, vec![(0, n)])])],
            1,
        ),
        (
            "lettres supprimées, sans ramasse-miettes",
            true,
            vec![update(&[(WRITER, 0, letters())], &[(WRITER, vec![(0, n)])])],
            1,
        ),
        (
            "lettres supprimées plus tard",
            false,
            vec![
                update(&[(WRITER, 0, letters())], &[]),
                update(&[], &[(WRITER, every_other.clone())]),
                update(&[], &[(WRITER, vec![(0, n)])]),
            ],
            1,
        ),
        // Les entrées d'une clé, chacune à la suite de la précédente, qu'elle remplace.
        (
            "entrées d'une clé",
            false,
            one(chain(8, n, 1, true, none)),
            2,
        ),
        (
            "entrées d'une clé, toutes supprimées",
            false,
            vec![update(
                &[(WRITER, 0, chain(8, n, 1, true, none))],
                &[(WRITER, vec![(0, n)])],
            )],
            1,
        ),
        // La scie : un élément de n lettres, coupé après chacune par la lettre d'un client plus
        // grand, que yrs range plus loin ; les morceaux se refondent.
        (
            "scie",
            false,
            vec![update(
                &[
                    (WRITER, 0, vec![item(4, None, None, false, n)]),
                    (95, 0, saw(false)),
                ],
                &[],
            )],
            1,
        ),
        (
            "scie sur la page",
            false,
            vec![whole(), update(&[(95, 0, saw(false))], &[])],
            33,
        ),
        (
            "scie sur la page, puis une lettre",
            false,
            vec![
                whole(),
                update(&[(95, 0, saw(false))], &[]),
                update(
                    &[(
                        WRITER,
                        n,
                        vec![item(4, Some((WRITER, n - 1)), None, false, 1)],
                    )],
                    &[],
                ),
            ],
            1,
        ),
        (
            "scie à rebours",
            false,
            vec![whole(), update(&[(95, 0, saw(true))], &[])],
            33,
        ),
        // Ce qui ne se fond pas : une forme par condition de `try_squash`.
        // Le voisin de droite : les lettres d'un client plus petit se posent entre les morceaux.
        (
            "scie d'un client plus petit",
            false,
            vec![update(
                &[
                    (WRITER, 0, vec![item(4, None, None, false, n)]),
                    (50, 0, saw(false)),
                ],
                &[],
            )],
            33,
        ),
        // L'origine droite : une lettre sur deux nomme la lettre du client 60, posée après elles.
        (
            "origines droites différentes",
            false,
            vec![
                update(&[(60, 0, vec![item(4, None, None, false, 1)])], &[]),
                update(
                    &[(
                        WRITER,
                        0,
                        chain(4, n, 1, false, |k| (k % 2 == 0).then_some((60, 0))),
                    )],
                    &[],
                ),
            ],
            33,
        ),
        // L'origine : chaque lettre suit la précédente dans la liste, mais nomme pour origine
        // une lettre du client 10, d'où une lettre plus petite (client 20) la sépare.
        (
            "voisins sans être l'origine",
            false,
            vec![
                update(&[(10, 0, vec![item(4, None, None, false, 1)])], &[]),
                update(
                    &[
                        (20, 0, vec![item(4, Some((10, 0)), None, false, 1)]),
                        (
                            WRITER,
                            0,
                            (0..n)
                                .map(|k| {
                                    let origin = if k == 0 { (20, 0) } else { (10, 0) };
                                    item(4, Some(origin), None, false, 1)
                                })
                                .collect(),
                        ),
                    ],
                    &[],
                ),
            ],
            33,
        ),
        // Supprimé ou non : sans ramasse-miettes, une lettre supprimée garde son texte.
        (
            "une lettre sur deux supprimée, sans ramasse-miettes",
            true,
            vec![update(
                &[(WRITER, 0, letters())],
                &[(WRITER, every_other.clone())],
            )],
            33,
        ),
        (
            "une lettre sur deux supprimée",
            false,
            vec![update(&[(WRITER, 0, letters())], &[(WRITER, every_other)])],
            33,
        ),
        // Le contenu : des lettres et des valeurs, des lettres et des marques, tour à tour.
        (
            "lettres et valeurs",
            false,
            one((0..n)
                .map(|k| {
                    item(
                        if k % 2 == 0 { 4 } else { 8 },
                        (k > 0).then(|| (WRITER, k - 1)),
                        None,
                        false,
                        1,
                    )
                })
                .collect()),
            33,
        ),
        (
            "lettres et marques",
            false,
            one((0..n)
                .map(|k| {
                    item(
                        if k % 2 == 0 { 4 } else { 6 },
                        (k > 0).then(|| (WRITER, k - 1)),
                        None,
                        false,
                        1,
                    )
                })
                .collect()),
            33,
        ),
        (
            "objets posés dans le texte",
            false,
            one(chain(5, n, 1, false, none)),
            33,
        ),
        // L'horloge : un trou entre deux lettres voisines.
        (
            "un trou entre les lettres",
            false,
            one((0..n)
                .flat_map(|k| {
                    let origin = (k > 0).then(|| (WRITER, 3 * k - 3));
                    [item(4, origin, None, false, 1), vec![10, 2]]
                })
                .collect()),
            33,
        ),
        // Les mêmes conditions, manquées à une seule jointure, au milieu : de part et d'autre, deux
        // suites qui se fondent. yrs fond celle de droite, s'arrête à la jointure, puis fond celle
        // de gauche.
        (
            "un trou au milieu des lettres",
            false,
            one((0..n)
                .flat_map(|k| {
                    let clock = if k < half { k } else { k + 2 };
                    let hole = (k == half && k > 0).then(|| vec![10, 2]);
                    let letter = item(
                        4,
                        (k > 0).then(|| (WRITER, clock - 1 - if k == half { 2 } else { 0 })),
                        None,
                        false,
                        1,
                    );
                    hole.into_iter().chain([letter])
                })
                .collect()),
            2,
        ),
        (
            "une origine droite pour la seconde moitié",
            false,
            vec![
                update(&[(95, 0, vec![item(4, None, None, false, 1)])], &[]),
                update(
                    &[(
                        WRITER,
                        0,
                        chain(4, n, 1, false, |k| (k >= half).then_some((95, 0))),
                    )],
                    &[],
                ),
            ],
            2,
        ),
        (
            "la lettre d'un client plus petit au milieu",
            false,
            vec![update(
                &[
                    (WRITER, 0, letters()),
                    (
                        50,
                        0,
                        (half > 0)
                            .then(|| item(4, Some((WRITER, half - 1)), None, false, 1))
                            .into_iter()
                            .collect(),
                    ),
                ],
                &[],
            )],
            2,
        ),
        (
            "une autre origine au milieu",
            false,
            vec![
                update(&[(10, 0, vec![item(4, None, None, false, 1)])], &[]),
                update(
                    &[
                        (20, 0, vec![item(4, Some((10, 0)), None, false, 1)]),
                        (
                            WRITER,
                            0,
                            (0..n)
                                .map(|k| {
                                    let origin = match k {
                                        0 => (20, 0),
                                        k if k == half => (10, 0),
                                        k => (WRITER, k - 1),
                                    };
                                    item(4, Some(origin), None, false, 1)
                                })
                                .collect(),
                        ),
                    ],
                    &[],
                ),
            ],
            2,
        ),
        (
            "la première moitié supprimée, sans ramasse-miettes",
            true,
            vec![update(
                &[(WRITER, 0, letters())],
                &[(WRITER, if half > 0 { vec![(0, half)] } else { vec![] })],
            )],
            2,
        ),
        (
            "des lettres puis des valeurs",
            false,
            one((0..n)
                .map(|k| {
                    let kind = if k < half { 4 } else { 8 };
                    item(kind, (k > 0).then(|| (WRITER, k - 1)), None, false, 1)
                })
                .collect()),
            2,
        ),
        (
            "une plage ramassée puis des lettres",
            false,
            one(std::iter::once(vec![0, 2])
                .chain((0..n).map(|k| item(4, (k > 0).then(|| (WRITER, k + 1)), None, false, 1)))
                .collect()),
            2,
        ),
    ]
}

/// Les blocs du client `WRITER` dans un état encodé : ses éléments et ses plages ramassées.
fn blocks_of_the_writer(state: &[u8]) -> usize {
    use yrs_upstream::updates::decoder::Decode;
    // yrs ne relit pas le JSON qu'il écrit : le compte est alors lu en tête de l'état, où
    // `WRITER` est le seul client.
    let Ok(update) = yrs_upstream::Update::decode_v1(state) else {
        assert_eq!((state[0], state[2]), (1, WRITER as u8));
        return usize::from(state[1]);
    };
    let shown = format!("{update:?}");
    shown.matches(&format!("(<{WRITER}#")).count() + shown.matches("gc(").count()
}

#[test]
fn hand_made_updates_are_alike() {
    for n in [1, 2, 3, 4, 5, 8, 33, 200] {
        for (name, skip_gc, updates, _) in forms(n) {
            match replayed_alike(skip_gc, &[("t", 't')], &updates) {
                Ok(steady) => assert!(steady, "« {name} », {n} éléments : à la reprise"),
                Err(why) => panic!("« {name} », {n} éléments : {why}"),
            }
        }
    }
}

/// Chaque forme fait ce que son nom dit : sans cela, une forme « qui ne se fond pas » qui ne
/// poserait rien passerait pour pareille dans les deux yrs.
#[test]
fn each_hand_made_form_squashes_or_not_as_it_says() {
    for (name, skip_gc, updates, expected) in forms(33) {
        let played = upstream::replay(1, skip_gc, &[("t", 't')], &updates);
        for (answer, _) in &played {
            assert_eq!(answer, "appliquée", "« {name} »");
        }
        let seen = &played.last().unwrap().1;
        assert_eq!(blocks_of_the_writer(&seen.0), expected, "« {name} »");
        if name == "un trou entre les lettres" {
            // Les lettres d'après le trou sont dans le texte, pas en attente.
            println!("{}", seen.3);
            assert!(seen.3.contains(&"x".repeat(33)), "{}", seen.3);
        }
    }
}

// Ce que de vrais clients ont écrit, donné à une page : les mises à jour du vrai Yjs tirées au
// hasard, les états du vrai BlockNote, les déplacements faits dans un navigateur. La page donne
// à yrs ce qu'elle en garde, et les deux yrs en font la même chose.

fn fixtures(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|at| u8::from_str_radix(&text[2 * at..2 * at + 2], 16).unwrap())
        .collect()
}

/// Une suite de mises à jour, donnée à une page relue de `base` : par le chemin d'un client, puis
/// par celui du journal, chacun jusqu'à son premier refus.
fn given_to_a_page(what: &str, base: &[u8], updates: &[Vec<u8>], compared: &mut Compared) {
    for stored in [false, true] {
        let Ok(page) = watching(what, compared, || PageDoc::from_updates([base])) else {
            panic!("{what} : la base ne se relit pas");
        };
        for update in updates {
            let kept = watching(what, compared, || match stored {
                true => page.apply_update(update).is_ok(),
                false => page.try_client_update(update).is_ok(),
            });
            if !kept {
                break;
            }
        }
    }
}

#[test]
fn what_the_real_yjs_wrote_is_squashed_alike() {
    let dir = fixtures("tests/fixtures/yjs_updates");
    let base: Vec<u8> = serde_json::from_value(json(&dir.join("base.json"))).unwrap();
    let mut compared = Compared::default();
    let scenarios: Vec<(String, Vec<Vec<u8>>)> =
        serde_json::from_value(json(&dir.join("scenarios.json"))).unwrap();
    for (name, updates) in &scenarios {
        given_to_a_page(name, &base, updates, &mut compared);
    }
    // L'essai élargi, s'il est là (hors du dépôt), à la place de celui du dépôt.
    let large = dir.join("random-large.json");
    let random = if large.exists() {
        large
    } else {
        dir.join("random.json")
    };
    let scenes: Vec<Vec<String>> = serde_json::from_value(json(&random)).unwrap();
    for (n, scene) in scenes.iter().enumerate() {
        let updates: Vec<Vec<u8>> = scene.iter().map(|update| unhex(update)).collect();
        given_to_a_page(&format!("scène {}", n + 1), &base, &updates, &mut compared);
    }
    // Les scènes d'un rétablissement : la page en récrit les formes N1 à N3 avant de les lire.
    for name in ["redo.json", "redo-blocks.json", "redo-crossing.json"] {
        let scenes: Vec<(u64, Vec<String>, serde_json::Value)> =
            serde_json::from_value(json(&dir.join(name))).unwrap();
        for (seed, scene, _) in &scenes {
            let updates: Vec<Vec<u8>> = scene.iter().map(|update| unhex(update)).collect();
            given_to_a_page(
                &format!("{name}, scène {seed}"),
                &base,
                &updates,
                &mut compared,
            );
        }
    }
    // Les étapes 2 à cheval sur ce que la page connaît.
    let astride: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_value(json(&dir.join("astride.json"))).unwrap();
    for (name, scene) in &astride {
        let updates: Vec<Vec<u8>> = ["first", "step2"]
            .iter()
            .map(|key| serde_json::from_value(scene[*key].clone()).unwrap())
            .collect();
        given_to_a_page(name, &base, &updates, &mut compared);
    }
    println!(
        "ESSAI du vrai Yjs : {} mises à jour gardées, comparées dans les deux yrs (dont {} où \
         yrs ne fait pas deux fois la même chose)",
        compared.updates, compared.retried
    );
    assert!(
        compared.updates > 2_000,
        "{} comparaisons",
        compared.updates
    );
}

#[test]
fn what_blocknote_and_a_browser_wrote_is_squashed_alike() {
    let mut compared = Compared::default();
    let (_, created) = PageDoc::new_page();
    // Les états exportés du vrai BlockNote : relus comme un journal, et reçus par une page neuve.
    let mut states = 0;
    for entry in std::fs::read_dir(fixtures("../../spec/mmd/fixtures/yjs")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "bin") {
            let state = std::fs::read(&path).unwrap();
            let what = path.display().to_string();
            given_to_a_page(&what, &state, &[], &mut compared);
            given_to_a_page(&what, &created, &[state], &mut compared);
            states += 1;
        }
    }
    assert!(states >= 4, "{states} états de BlockNote");
    // Les déplacements faits dans un navigateur, dans les deux ordres d'identifiants et les deux
    // ordres d'arrivée ; et l'état que l'acteur écrit après un report.
    let mut scenes = 0;
    for order in ["mona-first", "tom-first"] {
        let dir = fixtures("../mementum-sync/tests/fixtures/moves").join(order);
        for entry in std::fs::read_dir(dir).unwrap() {
            let scene = entry.unwrap().path();
            let read = |name: &str| std::fs::read(scene.join(name)).unwrap();
            let (base, mona, tom) = (read("base.bin"), read("mona.bin"), read("tom.bin"));
            let what = scene.display().to_string();
            given_to_a_page(&what, &base, &[mona.clone(), tom.clone()], &mut compared);
            given_to_a_page(&what, &base, &[tom, mona], &mut compared);
            // Toutes les scènes n'ont pas de report.
            if let Ok(carried) = std::fs::read(scene.join("carried.bin")) {
                given_to_a_page(&what, &carried, &[], &mut compared);
            }
            scenes += 1;
        }
    }
    assert!(scenes >= 30, "{scenes} scènes de déplacement");
    println!(
        "ESSAI de BlockNote et du navigateur : {} mises à jour gardées, comparées dans les deux \
         yrs (dont {} où yrs ne fait pas deux fois la même chose)",
        compared.updates, compared.retried
    );
    assert!(compared.updates > 200, "{} comparaisons", compared.updates);
}
