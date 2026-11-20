//! Les deux yrs côte à côte (Ruling R875) : celui du serveur (`vendor/yrs`, corrigé) et celui de
//! crates.io (d'origine), menés par le même code (`one_yrs.rs`, inclus une fois pour chacun).
//!
//! Le correctif ne doit rien changer à ce que yrs produit. [`watching`] le vérifie sur ce qu'une
//! page donne vraiment à yrs : chaque mise à jour qu'elle a intégrée et gardée, telle que yrs l'a
//! reçue (après la coupe et les règles qui récrivent), est donnée aux deux yrs à la suite de
//! l'état de la page d'avant, et tout ce qu'on voit d'eux doit être pareil. Les octets hostiles
//! n'arrivent ainsi au yrs d'origine, qui n'est pas sûr en mémoire, que passés par la garde, et
//! rien de ce que la page a refusé ne lui est donné.

use std::cell::RefCell;
use std::rc::Rc;

#[allow(dead_code)]
pub mod fixed {
    use yrs as y;
    include!("one_yrs.rs");
}

#[allow(dead_code)]
pub mod upstream {
    use yrs_upstream as y;
    include!("one_yrs.rs");
}

/// Une vue d'un document : l'état v1, l'état v2, le vecteur d'état, la lecture.
pub type Seen = (Vec<u8>, Vec<u8>, Vec<u8>, String);

/// Vrai si deux encodages sont les mêmes, à l'ordre près des entrées d'un objet : yrs écrit les
/// clés d'une valeur objet, et les réglages d'un sous-document, dans l'ordre d'une table de
/// hachage, qui change d'un document à l'autre dans un même processus. Deux encodages de mêmes
/// octets dans un autre ordre sont tenus pour pareils ; un bloc fondu ou non change leur nombre.
fn same_bytes(one: &[u8], other: &[u8]) -> bool {
    if one == other {
        return true;
    }
    let (mut one, mut other) = (one.to_vec(), other.to_vec());
    one.sort_unstable();
    other.sort_unstable();
    one == other
}

/// Dit en quoi deux vues d'un même document diffèrent, ou rien.
pub fn difference(fixed: &Seen, upstream: &Seen) -> Option<String> {
    if !same_bytes(&fixed.0, &upstream.0) {
        return Some(format!(
            "l'état encodé (v1) : {:?} au lieu de {:?}",
            fixed.0, upstream.0
        ));
    }
    if !same_bytes(&fixed.1, &upstream.1) {
        return Some("l'état encodé (v2)".to_string());
    }
    if fixed.2 != upstream.2 {
        return Some("le vecteur d'état".to_string());
    }
    if fixed.3 != upstream.3 {
        return Some(format!(
            "la lecture : {} au lieu de {}",
            fixed.3, upstream.3
        ));
    }
    None
}

/// Les deux yrs reçoivent `updates` une à une dans un document neuf, de types racines `roots`
/// (si `rewriting`, la mémoire libérée récrite avant chaque lecture, puis la clé `k` de `m` écrite :
/// `replay_rewriting_memory`). Rend la première différence.
fn replayed_once(
    skip_gc: bool,
    roots: &[(&str, char)],
    updates: &[Vec<u8>],
    rewriting: bool,
) -> Result<(), String> {
    let (fixed, upstream) = if rewriting {
        (
            fixed::replay_rewriting_memory(1, skip_gc, roots, updates),
            upstream::replay_rewriting_memory(1, skip_gc, roots, updates),
        )
    } else {
        (
            fixed::replay(1, skip_gc, roots, updates),
            upstream::replay(1, skip_gc, roots, updates),
        )
    };
    for (at, ((answer, seen), (their_answer, their_seen))) in
        fixed.into_iter().zip(upstream).enumerate()
    {
        if answer != their_answer {
            return Err(format!(
                "mise à jour {at} : « {answer} » au lieu de « {their_answer} »"
            ));
        }
        if let Some(what) = difference(&seen, &their_seen) {
            return Err(format!("après la mise à jour {at} ({answer}) : {what}"));
        }
    }
    Ok(())
}

/// Ce qu'on redonne aux deux yrs avant de les dire différents.
const TRIES: usize = 48;

/// Les deux yrs font la même chose de `updates`, reçues une à une dans un document neuf de types
/// racines `roots`. Rend vrai s'ils l'ont fait du premier coup, ou la différence.
///
/// yrs ne refait pas toujours deux fois la même chose des mêmes octets : en supprimant un élément
/// qui porte des attributs déjà supprimés, il fond les plages ramassées voisines dans l'ordre
/// d'une table de hachage (`TransactionMut::delete`, puis `merge_blocks`), et laisse, selon cet
/// ordre, une plage ou deux. Le yrs d'origine se sépare ainsi de lui-même. Une différence n'est
/// donc dite qu'après [`TRIES`] passages qui diffèrent tous : ce que le correctif changerait
/// différerait à chaque passage.
pub fn replayed_alike(
    skip_gc: bool,
    roots: &[(&str, char)],
    updates: &[Vec<u8>],
) -> Result<bool, String> {
    alike(skip_gc, roots, updates, false)
}

/// Comme [`replayed_alike`], la mémoire que yrs libère récrite avant chaque lecture, puis la clé
/// `k` de la map racine `m` écrite (Ruling R880) : une entrée de map qui pendrait vers un bloc
/// retiré se lit alors autrement que dans le témoin, ou arrête le processus. Sans cela, la lecture
/// d'un bloc libéré mais pas encore récrit ressemble à la bonne.
#[allow(dead_code)] // seul `squash_equivalence.rs` s'en sert
pub fn rewritten_alike(
    skip_gc: bool,
    roots: &[(&str, char)],
    updates: &[Vec<u8>],
) -> Result<bool, String> {
    alike(skip_gc, roots, updates, true)
}

fn alike(
    skip_gc: bool,
    roots: &[(&str, char)],
    updates: &[Vec<u8>],
    rewriting: bool,
) -> Result<bool, String> {
    let Err(why) = replayed_once(skip_gc, roots, updates, rewriting) else {
        return Ok(true);
    };
    for _ in 1..TRIES {
        if replayed_once(skip_gc, roots, updates, rewriting).is_ok() {
            return Ok(false);
        }
    }
    Err(why)
}

/// Les types racines d'une page.
pub const PAGE: [(&str, char); 2] = [(mementum_crdt::FRAGMENT, 'x'), (mementum_crdt::META, 'm')];

/// Ce que les pages ont gardé : l'état d'avant, et la mise à jour telle que yrs l'a reçue.
type Handed = Vec<(Vec<u8>, Vec<u8>)>;

/// Ce qui a été comparé : les mises à jour, et celles où il a fallu s'y reprendre.
#[derive(Default, Clone, Copy)]
pub struct Compared {
    pub updates: u64,
    pub retried: u64,
}

/// Compare les deux yrs sur une écriture du serveur qui ne passe pas par la lecture de la page
/// (l'écriture par différence, `PageDoc::write_diff`, relecture de S2, I4) : `update` donnée à la
/// suite de `state`, l'état de la page d'avant.
#[allow(dead_code)] // seul `hostile_at_random.rs` s'en sert
pub fn written_alike(what: &str, compared: &mut Compared, state: &[u8], update: &[u8]) {
    match replayed_alike(false, &PAGE, &[state.to_vec(), update.to_vec()]) {
        Ok(steady) => {
            compared.updates += 1;
            compared.retried += u64::from(!steady);
        }
        Err(why) => panic!(
            "{what} : le yrs corrigé n'est pas celui d'origine, {why} ; état {state:?}, écriture \
             {update:?}"
        ),
    }
}

/// Mène `run`, et compare les deux yrs sur chaque mise à jour que les pages de ce fil ont gardée
/// pendant ce temps. Rend ce que rend `run`, et ajoute à `compared` ; panique à la première
/// différence.
pub fn watching<T>(what: &str, compared: &mut Compared, run: impl FnOnce() -> T) -> T {
    let handed: Rc<RefCell<Handed>> = Rc::default();
    let sink = Rc::clone(&handed);
    mementum_crdt::hooks::watch_what_yrs_is_handed(Some(Box::new(move |state, update| {
        sink.borrow_mut().push((state.to_vec(), update.to_vec()));
    })));
    let out = run();
    mementum_crdt::hooks::watch_what_yrs_is_handed(None);
    for (state, update) in handed.take() {
        match replayed_alike(false, &PAGE, &[state.clone(), update.clone()]) {
            Ok(steady) => {
                compared.updates += 1;
                compared.retried += u64::from(!steady);
            }
            Err(why) => panic!(
                "{what} : le yrs corrigé n'est pas celui d'origine, {why} ; état {state:?}, mise à \
                 jour {update:?}"
            ),
        }
    }
    out
}
