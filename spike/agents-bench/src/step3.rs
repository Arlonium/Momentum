//! Step 3 : un contenu en ligne marqué (gras, lien, mention) écrit par préfixe et suffixe communs,
//! relu par le vrai Yjs (`probe/inline.mjs`), avec une frappe concurrente du vrai Yjs.
//!
//! `inline-base <dossier>` écrit, par scène : l'état de départ (`base.bin`, écrit par le
//! serveur), les deux écritures du serveur (`affix.bin`, `whole.bin`, faites sur une page relue de
//! `base.bin`), et ce que la page voulue se lit (`expected.json`, d'une page neuve écrite avec les
//! blocs voulus). La sonde y ajoute les frappes du vrai Yjs (`key-*.bin`) ; `inline-apply` relit
//! avec yrs toutes les combinaisons (`yrs-*.json`), que la sonde compare à sa propre lecture.

use std::path::Path;

use mementum_crdt::{FRAGMENT, PageDoc};
use mementum_md::{Block, parse};
use serde_json::{Map, Value, json};
use yrs::types::text::YChange;
use yrs::{Any, Out, ReadTxn, Text, Transact, Xml, XmlFragment, XmlOut};

use crate::diff::{InlineMode, Options, write_diff};

const BASE: &str = "Du **gras** au milieu, un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n";

/// (nom, MMD voulu) : chaque scène change le premier paragraphe et laisse le second.
pub const SCENES: &[(&str, &str)] = &[
    (
        "dans-le-gras",
        "Du **gros** au milieu, un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "a-la-frontiere",
        "Du **gras**, au milieu, un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "dans-le-lien",
        "Du **gras** au milieu, un [liens](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "apres-la-mention",
        "Du **gras** au milieu, un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin du paragraphe.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "marques-seules",
        "Du **gras** au *milieu*, un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "a-cheval",
        "Du **g**milieu, un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "en-tete",
        "Oui. Du **gras** au milieu, un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "lien-pose",
        "Du **gras** au [milieu](https://autre.example), un [lien](https://exemple.org) et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
    (
        "lien-retire",
        "Du **gras** au milieu, un lien et \
<mention type=\"page\" id=\"pg_1\">Page</mention> fin.\n\nUn paragraphe que personne ne touche.\n",
    ),
];

pub fn base(args: &[String]) {
    let dir = Path::new(&args[0]);
    let base_blocks = parse(BASE).unwrap();
    let (page, first) = PageDoc::new_page();
    let written = page.write_blocks(&base_blocks).unwrap();
    let state = page.encode_state();
    drop((first, written));
    let current = page.read_blocks();
    for (name, mmd) in SCENES {
        let scene = dir.join(name);
        std::fs::create_dir_all(&scene).unwrap();
        std::fs::write(scene.join("base.bin"), &state).unwrap();
        // Les blocs voulus portent les identifiants courants : la différence se juge sur le
        // contenu en ligne seul.
        let mut target = parse(mmd).unwrap();
        for (t, c) in target.iter_mut().zip(&current) {
            t.id = c.id.clone();
        }
        for (mode, inline) in [("affix", InlineMode::Affix), ("whole", InlineMode::Whole)] {
            let page = PageDoc::from_updates([state.as_slice()]).unwrap();
            let stats = write_diff(
                &page,
                &target,
                Options {
                    inline,
                    two_txns: false,
                },
            )
            .expect("page de forme connue");
            assert_eq!(stats.updates.len(), 1);
            println!(
                "{name} {mode} : {} octets ; préfixe {} ; entier {}",
                stats.bytes(),
                stats.inline_affix,
                stats.inline_whole
            );
            std::fs::write(scene.join(format!("{mode}.bin")), &stats.updates[0]).unwrap();
            if let Some(&(child, at, len)) = stats.affix_at.first() {
                std::fs::write(
                    scene.join("edit.json"),
                    json!({ "child": child, "at": at, "len": len }).to_string(),
                )
                .unwrap();
            }
        }
        let fresh = {
            let (page, _) = PageDoc::new_page();
            page.write_blocks(&target).unwrap();
            page
        };
        std::fs::write(
            scene.join("expected.json"),
            serde_json::to_string_pretty(&read_page(&fresh)).unwrap(),
        )
        .unwrap();
    }
}

/// Relit avec yrs chaque combinaison que la sonde a préparée : `combos.json` liste des suites de
/// fichiers à appliquer dans l'ordre, et le nom du fichier de sortie.
pub fn apply(args: &[String]) {
    let dir = Path::new(&args[0]);
    let combos: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("combos.json")).unwrap()).unwrap();
    for combo in combos.as_array().unwrap() {
        let scene = dir.join(combo["scene"].as_str().unwrap());
        let files: Vec<Vec<u8>> = combo["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| std::fs::read(scene.join(f.as_str().unwrap())).unwrap())
            .collect();
        let page = PageDoc::from_updates(files.iter().map(Vec::as_slice)).unwrap();
        let out = scene.join(combo["out"].as_str().unwrap());
        std::fs::write(out, serde_json::to_string_pretty(&read_page(&page)).unwrap()).unwrap();
    }
}

/// Chaque bloc de premier niveau : le type de son nœud de contenu et ses enfants, textes (leurs
/// plages, par `diff`, jamais `get_string`) et éléments en ligne.
pub fn read_page(page: &PageDoc) -> Value {
    let doc = page.yrs_doc();
    let fragment = doc.get_or_insert_xml_fragment(FRAGMENT);
    let txn = doc.transact();
    let Some(XmlOut::Element(group)) = fragment.get(&txn, 0) else {
        return json!(null);
    };
    let mut blocks = Vec::new();
    for container in group.children(&txn) {
        let XmlOut::Element(container) = container else { continue };
        for content in container.children(&txn) {
            let XmlOut::Element(content) = content else { continue };
            if content.tag().as_ref() == "blockGroup" {
                continue;
            }
            let mut children = Vec::new();
            for child in content.children(&txn) {
                match child {
                    XmlOut::Text(text) => {
                        let runs: Vec<Value> = text
                            .diff(&txn, YChange::identity)
                            .into_iter()
                            .map(|d| {
                                let insert = match d.insert {
                                    Out::Any(a) => any_json(&a),
                                    other => json!(format!("{other:?}")),
                                };
                                let attrs: Map<String, Value> = d
                                    .attributes
                                    .map(|a| {
                                        a.iter()
                                            .map(|(k, v)| (k.to_string(), any_json(v)))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                json!({ "insert": insert, "attributes": attrs })
                            })
                            .collect();
                        children.push(json!({ "text": runs }));
                    }
                    XmlOut::Element(e) => {
                        let attrs: Map<String, Value> = e
                            .attributes(&txn)
                            .map(|(k, v)| {
                                let v = match v {
                                    Out::Any(a) => any_json(&a),
                                    other => json!(format!("{other:?}")),
                                };
                                (k.to_string(), v)
                            })
                            .collect();
                        children.push(json!({ "el": e.tag().to_string(), "attrs": attrs }));
                    }
                    XmlOut::Fragment(_) => {}
                }
            }
            blocks.push(json!({ "type": content.tag().to_string(), "children": children }));
            break;
        }
    }
    json!(blocks)
}

fn any_json(a: &Any) -> Value {
    match a {
        Any::Null | Any::Undefined => Value::Null,
        Any::Bool(b) => json!(b),
        Any::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 9e15 => json!(f as i64),
            Some(f) => json!(f),
            None => Value::Null,
        },
        Any::String(s) => json!(s.as_ref()),
        Any::Buffer(b) => json!(format!("<{} octets>", b.len())),
        Any::Array(items) => Value::Array(items.iter().map(any_json).collect()),
        Any::Map(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), any_json(v))).collect()),
    }
}
