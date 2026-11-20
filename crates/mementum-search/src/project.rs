//! Projection d'une page en lignes d'index (plan du lot 4, « Quoi indexer » ; Rulings R901, R912,
//! R919).
//!
//! Une ligne par bloc qui porte du texte cherché ou un lien vers une page. Ce qui est cherché d'un
//! bloc tient à son type ([`rule`]) : le texte de son contenu en ligne, ses cellules, quelques
//! props, ou rien. Les enfants d'un bloc ont chacun leur ligne, que leur parent en ait une ou non.
//!
//! La projection est pure : elle ne lit que les blocs donnés, et rend toujours la même chose pour
//! la même page. Elle croît comme la page, sous trois bornes (R912) : le texte d'un bloc, le texte
//! d'une page, les liens d'une page ; ce qui dépasse n'est pas indexé, et la page est dite coupée.

use std::borrow::Cow;
use std::collections::HashSet;

use mementum_md::{Block, Content, Inline, PropValue};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::fold::nfc;

/// Texte indexé d'un bloc, en octets, au plus (Ruling R912). Le reste est coupé, entre deux
/// caractères.
pub const MAX_BLOCK_TEXT_BYTES: usize = 64 << 10;
/// Texte indexé d'une page, tous blocs réunis, en octets, au plus (Ruling R912).
pub const MAX_PAGE_TEXT_BYTES: usize = 8 << 20;
/// Liens vers des pages gardés pour une page, au plus (Ruling R912), comptés par bloc et par page
/// visée.
pub const MAX_PAGE_LINKS: usize = 10_000;
/// Longueur maximale, en octets, d'un identifiant de bloc pris pour clé (Ruling R941). Un
/// identifiant est une chaîne que le client choisit, sans borne ; BlockNote écrit des UUID (36
/// octets). Au-delà, le bloc est indexé sous une clé de remplacement.
pub const MAX_BLOCK_ID_BYTES: usize = 100;

/// Ce qui est cherché d'un bloc, d'après son type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Le texte de son contenu en ligne : marques retirées, le libellé d'une mention, le texte
    /// d'un lien (pas son adresse), le LaTeX d'une formule.
    Inline,
    /// Le texte de ses cellules, une espace entre deux.
    Table,
    /// Ces props, quand elles sont du texte, dans cet ordre, une espace entre deux.
    Props(&'static [&'static str]),
    /// Un lien de page : son titre copié (`title`, R251), et la page qu'il vise (`pageId`).
    PageLink,
    /// Rien. Ses enfants sont parcourus.
    Nothing,
}

/// La table : un type du registre (`spec/schema/blocks.json`), sa règle. Un test échoue si le
/// registre gagne un type qu'elle ne connaît pas.
const RULES: &[(&str, Rule)] = &[
    ("paragraph", Rule::Inline),
    ("heading", Rule::Inline),
    ("bulletListItem", Rule::Inline),
    ("numberedListItem", Rule::Inline),
    ("checkListItem", Rule::Inline),
    ("toggleListItem", Rule::Inline),
    ("quote", Rule::Inline),
    ("codeBlock", Rule::Inline),
    ("callout", Rule::Inline),
    ("table", Rule::Table),
    ("image", Rule::Props(&["caption", "name"])),
    ("video", Rule::Props(&["caption", "name"])),
    ("audio", Rule::Props(&["caption", "name"])),
    ("file", Rule::Props(&["caption", "name"])),
    ("bookmark", Rule::Props(&["url"])),
    ("equation", Rule::Props(&["latex"])),
    ("pageLink", Rule::PageLink),
    ("divider", Rule::Nothing),
    ("toc", Rule::Nothing),
    ("columnList", Rule::Nothing),
    ("column", Rule::Nothing),
    ("databaseRef", Rule::Nothing),
    ("syncedBlock", Rule::Nothing),
    // Un bloc d'un autre schéma, ou étranger : son texte n'est pas cherché (décision L17).
    ("unknown", Rule::Nothing),
];

fn entry(kind: &str) -> Option<&'static (&'static str, Rule)> {
    RULES.iter().find(|(name, _)| *name == kind)
}

/// La règle d'un type de bloc ; `None` pour un type que le registre ne connaît pas, lu comme un
/// bloc inconnu (rien de lui, ses enfants parcourus).
pub fn rule(kind: &str) -> Option<Rule> {
    entry(kind).map(|(_, rule)| *rule)
}

/// Une ligne d'index : ce qui se cherche d'un bloc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedBlock {
    /// La clé de la ligne dans sa page : l'identifiant du bloc, ou une clé de remplacement
    /// ([`is_stand_in`]). Unique dans une projection.
    pub block_id: String,
    /// Le type du bloc, un nom du registre.
    pub kind: &'static str,
    /// Le texte cherché, en NFC, d'au plus [`MAX_BLOCK_TEXT_BYTES`] octets. Vide pour un bloc
    /// qui ne porte que des liens.
    pub text: String,
    /// L'empreinte de `(kind, text, links)`, et de rien d'autre : une ligne dont l'empreinte n'a
    /// pas changé n'est pas récrite (Ruling R919).
    pub hash: [u8; 32],
    /// Les pages que le bloc vise, rangées, sans doublon.
    pub links: Vec<Uuid>,
}

/// La projection d'une page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Projection {
    /// Les lignes, dans l'ordre du document, un parent avant ses enfants.
    pub rows: Vec<IndexedBlock>,
    /// Vrai si une borne a fait laisser du texte ou des liens hors de l'index.
    pub truncated: bool,
}

/// Vrai pour une clé de remplacement (`#<rang>`), donnée à un bloc sans identifiant, ou dont
/// l'identifiant ne peut pas servir de clé. Ce n'est pas un bloc à atteindre : la réponse d'une
/// recherche ne la rend pas comme tel. Aucun identifiant de bloc gardé pour clé ne commence par
/// `#`.
pub fn is_stand_in(key: &str) -> bool {
    key.starts_with('#')
}

/// La page de l'instance que vise l'adresse d'un lien : le chemin exact `/p/<uuid>`, précédé ou
/// non de l'origine publique `origin` (`schéma://hôte[:port]`, comparée sans casse), suivi ou non
/// d'un fragment (`#b=<bloc>`). L'identifiant s'écrit à tirets, sans casse. Rien d'autre n'est
/// l'adresse d'une page : ni un autre hôte, ni une requête, ni un segment de plus, ni un
/// identifiant sans tirets (table de cas : `spec/search/projection.json`, « links »).
pub fn page_link(href: &str, origin: Option<&str>) -> Option<Uuid> {
    link_target(href, clean_origin(origin))
}

fn clean_origin(origin: Option<&str>) -> Option<&str> {
    origin
        .map(|origin| origin.trim_end_matches('/'))
        .filter(|origin| !origin.is_empty())
}

fn link_target(href: &str, origin: Option<&str>) -> Option<Uuid> {
    let path = origin
        .and_then(|origin| {
            let host = href.get(..origin.len())?;
            host.eq_ignore_ascii_case(origin)
                .then(|| &href[origin.len()..])
        })
        .unwrap_or(href);
    let rest = path.strip_prefix("/p/")?;
    let id = rest.split_once('#').map_or(rest, |(id, _fragment)| id);
    hyphenated(id)
}

/// L'identifiant `id`, s'il est écrit à tirets (8-4-4-4-12 chiffres hexadécimaux, sans casse) :
/// de 36 octets, `Uuid::parse_str` ne lit que cette forme. Seul, il lirait aussi les formes sans
/// tirets, entre accolades et `urn:uuid:`, que l'application n'écrit ni ne suit (la leçon de
/// R754).
fn hyphenated(id: &str) -> Option<Uuid> {
    (id.len() == 36).then(|| Uuid::parse_str(id).ok())?
}

/// Les blocs d'une page, à toute profondeur, dans l'ordre du document, un parent avant ses
/// enfants. Sans récursion : la profondeur d'une page ne pèse pas sur la pile.
fn walk(blocks: &[Block]) -> impl Iterator<Item = &Block> {
    let mut levels = vec![blocks.iter()];
    std::iter::from_fn(move || {
        loop {
            match levels.last_mut()?.next() {
                Some(block) => {
                    levels.push(block.children.iter());
                    return Some(block);
                }
                None => {
                    levels.pop();
                }
            }
        }
    })
}

/// Vrai si cet identifiant peut être la clé d'une ligne, à l'unicité près.
fn usable(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_BLOCK_ID_BYTES && !is_stand_in(id)
}

/// Les identifiants que plus d'un bloc de la page porte.
fn repeated_ids(blocks: &[Block]) -> HashSet<&str> {
    let mut seen = HashSet::new();
    let mut repeated = HashSet::new();
    for block in walk(blocks) {
        let id = block.id.as_str();
        if usable(id) && !seen.insert(id) {
            repeated.insert(id);
        }
    }
    repeated
}

/// Ce qui se lit d'un bloc : son texte, dans la place qui reste, et les pages qu'il vise.
struct Reader<'a> {
    text: String,
    /// La place du texte, en octets.
    room: usize,
    /// Du texte a été laissé.
    cut: bool,
    /// Une espace est due avant le prochain texte.
    gap: bool,
    links: Vec<Uuid>,
    origin: Option<&'a str>,
}

impl Reader<'_> {
    /// Ajoute du texte, coupé entre deux caractères à la borne. Après une coupe, plus rien
    /// n'entre : le texte gardé est un début du texte du bloc.
    fn push(&mut self, piece: &str) {
        if piece.is_empty() || self.cut {
            return;
        }
        if std::mem::take(&mut self.gap) && !self.text.is_empty() {
            self.push(" ");
        }
        let free = self.room - self.text.len();
        if piece.len() <= free {
            self.text.push_str(piece);
        } else {
            self.cut = true;
            self.text
                .push_str(&piece[..piece.floor_char_boundary(free)]);
        }
    }

    fn inlines(&mut self, content: &[Inline]) {
        for inline in content {
            match inline {
                Inline::Text { text, .. } => self.push(text),
                // Le texte d'un lien, jamais son adresse.
                Inline::Link { href, children } => {
                    self.links.extend(link_target(href, self.origin));
                    self.inlines(children);
                }
                Inline::Mention { kind, id, label } => {
                    if kind == "page" {
                        self.links.extend(hyphenated(id));
                    }
                    self.push(label);
                }
                Inline::Math { latex } => self.push(latex),
                Inline::HardBreak => self.push(" "),
            }
        }
    }

    fn prop(&mut self, block: &Block, name: &str) {
        if let Some(PropValue::Str(value)) = block.props.get(name) {
            self.gap = true;
            self.push(value);
        }
    }

    fn block(&mut self, block: &Block, rule: Rule) {
        match (rule, &block.content) {
            (Rule::Inline, Content::Inline(content)) => self.inlines(content),
            (Rule::Table, Content::Table(table)) => {
                for cell in table.rows.iter().flatten() {
                    self.gap = true;
                    self.inlines(&cell.content);
                }
            }
            (Rule::Props(names), _) => {
                for name in names {
                    self.prop(block, name);
                }
            }
            (Rule::PageLink, _) => {
                self.prop(block, "title");
                if let Some(PropValue::Str(id)) = block.props.get("pageId") {
                    self.links.extend(hyphenated(id));
                }
            }
            _ => {}
        }
    }
}

fn hash(kind: &str, text: &str, links: &[Uuid]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    // Chaque part précédée de sa longueur : deux lignes différentes ne s'écrivent jamais pareil.
    hasher.update((kind.len() as u64).to_le_bytes());
    hasher.update(kind);
    hasher.update((text.len() as u64).to_le_bytes());
    hasher.update(text);
    for link in links {
        hasher.update(link.as_bytes());
    }
    hasher.finalize().into()
}

/// Projette une page lue en lignes d'index.
///
/// `origin` est l'origine publique de l'instance (`https://notes.exemple.fr`), si elle en a une :
/// un lien écrit sous elle vers `/p/<uuid>` vise une page de l'instance, comme le chemin seul
/// ([`page_link`]). Elle ne change rien d'autre, et pas l'empreinte d'une ligne qui ne porte pas
/// un tel lien.
///
/// Un bloc sans identifiant, dont l'identifiant paraît plus d'une fois dans la page, dépasse
/// [`MAX_BLOCK_ID_BYTES`] ou commence par `#`, est indexé sous la clé `#<rang>`, son rang dans la
/// page (tous les blocs comptés, à partir de zéro, dans l'ordre du document).
pub fn project(blocks: &[Block], origin: Option<&str>) -> Projection {
    let origin = clean_origin(origin);
    let repeated = repeated_ids(blocks);
    let mut projection = Projection::default();
    let mut text_room = MAX_PAGE_TEXT_BYTES;
    let mut link_room = MAX_PAGE_LINKS;
    for (rank, block) in walk(blocks).enumerate() {
        let Some(&(kind, rule)) = entry(&block.kind) else {
            continue;
        };
        let mut reader = Reader {
            text: String::new(),
            room: MAX_BLOCK_TEXT_BYTES.min(text_room),
            cut: false,
            gap: false,
            links: Vec::new(),
            origin,
        };
        reader.block(block, rule);
        let Reader {
            text,
            room,
            cut,
            mut links,
            ..
        } = reader;
        projection.truncated |= cut;
        // En NFC (Ruling R926), sur le texte entier du bloc : une lettre et son accent écrits
        // dans deux plages voisines se composent. La forme NFC peut être plus longue.
        let composed = match nfc(&text) {
            Cow::Borrowed(_) => None,
            Cow::Owned(composed) => Some(composed),
        };
        let text = match composed {
            None => text,
            Some(mut composed) => {
                if composed.len() > room {
                    composed.truncate(composed.floor_char_boundary(room));
                    projection.truncated = true;
                }
                composed
            }
        };
        links.sort_unstable();
        links.dedup();
        if links.len() > link_room {
            links.truncate(link_room);
            projection.truncated = true;
        }
        if text.trim().is_empty() && links.is_empty() {
            continue;
        }
        text_room -= text.len();
        link_room -= links.len();
        let id = block.id.as_str();
        let block_id = if usable(id) && !repeated.contains(id) {
            id.to_string()
        } else {
            format!("#{rank}")
        };
        projection.rows.push(IndexedBlock {
            block_id,
            kind,
            hash: hash(kind, &text, &links),
            text,
            links,
        });
    }
    projection
}
