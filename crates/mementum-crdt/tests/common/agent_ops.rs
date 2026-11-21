//! Des opérations d'agent tirées au hasard (passe de fin du lot 5, point 2 du plan) : une liste
//! d'opérations de la Task S3 (`str_replace`, `insert`, `replace_block`, `delete_block`,
//! `move_block`, `replace_page`), bâtie sur les blocs qu'un agent a lus, avec ses ancres (l'alias
//! d'un bloc, son identifiant entier, une ancre qui ne désigne rien) et ses MMD. Elle est menée
//! par `ops::plan` puis `PageDoc::apply`, comme l'acteur le fait d'une écriture d'agent
//! (`mementum-sync`, `actor/agent.rs`) : par `reload.rs` (la page relue de son journal est la page
//! vivante) et par `hostile_at_random.rs` (les écritures données aux deux yrs, `twins`).
//!
//! Le hasard est donné par `below(n)`, un nombre de `0` à `n - 1` : le même code sert un
//! générateur de proptest (une graine) et celui de l'essai hostile.

// Chaque essai n'en prend qu'une part (`Seeded` ne sert qu'à `reload.rs`).
#![allow(dead_code)]

use mementum_crdt::anchors::Anchors;
use mementum_crdt::ops::{Operation, Place};
use mementum_md::{Block, Content, Inline, block_text, parse};

/// Les MMD que les opérations écrivent : un paragraphe, un titre, des listes imbriquées, une
/// tâche, un dépliant, une citation, du code, un tableau, des colonnes, un lien de page, un bloc
/// inconnu, une image du web (refusée à une écriture par jeton, R1235), une ancre écrite par
/// l'agent (que l'insertion remplace par un identifiant neuf), du texte marqué et un lien.
pub const SNIPPETS: [&str; 15] = [
    "Paragraphe de l'agent.\n",
    "# Un titre\n",
    "- Premier point\n  - Point imbriqué\n- Second point\n",
    "- [ ] Une tâche\n",
    "<toggle title=\"Détails\">\nDedans.\n</toggle>\n",
    "> Une citation.\n",
    "```\nfn main() {}\n```\n",
    "| A | B |\n| --- | --- |\n| 1 | 2 |\n",
    "<columns>\n<column>\nGauche.\n</column>\n<column>\nDroite.\n</column>\n</columns>\n",
    "<page id=\"pg_agent\">Sous-page</page>\n",
    "<block type=\"future_widget\" id=\"b_agent\" data=\"eyJ4IjoxfQ==\"/>\n",
    "![Logo](https://example.com/logo.png)\n",
    "Bloc ancré ^p9\n",
    "Du **gras**, de l’*italique* et un [lien](https://mementum.dev).\n",
    "Un\n\nDeux\n\nTrois\n",
];

/// Les textes qu'un `str_replace` écrit : vide (un retrait), une lettre, un accent, un emoji, une
/// fin de ligne, un texte plus long.
const NEW_TEXTS: [&str; 7] = ["", "x", "Zé", "🚀", "a\nb", "texte de l'agent", "abc"];

/// Les identifiants des blocs, à toute profondeur, dans l'ordre de la page.
pub fn ids(blocks: &[Block], out: &mut Vec<String>) {
    for b in blocks {
        if !b.id.is_empty() {
            out.push(b.id.clone());
        }
        ids(&b.children, out);
    }
}

/// Les blocs, à toute profondeur, dans l'ordre de la page.
fn all<'a>(blocks: &'a [Block], out: &mut Vec<&'a Block>) {
    for b in blocks {
        out.push(b);
        all(&b.children, out);
    }
}

fn pick<T: Clone>(items: &[T], below: &mut dyn FnMut(u64) -> u64) -> Option<T> {
    (!items.is_empty()).then(|| items[below(items.len() as u64) as usize].clone())
}

/// Une ancre : l'alias d'un bloc lu (le plus souvent : c'est ce que rend `fetch`), son identifiant
/// entier, ou une ancre qui ne désigne aucun bloc.
fn anchor(ids: &[String], anchors: &Anchors, below: &mut dyn FnMut(u64) -> u64) -> String {
    let Some(id) = pick(ids, below) else {
        return "bzzzz".into();
    };
    match below(8) {
        0 => id,
        1 => "b_inconnu".into(),
        _ => anchors.alias(&id).unwrap_or(id),
    }
}

fn place(ids: &[String], anchors: &Anchors, below: &mut dyn FnMut(u64) -> u64) -> Place {
    match below(4) {
        0 => Place::Start,
        1 => Place::End,
        2 => Place::Before(anchor(ids, anchors, below)),
        _ => Place::After(anchor(ids, anchors, below)),
    }
}

fn snippet(below: &mut dyn FnMut(u64) -> u64) -> Vec<Block> {
    let mut blocks = Vec::new();
    for _ in 0..1 + below(2) {
        blocks.extend(
            parse(SNIPPETS[below(SNIPPETS.len() as u64) as usize]).expect("un MMD lisible"),
        );
    }
    blocks
}

/// Un morceau du texte brut d'un bloc lu : un début de caractère, de un à six caractères.
fn old_text(blocks: &[Block], below: &mut dyn FnMut(u64) -> u64) -> String {
    let mut flat = Vec::new();
    all(blocks, &mut flat);
    let texts: Vec<Vec<char>> = flat
        .iter()
        .map(|b| block_text(b).chars().collect::<Vec<char>>())
        .filter(|t| !t.is_empty())
        .collect();
    match pick(&texts, below) {
        Some(chars) => {
            let start = below(chars.len() as u64) as usize;
            let len = 1 + below(6.min((chars.len() - start) as u64)) as usize;
            chars[start..start + len].iter().collect()
        }
        None => "abc".into(),
    }
}

/// Les blocs qu'un agent récrit par `replace_page` : ceux qu'il a lus, chacun sous son alias (ce
/// que `fetch` rend en ancre), parfois sans ancre (un bloc qu'il a récrit sans la garder), puis
/// retouchés : un texte changé, un bloc retiré, un bloc neuf, l'ordre retourné.
fn rewritten(blocks: &[Block], anchors: &Anchors, below: &mut dyn FnMut(u64) -> u64) -> Vec<Block> {
    fn aliased(blocks: &mut [Block], anchors: &Anchors, below: &mut dyn FnMut(u64) -> u64) {
        for b in blocks {
            b.id = match below(6) {
                0 => String::new(),
                1 => b.id.clone(),
                _ => anchors.alias(&b.id).unwrap_or_default(),
            };
            aliased(&mut b.children, anchors, below);
        }
    }
    let mut blocks = blocks.to_vec();
    aliased(&mut blocks, anchors, below);
    for _ in 0..1 + below(3) {
        let n = blocks.len();
        match below(5) {
            0 if n > 0 => {
                let at = below(n as u64) as usize;
                if let Content::Inline(content) = &mut blocks[at].content {
                    content.push(Inline::Text {
                        text: "récrit".into(),
                        styles: Default::default(),
                    });
                }
            }
            1 if n > 1 => {
                blocks.remove(below(n as u64) as usize);
            }
            2 => {
                let at = below(n as u64 + 1) as usize;
                for (k, b) in snippet(below).into_iter().enumerate() {
                    blocks.insert(at + k, b);
                }
            }
            3 => blocks.reverse(),
            _ => {}
        }
    }
    blocks
}

/// Une liste d'une à quatre opérations, bâtie sur `blocks`, les blocs lus par l'agent.
pub fn random_ops(blocks: &[Block], below: &mut dyn FnMut(u64) -> u64) -> Vec<Operation> {
    let mut known = Vec::new();
    ids(blocks, &mut known);
    let anchors = Anchors::of(blocks);
    let mut ops = Vec::new();
    for _ in 0..1 + below(4) {
        let op = match below(12) {
            0..=3 => Operation::StrReplace {
                old: old_text(blocks, below),
                new: NEW_TEXTS[below(NEW_TEXTS.len() as u64) as usize].into(),
                anchor: (below(3) == 0).then(|| anchor(&known, &anchors, below)),
                replace_all: below(3) == 0,
            },
            4 | 5 => Operation::Insert {
                at: place(&known, &anchors, below),
                blocks: snippet(below),
            },
            6 => Operation::ReplaceBlock {
                anchor: anchor(&known, &anchors, below),
                blocks: snippet(below),
                allow_deleting_content: below(2) == 0,
            },
            7 => Operation::DeleteBlock {
                anchor: anchor(&known, &anchors, below),
                allow_deleting_content: below(2) == 0,
            },
            8 | 9 => Operation::MoveBlock {
                anchor: anchor(&known, &anchors, below),
                to: place(&known, &anchors, below),
            },
            _ => Operation::ReplacePage {
                blocks: if below(4) == 0 {
                    snippet(below)
                } else {
                    rewritten(blocks, &anchors, below)
                },
                allow_deleting_content: below(2) == 0,
            },
        };
        ops.push(op);
    }
    ops
}

/// Vrai si la page écrite (`page`, relue) est le plan (`plan`, normalisé) : mêmes blocs, à ceci
/// près qu'un bloc sans identifiant dans le plan en reçoit un neuf de l'écriture (les blocs
/// neufs sont nommés avant le plan, R1337 ; un bloc que la page portait sans identifiant, celui
/// d'une page créée par le serveur et jamais ouverte, en reçoit un aussi). C'est la promesse de
/// `ops` (« le plan appliqué est le plan vérifié »), au nommage près.
pub fn plan_written(page: &[Block], plan: &[Block]) -> bool {
    page.len() == plan.len()
        && page.iter().zip(plan).all(|(p, q)| {
            (q.id.is_empty() || p.id == q.id)
                && p.kind == q.kind
                && p.props == q.props
                && p.content == q.content
                && plan_written(&p.children, &q.children)
        })
}

/// Un générateur de nombres pour une graine (splitmix64), pour qui n'en a pas.
pub struct Seeded(pub u64);

impl Seeded {
    pub fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        if n == 0 { 0 } else { z % n }
    }
}
