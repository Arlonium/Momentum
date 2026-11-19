//! Corpus fait par programme, toujours le même pour une échelle donnée : du français
//! (accents compris), des mots tirés selon une loi de Zipf, et quelques termes plantés pour
//! les questions de la Task S0.
//!
//! Espace A : `big` grandes pages (la page 0 a `mastodon` blocs, les autres `per_page`), toutes
//! titrées « Projet … » ; `small` petites pages d'un bloc, titrées « Note … ».
//! Espace B (jamais permis) : `b_pages` pages de `per_page_b` blocs, dont chaque bloc porte
//! « canalcent ».
//! « canalzero » : un bloc de chacune des pages 1 à 10 de A. « canalcent » : un autre bloc de
//! ces mêmes pages, et tout l'espace B.

use std::collections::HashSet;

pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

const BASE: &str = "de la le et les des en un une du que est pour qui dans a par plus pas au sur ne se ce il \
sont avec son cette ou mais comme nous tout aux été être fait elle ses leur où très même aussi après \
déjà dès près élève école équipe état étude réunion décision données système modèle créer créée \
tâche bâtir côté coût août forêt fenêtre être prêt bientôt maître naître île dîner sûr mûr goût \
çà leçon façon reçu garçon français française cœur œuvre sœur œil naïf maïs noël ambiguë \
page pages bloc blocs espace recherche résultat résultats titre texte lien liens mot mots index \
serveur client budget mesure mesures mémoire démarrage frappe version histoire commentaire \
fichier fichiers image tableau liste paragraphe compte comptes droit droits accès lecture écriture \
cheval chevaux journal journaux travail travaux national nationale nationaux manger mangé mangeons \
petit petite petits grande grand grands nouveau nouvelle nouveaux premier première dernière";

const ONSETS: &[&str] = &[
    "b", "c", "ch", "d", "f", "g", "j", "l", "m", "n", "p", "qu", "r", "s", "t", "v", "br", "cr", "dr", "fr",
    "gr", "pl", "pr", "tr", "bl", "cl",
];
const VOWELS: &[&str] = &[
    "a", "e", "i", "o", "u", "é", "è", "ou", "ai", "on", "an", "in", "eu", "oi", "au", "ê", "â", "î", "ô", "û",
];
const ENDS: &[&str] = &["", "", "e", "s", "er", "tion", "ment", "eur", "ique", "age", "é", "ée", "ant", "aire"];

pub struct Vocab {
    pub words: Vec<String>,
    cum: Vec<f64>,
}

impl Vocab {
    pub fn new(size: usize) -> Vocab {
        let mut rng = Rng(7);
        let mut seen: HashSet<String> = HashSet::new();
        let mut words: Vec<String> = Vec::with_capacity(size);
        for w in BASE.split_whitespace() {
            if seen.insert(w.to_string()) {
                words.push(w.to_string());
            }
        }
        while words.len() < size {
            let syll = 2 + rng.below(3);
            let mut w = String::new();
            for _ in 0..syll {
                w.push_str(ONSETS[rng.below(ONSETS.len() as u64) as usize]);
                w.push_str(VOWELS[rng.below(VOWELS.len() as u64) as usize]);
            }
            w.push_str(ENDS[rng.below(ENDS.len() as u64) as usize]);
            if seen.insert(w.clone()) {
                words.push(w);
            }
        }
        let mut cum = Vec::with_capacity(size);
        let mut total = 0.0;
        for rank in 0..size {
            total += 1.0 / (rank as f64 + 2.7);
            cum.push(total);
        }
        for c in &mut cum {
            *c /= total;
        }
        Vocab { words, cum }
    }
    pub fn pick(&self, rng: &mut Rng) -> &str {
        let u = rng.unit();
        let i = self.cum.partition_point(|c| *c < u);
        &self.words[i.min(self.words.len() - 1)]
    }
}

#[derive(Clone, Copy)]
pub struct Scale {
    pub big: u32,
    pub per_page: u32,
    pub mastodon: u32,
    pub small: u32,
    pub b_pages: u32,
    pub per_page_b: u32,
}

impl Scale {
    /// `1.0` : 10 000 grandes pages et un million de blocs, 90 000 petites pages, 100 000 blocs
    /// dans l'espace B.
    pub fn of(factor: f64) -> Scale {
        let n = |v: f64| ((v * factor).round() as u32).max(1);
        Scale {
            big: n(10_000.0).max(21),
            per_page: 99,
            mastodon: n(10_000.0),
            small: n(90_000.0),
            b_pages: n(1_000.0),
            per_page_b: 100,
        }
    }
    pub fn pages_a(&self) -> u32 {
        self.big + self.small
    }
}

pub struct Entry<'a> {
    pub space: &'static str,
    /// Numéro de page, unique dans le corpus.
    pub page: u32,
    /// 0 : l'entrée du titre seul.
    pub block: u32,
    pub title: &'a str,
    pub text: &'a str,
}

/// L'identifiant d'une page : 128 bits tirés de son numéro, comme un UUID.
pub fn page_id(page: u32) -> u128 {
    let mut r = Rng(0xA5A5_0000 + page as u64);
    ((r.next() as u128) << 64) | r.next() as u128
}

pub fn page_hex(page: u32) -> String {
    format!("{:032x}", page_id(page))
}

fn sentence(v: &Vocab, rng: &mut Rng, out: &mut String) {
    out.clear();
    let n = 5 + rng.below(36);
    for i in 0..n {
        let w = v.pick(rng);
        if i == 0 {
            let mut cs = w.chars();
            if let Some(c) = cs.next() {
                out.extend(c.to_uppercase());
                out.push_str(cs.as_str());
            }
        } else {
            out.push_str(w);
        }
        if i + 1 < n {
            out.push_str(if rng.below(12) == 0 { ", " } else { " " });
        }
    }
    out.push('.');
}

pub fn stream(scale: &Scale, v: &Vocab, mut f: impl FnMut(Entry)) {
    let mut rng = Rng(42);
    let mut title = String::new();
    let mut text = String::new();
    // Espace A, grandes pages.
    for page in 0..scale.big {
        title.clear();
        title.push_str("Projet ");
        if page == 0 {
            title.push_str("mastodonte ");
        }
        for i in 0..3 {
            if i > 0 {
                title.push(' ');
            }
            title.push_str(v.pick(&mut rng));
        }
        f(Entry { space: "a", page, block: 0, title: &title, text: "" });
        let blocks = if page == 0 { scale.mastodon } else { scale.per_page };
        for block in 1..=blocks {
            sentence(v, &mut rng, &mut text);
            if (1..=10).contains(&page) && block == 5 {
                text.push_str(" canalzero");
            }
            if (1..=10).contains(&page) && block == 6 {
                text.push_str(" canalcent");
            }
            f(Entry { space: "a", page, block, title: &title, text: &text });
        }
    }
    // Espace A, petites pages.
    for i in 0..scale.small {
        let page = scale.big + i;
        title.clear();
        title.push_str("Note ");
        title.push_str(v.pick(&mut rng));
        title.push(' ');
        title.push_str(v.pick(&mut rng));
        f(Entry { space: "a", page, block: 0, title: &title, text: "" });
        sentence(v, &mut rng, &mut text);
        f(Entry { space: "a", page, block: 1, title: &title, text: &text });
    }
    // Espace B : rien n'y est jamais permis.
    for i in 0..scale.b_pages {
        let page = scale.big + scale.small + i;
        title.clear();
        title.push_str("Projet ");
        title.push_str(v.pick(&mut rng));
        title.push(' ');
        title.push_str(v.pick(&mut rng));
        f(Entry { space: "b", page, block: 0, title: &title, text: "" });
        for block in 1..=scale.per_page_b {
            sentence(v, &mut rng, &mut text);
            text.push_str(" canalcent");
            f(Entry { space: "b", page, block, title: &title, text: &text });
        }
    }
}

/// Minuscules et accents pliés, pour les caractères du corpus (le produit prendra une table
/// complète : ici, celle du français suffit).
pub fn fold(s: &str, out: &mut String) {
    out.clear();
    for c in s.chars() {
        for l in c.to_lowercase() {
            match l {
                'à' | 'â' | 'ä' => out.push('a'),
                'é' | 'è' | 'ê' | 'ë' => out.push('e'),
                'î' | 'ï' => out.push('i'),
                'ô' | 'ö' => out.push('o'),
                'ù' | 'û' | 'ü' => out.push('u'),
                'ç' => out.push('c'),
                'œ' => out.push_str("oe"),
                'æ' => out.push_str("ae"),
                other => out.push(other),
            }
        }
    }
}

pub const MAX_START_CHARS: usize = 10;

/// Les débuts de mot de 2 à 10 caractères d'un texte déjà plié (R920).
pub fn starts(folded: &str, mut f: impl FnMut(&str)) {
    for word in folded.split(|c: char| !c.is_alphanumeric()) {
        let mut n = 0;
        for (i, c) in word.char_indices() {
            n += 1;
            if n > MAX_START_CHARS {
                break;
            }
            if n >= 2 {
                f(&word[..i + c.len_utf8()]);
            }
        }
    }
}
