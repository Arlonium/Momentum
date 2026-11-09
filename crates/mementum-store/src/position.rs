//! Index fractionnaire : des positions de texte qui se trient en ordre d'octets.
//!
//! Alphabet ordonné `0-9A-Za-z` (62 chiffres, ordre ASCII croissant). Une position valide ne se
//! termine jamais par `0` (le plus petit chiffre) : sinon plus rien ne tient strictement sous
//! elle. Les positions produites respectent cette règle. Pas de rééquilibrage dans ce lot : des
//! insertions répétées au même endroit allongent la clé d'un caractère de temps en temps.

const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = DIGITS.len();

fn digit_value(c: Option<u8>) -> usize {
    c.and_then(|c| DIGITS.iter().position(|&d| d == c))
        .unwrap_or(0)
}

/// Milieu strict entre `a` (borne basse, vide = sans borne) et `b` (borne haute, `None` = sans
/// borne), avec `a < b` en ordre d'octets.
fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        // Préfixe commun : on le garde tel quel et on travaille sur la suite.
        let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        if n > 0 {
            let mut out = b[..n].to_vec();
            out.extend(midpoint(&a[n..], Some(&b[n..])));
            return out;
        }
    }
    let da = digit_value(a.first().copied());
    let db = match b {
        Some(b) => digit_value(b.first().copied()),
        None => BASE,
    };
    if db - da > 1 {
        return vec![DIGITS[(da + db) / 2]];
    }
    if let Some(b) = b {
        // Même chiffre (borne basse vide, borne haute commençant par `0`) : on garde ce `0` et on
        // cherche sous la suite de la borne haute. Évite de rendre une clé finissant par `0`.
        if da == db {
            let mut out = vec![b[0]];
            out.extend(midpoint(&[], Some(&b[1..])));
            return out;
        }
        // Chiffres consécutifs : on reprend le chiffre haut seul s'il y a une suite…
        if b.len() > 1 {
            return vec![b[0]];
        }
    }
    // … sinon on garde le chiffre bas et on cherche plus loin, sans borne haute.
    let mut out = vec![DIGITS[da]];
    out.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
    out
}

/// Renvoie une position strictement comprise entre `before` et `after`, en ordre d'octets.
/// `None` = sans borne de ce côté. Les bornes doivent vérifier `before < after`.
pub fn position_between(before: Option<&str>, after: Option<&str>) -> String {
    let a = before.unwrap_or("").as_bytes();
    let out = midpoint(a, after.map(str::as_bytes));
    // Les chiffres sont de l'ASCII : la conversion ne peut pas échouer.
    String::from_utf8(out).expect("alphabet ASCII")
}
