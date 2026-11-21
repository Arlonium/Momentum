//! Les alias de blocs et les empreintes de blocs des agents (Rulings R1224, R1226, R1231 ; Task S3
//! du lot 5).
//!
//! **Un alias** est `b` suivi des premiers caractères, en base32 minuscule (RFC 4648 :
//! `a`…`z`, `2`…`7`), du SHA-256 de l'identifiant du bloc : quatre au moins, autant qu'il en faut
//! pour être seul dans la page lue ([`Anchors::of`]), et jamais l'identifiant d'un autre bloc. Il
//! se résout sans rien garder : le bloc de la page dont le hachage commence ainsi ; l'identifiant
//! entier d'un bloc est toujours accepté. Un alias ne dépend que de l'identifiant de son bloc et de
//! ses voisins de hachage : un bloc inséré au-dessus ne le change pas, sauf s'il partage son début
//! (l'alias s'allonge alors, et l'ancien désigne deux blocs : `ambiguous_anchor`).
//!
//! **Les empreintes** d'un bloc ([`BlockHash`]) : son identifiant ([`id_key`] : les 64 premiers
//! bits du SHA-256 de l'identifiant, ceux de son alias, si bien qu'un bloc parti se nomme encore
//! par son alias), son contenu (type, props, contenu en ligne ou tableau, sans les enfants) et sa
//! place (le parent et le frère d'avant), ces deux-là sous une clé tirée au démarrage du processus
//! ([`HashKey`]) : un client ne fabrique pas deux contenus de même empreinte. Un bloc déplacé, ou
//! une liste dont l'ordre change, change de place.
//!
//! Tout se calcule en une passe sur les blocs lus une fois (R641) : un index, puis des recherches
//! dans l'index, jamais un parcours de la page par bloc cherché.

use std::collections::hash_map::{DefaultHasher, RandomState};
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};

use mementum_md::{Block, Content, Inline, PropValue, Props, Styles, Table};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Caractères d'un alias après le `b`, au moins.
pub const MIN_ALIAS_CHARS: usize = 4;

/// Caractères base32 d'un SHA-256 entier (256 bits, cinq par caractère, le dernier complété).
const DIGEST_CHARS: usize = 52;

/// Caractères base32 entiers que portent les 64 bits de [`id_key`].
const KEY_CHARS: usize = 12;

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// La clé des empreintes de contenu et de place, tirée au démarrage du processus (R1226) : deux
/// clés donnent deux empreintes. Les empreintes ne valent que dans le processus qui les a
/// calculées. C'est le SipHash de la bibliothèque standard, sous une clé tirée au hasard
/// (`RandomState`), jamais `DefaultHasher::new`, à clé fixe.
#[derive(Clone, Debug)]
pub struct HashKey(RandomState);

impl HashKey {
    /// Une clé neuve, tirée au hasard.
    pub fn new() -> HashKey {
        HashKey(RandomState::new())
    }

    fn hasher(&self, domain: u8) -> DefaultHasher {
        let mut h = self.0.build_hasher();
        h.write_u8(domain);
        h
    }
}

impl Default for HashKey {
    fn default() -> HashKey {
        HashKey::new()
    }
}

fn digest(id: &str) -> [u8; 32] {
    Sha256::digest(id.as_bytes()).into()
}

/// L'empreinte d'un identifiant de bloc : les 64 premiers bits de son SHA-256, ceux dont son
/// alias est écrit (R1224).
pub fn id_key(id: &str) -> u64 {
    let d = digest(id);
    u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]])
}

/// Les `chars` premiers caractères base32 de `bytes`, les bits du dernier octet complétés de
/// zéros.
fn base32(bytes: &[u8], chars: usize) -> String {
    let mut out = String::with_capacity(chars);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &byte in bytes {
        acc = (acc << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(BASE32[((acc >> bits) & 31) as usize]));
            if out.len() == chars {
                return out;
            }
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 && out.len() < chars {
        out.push(char::from(BASE32[((acc << (5 - bits)) & 31) as usize]));
    }
    out
}

/// Le `j`-ième caractère base32 d'un hachage, en valeur (0 à 31) : ses bits `5j` à `5j + 4`,
/// depuis le bit de poids fort, le dernier complété de zéros.
fn group(d: &[u8; 32], j: usize) -> u8 {
    let bit = j * 5;
    let (byte, offset) = (bit / 8, bit % 8);
    let high = u16::from(d[byte]) << 8;
    let low = d.get(byte + 1).map_or(0, |&b| u16::from(b));
    (((high | low) >> (11 - offset)) & 31) as u8
}

/// Caractères base32 communs en tête de deux hachages.
fn common_chars(a: &[u8; 32], b: &[u8; 32]) -> usize {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        None => DIGEST_CHARS,
        Some(i) => (i * 8 + (a[i] ^ b[i]).leading_zeros() as usize) / 5,
    }
}

/// La valeur d'un caractère base32.
fn value_of(c: u8) -> u8 {
    match c {
        b'a'..=b'z' => c - b'a',
        _ => c - b'2' + 26,
    }
}

/// Le début d'un hachage comparé à des caractères base32 (en valeurs).
fn cmp_prefix(d: &[u8; 32], prefix: &[u8]) -> std::cmp::Ordering {
    for (j, &p) in prefix.iter().enumerate() {
        match group(d, j).cmp(&p) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Ce qu'une ancre ne désigne pas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorError {
    /// Aucun bloc de la page : `stale_anchor`.
    Stale,
    /// Plusieurs blocs : `ambiguous_anchor`.
    Ambiguous,
}

/// L'index des ancres d'une page : l'alias d'un bloc, et la résolution d'une ancre (un alias, ou
/// l'identifiant entier d'un bloc). Bâti une fois, sur les blocs lus : le hachage de chaque
/// identifiant, rangé ; un alias se calcule à la demande, d'après ses deux voisins de hachage
/// (mesure de la Task S3 : bâtir les 10 000 alias d'une page d'un coup coûtait 9 ms).
#[derive(Debug, Clone, Default)]
pub struct Anchors {
    /// Le SHA-256 de l'identifiant de chaque bloc qui en a un, et l'identifiant, rangés par
    /// hachage : deux blocs du même identifiant sont voisins.
    sorted: Vec<([u8; 32], String)>,
}

impl Anchors {
    /// L'index des blocs `blocks`, à toute profondeur. Un bloc sans identifiant n'a pas d'alias ;
    /// deux blocs du même identifiant ont le hachage entier, qui les désigne tous deux.
    pub fn of(blocks: &[Block]) -> Anchors {
        fn walk(blocks: &[Block], into: &mut Vec<([u8; 32], String)>) {
            for b in blocks {
                if !b.id.is_empty() {
                    into.push((digest(&b.id), b.id.clone()));
                }
                walk(&b.children, into);
            }
        }
        let mut sorted = Vec::new();
        walk(blocks, &mut sorted);
        sorted.sort_unstable();
        Anchors { sorted }
    }

    /// Les rangs des blocs dont le hachage est `d`.
    fn equal(&self, d: &[u8; 32]) -> std::ops::Range<usize> {
        let start = self.sorted.partition_point(|(e, _)| e < d);
        let end = start + self.sorted[start..].partition_point(|(e, _)| e == d);
        start..end
    }

    /// L'alias du bloc d'identifiant `id`, s'il est dans la page et que son identifiant n'est pas
    /// vide : autant de caractères qu'il en faut pour être seul, quatre au moins, et jamais
    /// l'identifiant d'un autre bloc.
    pub fn alias(&self, id: &str) -> Option<String> {
        if id.is_empty() {
            return None;
        }
        let d = digest(id);
        let range = self.equal(&d);
        if !self.sorted[range.clone()].iter().any(|(_, i)| i == id) {
            return None;
        }
        let before = range
            .start
            .checked_sub(1)
            .map_or(0, |p| common_chars(&self.sorted[p].0, &d));
        let after = self
            .sorted
            .get(range.end)
            .map_or(0, |(e, _)| common_chars(e, &d));
        let chars = if range.len() > 1 {
            DIGEST_CHARS
        } else {
            (before.max(after) + 1).clamp(MIN_ALIAS_CHARS, DIGEST_CHARS)
        };
        Some(format!("b{}", base32(&d, chars)))
    }

    /// L'identifiant du bloc que désigne `anchor` : une ancre qui a la forme d'un alias
    /// ([`is_alias`]) désigne le bloc dont le hachage commence ainsi, **jamais** un bloc par son
    /// identifiant (relecture de S3, M4 : un client qui choisit ses identifiants ne capte pas
    /// l'alias d'un autre bloc) ; toute autre ancre, le bloc qui porte cet identifiant. Un seul,
    /// ou le refus qui dit pourquoi.
    pub fn resolve(&self, anchor: &str) -> Result<&str, AnchorError> {
        let mut found: Option<&str> = None;
        let mut count = 0;
        if is_alias(anchor) {
            let prefix: Vec<u8> = anchor[1..].bytes().map(value_of).collect();
            let start = self
                .sorted
                .partition_point(|(d, _)| cmp_prefix(d, &prefix).is_lt());
            for (d, id) in &self.sorted[start..] {
                if !cmp_prefix(d, &prefix).is_eq() || count > 1 {
                    break;
                }
                found = Some(id.as_str());
                count += 1;
            }
        } else {
            for (_, id) in &self.sorted[self.equal(&digest(anchor))] {
                if id == anchor {
                    found = Some(id.as_str());
                    count += 1;
                }
            }
        }
        match (count, found) {
            (1, Some(id)) => Ok(id),
            (0, _) => Err(AnchorError::Stale),
            _ => Err(AnchorError::Ambiguous),
        }
    }
}

/// Un alias du bloc d'identifiant `id` qui se donne sans lire sa page (l'outil `search`, lot 5,
/// Task M2) : `b` et les douze premiers caractères base32 de son hachage, ceux de [`id_key`]. Il se
/// résout comme tout alias, par un début de hachage ([`Anchors::resolve`]) : plus long que celui
/// que rend la page lue, il ne désigne deux blocs que si leurs hachages ont soixante bits
/// communs, ou s'ils portent le même identifiant. `None` pour un identifiant vide.
pub fn standalone_alias(id: &str) -> Option<String> {
    (!id.is_empty()).then(|| format!("b{}", base32(&digest(id), KEY_CHARS)))
}

/// Vrai si l'empreinte d'identifiant `key` ([`id_key`]) commence par l'alias `alias` (ses douze
/// premiers caractères au plus : les 64 bits n'en portent pas plus).
pub(crate) fn key_matches_alias(key: u64, alias: &str) -> bool {
    let mut d = [0u8; 32];
    d[..8].copy_from_slice(&key.to_be_bytes());
    let prefix: Vec<u8> = alias[1..].bytes().take(KEY_CHARS).map(value_of).collect();
    cmp_prefix(&d, &prefix).is_eq()
}

/// La plus petite empreinte d'identifiant ([`id_key`]) qui commence par l'alias `alias` (ses douze
/// premiers caractères au plus).
pub(crate) fn alias_floor(alias: &str) -> u64 {
    let (mut value, mut bits) = (0u64, 0u32);
    for c in alias[1..].bytes().take(KEY_CHARS) {
        value = (value << 5) | u64::from(value_of(c));
        bits += 5;
    }
    value << (64 - bits)
}

/// Vrai si `anchor` a la forme d'un alias : `b`, puis de 4 à 52 caractères base32 minuscules.
pub fn is_alias(anchor: &str) -> bool {
    let Some(rest) = anchor.strip_prefix('b') else {
        return false;
    };
    (MIN_ALIAS_CHARS..=DIGEST_CHARS).contains(&rest.len())
        && rest
            .bytes()
            .all(|c| c.is_ascii_lowercase() || (b'2'..=b'7').contains(&c))
}

/// Les empreintes d'un bloc (R1226) : 24 octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockHash {
    /// [`id_key`] de son identifiant.
    pub id: u64,
    /// Type, props et contenu, sans les enfants, sous la clé du processus.
    pub content: u64,
    /// Le parent et le frère d'avant, sous la clé du processus.
    pub place: u64,
}

/// Les empreintes des blocs d'une page, dans l'ordre du document (préfixe : un bloc, puis ses
/// enfants).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockHashes {
    blocks: Vec<BlockHash>,
}

impl BlockHashes {
    /// Les empreintes, dans l'ordre du document.
    pub fn as_slice(&self) -> &[BlockHash] {
        &self.blocks
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Ce que ces empreintes pèsent en mémoire, à peu près (pour un cache borné en octets).
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<BlockHashes>() + self.blocks.capacity() * size_of::<BlockHash>()
    }

    /// Les empreintes par identifiant : `None` pour un identifiant que plusieurs blocs portent.
    fn by_id(&self) -> HashMap<u64, Option<usize>> {
        let mut out: HashMap<u64, Option<usize>> = HashMap::with_capacity(self.blocks.len());
        for (rank, h) in self.blocks.iter().enumerate() {
            out.entry(h.id)
                .and_modify(|seen| *seen = None)
                .or_insert(Some(rank));
        }
        out
    }
}

/// Les empreintes de `blocks` sous la clé `key`.
pub fn hashes(blocks: &[Block], key: &HashKey) -> BlockHashes {
    fn walk(blocks: &[Block], parent: Option<&str>, key: &HashKey, out: &mut Vec<BlockHash>) {
        let mut previous: Option<&str> = None;
        for b in blocks {
            out.push(BlockHash {
                id: id_key(&b.id),
                content: content_hash(b, key),
                place: place_hash(parent, previous, key),
            });
            walk(&b.children, Some(&b.id), key, out);
            previous = Some(&b.id);
        }
    }
    let mut out = Vec::new();
    walk(blocks, None, key, &mut out);
    // Gardées dans un cache borné en octets (S7) : 24 octets par bloc, pas la marge du vecteur.
    out.shrink_to_fit();
    BlockHashes { blocks: out }
}

fn feed_str(h: &mut impl Hasher, s: &str) {
    h.write_usize(s.len());
    h.write(s.as_bytes());
}

fn feed_opt(h: &mut impl Hasher, s: &Option<String>) {
    match s {
        None => h.write_u8(0),
        Some(s) => {
            h.write_u8(1);
            feed_str(h, s);
        }
    }
}

fn feed_props(h: &mut impl Hasher, props: &Props) {
    h.write_usize(props.len());
    for (name, value) in props {
        feed_str(h, name);
        match value {
            PropValue::Str(s) => {
                h.write_u8(0);
                feed_str(h, s);
            }
            PropValue::Int(i) => {
                h.write_u8(1);
                h.write_i64(*i);
            }
            PropValue::Float(f) => {
                h.write_u8(2);
                h.write_u64(f.to_bits());
            }
            PropValue::Bool(b) => {
                h.write_u8(3);
                h.write_u8(u8::from(*b));
            }
        }
    }
}

fn feed_styles(h: &mut impl Hasher, s: &Styles) {
    h.write_u8(
        u8::from(s.bold)
            | u8::from(s.italic) << 1
            | u8::from(s.underline) << 2
            | u8::from(s.strike) << 3
            | u8::from(s.code) << 4,
    );
    feed_opt(h, &s.text_color);
    feed_opt(h, &s.background_color);
}

fn feed_inline(h: &mut impl Hasher, content: &[Inline]) {
    h.write_usize(content.len());
    for inline in content {
        match inline {
            Inline::Text { text, styles } => {
                h.write_u8(0);
                feed_str(h, text);
                feed_styles(h, styles);
            }
            Inline::Link { href, children } => {
                h.write_u8(1);
                feed_str(h, href);
                feed_inline(h, children);
            }
            Inline::Mention { kind, id, label } => {
                h.write_u8(2);
                feed_str(h, kind);
                feed_str(h, id);
                feed_str(h, label);
            }
            Inline::Math { latex } => {
                h.write_u8(3);
                feed_str(h, latex);
            }
            Inline::HardBreak => h.write_u8(4),
        }
    }
}

fn feed_table(h: &mut impl Hasher, t: &Table) {
    h.write_u32(t.header_rows);
    h.write_u32(t.header_cols);
    h.write_usize(t.column_widths.len());
    for width in &t.column_widths {
        match width {
            None => h.write_u8(0),
            Some(w) => {
                h.write_u8(1);
                h.write_u64(w.to_bits());
            }
        }
    }
    h.write_usize(t.rows.len());
    for row in &t.rows {
        h.write_usize(row.len());
        for cell in row {
            feed_inline(h, &cell.content);
            feed_props(h, &cell.props);
        }
    }
}

/// L'empreinte du contenu d'un bloc : type, props, contenu, sans les enfants.
pub(crate) fn content_hash(b: &Block, key: &HashKey) -> u64 {
    let mut h = key.hasher(b'C');
    feed_str(&mut h, &b.kind);
    feed_props(&mut h, &b.props);
    match &b.content {
        Content::None => h.write_u8(0),
        Content::Inline(content) => {
            h.write_u8(1);
            feed_inline(&mut h, content);
        }
        Content::Table(table) => {
            h.write_u8(2);
            feed_table(&mut h, table);
        }
    }
    h.finish()
}

/// L'empreinte de la place d'un bloc : son parent (aucun : la racine) et son frère d'avant
/// (aucun : le premier).
pub(crate) fn place_hash(parent: Option<&str>, previous: Option<&str>, key: &HashKey) -> u64 {
    let mut h = key.hasher(b'P');
    feed_opt(&mut h, &parent.map(str::to_string));
    feed_opt(&mut h, &previous.map(str::to_string));
    h.finish()
}

/// Pourquoi un bloc est dit changé ([`changed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Son contenu a changé.
    Content,
    /// Sa place a changé (déplacé, ou un frère d'avant arrivé ou parti).
    Place,
    /// Il n'est plus dans la page.
    Gone,
    /// Un bloc est arrivé juste après lui (ou en tête de ses enfants).
    Arrival,
}

/// Un bloc dit changé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedBlock {
    /// Son alias : celui de la page d'avant pour un bloc parti, celui de la page d'aujourd'hui
    /// sinon.
    pub alias: String,
    /// Son rang dans l'ordre du document de la page d'avant (`before`), s'il y était : ce qui
    /// permet de ne dire que les blocs déjà servis d'une lecture en plusieurs tranches.
    pub rank: Option<usize>,
    pub why: Why,
}

/// Ce qui a changé d'une page entre deux lectures (R1226).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changed {
    /// Quelque chose a changé, même ce qu'aucun alias ne nomme (un bloc arrivé en tête de page).
    pub any: bool,
    /// Les blocs changés, partis, et ceux après lesquels un bloc est arrivé, dans l'ordre du
    /// document d'aujourd'hui, puis les partis dans l'ordre d'avant.
    pub blocks: Vec<ChangedBlock>,
}

/// L'alias d'un bloc parti, d'après la seule empreinte de son identifiant : autant de caractères
/// qu'il en fallait pour être seul parmi les blocs de `keys` (rangées, `key` comprise), 12 au plus
/// (au-delà, deux blocs dont les 60 premiers bits du hachage sont égaux : fabriqués, et que
/// l'alias de 12 caractères désigne tous deux). La règle « jamais l'identifiant d'un autre bloc »
/// d'[`Anchors::of`] n'y est pas rejouée : les identifiants partis ne sont pas gardés.
fn alias_of_key(key: u64, keys: &[u64]) -> String {
    let at = keys.partition_point(|&k| k < key);
    // Caractères base32 communs : chacun porte cinq bits, depuis le bit de poids fort.
    let common = |other: u64| ((key ^ other).leading_zeros() / 5) as usize;
    let before = at.checked_sub(1).map_or(0, |p| common(keys[p]));
    let after = keys.get(at + 1).map_or(0, |&k| common(k));
    let chars = (before.max(after) + 1).clamp(MIN_ALIAS_CHARS, KEY_CHARS);
    format!("b{}", base32(&key.to_be_bytes(), chars))
}

/// Ce qui a changé entre `before` (les empreintes d'une lecture) et la page d'aujourd'hui : ses
/// blocs `now` et leurs empreintes `now_hashes`, calculées sous la même clé, dans l'ordre du
/// document. Un identifiant que plusieurs blocs portent, d'un côté ou de l'autre, est dit changé.
pub fn changed(before: &BlockHashes, now: &[Block], now_hashes: &BlockHashes) -> Changed {
    struct Walk<'b> {
        before: &'b BlockHashes,
        before_ids: HashMap<u64, Option<usize>>,
        now_ids: HashMap<u64, Option<usize>>,
        hashes: &'b [BlockHash],
        /// Les empreintes d'identifiant d'avant, rangées : l'alias servi se calcule parmi elles.
        keys: Vec<u64>,
        next: usize,
        out: Changed,
    }
    impl Walk<'_> {
        fn rank(&self, id: &str) -> Option<usize> {
            self.before_ids.get(&id_key(id)).copied().flatten()
        }
        /// Le bloc se nomme par l'alias **servi** (relecture de S3, M5), calculé parmi les blocs
        /// d'avant : un bloc qui n'y était pas, ou que deux blocs nommaient, n'a pas été servi.
        fn say(&mut self, block: &Block, rank: Option<usize>, why: Why) {
            self.out.any = true;
            let key = id_key(&block.id);
            if !block.id.is_empty() && matches!(self.before_ids.get(&key), Some(Some(_))) {
                self.out.blocks.push(ChangedBlock {
                    alias: alias_of_key(key, &self.keys),
                    rank,
                    why,
                });
            }
        }
        fn list(&mut self, blocks: &[Block], parent: Option<&Block>) {
            let mut previous: Option<&Block> = None;
            for b in blocks {
                let Some(h) = self.hashes.get(self.next).copied() else {
                    return;
                };
                self.next += 1;
                let unique_now = matches!(self.now_ids.get(&h.id), Some(Some(_)));
                match self.before_ids.get(&h.id).copied() {
                    Some(Some(rank)) if unique_now => {
                        let was = self.before.blocks[rank];
                        if was.content != h.content {
                            self.say(b, Some(rank), Why::Content);
                        } else if was.place != h.place {
                            self.say(b, Some(rank), Why::Place);
                        }
                    }
                    Some(_) => self.say(b, None, Why::Content),
                    None => match previous.or(parent) {
                        Some(reported) => {
                            let rank = self.rank(&reported.id);
                            self.say(reported, rank, Why::Arrival);
                        }
                        None => self.out.any = true,
                    },
                }
                self.list(&b.children, Some(b));
                previous = Some(b);
            }
        }
    }
    let mut keys: Vec<u64> = before.blocks.iter().map(|h| h.id).collect();
    keys.sort_unstable();
    let mut walk = Walk {
        before,
        before_ids: before.by_id(),
        now_ids: now_hashes.by_id(),
        hashes: now_hashes.as_slice(),
        keys,
        next: 0,
        out: Changed::default(),
    };
    walk.list(now, None);
    for (rank, h) in before.blocks.iter().enumerate() {
        if !walk.now_ids.contains_key(&h.id) {
            walk.out.any = true;
            walk.out.blocks.push(ChangedBlock {
                alias: alias_of_key(h.id, &walk.keys),
                rank: Some(rank),
                why: Why::Gone,
            });
        }
    }
    walk.out
}

// ======================================================================= fusion à trois

/// Les blocs en cause d'une fusion à trois refusée, dans l'ordre de la page d'aujourd'hui : ceux
/// que l'agent et un autre ont changés tous deux (leur MMD à jour est ce que le refus montre).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conflict {
    pub blocks: Vec<String>,
}

/// Un bloc à plat : ses empreintes, son parent et son frère d'avant (par rang dans la liste à
/// plat).
struct Flat<'b> {
    block: &'b Block,
    key: u64,
    content: u64,
    place: u64,
    parent: Option<usize>,
    previous: Option<usize>,
}

fn flatten<'b>(blocks: &'b [Block], key: &HashKey) -> Vec<Flat<'b>> {
    fn walk<'b>(
        blocks: &'b [Block],
        parent: Option<usize>,
        key: &HashKey,
        out: &mut Vec<Flat<'b>>,
    ) {
        let mut previous: Option<usize> = None;
        for b in blocks {
            let at = out.len();
            out.push(Flat {
                block: b,
                key: id_key(&b.id),
                content: content_hash(b, key),
                place: place_hash(
                    parent.map(|p| out[p].block.id.as_str()),
                    previous.map(|p| out[p].block.id.as_str()),
                    key,
                ),
                parent,
                previous,
            });
            walk(&b.children, Some(at), key, out);
            previous = Some(at);
        }
    }
    let mut out = Vec::new();
    walk(blocks, None, key, &mut out);
    out
}

/// Une place où un bloc arrive : en tête des enfants d'un parent (aucun : la racine), ou après un
/// bloc.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Slot {
    Head(Option<u64>),
    After(u64),
}

/// Ce qu'un côté a fait d'un bloc depuis `base`.
#[derive(Clone, Copy, Default, Debug)]
struct Change {
    content: bool,
    place: bool,
    new: bool,
    removed: bool,
    /// Les empreintes que ce côté donne au bloc : deux côtés qui font le même changement ne se
    /// heurtent pas (relecture de S3, M3).
    now_content: u64,
    now_place: u64,
}

/// Ce qu'un côté (les blocs de l'agent, ou la page d'aujourd'hui) a changé depuis `base`.
struct Side<'b> {
    flat: Vec<Flat<'b>>,
    /// Rang de chaque identifiant ; `None` : plusieurs blocs le portent.
    at: HashMap<u64, Option<usize>>,
    changes: HashMap<u64, Change>,
    /// Les places où ce côté pose un bloc (déplacé ou neuf), et le bloc qu'il y pose.
    slots: HashMap<Slot, u64>,
}

impl<'b> Side<'b> {
    fn new(blocks: &'b [Block], key: &HashKey) -> Side<'b> {
        let flat = flatten(blocks, key);
        let mut at: HashMap<u64, Option<usize>> = HashMap::with_capacity(flat.len());
        for (i, f) in flat.iter().enumerate() {
            at.entry(f.key)
                .and_modify(|seen| *seen = None)
                .or_insert(Some(i));
        }
        Side {
            flat,
            at,
            changes: HashMap::new(),
            slots: HashMap::new(),
        }
    }

    fn slot_of(&self, f: &Flat<'_>) -> Slot {
        match f.previous {
            Some(p) => Slot::After(self.flat[p].key),
            None => Slot::Head(f.parent.map(|p| self.flat[p].key)),
        }
    }

    /// Relève ce que ce côté a changé depuis `base` (ses empreintes par identifiant).
    fn judge(&mut self, base: &BlockHashes, base_at: &HashMap<u64, Option<usize>>) {
        for f in &self.flat {
            let mut change = match base_at.get(&f.key) {
                Some(Some(rank)) => {
                    let was = base.blocks[*rank];
                    Change {
                        content: was.content != f.content,
                        place: was.place != f.place,
                        ..Change::default()
                    }
                }
                // Plusieurs blocs de `base` portaient cet identifiant : tout est changé.
                Some(None) => Change {
                    content: true,
                    place: true,
                    ..Change::default()
                },
                None => Change {
                    new: true,
                    ..Change::default()
                },
            };
            change.now_content = f.content;
            change.now_place = f.place;
            if change.content || change.place || change.new {
                self.changes.insert(f.key, change);
            }
            if change.place || change.new {
                self.slots.insert(self.slot_of(f), f.key);
            }
        }
        for (key, rank) in base_at {
            if rank.is_some() && !self.at.contains_key(key) {
                self.changes.insert(
                    *key,
                    Change {
                        removed: true,
                        ..Change::default()
                    },
                );
            }
        }
    }

    fn unique(&self) -> bool {
        self.at.values().all(Option::is_some)
    }
}

/// Deux changements d'un même bloc qui ne se fondent pas : le même changement fait des deux côtés
/// (le même texte, la même place, le même retrait ; un agent qui rejoue son écriture) se fond.
fn clash(a: Change, h: Change) -> bool {
    let content = a.now_content != h.now_content;
    let place = a.now_place != h.now_place;
    (a.content && h.content && content)
        || (a.place && h.place && place)
        || (a.new && h.new && (content || place))
        || (a.removed && (h.content || h.place))
        || (h.removed && (a.content || a.place))
}

/// Un bloc de la fusion : d'où viennent son contenu et sa place.
struct Merged<'b> {
    block: &'b Block,
    parent: Option<u64>,
    previous: Option<u64>,
}

/// La fusion à trois de `replace_page` (R1231) : `written`, les blocs de l'agent (identifiants
/// résolus ; un bloc sans identifiant est un bloc neuf), écrits sur la page de `base` ; `now`, la
/// page d'aujourd'hui. Rend les blocs à écrire : ce que l'agent a changé depuis `base` tel qu'il
/// l'écrit, le reste tel qu'il est aujourd'hui.
///
/// Un bloc change de contenu, de place, naît ou part ; refusé (`Conflict`) : un même bloc changé
/// des deux côtés dans la même dimension (contenu, ou place), retiré d'un côté et changé de
/// l'autre ; deux blocs arrivés au même endroit (après le même bloc, ou en tête des mêmes
/// enfants) ; un identifiant que plusieurs blocs portent ; et une fusion qui ne se bâtit pas en un
/// arbre (un bloc dont le parent ou le frère d'avant est parti, deux blocs à la même place, un
/// cycle).
pub fn three_way(
    base: &BlockHashes,
    written: &[Block],
    now: &[Block],
    key: &HashKey,
) -> Result<Vec<Block>, Conflict> {
    let written = with_ids(written);
    let base_at = base.by_id();
    let mut agent = Side::new(&written, key);
    let mut others = Side::new(now, key);
    if !agent.unique() || !others.unique() {
        return Err(conflict_on(&others, |f| {
            matches!(others.at.get(&f.key), Some(None))
        }));
    }
    agent.judge(base, &base_at);
    others.judge(base, &base_at);

    let mut clashes: HashSet<u64> = HashSet::new();
    for (key, a) in &agent.changes {
        if let Some(h) = others.changes.get(key)
            && clash(*a, *h)
        {
            clashes.insert(*key);
        }
    }
    // Un bloc neuf de l'agent qui porte le contenu d'un bloc qu'un autre a retiré : le bloc lu,
    // récrit sans ancre, qui ressusciterait (relecture de S3, I2).
    let removed_by_others: HashSet<u64> = base_at
        .iter()
        .filter(|(key, rank)| rank.is_some() && !others.at.contains_key(*key))
        .filter_map(|(_, rank)| rank.map(|r| base.blocks[r].content))
        .collect();
    let resurrected = agent.flat.iter().any(|f| {
        agent.changes.get(&f.key).is_some_and(|c| c.new) && removed_by_others.contains(&f.content)
    });
    // Deux côtés qui posent deux blocs différents au même endroit.
    let slot_clashes: HashSet<Slot> = agent
        .slots
        .iter()
        .filter(|(slot, claim)| others.slots.get(*slot).is_some_and(|other| other != *claim))
        .map(|(slot, _)| *slot)
        .collect();
    if !clashes.is_empty() || !slot_clashes.is_empty() || resurrected {
        return Err(conflict_on(&others, |f| {
            clashes.contains(&f.key)
                || slot_clashes.contains(&Slot::After(f.key))
                || slot_clashes.contains(&others.slot_of(f))
        }));
    }

    // Chaque bloc gardé, son contenu et sa place pris du côté qui les a changés.
    let mut merged: HashMap<u64, Merged<'_>> = HashMap::new();
    let place = |side: &Side<'_>, f: &Flat<'_>| {
        (
            f.parent.map(|p| side.flat[p].key),
            f.previous.map(|p| side.flat[p].key),
        )
    };
    for f in &agent.flat {
        let change = agent.changes.get(&f.key).copied().unwrap_or_default();
        let theirs = match others.at.get(&f.key) {
            Some(Some(i)) => Some(&others.flat[*i]),
            _ => None,
        };
        if !(change.content || change.place || change.new) && theirs.is_none() {
            continue; // retiré par un autre, et laissé tel quel par l'agent
        }
        let block = match theirs {
            Some(t) if !(change.content || change.new) => t.block,
            _ => f.block,
        };
        let (parent, previous) = match theirs {
            Some(t) if !(change.place || change.new) => place(&others, t),
            _ => place(&agent, f),
        };
        merged.insert(
            f.key,
            Merged {
                block,
                parent,
                previous,
            },
        );
    }
    for f in &others.flat {
        if agent.at.contains_key(&f.key) {
            continue;
        }
        if matches!(base_at.get(&f.key), Some(Some(_))) {
            continue; // retiré par l'agent
        }
        let (parent, previous) = place(&others, f);
        merged.insert(
            f.key,
            Merged {
                block: f.block,
                parent,
                previous,
            },
        );
    }

    // L'arbre : chaque place n'a qu'un bloc, et tout bloc s'atteint depuis la racine.
    let mut claims: HashMap<(Option<u64>, Option<u64>), u64> = HashMap::new();
    let mut doubled: HashSet<u64> = HashSet::new();
    for (key, m) in &merged {
        if let Some(other) = claims.insert((m.parent, m.previous), *key) {
            doubled.extend([*key, other]);
        }
    }
    let unbuilt = |keys: &HashSet<u64>| conflict_on(&others, |f| keys.contains(&f.key));
    if !doubled.is_empty() {
        return Err(unbuilt(&doubled));
    }
    let mut seen: HashSet<u64> = HashSet::with_capacity(merged.len());
    let tree = build(None, &merged, &claims, &mut seen);
    if seen.len() != merged.len() {
        let lost: HashSet<u64> = merged
            .iter()
            .filter(|(key, _)| !seen.contains(key))
            .flat_map(|(key, m)| [Some(*key), m.parent, m.previous])
            .flatten()
            .collect();
        return Err(unbuilt(&lost));
    }
    Ok(tree)
}

/// Les enfants de `parent` dans la fusion, de proche en proche par leur frère d'avant.
fn build(
    parent: Option<u64>,
    merged: &HashMap<u64, Merged<'_>>,
    claims: &HashMap<(Option<u64>, Option<u64>), u64>,
    seen: &mut HashSet<u64>,
) -> Vec<Block> {
    let mut out = Vec::new();
    let mut previous = None;
    while let Some(&key) = claims.get(&(parent, previous)) {
        if !seen.insert(key) {
            break; // un cycle : le bloc ne s'atteint pas, et la fusion est refusée
        }
        let m = &merged[&key];
        out.push(Block {
            children: build(Some(key), merged, claims, seen),
            ..clone_node(m.block)
        });
        previous = Some(key);
    }
    out
}

/// Un bloc sans ses enfants.
fn clone_node(b: &Block) -> Block {
    Block {
        id: b.id.clone(),
        kind: b.kind.clone(),
        props: b.props.clone(),
        content: b.content.clone(),
        children: Vec::new(),
    }
}

/// Les blocs, un identifiant neuf (UUIDv7) donné à ceux qui n'en ont pas.
pub(crate) fn with_ids(blocks: &[Block]) -> Vec<Block> {
    fn fill(blocks: &mut [Block]) {
        for b in blocks {
            if b.id.is_empty() {
                b.id = Uuid::now_v7().to_string();
            }
            fill(&mut b.children);
        }
    }
    let mut blocks = blocks.to_vec();
    fill(&mut blocks);
    blocks
}

/// Le refus d'une fusion : les blocs de la page d'aujourd'hui que `chosen` retient, dans l'ordre
/// de la page.
fn conflict_on(side: &Side<'_>, chosen: impl Fn(&Flat<'_>) -> bool) -> Conflict {
    let mut seen = HashSet::new();
    Conflict {
        blocks: side
            .flat
            .iter()
            .filter(|f| chosen(f))
            .map(|f| f.block.id.clone())
            .filter(|id| seen.insert(id.clone()))
            .collect(),
    }
}
