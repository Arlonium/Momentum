//! Step 4 : la restriction d'un `SpaceAccess` à la portée d'un jeton (prototype de
//! `SpaceAccess::narrowed`, Ruling R1217), sur 10 000 et 100 000 pages, portée de 1 et de 50 pages,
//! et la réponse pour une page hors de la portée, contre une page inconnue.
//!
//! `SpaceAccess` garde ses nœuds privés : le prototype part de `visible()` (ce que la restriction
//! lira dans `mementum-acl`) et bâtit la liste restreinte, triée par identifiant, comme le ferait
//! `narrowed` ; le coût est celui de cette liste.

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

use mementum_acl::{Grant, Level, NodeAccess, Role, Section, SpaceAccess, Subject, TreeNode};
use uuid::Uuid;

use crate::stats::summary;

/// Un générateur congruentiel : le même arbre à chaque passage.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Un espace de `n` pages : 1 % de racines, les autres sous une page plus ancienne tirée parmi
/// les 200 dernières (un arbre profond par endroits, large ailleurs), quelques-unes à la corbeille.
fn space(n: usize, seed: u64) -> (Vec<TreeNode>, Vec<(Uuid, Grant)>) {
    let mut rng = Lcg(seed);
    let mut nodes: Vec<TreeNode> = Vec::with_capacity(n);
    let mut grants = Vec::new();
    for i in 0..n {
        let id = Uuid::from_u64_pair(rng.next() << 20 | i as u64, rng.next());
        let parent = if i == 0 || rng.below(100) == 0 {
            None
        } else {
            let back = (rng.below(200.min(i as u64)) + 1) as usize;
            Some(nodes[i - back].id)
        };
        if parent.is_none() {
            grants.push((
                id,
                Grant {
                    subject: Subject::Members,
                    level: Level::Edit,
                },
            ));
        }
        nodes.push(TreeNode {
            id,
            parent_id: parent,
            trashed: rng.below(500) == 0,
        });
    }
    (nodes, grants)
}

#[derive(Clone, Copy)]
struct Narrow {
    access: NodeAccess,
    barred: u16,
}

/// Sans `roots` : chaque page vue, son niveau ramené sous `cap`, l'interdit posé.
fn narrowed_all(full: &SpaceAccess, cap: Level, barred: u16) -> Vec<(Uuid, Narrow)> {
    full.visible()
        .map(|(id, a)| {
            (
                id,
                Narrow {
                    access: NodeAccess {
                        level: a.level.min(cap),
                        ..*a
                    },
                    barred,
                },
            )
        })
        .collect()
}

/// Avec `roots` : les pages vues qui sont une des racines ou sous l'une d'elles. Chaque page
/// remonte ses parents visibles jusqu'à une réponse connue (mémo), si bien que chaque page n'est
/// jugée qu'une fois. Le `visible_parent` d'une racine devient `None`, et la section `Shared`.
fn narrowed_roots(
    full: &SpaceAccess,
    cap: Level,
    barred: u16,
    roots: &[Uuid],
) -> Vec<(Uuid, Narrow)> {
    let roots: HashSet<Uuid> = roots.iter().copied().collect();
    let mut known: HashMap<Uuid, bool> = HashMap::new();
    let mut path = Vec::new();
    let mut out = Vec::new();
    for (id, a) in full.visible() {
        let mut at = id;
        let mut parent = a.visible_parent;
        let inside = loop {
            if roots.contains(&at) {
                break true;
            }
            if let Some(&k) = known.get(&at) {
                break k;
            }
            path.push(at);
            match parent {
                None => break false,
                Some(p) => {
                    at = p;
                    parent = full.node(p).and_then(|n| n.visible_parent);
                }
            }
        };
        for p in path.drain(..) {
            known.insert(p, inside);
        }
        if inside {
            let root = roots.contains(&id);
            out.push((
                id,
                Narrow {
                    access: NodeAccess {
                        level: a.level.min(cap),
                        section: Section::Shared,
                        visible_parent: if root { None } else { a.visible_parent },
                        ..*a
                    },
                    barred,
                },
            ));
        }
    }
    out
}

/// Comme [`narrowed_roots`], le mémo dans un tableau rangé comme les pages vues, le parent trouvé
/// par dichotomie (ce que `narrowed` peut faire dans `mementum-acl` sans changer `SpaceAccess`).
fn narrowed_roots_vec(
    full: &SpaceAccess,
    cap: Level,
    barred: u16,
    roots: &[Uuid],
) -> Vec<(Uuid, Narrow)> {
    let visible: Vec<(Uuid, &NodeAccess)> = full.visible().collect();
    let index = |id: Uuid| visible.binary_search_by_key(&id, |(i, _)| *i).ok();
    // 0 : inconnu, 1 : dedans, 2 : dehors.
    let mut known = vec![0u8; visible.len()];
    for r in roots {
        if let Some(i) = index(*r) {
            known[i] = 1;
        }
    }
    let mut path = Vec::new();
    let mut out = Vec::new();
    for k in 0..visible.len() {
        let mut at = k;
        let verdict = loop {
            if known[at] != 0 {
                break known[at];
            }
            path.push(at);
            match visible[at].1.visible_parent.and_then(index) {
                Some(p) => at = p,
                None => break 2,
            }
        };
        for p in path.drain(..) {
            known[p] = verdict;
        }
        if verdict == 1 {
            let (id, a) = visible[k];
            let root = roots.contains(&id);
            out.push((
                id,
                Narrow {
                    access: NodeAccess {
                        level: a.level.min(cap),
                        section: Section::Shared,
                        visible_parent: if root { None } else { a.visible_parent },
                        ..*a
                    },
                    barred,
                },
            ));
        }
    }
    out
}

/// Le rang du parent visible de chaque page vue, calculé une fois (ce que `SpaceAccess` pourrait
/// garder avec le calcul du compte, en cache par révision).
fn parent_ranks(full: &SpaceAccess) -> Vec<u32> {
    let visible: Vec<(Uuid, &NodeAccess)> = full.visible().collect();
    visible
        .iter()
        .map(|(_, a)| {
            a.visible_parent
                .and_then(|p| visible.binary_search_by_key(&p, |(i, _)| *i).ok())
                .map_or(u32::MAX, |i| i as u32)
        })
        .collect()
}

/// Avec les rangs des parents gardés : un seul passage, sans recherche.
fn narrowed_roots_ranked(
    full: &SpaceAccess,
    ranks: &[u32],
    cap: Level,
    barred: u16,
    roots: &[Uuid],
) -> Vec<(Uuid, Narrow)> {
    let visible: Vec<(Uuid, &NodeAccess)> = full.visible().collect();
    let mut known = vec![0u8; visible.len()];
    for r in roots {
        if let Ok(i) = visible.binary_search_by_key(r, |(i, _)| *i) {
            known[i] = 1;
        }
    }
    let mut path = Vec::new();
    let mut out = Vec::new();
    for k in 0..visible.len() {
        let mut at = k;
        let verdict = loop {
            if known[at] != 0 {
                break known[at];
            }
            path.push(at);
            match ranks[at] {
                u32::MAX => break 2,
                p => at = p as usize,
            }
        };
        for p in path.drain(..) {
            known[p] = verdict;
        }
        if verdict == 1 {
            let (id, a) = visible[k];
            out.push((
                id,
                Narrow {
                    access: NodeAccess {
                        level: a.level.min(cap),
                        section: Section::Shared,
                        visible_parent: if known_root(roots, id) { None } else { a.visible_parent },
                        ..*a
                    },
                    barred,
                },
            ));
        }
    }
    out
}

fn known_root(roots: &[Uuid], id: Uuid) -> bool {
    roots.contains(&id)
}

/// Une page seule est-elle dans la portée ? Ses parents visibles remontés, sans rien bâtir.
fn in_scope(full: &SpaceAccess, roots: &HashSet<Uuid>, id: Uuid) -> bool {
    let mut at = id;
    loop {
        if roots.contains(&at) {
            return true;
        }
        match full.node(at).and_then(|n| n.visible_parent) {
            Some(p) => at = p,
            None => return false,
        }
    }
}

fn time<T>(reps: usize, mut f: impl FnMut() -> T) -> Vec<Duration> {
    // Un tour de chauffe.
    black_box(f());
    (0..reps)
        .map(|_| {
            let start = Instant::now();
            black_box(f());
            start.elapsed()
        })
        .collect()
}

fn subtree_size(full: &SpaceAccess, roots: &[Uuid]) -> usize {
    narrowed_roots(full, Level::Edit, 0, roots).len()
}

pub fn run(args: &[String]) {
    let reps: usize = args
        .iter()
        .position(|a| a == "--reps")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let user = Uuid::from_u64_pair(1, 1);
    for n in [10_000usize, 100_000] {
        let (nodes, grants) = space(n, 42);
        let compute = time(reps.min(50), || {
            SpaceAccess::compute(&nodes, &grants, user, Some(Role::Member))
        });
        let full = SpaceAccess::compute(&nodes, &grants, user, Some(Role::Member));
        let seen = full.visible().count();
        println!("=== espace de {n} pages, {seen} vues par le compte (membre)");
        println!("  calcul du compte (SpaceAccess::compute, gardé en cache par révision) : {}", summary(&compute));
        let all = time(reps, || narrowed_all(&full, Level::Read, 0b111));
        println!("  restriction sans portée de pages (niveau, interdits) : {}", summary(&all));
        let ranks = time(reps.min(50), || parent_ranks(&full));
        println!("  rangs des parents visibles (une fois par calcul gardé) : {}", summary(&ranks));

        // Des racines de portée : une feuille, une page au milieu, une racine d'espace, 50 pages.
        let mut rng = Lcg(7);
        let ids: Vec<Uuid> = full.visible().map(|(id, _)| id).collect();
        let leaf = ids
            .iter()
            .copied()
            .find(|id| !nodes.iter().any(|t| t.parent_id == Some(*id)))
            .unwrap();
        let mid = nodes[n / 2].id;
        let top = nodes[0].id;
        let fifty: Vec<Uuid> = (0..50).map(|_| ids[rng.below(ids.len() as u64) as usize]).collect();
        for (label, roots) in [
            ("1 page (une feuille)", vec![leaf]),
            ("1 page (au milieu de l'arbre)", vec![mid]),
            ("1 page (la première racine)", vec![top]),
            ("50 pages tirées au hasard", fifty.clone()),
        ] {
            let kept = subtree_size(&full, &roots);
            let t = time(reps, || narrowed_roots(&full, Level::Edit, 0b111, &roots));
            println!("  restriction à {label} ({kept} pages gardées) : {}", summary(&t));
            assert_eq!(narrowed_roots_vec(&full, Level::Edit, 0, &roots).len(), kept);
            let t = time(reps, || narrowed_roots_vec(&full, Level::Edit, 0b111, &roots));
            println!("    mémo en tableau, parents par dichotomie : {}", summary(&t));
            let ranks = parent_ranks(&full);
            assert_eq!(narrowed_roots_ranked(&full, &ranks, Level::Edit, 0, &roots).len(), kept);
            let t = time(reps, || narrowed_roots_ranked(&full, &ranks, Level::Edit, 0b111, &roots));
            println!("    rangs des parents gardés avec le calcul : {}", summary(&t));
            let set: HashSet<Uuid> = roots.iter().copied().collect();
            // Une page vue hors de la portée : la restriction entière, puis la recherche.
            let outside = ids
                .iter()
                .copied()
                .find(|id| !in_scope(&full, &set, *id))
                .unwrap();
            let by_narrowing = time(reps, || {
                let narrowed = narrowed_roots(&full, Level::Edit, 0b111, &roots);
                narrowed.binary_search_by_key(&outside, |(id, _)| *id).is_ok()
            });
            let by_walk = time(reps, || in_scope(&full, &set, outside));
            println!(
                "    page vue hors portée → 404, par la restriction entière : {} ; par la remontée de ses parents : {}",
                summary(&by_narrowing),
                summary(&by_walk)
            );
        }
        let unknown = Uuid::from_u64_pair(u64::MAX, 3);
        let t = time(reps, || full.node(unknown).is_none());
        println!("  page inconnue → 404 (une recherche dans le calcul gardé) : {}", summary(&t));
    }
}
