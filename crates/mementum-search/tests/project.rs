//! La projection d'une page en lignes d'index (plan du lot 4, Task S2 ; « Quoi indexer », R912).

use std::collections::HashSet;

use mementum_md::{Block, Cell, Content, Inline, PropValue, Props, Styles, Table};
use mementum_search::{
    IndexedBlock, MAX_BLOCK_ID_BYTES, MAX_BLOCK_TEXT_BYTES, MAX_PAGE_LINKS, MAX_PAGE_TEXT_BYTES,
    PROJECTION_VERSION, Rule, is_stand_in, kind_named, page_link, project, rule,
};
use serde_json::Value;
use uuid::Uuid;

const PLAN: &str = "0193b2c4-5d6e-7f80-9a1b-2c3d4e5f6a7b";
const OTHER: &str = "0193b2c4-9e8f-7a6b-8c5d-4e3f2a1b0c9d";
const ORIGIN: &str = "https://notes.exemple.fr";

fn uuid(id: &str) -> Uuid {
    Uuid::parse_str(id).unwrap()
}

fn block(id: &str, kind: &str, content: Content) -> Block {
    Block {
        id: id.to_string(),
        kind: kind.to_string(),
        props: Props::new(),
        content,
        children: Vec::new(),
    }
}

fn plain(text: &str) -> Inline {
    Inline::Text {
        text: text.to_string(),
        styles: Styles::default(),
    }
}

fn inline(id: &str, kind: &str, content: Vec<Inline>) -> Block {
    block(id, kind, Content::Inline(content))
}

fn para(id: &str, text: &str) -> Block {
    inline(id, "paragraph", vec![plain(text)])
}

fn with_children(mut parent: Block, children: Vec<Block>) -> Block {
    parent.children = children;
    parent
}

fn with_prop(mut block: Block, name: &str, value: PropValue) -> Block {
    block.props.insert(name.to_string(), value);
    block
}

fn mention(kind: &str, id: &str, label: &str) -> Inline {
    Inline::Mention {
        kind: kind.to_string(),
        id: id.to_string(),
        label: label.to_string(),
    }
}

fn link(href: &str, text: &str) -> Inline {
    Inline::Link {
        href: href.to_string(),
        children: vec![plain(text)],
    }
}

/// La seule ligne d'une page d'un bloc.
fn only(blocks: &[Block]) -> IndexedBlock {
    let projection = project(blocks, None);
    assert_eq!(projection.rows.len(), 1, "{:?}", projection.rows);
    assert!(!projection.truncated);
    projection.rows.into_iter().next().unwrap()
}

/// Ce qu'une ligne dit, sans sa clé ni son empreinte.
fn said(rows: &[IndexedBlock]) -> Vec<(&str, &str, Vec<String>)> {
    rows.iter()
        .map(|row| {
            let links = row.links.iter().map(Uuid::to_string).collect();
            (row.kind, row.text.as_str(), links)
        })
        .collect()
}

fn keys(rows: &[IndexedBlock]) -> Vec<&str> {
    rows.iter().map(|row| row.block_id.as_str()).collect()
}

/// Échoue si `spec/schema/blocks.json` gagne un type que la table de la projection ne connaît
/// pas : un bloc neuf doit dire ce qui se cherche de lui.
#[test]
fn every_block_of_the_registry_has_its_rule() {
    let registry: Value =
        serde_json::from_str(include_str!("../../../spec/schema/blocks.json")).unwrap();
    let blocks = registry["blocks"].as_array().unwrap();
    assert!(blocks.len() >= 24, "le registre est lu");
    for spec in blocks {
        let name = spec["name"].as_str().unwrap();
        let Some(found) = rule(name) else {
            panic!("le bloc « {name} » du registre n'a pas de règle de projection");
        };
        // La règle suit la forme du contenu que le registre déclare.
        match spec["content"].as_str().unwrap() {
            "inline" => assert_eq!(found, Rule::Inline, "{name}"),
            "table" => assert_eq!(found, Rule::Table, "{name}"),
            "none" => assert!(!matches!(found, Rule::Inline | Rule::Table), "{name}"),
            other => panic!("forme de contenu inconnue du test : {other}"),
        }
        // Une prop que la règle lit existe dans le registre.
        let props: Vec<&str> = spec["props"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        let read: &[&str] = match found {
            Rule::Props(names) => names,
            Rule::PageLink => &["pageId", "title"],
            _ => &[],
        };
        for prop in read {
            assert!(props.contains(prop), "{name} n'a pas de prop {prop}");
        }
    }
    // Témoin : un type hors du registre n'a pas de règle.
    assert_eq!(rule("future_widget"), None);
    assert_eq!(rule(""), None);
    // Un bloc inconnu est dans le registre, et rien de lui n'est cherché (décision L17).
    assert_eq!(rule("unknown"), Some(Rule::Nothing));
}

/// La table de cas partagée, écrite à la main : un MMD, ses lignes.
#[test]
fn the_cases_project_as_written() {
    let table: Value =
        serde_json::from_str(include_str!("../../../spec/search/projection.json")).unwrap();
    let cases = table["blocks"].as_array().unwrap();
    assert!(cases.len() >= 20, "la table est lue");
    for case in cases {
        let why = case["why"].as_str().unwrap();
        let blocks = mementum_md::parse(case["mmd"].as_str().unwrap())
            .unwrap_or_else(|e| panic!("{why} : MMD refusé, {e}"));
        let projection = project(&blocks, None);
        assert!(!projection.truncated, "{why}");
        let expected: Vec<(&str, &str, Vec<String>)> = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let links = row["links"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|l| l.as_str().unwrap().to_string())
                    .collect();
                (
                    row["kind"].as_str().unwrap(),
                    row["text"].as_str().unwrap(),
                    links,
                )
            })
            .collect();
        assert_eq!(said(&projection.rows), expected, "{why}");
    }
}

type Row = (&'static str, &'static str);

/// Les fixtures MMD du dépôt, et ce qui se cherche d'elles : écrit à la main d'après « Quoi
/// indexer », jamais recopié de la sortie.
const ALL_BLOCKS: &[Row] = &[
    ("heading", "Toutes les formes"),
    ("paragraph", "Chaque type de bloc, une fois."),
    (
        "paragraph",
        "Du gras, de l’italique, du barré, du code, du souligné, de la couleur, un lien et E = mc^2.",
    ),
    (
        "paragraph",
        "Voir Spécification, demander à Lenny avant le 1 déc. 2026.",
    ),
    (
        "paragraph",
        "Première ligne seconde ligne, après un saut de ligne.",
    ),
    ("bulletListItem", "Premier point"),
    ("bulletListItem", "Point imbriqué"),
    ("bulletListItem", "Second point"),
    ("numberedListItem", "Étape un"),
    ("numberedListItem", "Étape deux"),
    ("checkListItem", "Partage des coffres"),
    ("toggleListItem", "Détails techniques"),
    ("paragraph", "Le serveur tourne sur un Raspberry Pi 4."),
    ("quote", "Une citation courte."),
    (
        "codeBlock",
        "fn main() {\n    println!(\"Café 🚀 déjà\");\n}",
    ),
    ("image", "Logo Mementum"),
    ("file", "spec.pdf"),
    ("table", "Nom Statut Prototype En cours Benchmark Terminé"),
    ("callout", "Penser à la migration des données."),
    ("equation", r"\int_0^1 x^2 \, dx = \frac{1}{3}"),
    ("bookmark", "https://mementum.dev"),
    ("paragraph", "Colonne de gauche."),
    ("paragraph", "Colonne de droite."),
    ("pageLink", "Sous-page"),
    ("paragraph", "Contenu synchronisé."),
];

const NON_DEFAULT: &[Row] = &[
    ("paragraph", "Paragraphe coloré et centré"),
    ("heading", "Titre repliable"),
    ("paragraph", "Contenu du titre."),
    ("table", "Nom Statut Prototype En cours"),
    ("table", "Fusionnée Seule Haute Milieu Droite Bas Coin"),
    ("image", "Logo sans aperçu logo.png"),
    ("paragraph", "Avant le saut après le saut"),
    ("paragraph", "Paragraphe parent"),
    ("paragraph", "Paragraphe enfant"),
    ("quote", "Citation parente"),
    ("paragraph", "Enfant de la citation"),
    ("paragraph", "Premier enfant du callout vide"),
    ("numberedListItem", "Trois"),
    ("numberedListItem", "Quatre"),
    (
        "paragraph",
        "Du texte surligné, un lien encodé et du souligné coloré.",
    ),
    ("checkListItem", "Tâche faite"),
    ("codeBlock", "let x = 1;"),
    ("codeBlock", "inconnu de la coloration"),
    ("paragraph", "Étroite."),
    ("paragraph", "Large."),
    ("video", "Démo"),
    ("file", "Le fichier spec.pdf"),
];

const RICH_BLOCKS: &[Row] = &[
    ("heading", "Blocs riches"),
    (
        "paragraph",
        "Ce que les écrans de la partie B savent poser.",
    ),
    ("callout", "Une idée, à l’icône et à la couleur par défaut."),
    ("paragraph", "Enfant du callout par défaut."),
    ("callout", "Attention, sur fond gris."),
    ("paragraph", "Enfant du callout gris."),
    ("callout", "Important, sur fond marron."),
    ("paragraph", "Enfant du callout marron."),
    ("callout", "Fait, sur fond rouge."),
    ("paragraph", "Enfant du callout rouge."),
    ("callout", "Question, sur fond orange."),
    ("paragraph", "Enfant du callout orange."),
    ("callout", "Épinglé, sur fond jaune."),
    ("paragraph", "Enfant du callout jaune."),
    ("callout", "Urgent, sur fond vert."),
    ("paragraph", "Enfant du callout vert."),
    ("callout", "Note, sur fond bleu."),
    ("paragraph", "Premier enfant du callout bleu."),
    ("paragraph", "Second enfant du callout bleu."),
    ("paragraph", "Enfant d’un callout violet sans texte."),
    ("callout", "Rose, avec une liste."),
    ("bulletListItem", "Premier point du callout rose"),
    ("bulletListItem", "Second point du callout rose"),
    ("heading", "Formules"),
    (
        "equation",
        "\\begin{aligned}\na &= b + c \\\\\nd &= e\n\\end{aligned}",
    ),
    ("equation", "a\n$$\nb"),
    (
        "paragraph",
        r"a^2 + b^2 = c^2 en tête, \frac{1}{2} au milieu, et à la fin \sqrt{2}",
    ),
    (
        "paragraph",
        r"\text{aire $x^{2}$} porte une formule dans son texte, et x\ finit par une barre oblique.",
    ),
    (
        "table",
        r"Formule dans une cellule Mention dans une cellule \text{$a|b$} et \frac{1}{2} 1 déc. 2026",
    ),
    ("heading", "Liens et mentions"),
    (
        "bookmark",
        "https://exemple.org/recherche?q=blocs&tri=date#resultats",
    ),
    ("pageLink", "Plan & budget <2027>"),
    (
        "paragraph",
        "Plan & budget <2027> est relu par Léa Martin avant le 1 déc. 2026.",
    ),
    ("heading", "Fichiers de l’instance"),
    ("image", "Le schéma « été » schéma \"été\".png"),
    ("video", "démo \"finale\".webm"),
    ("audio", "entretien \"Léa\".ogg"),
    ("file", "Rapport \"été\" 2026.pdf"),
];

#[test]
fn the_fixtures_project_as_written() {
    for (name, mmd, expected, linking) in [
        (
            "all-blocks",
            include_str!("../../../spec/mmd/fixtures/all-blocks.mmd"),
            ALL_BLOCKS,
            // Ses mentions et son lien de page portent des identifiants qui n'en sont pas
            // (`pg_spec01`) : aucune page visée.
            &[][..],
        ),
        (
            "non-default",
            include_str!("../../../spec/mmd/fixtures/non-default.mmd"),
            NON_DEFAULT,
            &[][..],
        ),
        (
            "rich-blocks",
            include_str!("../../../spec/mmd/fixtures/rich-blocks.mmd"),
            RICH_BLOCKS,
            // Le lien de page et la mention de page visent la même page ; les adresses des
            // fichiers ne sont pas des liens vers une page.
            &["Plan & budget <2027>", "Plan & budget <2027> est relu par"][..],
        ),
    ] {
        let blocks = mementum_md::parse(mmd).unwrap_or_else(|e| panic!("{name} : {e}"));
        let projection = project(&blocks, Some(ORIGIN));
        assert!(!projection.truncated, "{name}");
        let read: Vec<(&str, &str)> = projection
            .rows
            .iter()
            .map(|row| (row.kind, row.text.as_str()))
            .collect();
        assert_eq!(read, expected, "{name}");
        for row in &projection.rows {
            let expected_links = if linking.iter().any(|start| row.text.starts_with(start)) {
                vec![uuid(PLAN)]
            } else {
                Vec::new()
            };
            assert_eq!(row.links, expected_links, "{name} : {:?}", row.text);
        }
        // Chaque ligne a sa clé, et une seule.
        let distinct: HashSet<&str> = keys(&projection.rows).into_iter().collect();
        assert_eq!(distinct.len(), projection.rows.len(), "{name}");
    }
}

#[test]
fn marks_are_dropped_and_text_is_kept() {
    let styled = |text: &str, styles: Styles| Inline::Text {
        text: text.to_string(),
        styles,
    };
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![
            plain("Du "),
            styled(
                "gras",
                Styles {
                    bold: true,
                    ..Styles::default()
                },
            ),
            plain(", du "),
            styled(
                "code barré souligné",
                Styles {
                    code: true,
                    strike: true,
                    underline: true,
                    italic: true,
                    ..Styles::default()
                },
            ),
            plain(" et de la "),
            styled(
                "couleur",
                Styles {
                    text_color: Some("red".to_string()),
                    background_color: Some("yellow".to_string()),
                    ..Styles::default()
                },
            ),
            plain("."),
        ],
    )]);
    assert_eq!(
        row.text,
        "Du gras, du code barré souligné et de la couleur."
    );
    assert_eq!(row.kind, "paragraph");
    assert_eq!(row.block_id, "a");
    // Aucune couleur n'entre dans le texte.
    assert!(!row.text.contains("red") && !row.text.contains("yellow"));
    // Les marques ne changent rien à la ligne : le même texte sans marque a la même empreinte.
    let bare = only(&[para(
        "a",
        "Du gras, du code barré souligné et de la couleur.",
    )]);
    assert_eq!(row, bare);
}

#[test]
fn a_mention_is_read_by_its_label_and_linked_by_its_id() {
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![
            plain("Voir "),
            mention("page", PLAN, "Plan"),
            plain(" avec "),
            mention("user", OTHER, "Léa Martin"),
            plain(" le "),
            mention("date", "2026-12-01", "1 déc. 2026"),
        ],
    )]);
    assert_eq!(row.text, "Voir Plan avec Léa Martin le 1 déc. 2026");
    // Seule la mention d'une page vise une page : ni une personne, ni une date.
    assert_eq!(row.links, [uuid(PLAN)]);
    // L'identifiant n'est pas un texte cherché.
    assert!(!row.text.contains("0193"));

    // Une mention de page dont l'identifiant n'en est pas un, ou pas écrit à tirets : le
    // libellé, aucune page visée (la leçon de R754 : jamais `Uuid::parse_str` seul).
    for id in [
        "pg_spec01",
        "",
        "0193b2c45d6e7f809a1b2c3d4e5f6a7b",
        "{0193b2c4-5d6e-7f80-9a1b-2c3d4e5f6a7b}",
        "urn:uuid:0193b2c4-5d6e-7f80-9a1b-2c3d4e5f6a7b",
        " 0193b2c4-5d6e-7f80-9a1b-2c3d4e5f6a7b",
    ] {
        let row = only(&[inline("a", "paragraph", vec![mention("page", id, "Plan")])]);
        assert_eq!(row.text, "Plan");
        assert!(row.links.is_empty(), "{id:?}");
    }
    // Sans casse, rendue en minuscules.
    let upper = PLAN.to_uppercase();
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![mention("page", &upper, "Plan")],
    )]);
    assert_eq!(row.links, [uuid(PLAN)]);

    // Une mention sans libellé qui vise une page : pas de texte, mais la ligne porte le lien.
    let row = only(&[inline("a", "paragraph", vec![mention("page", PLAN, "")])]);
    assert_eq!((row.text.as_str(), &row.links[..]), ("", &[uuid(PLAN)][..]));
}

#[test]
fn a_link_to_a_page_of_the_instance_is_a_link_and_an_outside_address_is_not() {
    let table: Value =
        serde_json::from_str(include_str!("../../../spec/search/projection.json")).unwrap();
    let cases = table["links"].as_array().unwrap();
    assert!(cases.len() >= 35, "la table est lue");
    let (mut linked, mut unlinked) = (0, 0);
    for case in cases {
        let why = case["why"].as_str().unwrap();
        let href = case["href"].as_str().unwrap();
        let origin = case["origin"].as_str();
        let expected = case["page"].as_str().map(uuid);
        assert_eq!(page_link(href, origin), expected, "{why} : {href:?}");
        // La même règle par la projection : un lien d'un texte, et d'une cellule de tableau.
        let cell = Cell {
            content: vec![link(href, "dans une cellule")],
            props: Props::new(),
        };
        let blocks = [
            inline(
                "a",
                "paragraph",
                vec![plain("Voir "), link(href, "ce lien")],
            ),
            block(
                "b",
                "table",
                Content::Table(Table {
                    header_rows: 1,
                    header_cols: 0,
                    column_widths: vec![None],
                    rows: vec![vec![cell]],
                }),
            ),
        ];
        let projection = project(&blocks, origin);
        assert_eq!(keys(&projection.rows), ["a", "b"], "{why}");
        for row in &projection.rows {
            assert_eq!(row.links, expected.into_iter().collect::<Vec<_>>(), "{why}");
        }
        // Le texte d'un lien est cherché, jamais son adresse.
        assert_eq!(projection.rows[0].text, "Voir ce lien", "{why}");
        assert_eq!(projection.rows[1].text, "dans une cellule", "{why}");
        match expected {
            Some(_) => linked += 1,
            None => unlinked += 1,
        }
    }
    assert!(
        linked >= 8 && unlinked >= 20,
        "la table porte les deux issues"
    );

    // L'adresse d'un lien n'est pas indexée, même quand elle porte des mots.
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![link("https://mementum.dev/secret-budget", "ici")],
    )]);
    assert_eq!(row.text, "ici");
    assert!(!row.text.contains("budget") && !row.text.contains("mementum"));

    // Une origine vide n'en est pas une : elle ne fait pas d'une adresse quelconque un chemin.
    assert_eq!(page_link(&format!("/p/{PLAN}"), Some("")), Some(uuid(PLAN)));
    assert_eq!(
        page_link(&format!("https://x.example/p/{PLAN}"), Some("")),
        None
    );
    assert_eq!(page_link(&format!("//p/{PLAN}"), Some("/")), None);
}

#[test]
fn a_page_link_block_links_its_page_and_reads_its_copied_title() {
    let page_link_block = |page_id: PropValue, title: &str| {
        let block = with_prop(block("a", "pageLink", Content::None), "pageId", page_id);
        with_prop(block, "title", PropValue::Str(title.to_string()))
    };
    let row = only(&[page_link_block(PropValue::Str(PLAN.to_string()), "Plan")]);
    assert_eq!((row.kind, row.text.as_str()), ("pageLink", "Plan"));
    assert_eq!(row.links, [uuid(PLAN)]);
    // Sans titre copié : la ligne ne porte que le lien.
    let row = only(&[page_link_block(PropValue::Str(PLAN.to_string()), "")]);
    assert_eq!((row.text.as_str(), &row.links[..]), ("", &[uuid(PLAN)][..]));
    // Un identifiant qui n'en est pas un, sans tirets, ou qui n'est pas du texte : aucun lien.
    for id in [
        PropValue::Str("pg_child01".to_string()),
        PropValue::Str(PLAN.replace('-', "")),
        PropValue::Int(1),
    ] {
        let row = only(&[page_link_block(id, "Plan")]);
        assert!(row.links.is_empty());
    }
    // L'adresse d'un fichier, d'un signet : jamais un lien vers une page, même si elle en a la
    // forme (seuls un lien d'un texte, une mention et un lien de page en sont).
    let address = format!("/p/{PLAN}");
    for kind in ["image", "video", "audio", "file", "bookmark"] {
        let block = with_prop(
            block("a", kind, Content::None),
            "url",
            PropValue::Str(address.clone()),
        );
        let block = with_prop(block, "name", PropValue::Str("nom".to_string()));
        let projection = project(&[block], None);
        for row in &projection.rows {
            assert!(row.links.is_empty(), "{kind}");
        }
    }
}

#[test]
fn props_that_are_not_text_are_not_read() {
    // Une prop d'un autre type que du texte (un autre client écrit ce qu'il veut) : rien.
    let image = with_prop(
        block("a", "image", Content::None),
        "caption",
        PropValue::Int(3),
    );
    let image = with_prop(image, "name", PropValue::Bool(true));
    assert!(project(&[image], None).rows.is_empty());
    // Ce qui n'est pas dans la règle n'est pas cherché : la couleur, l'icône, la langue.
    let callout = with_prop(
        inline("a", "callout", vec![plain("Idée")]),
        "icon",
        PropValue::Str("ampoule".to_string()),
    );
    let callout = with_prop(callout, "color", PropValue::Str("purple".to_string()));
    assert_eq!(only(&[callout]).text, "Idée");
    let code = with_prop(
        inline("a", "codeBlock", vec![plain("let x = 1;")]),
        "language",
        PropValue::Str("rust".to_string()),
    );
    assert_eq!(only(&[code]).text, "let x = 1;");
    // L'adresse d'un fichier n'est pas cherchée, sa légende et son nom le sont.
    let file = with_prop(
        block("a", "file", Content::None),
        "url",
        PropValue::Str("https://mementum.dev/secret.pdf".to_string()),
    );
    let file = with_prop(file, "name", PropValue::Str("rapport.pdf".to_string()));
    let file = with_prop(file, "caption", PropValue::Str("Le rapport".to_string()));
    assert_eq!(only(&[file]).text, "Le rapport rapport.pdf");
}

#[test]
fn a_block_without_id_or_with_a_repeated_id_gets_a_stand_in_key() {
    let long = "x".repeat(MAX_BLOCK_ID_BYTES + 1);
    let at_bound = "y".repeat(MAX_BLOCK_ID_BYTES);
    let blocks = [
        para("a", "zéro"),
        para("", "un"),
        para("dup", "deux"),
        block("d", "divider", Content::None),
        with_children(
            para("dup", "quatre"),
            vec![para("", "cinq"), para("e", "six")],
        ),
        para("#1", "sept"),
        para(&long, "huit"),
        para(&at_bound, "neuf"),
        // Un bloc sans ligne compte aussi pour un identifiant répété.
        block("twice", "divider", Content::None),
        para("twice", "onze"),
    ];
    let projection = project(&blocks, None);
    assert_eq!(
        keys(&projection.rows),
        [
            "a",
            "#1",
            "#2",
            "#4",
            "#5",
            "e",
            "#7",
            "#8",
            at_bound.as_str(),
            "#11"
        ]
    );
    // Une clé de remplacement se reconnaît, un identifiant jamais pris pour elle.
    for row in &projection.rows {
        let stand_in = row.block_id.starts_with('#');
        assert_eq!(is_stand_in(&row.block_id), stand_in, "{}", row.block_id);
    }
    assert!(!is_stand_in("a") && !is_stand_in("") && is_stand_in("#0"));
    // Les clés d'une page sont toutes différentes, quoi qu'un client ait écrit comme identifiants.
    let distinct: HashSet<&str> = keys(&projection.rows).into_iter().collect();
    assert_eq!(distinct.len(), projection.rows.len());
    assert!(!projection.truncated);
}

#[test]
fn an_unknown_block_is_not_indexed_and_its_children_are() {
    let unknown = with_prop(
        block(
            "u",
            "unknown",
            Content::Inline(vec![plain("texte étranger")]),
        ),
        "data",
        PropValue::Str("eyJ0ZXh0Ijoic2VjcmV0In0=".to_string()),
    );
    let unknown = with_prop(
        unknown,
        "originalType",
        PropValue::Str("future_widget".to_string()),
    );
    let blocks = [
        with_children(unknown, vec![para("c1", "enfant de l'inconnu")]),
        // Un type que le registre ne connaît pas se lit comme un bloc inconnu.
        with_children(
            block("f", "future_widget", Content::Inline(vec![plain("futur")])),
            vec![para("c2", "enfant du futur")],
        ),
    ];
    let projection = project(&blocks, None);
    assert_eq!(
        said(&projection.rows),
        [
            ("paragraph", "enfant de l'inconnu", vec![]),
            ("paragraph", "enfant du futur", vec![]),
        ]
    );
    assert_eq!(keys(&projection.rows), ["c1", "c2"]);
}

/// Un bloc sans texte (colonnes, séparateur, bloc synchronisé, paragraphe vide) n'a pas de
/// ligne ; ses enfants, à toute profondeur, ont la leur.
#[test]
fn the_children_of_a_block_without_text_are_indexed() {
    let column =
        |id: &str, child: Block| with_children(block(id, "column", Content::None), vec![child]);
    let blocks = [
        with_children(
            block("cols", "columnList", Content::None),
            vec![
                column("c1", para("g", "gauche")),
                column(
                    "c2",
                    with_children(
                        block("s", "syncedBlock", Content::None),
                        vec![with_children(
                            para("empty", ""),
                            vec![with_children(
                                block("toc", "toc", Content::None),
                                vec![para("deep", "au fond")],
                            )],
                        )],
                    ),
                ),
            ],
        ),
        with_children(para("blank", " \t "), vec![para("after", "après")]),
        // Un bloc qui a du texte et des enfants : chacun sa ligne, le parent d'abord.
        with_children(para("p", "parent"), vec![para("k", "enfant")]),
    ];
    let projection = project(&blocks, None);
    assert_eq!(keys(&projection.rows), ["g", "deep", "after", "p", "k"]);
    assert_eq!(projection.rows[1].text, "au fond");
}

#[test]
fn a_table_is_read_cell_by_cell() {
    let cell = |content: Vec<Inline>| Cell {
        content,
        props: Props::new(),
    };
    let table = block(
        "t",
        "table",
        Content::Table(Table {
            header_rows: 1,
            header_cols: 0,
            column_widths: vec![None, None],
            rows: vec![
                vec![cell(vec![plain("Nom")]), cell(vec![plain("Statut")])],
                vec![cell(vec![]), cell(vec![mention("page", PLAN, "Plan")])],
                vec![cell(vec![plain("En "), plain("cours")]), cell(vec![])],
            ],
        }),
    );
    let row = only(&[table]);
    // Une espace entre deux cellules, aucune pour une cellule vide, aucune au bord.
    assert_eq!(row.text, "Nom Statut Plan En cours");
    assert_eq!(row.links, [uuid(PLAN)]);
}

#[test]
fn text_in_nfd_is_projected_in_nfc() {
    // « élève » en accents combinants, coupé entre deux textes au milieu d'un caractère.
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![plain("e\u{301}le"), plain("\u{300}ve")],
    )]);
    assert_eq!(row.text, "élève");
    assert_eq!(row, only(&[para("a", "élève")]));
}

#[test]
fn a_block_over_the_bound_is_cut_between_characters() {
    // Des caractères de deux octets décalés d'un octet : la borne tombe au milieu de l'un d'eux.
    let long = format!("a{}", "é".repeat(MAX_BLOCK_TEXT_BYTES));
    let projection = project(&[para("a", &long)], None);
    assert!(projection.truncated);
    let text = &projection.rows[0].text;
    assert_eq!(
        text.len(),
        MAX_BLOCK_TEXT_BYTES - 1,
        "coupé avant le caractère entamé"
    );
    assert!(long.starts_with(text.as_str()));

    // Témoin : à la borne juste, rien n'est coupé.
    let exact = "é".repeat(MAX_BLOCK_TEXT_BYTES / 2);
    let projection = project(&[para("a", &exact)], None);
    assert!(!projection.truncated);
    assert_eq!(projection.rows[0].text, exact);

    // Par plusieurs textes, des props, des cellules : la même borne.
    let piece = "né ".repeat(1000);
    let many = inline("a", "paragraph", (0..30).map(|_| plain(&piece)).collect());
    let projection = project(&[many], None);
    assert!(projection.truncated);
    assert!(projection.rows[0].text.len() <= MAX_BLOCK_TEXT_BYTES);
    assert!(projection.rows[0].text.len() > MAX_BLOCK_TEXT_BYTES - 4);
    let caption = with_prop(
        block("a", "image", Content::None),
        "caption",
        PropValue::Str("é".repeat(MAX_BLOCK_TEXT_BYTES)),
    );
    let projection = project(&[caption], None);
    assert!(projection.truncated);
    assert_eq!(projection.rows[0].text.len(), MAX_BLOCK_TEXT_BYTES);

    // En accents combinants : le texte gardé est en NFC, sous la borne ; hors de son dernier
    // caractère (la coupe peut séparer une lettre de son accent), c'est le début du texte.
    let nfd = "e\u{301}".repeat(MAX_BLOCK_TEXT_BYTES);
    let projection = project(&[para("a", &nfd)], None);
    assert!(projection.truncated);
    let text = &projection.rows[0].text;
    assert!(text.len() <= MAX_BLOCK_TEXT_BYTES);
    let mut kept = text.chars();
    kept.next_back();
    assert!(kept.clone().count() > 10_000);
    assert!(kept.all(|c| c == 'é'));

    // Un lien qui suit la coupe est encore lu : la borne est celle du texte.
    let cut_then_link = inline(
        "a",
        "paragraph",
        vec![plain(&long), mention("page", PLAN, "Plan")],
    );
    let projection = project(&[cut_then_link], None);
    assert!(projection.truncated);
    assert_eq!(projection.rows[0].links, [uuid(PLAN)]);
    assert!(!projection.rows[0].text.contains("Plan"));
}

#[test]
fn a_page_over_the_bound_is_marked_truncated() {
    let full = "x".repeat(MAX_BLOCK_TEXT_BYTES);
    let count = MAX_PAGE_TEXT_BYTES / MAX_BLOCK_TEXT_BYTES;
    let page = |blocks: usize, last: &str| -> Vec<Block> {
        let mut page: Vec<Block> = (0..blocks).map(|i| para(&format!("b{i}"), &full)).collect();
        page.push(para("last", last));
        page
    };
    // Témoin : à la borne juste, tout est gardé.
    let projection = project(&page(count - 1, &full), None);
    assert!(!projection.truncated);
    assert_eq!(projection.rows.len(), count);
    let total: usize = projection.rows.iter().map(|row| row.text.len()).sum();
    assert_eq!(total, MAX_PAGE_TEXT_BYTES);

    // Un bloc de plus : son texte n'est pas indexé, la page est dite coupée.
    let projection = project(&page(count, "débordement"), None);
    assert!(projection.truncated);
    assert_eq!(projection.rows.len(), count);
    assert!(projection.rows.iter().all(|row| row.block_id != "last"));

    // Un bloc à cheval sur la borne est coupé entre deux caractères.
    let mut astride = page(count - 1, &"x".repeat(MAX_BLOCK_TEXT_BYTES - 3));
    astride.push(para("tail", "éééé"));
    let projection = project(&astride, None);
    assert!(projection.truncated);
    assert_eq!(projection.rows.last().unwrap().text, "é");
    let total: usize = projection.rows.iter().map(|row| row.text.len()).sum();
    assert_eq!(total, MAX_PAGE_TEXT_BYTES - 1);

    // Une page pleine suivie de blocs sans texte n'est pas dite coupée ; et un bloc d'après la
    // borne garde ses liens.
    let mut then_links = page(count - 1, &full);
    then_links.push(block("d", "divider", Content::None));
    then_links.push(para("e", ""));
    then_links.push(inline("m", "paragraph", vec![mention("page", PLAN, "")]));
    let projection = project(&then_links, None);
    assert!(!projection.truncated);
    assert_eq!(projection.rows.last().unwrap().links, [uuid(PLAN)]);
}

#[test]
fn a_page_over_the_link_bound_is_marked_truncated() {
    let target = |i: usize| format!("0193b2c4-5d6e-7f80-9a1b-{i:012x}");
    let page = |links: usize| -> Vec<Block> {
        // Cent liens par bloc.
        (0..links)
            .collect::<Vec<_>>()
            .chunks(100)
            .enumerate()
            .map(|(b, chunk)| {
                let content = chunk
                    .iter()
                    .map(|i| mention("page", &target(*i), "p"))
                    .collect();
                inline(&format!("b{b}"), "paragraph", content)
            })
            .collect()
    };
    let count = |blocks: &[Block]| -> (usize, bool) {
        let projection = project(blocks, None);
        let links = projection.rows.iter().map(|row| row.links.len()).sum();
        (links, projection.truncated)
    };
    // Témoin : à la borne juste, tout est gardé.
    assert_eq!(count(&page(MAX_PAGE_LINKS)), (MAX_PAGE_LINKS, false));
    assert_eq!(count(&page(MAX_PAGE_LINKS + 1)), (MAX_PAGE_LINKS, true));
    assert_eq!(count(&page(3 * MAX_PAGE_LINKS)), (MAX_PAGE_LINKS, true));
    // La même page visée cent mille fois dans un bloc ne compte qu'une fois.
    let same = inline(
        "a",
        "paragraph",
        (0..100_000).map(|_| mention("page", PLAN, "")).collect(),
    );
    assert_eq!(count(&[same]), (1, false));
    // Les liens d'un bloc sont rangés, sans doublon, quel que soit leur ordre dans le texte.
    let mixed = inline(
        "a",
        "paragraph",
        vec![
            mention("page", &target(2), "b"),
            mention("page", &target(1), "a"),
            mention("page", &target(2), "b"),
        ],
    );
    assert_eq!(only(&[mixed]).links, [uuid(&target(1)), uuid(&target(2))]);
}

#[test]
fn the_hash_changes_with_the_text_the_type_and_the_links_and_with_nothing_else() {
    let base = || inline("a", "paragraph", vec![plain("Voir le plan")]);
    let hash = |block: Block| only(&[block]).hash;
    let reference = hash(base());
    assert_ne!(reference, [0; 32]);

    // Le texte.
    assert_ne!(hash(para("a", "Voir le plan.")), reference);
    assert_ne!(hash(para("a", "voir le plan")), reference);
    // Le type.
    assert_ne!(
        hash(inline("a", "heading", vec![plain("Voir le plan")])),
        reference
    );
    assert_ne!(
        hash(inline("a", "quote", vec![plain("Voir le plan")])),
        reference
    );
    // Les liens : le même texte, qui vise une page, une autre, deux.
    let to = |href: &str| {
        inline(
            "a",
            "paragraph",
            vec![plain("Voir "), link(href, "le plan")],
        )
    };
    let linked = hash(to(&format!("/p/{PLAN}")));
    assert_ne!(linked, reference);
    assert_ne!(hash(to(&format!("/p/{OTHER}"))), linked);
    let both = inline(
        "a",
        "paragraph",
        vec![
            link(&format!("/p/{PLAN}"), "Voir "),
            link(&format!("/p/{OTHER}"), "le plan"),
        ],
    );
    assert_ne!(hash(both), linked);
    // Un lien vers ailleurs n'est pas un lien vers une page : rien ne change.
    assert_eq!(hash(to("https://mementum.dev")), reference);

    // Rien d'autre : l'identifiant, les props, les marques, le découpage du texte, les enfants,
    // la place dans la page, l'origine publique.
    assert_eq!(hash(para("b", "Voir le plan")), reference);
    assert_eq!(hash(para("", "Voir le plan")), reference);
    let colored = with_prop(base(), "textColor", PropValue::Str("red".to_string()));
    let colored = with_prop(
        colored,
        "textAlignment",
        PropValue::Str("center".to_string()),
    );
    assert_eq!(hash(colored), reference);
    let bold = Inline::Text {
        text: "le plan".to_string(),
        styles: Styles {
            bold: true,
            ..Styles::default()
        },
    };
    assert_eq!(
        hash(inline("a", "paragraph", vec![plain("Voir "), bold])),
        reference
    );
    let nested = [with_children(base(), vec![para("c", "enfant")])];
    assert_eq!(project(&nested, None).rows[0].hash, reference);
    let moved = [
        para("x", "avant"),
        block("d", "divider", Content::None),
        base(),
    ];
    assert_eq!(project(&moved, None).rows[1].hash, reference);
    assert_eq!(project(&[base()], Some(ORIGIN)).rows[0].hash, reference);

    // Le type et le texte ne se confondent pas à leur frontière, ni le texte et les liens.
    let heading_level = with_prop(
        inline("a", "heading", vec![plain("Voir le plan")]),
        "level",
        PropValue::Int(3),
    );
    assert_eq!(
        hash(heading_level),
        hash(inline("a", "heading", vec![plain("Voir le plan")]))
    );
}

/// R948 : une mention ou une formule collée au texte ne se fond pas avec lui. Une espace les
/// sépare là où deux mots se toucheraient, et là seulement : la ponctuation reste collée.
#[test]
fn a_mention_or_a_formula_stuck_to_the_text_is_kept_apart() {
    let math = |latex: &str| Inline::Math {
        latex: latex.to_string(),
    };
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![
            plain("voir"),
            mention("user", OTHER, "Léa"),
            plain("et"),
            math("x^2"),
            plain("fin"),
        ],
    )]);
    assert_eq!(row.text, "voir Léa et x^2 fin");
    // Deux mentions, deux formules qui se suivent.
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![
            mention("user", OTHER, "Léa"),
            mention("page", PLAN, "Plan"),
            math("a"),
            math("b"),
        ],
    )]);
    assert_eq!(row.text, "Léa Plan a b");
    // Rien n'est ajouté là où une espace ou une ponctuation sépare déjà, ni au bord.
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![
            mention("user", OTHER, "Léa"),
            plain(", puis "),
            math("x"),
            plain(". ("),
            mention("page", PLAN, "Plan"),
            plain(")"),
        ],
    )]);
    assert_eq!(row.text, "Léa, puis x. (Plan)");
    // Un libellé vide ne laisse pas d'espace de plus, et deux textes voisins restent collés (ce
    // sont les morceaux d'un même texte, coupé par ses marques).
    let row = only(&[inline(
        "a",
        "paragraph",
        vec![
            plain("avant "),
            mention("page", PLAN, ""),
            plain("après"),
            plain("coup"),
        ],
    )]);
    assert_eq!(row.text, "avant aprèscoup");
    // Dans une cellule aussi.
    let cell = Cell {
        content: vec![plain("total"), math("n"), plain("lignes")],
        props: Props::new(),
    };
    let table = block(
        "t",
        "table",
        Content::Table(Table {
            header_rows: 1,
            header_cols: 0,
            column_widths: vec![None],
            rows: vec![vec![cell]],
        }),
    );
    assert_eq!(only(&[table]).text, "total n lignes");
}

/// Le type d'une ligne relue de la base se refait par `kind_named` ; la version de la projection
/// dit quand tout est à reprojeter.
#[test]
fn a_kind_read_back_is_a_name_of_the_table() {
    let registry: Value =
        serde_json::from_str(include_str!("../../../spec/schema/blocks.json")).unwrap();
    for spec in registry["blocks"].as_array().unwrap() {
        let name = spec["name"].as_str().unwrap();
        assert_eq!(kind_named(name), Some(name));
    }
    for other in ["", "future_widget", "Paragraph", "paragraph "] {
        assert_eq!(kind_named(other), None, "{other:?}");
    }
    // Le type d'une ligne projetée se relit tel quel.
    let row = only(&[para("a", "texte")]);
    let stored: String = row.kind.to_string();
    assert_eq!(kind_named(&stored), Some(row.kind));
    assert_eq!(PROJECTION_VERSION, 1);
}
