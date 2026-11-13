//! `PUT /mmd` garde l'identifiant des blocs qu'il récrit (Ruling R201) : `PageDoc::rewrite_blocks`
//! donne à un bloc lu sans identifiant celui du bloc courant qui lui correspond, avant d'écrire.

use std::collections::HashSet;

use mementum_crdt::PageDoc;
use mementum_md::{Block, RenderOpts, arb, block_text, parse, render};
use proptest::prelude::*;

/// Les blocs de la page, à toute profondeur, dans l'ordre du document : `(texte, identifiant)`.
fn ids(blocks: &[Block]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<&Block> = blocks.iter().rev().collect();
    while let Some(block) = stack.pop() {
        out.push((block_text(block), block.id.clone()));
        stack.extend(block.children.iter().rev());
    }
    out
}

/// Une page qui porte `mmd`, écrite comme par `PUT /mmd`.
fn page_with(mmd: &str) -> PageDoc {
    let (page, _) = PageDoc::new_page();
    page.rewrite_blocks(&parse(mmd).unwrap()).unwrap();
    page
}

/// Le MMD de la page, tel que `GET /mmd` le rend : sans ancres.
fn read_mmd(page: &PageDoc) -> String {
    render(&page.read_blocks(), RenderOpts::default()).markdown
}

/// Chaque identifiant de la page une seule fois.
fn assert_unique(page: &PageDoc) {
    let all = ids(&page.read_blocks());
    let distinct: HashSet<&String> = all.iter().map(|(_, id)| id).collect();
    assert_eq!(
        distinct.len(),
        all.len(),
        "un identifiant en double : {all:?}"
    );
}

/// La sonde de la relecture : relire la page puis la récrire telle quelle ne change aucun
/// identifiant, blocs imbriqués compris.
#[test]
fn identical_rewrite_keeps_every_id() {
    let page = page_with("Premier\n\n- Point\n  - Imbriqué\n  - Encore\n- Autre\n\nDernier\n");
    let before = ids(&page.read_blocks());
    page.rewrite_blocks(&parse(&read_mmd(&page)).unwrap())
        .unwrap();
    assert_eq!(ids(&page.read_blocks()), before);
}

/// Un bloc modifié, un inséré, un retiré, deux échangés : les blocs inchangés gardent leur
/// identifiant, les deux échangés compris ; le bloc modifié garde le sien (même place entre les
/// mêmes blocs inchangés, même type) ; l'inséré en reçoit un neuf ; celui du retiré n'est plus.
#[test]
fn edited_rewrite_keeps_the_ids_of_unchanged_blocks() {
    let page = page_with("Un\n\nDeux\n\nTrois\n\nQuatre\n\nCinq\n\nSix\n");
    let before = ids(&page.read_blocks());
    let id = |text: &str| before.iter().find(|(t, _)| t == text).unwrap().1.clone();
    page.rewrite_blocks(&parse("Nouveau\n\nUn\n\nDeux bis\n\nCinq\n\nQuatre\n\nSix\n").unwrap())
        .unwrap();
    let after = ids(&page.read_blocks());
    let kept: Vec<(String, Option<String>)> = after
        .iter()
        .map(|(text, block)| {
            let old = before.iter().any(|(_, id)| id == block);
            (text.clone(), old.then(|| block.clone()))
        })
        .collect();
    assert_eq!(
        kept,
        vec![
            ("Nouveau".to_string(), None),
            ("Un".to_string(), Some(id("Un"))),
            ("Deux bis".to_string(), Some(id("Deux"))),
            ("Cinq".to_string(), Some(id("Cinq"))),
            ("Quatre".to_string(), Some(id("Quatre"))),
            ("Six".to_string(), Some(id("Six"))),
        ]
    );
    assert_unique(&page);
}

/// Les enfants se rapprochent liste par liste, sous leur parent rapproché : un enfant modifié
/// garde son identifiant, un enfant ajouté en reçoit un neuf, et un bloc qui change de parent
/// n'est pas reconnu (il en reçoit un neuf).
#[test]
fn nested_blocks_are_matched_under_their_parent() {
    let page = page_with("- Parent\n  - Enfant un\n  - Enfant deux\n- Autre\n");
    let before = ids(&page.read_blocks());
    let id = |text: &str| before.iter().find(|(t, _)| t == text).unwrap().1.clone();
    page.rewrite_blocks(
        &parse("- Parent\n  - Enfant un modifié\n  - Enfant deux\n  - Enfant trois\n- Autre\n")
            .unwrap(),
    )
    .unwrap();
    let after = ids(&page.read_blocks());
    let got = |text: &str| after.iter().find(|(t, _)| t == text).unwrap().1.clone();
    assert_eq!(got("Parent"), id("Parent"));
    assert_eq!(got("Enfant un modifié"), id("Enfant un"));
    assert_eq!(got("Enfant deux"), id("Enfant deux"));
    assert_eq!(got("Autre"), id("Autre"));
    assert!(before.iter().all(|(_, old)| *old != got("Enfant trois")));
    assert_unique(&page);
}

/// Deux blocs identiques : chacun garde le sien, dans l'ordre ; quand il n'en reste qu'un, c'est
/// le premier qui garde son identifiant ; aucun identifiant n'est donné deux fois.
#[test]
fn identical_blocks_keep_their_own_ids_without_duplicates() {
    let page = page_with("Même\n\nAutre\n\nMême\n");
    let before = ids(&page.read_blocks());
    page.rewrite_blocks(&parse("Même\n\nMême\n").unwrap())
        .unwrap();
    let after = ids(&page.read_blocks());
    assert_eq!(after[0].1, before[0].1, "le premier");
    assert_eq!(after[1].1, before[2].1, "le second");
    assert_unique(&page);

    page.rewrite_blocks(&parse("Même\n").unwrap()).unwrap();
    assert_eq!(ids(&page.read_blocks())[0].1, before[0].1, "le premier");
}

/// Un identifiant écrit dans le MMD (une ancre) gagne, et n'est donné à aucun autre bloc ; le
/// bloc courant qui le portait ne donne pas le sien à un autre bloc.
#[test]
fn explicit_ids_win() {
    let page = page_with("Premier\n\nSecond\n");
    let before = ids(&page.read_blocks());
    let first = before[0].1.clone();
    page.rewrite_blocks(&parse(&format!("Second ^{first}\n\nPremier\n")).unwrap())
        .unwrap();
    let after = ids(&page.read_blocks());
    assert_eq!(after[0], ("Second".to_string(), first.clone()));
    assert_ne!(after[1].1, first);
    assert_unique(&page);
}

/// Une restauration écrit les identifiants de sa version : `write_blocks` ne rapproche rien.
#[test]
fn write_blocks_matches_nothing() {
    let page = page_with("Premier\n");
    let before = ids(&page.read_blocks());
    page.write_blocks(&parse("Premier\n").unwrap()).unwrap();
    assert_ne!(ids(&page.read_blocks()), before);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    /// Après un `PUT` du MMD lu, tous les identifiants sont ceux d'avant.
    #[test]
    fn rewriting_what_was_read_keeps_every_id(t in arb::tree()) {
        let (page, _) = PageDoc::new_page();
        page.write_blocks(&t).unwrap();
        let before = ids(&page.read_blocks());
        let read = parse(&read_mmd(&page)).unwrap();
        page.rewrite_blocks(&read).unwrap();
        prop_assert_eq!(ids(&page.read_blocks()), before);
    }
}
