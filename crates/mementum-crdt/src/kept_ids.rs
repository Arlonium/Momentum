//! Les identifiants des blocs que `PUT /mmd` récrit (Ruling R201).
//!
//! `GET /mmd` rend le MMD sans ancres : un agent qui relisait une page puis la récrivait telle
//! quelle donnait un identifiant neuf à chaque bloc, et chaque fil de commentaires passait sous
//! « Bloc supprimé » (constat S3 de la relecture finale). Avant d'écrire, un bloc lu sans
//! identifiant reçoit donc celui du bloc courant qui lui correspond, liste de frères par liste de
//! frères, en trois passes :
//!
//! 1. une plus longue sous-suite commune sur le type, le contenu et les props (pas les
//!    enfants), approchée comme le fait le « patience diff » : le préfixe et le suffixe communs,
//!    puis les blocs uniques des deux côtés, pris dans l'ordre par une plus longue sous-suite
//!    croissante, et de même entre eux ;
//! 2. pour ce qui reste, un bloc identique ailleurs dans la même liste, le premier d'abord : un
//!    bloc déplacé parmi ses frères (deux blocs échangés) garde le sien ;
//! 3. pour ce qui reste encore, le bloc à la même place entre deux blocs gardés à la passe 1, s'il
//!    est du même type : un bloc modifié garde le sien.
//!
//! Puis les enfants de chaque paire, de même. Un bloc qui porte déjà un identifiant (une ancre
//! du MMD) le garde, et le bloc courant qui portait cet identifiant ne donne le sien à personne ;
//! aucun identifiant n'est donné deux fois. Le travail de la passe 1 est borné
//! ([`WORK_PER_BLOCK`]) : au-delà, ce qui reste passe aux passes suivantes ; le tout est linéaire
//! ou presque (des tables de hachage et des tris).
//!
//! Ce que la règle ne fait pas : garder les éditions concurrentes (l'écriture remplace toujours
//! tout le contenu, Ruling R188), ni reconnaître un bloc qui change de parent, ni un bloc à la
//! fois modifié et déplacé : il reçoit un identifiant neuf. Un bloc retiré et un bloc inséré à
//! sa place, du même type, sont lus comme un bloc modifié. C'est un sous-ensemble de l'écriture
//! par différence du lot 5 (Ruling R40). Une restauration n'y passe pas : ses blocs portent déjà
//! leurs identifiants.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::hash::Hasher;

use mementum_md::Block;

/// Travail permis à la passe 1, par bloc des deux arbres (blocs relus d'une plage qu'on
/// partage) : au-delà, elle s'arrête. Ce qui coûterait plus est un ordre si bouleversé que la
/// sous-suite commune n'y garderait presque rien.
const WORK_PER_BLOCK: usize = 32;

/// Donne aux blocs de `written` sans identifiant celui du bloc de `current` qui leur
/// correspond (voir le module).
pub(crate) fn keep_ids(current: &[Block], written: &mut [Block]) {
    let mut claimed = HashSet::new();
    explicit_ids(written, &mut claimed);
    let blocks = count(current) + count(written);
    let mut ctx = Ctx {
        claimed,
        given: HashSet::new(),
        budget: WORK_PER_BLOCK.saturating_mul(blocks).saturating_add(1024),
    };
    match_lists(current, written, &mut ctx);
}

struct Ctx {
    /// Les identifiants que `written` porte déjà : aucun autre bloc ne les reçoit.
    claimed: HashSet<String>,
    /// Ceux déjà donnés.
    given: HashSet<String>,
    /// Le travail qui reste à la passe 1.
    budget: usize,
}

fn explicit_ids(blocks: &[Block], into: &mut HashSet<String>) {
    for block in blocks {
        if !block.id.is_empty() {
            into.insert(block.id.clone());
        }
        explicit_ids(&block.children, into);
    }
}

fn count(blocks: &[Block]) -> usize {
    blocks.iter().map(|b| 1 + count(&b.children)).sum()
}

/// Une liste de frères, puis les enfants de chaque paire. La profondeur est celle de l'arbre,
/// bornée par le MMD (`MAX_DEPTH`).
fn match_lists(old: &[Block], new: &mut [Block], ctx: &mut Ctx) {
    let old_keys: Vec<u64> = old.iter().map(key).collect();
    let new_keys: Vec<u64> = new.iter().map(key).collect();
    let equal = |o: usize, n: usize| -> bool { same(&old[o], &new[n]) };
    let anchors = common(&old_keys, &new_keys, &equal, &mut ctx.budget);
    let mut old_taken = vec![false; old.len()];
    let mut new_taken = vec![false; new.len()];
    for &(o, n) in &anchors {
        old_taken[o] = true;
        new_taken[n] = true;
    }
    let mut pairs = anchors.clone();

    // Passe 2 : un bloc identique ailleurs dans la liste, le premier d'abord.
    let mut free: HashMap<u64, VecDeque<usize>> = HashMap::new();
    for (o, k) in old_keys.iter().enumerate().filter(|(o, _)| !old_taken[*o]) {
        free.entry(*k).or_default().push_back(o);
    }
    let waiting: Vec<usize> = (0..new.len()).filter(|n| !new_taken[*n]).collect();
    for n in waiting {
        let Some(candidates) = free.get_mut(&new_keys[n]) else {
            continue;
        };
        let found = candidates.iter().position(|&o| equal(o, n));
        if let Some(o) = found.and_then(|at| candidates.remove(at)) {
            old_taken[o] = true;
            new_taken[n] = true;
            pairs.push((o, n));
        }
    }

    // Passe 3 : à la même place entre deux blocs gardés à la passe 1, du même type.
    let mut bounds = vec![(0, 0)];
    bounds.extend(anchors.iter().map(|&(o, n)| (o + 1, n + 1)));
    let ends = anchors
        .iter()
        .copied()
        .chain(std::iter::once((old.len(), new.len())));
    for ((o0, n0), (o1, n1)) in bounds.into_iter().zip(ends) {
        let olds = (o0..o1).filter(|o| !old_taken[*o]);
        let news = (n0..n1).filter(|n| !new_taken[*n]);
        for (o, n) in olds.zip(news) {
            if old[o].kind == new[n].kind {
                pairs.push((o, n));
            }
        }
    }

    for (o, n) in pairs {
        let id = &old[o].id;
        if new[n].id.is_empty()
            && !id.is_empty()
            && !ctx.claimed.contains(id)
            && ctx.given.insert(id.clone())
        {
            new[n].id = id.clone();
        }
        match_lists(&old[o].children, &mut new[n].children, ctx);
    }
}

/// Le type, les props et le contenu d'un bloc, hachés : ce qui fait que deux blocs sont les
/// mêmes, hors identifiant et enfants. Une égalité de clés est vérifiée par [`same`].
fn key(block: &Block) -> u64 {
    struct Feed<'a>(&'a mut DefaultHasher);
    impl std::fmt::Write for Feed<'_> {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes());
            Ok(())
        }
    }
    let mut hasher = DefaultHasher::new();
    // Écrire dans un hachage ne peut pas échouer.
    let _ = write!(
        Feed(&mut hasher),
        "{:?}\u{0}{:?}\u{0}{:?}",
        block.kind,
        block.props,
        block.content
    );
    hasher.finish()
}

fn same(a: &Block, b: &Block) -> bool {
    a.kind == b.kind && a.props == b.props && a.content == b.content
}

/// La passe 1 : des paires `(ancien, nouveau)` de blocs identiques, croissantes des deux côtés,
/// rangées par nouvel index.
fn common(
    old: &[u64],
    new: &[u64],
    same: &dyn Fn(usize, usize) -> bool,
    budget: &mut usize,
) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut ranges = vec![(0, old.len(), 0, new.len())];
    while let Some((mut o0, mut o1, mut n0, mut n1)) = ranges.pop() {
        while o0 < o1 && n0 < n1 && old[o0] == new[n0] && same(o0, n0) {
            pairs.push((o0, n0));
            o0 += 1;
            n0 += 1;
        }
        while o0 < o1 && n0 < n1 && old[o1 - 1] == new[n1 - 1] && same(o1 - 1, n1 - 1) {
            o1 -= 1;
            n1 -= 1;
            pairs.push((o1, n1));
        }
        let work = (o1 - o0) + (n1 - n0);
        if o0 == o1 || n0 == n1 || work > *budget {
            continue;
        }
        *budget -= work;
        // Par clé : combien de fois des deux côtés, et où (la dernière fois).
        let mut seen: HashMap<u64, (u32, usize, u32, usize)> = HashMap::new();
        for (o, k) in (o0..o1).zip(&old[o0..o1]) {
            let entry = seen.entry(*k).or_insert((0, 0, 0, 0));
            entry.0 += 1;
            entry.1 = o;
        }
        for (n, k) in (n0..n1).zip(&new[n0..n1]) {
            if let Some(entry) = seen.get_mut(k) {
                entry.2 += 1;
                entry.3 = n;
            }
        }
        let mut unique: Vec<(usize, usize)> = seen
            .values()
            .filter(|&&(olds, o, news, n)| olds == 1 && news == 1 && same(o, n))
            .map(|&(_, o, _, n)| (n, o))
            .collect();
        unique.sort_unstable();
        let anchors = longest_increasing(&unique);
        if anchors.is_empty() {
            continue;
        }
        let (mut po, mut pn) = (o0, n0);
        for &(o, n) in &anchors {
            ranges.push((po, o, pn, n));
            pairs.push((o, n));
            (po, pn) = (o + 1, n + 1);
        }
        ranges.push((po, o1, pn, n1));
    }
    pairs.sort_unstable_by_key(|&(_, n)| n);
    pairs
}

/// La plus longue sous-suite d'`items` (`(nouveau, ancien)`, rangés par nouvel index) dont les
/// anciens index croissent, en `(ancien, nouveau)` : le tri par piles, en O(k log k).
fn longest_increasing(items: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous: Vec<Option<usize>> = vec![None; items.len()];
    for (i, &(_, o)) in items.iter().enumerate() {
        let pile = tails.partition_point(|&t| items[t].1 < o);
        if pile > 0 {
            previous[i] = Some(tails[pile - 1]);
        }
        if pile == tails.len() {
            tails.push(i);
        } else {
            tails[pile] = i;
        }
    }
    let mut out = Vec::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        out.push((items[i].1, items[i].0));
        at = previous[i];
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_increasing_keeps_the_order_of_both_sides() {
        // Nouveaux 0..4 face aux anciens 3, 0, 1, 4 : la plus longue suite est 0, 1, 4.
        let items = [(0, 3), (1, 0), (2, 1), (3, 4)];
        assert_eq!(longest_increasing(&items), vec![(0, 1), (1, 2), (4, 3)]);
        assert!(longest_increasing(&[]).is_empty());
    }

    /// Le coût du rapprochement seul sur une page de 10 000 blocs (un sur dix avec deux enfants),
    /// à lancer en release : la même page, la même page renversée, et la page où un bloc sur dix
    /// est modifié et un sur vingt retiré.
    #[test]
    #[ignore = "mesure, à lancer en --release"]
    fn measure_matching_of_10000_blocks() {
        let mmd: String = (0..10_000)
            .map(|i| {
                if i % 10 == 0 {
                    format!("- Bloc numéro {i}\n  - Enfant a\n  - Enfant b\n")
                } else {
                    format!("Bloc numéro {i}, avec un peu de **texte**.\n")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let current = crate::PageDoc::new_page().0;
        current
            .write_blocks(&mementum_md::parse(&mmd).unwrap())
            .unwrap();
        let current = current.read_blocks();
        let fresh = |mut blocks: Vec<Block>| {
            fn clear(blocks: &mut [Block]) {
                for b in blocks {
                    b.id.clear();
                    clear(&mut b.children);
                }
            }
            clear(&mut blocks);
            blocks
        };
        let same = fresh(current.clone());
        let mut reversed = same.clone();
        reversed.reverse();
        let edited: Vec<Block> = same
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 20 != 7)
            .map(|(i, b)| {
                let mut b = b.clone();
                if i % 10 == 3 {
                    b.content = mementum_md::Content::Inline(vec![]);
                }
                b
            })
            .collect();
        for (what, written) in [
            ("la même page", same),
            ("renversée", reversed),
            ("modifiée", edited),
        ] {
            let mut times = Vec::new();
            let mut kept = 0;
            for _ in 0..10 {
                let mut written = written.clone();
                let start = std::time::Instant::now();
                keep_ids(&current, &mut written);
                times.push(start.elapsed());
                kept = count_kept(&written);
            }
            times.sort();
            println!(
                "MESURE rapprochement des identifiants, 10 000 blocs, {what} : p50 {:?}, max \
                 {:?} ; {kept} blocs gardent leur identifiant",
                times[5], times[9]
            );
        }
    }

    fn count_kept(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|b| usize::from(!b.id.is_empty()) + count_kept(&b.children))
            .sum()
    }

    /// Une liste renversée de 10 000 blocs, tous différents : la passe 1 ne garde qu'une paire,
    /// sans dépasser son budget, et en temps linéaire.
    #[test]
    fn a_reversed_list_stays_within_the_budget() {
        let old: Vec<u64> = (0..10_000).collect();
        let new: Vec<u64> = (0..10_000).rev().collect();
        let mut budget = WORK_PER_BLOCK * 20_000;
        let pairs = common(&old, &new, &|o, n| old[o] == new[n], &mut budget);
        assert_eq!(pairs.len(), 1);
        assert!(budget > 0);
    }
}
