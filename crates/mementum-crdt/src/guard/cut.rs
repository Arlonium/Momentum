//! La coupe d'un élément à cheval sur ce que la page connaît (Ruling R784).
//!
//! Avant d'intégrer une mise à jour, yrs en retire ce que la page connaît déjà (`exclude`,
//! `update.rs`) : il coupe en deux tout élément dont une part seulement est connue
//! (`ItemPtr::splice`, `block.rs`). La moitié droite reçoit pour origine l'unité d'avant elle,
//! mais **garde le parent et la clé que disent les octets** ; la moitié gauche garde un pointeur
//! vers la moitié droite, même jetée. Un élément dont la part connue est le début devenait ainsi
//! l'entrée courante d'une map d'un autre parent que son voisin de gauche, l'ancienne entrée
//! n'était plus tenue par rien, et la page lisait de la mémoire libérée (constat C1 de la
//! re-relecture 3 de la garde des attributs).
//!
//! Le tour 2 refusait tout élément à cheval (Ruling R740), sur la foi que Yjs n'en envoie pas.
//! C'est faux : l'étape 2 d'un onglet dont une frappe est arrivée entre-temps par son ancienne
//! connexion en porte un (constat C1 de la re-relecture 4). La mise à jour est donc **récrite**
//! ici, avant que la garde et yrs la lisent, pour que yrs n'ait plus jamais d'élément à couper :
//!
//! - une part connue devient un trou de la mise à jour (`Skip`), ce que yrs en aurait fait ;
//! - une part neuve qui suit une part connue est écrite comme `Item.write(encoder, offset)` de
//!   Yjs l'écrit pour un destinataire qui connaît le début : à son horloge, d'origine l'unité
//!   d'avant elle, **sans parent ni clé nommés** (son voisin de gauche les lui donne), l'origine
//!   droite inchangée, le contenu coupé ;
//! - une part neuve qui commence l'élément garde son en-tête, le contenu coupé à droite (ce cas
//!   ne se présente que dans une page qui a un trou, relue d'un journal d'avant le Ruling R131).
//!
//! Le parent des octets d'un élément à cheval n'est ainsi jamais cru : la part intégrée est un
//! élément ordinaire, placé par son origine, que toutes les gardes jugent comme tel. Pour un
//! client honnête, c'est exactement ce que Yjs intègre de la mise à jour d'origine (le voisin de
//! gauche est le début du même élément) : `crates/mementum-crdt/tests/astride.rs` le compare au
//! vrai Yjs.
//!
//! Une chaîne se coupe en unités UTF-16 ; coupée au milieu d'une paire de substitution, chaque
//! moitié de la paire devient U+FFFD, comme dans Yjs (`ContentString.splice`) et dans yrs. Un
//! contenu JSON à plusieurs valeurs reste refusé : yrs et Yjs n'en lisent pas le même nombre, et
//! aucun client n'en écrit (Ruling R702). Les autres contenus valent une unité : jamais à cheval.

use std::collections::HashMap;

use yrs::StateVector;
use yrs::block::{
    BLOCK_GC_REF_NUMBER, BLOCK_ITEM_ANY_REF_NUMBER, BLOCK_ITEM_DELETED_REF_NUMBER,
    BLOCK_ITEM_STRING_REF_NUMBER, BLOCK_SKIP_REF_NUMBER, ClientID, HAS_ORIGIN, HAS_RIGHT_ORIGIN,
};
use yrs::encoding::read::{Cursor, Read};

use super::{ASTRIDE, JsonCount, MAX_CLIENT, decode_error, read_content, read_id, skip_any};
use crate::CrdtError;

/// Par client, les plages `[début, fin)` que la page connaît **au-delà de son premier trou**,
/// triées et disjointes. Vide pour une page sans trou : elle connaît `[0, plancher)`, rien
/// d'autre.
pub(crate) type Beyond = HashMap<u64, Vec<(u32, u32)>>;

fn var(out: &mut Vec<u8>, mut value: u64) {
    while value > 0x7F {
        out.push(0x80 | (value & 0x7F) as u8);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Les parts de `[start, end)`, dans l'ordre, chacune connue (`true`) ou neuve.
fn parts(
    floor: u32,
    beyond: Option<&Vec<(u32, u32)>>,
    start: u32,
    end: u32,
) -> Vec<(u32, u32, bool)> {
    let mut out: Vec<(u32, u32, bool)> = Vec::new();
    let mut at = start;
    let known = |from: u32, to: u32, out: &mut Vec<(u32, u32, bool)>, at: &mut u32| {
        let (from, to) = (from.max(*at), to.min(end));
        if from >= to {
            return;
        }
        if from > *at {
            out.push((*at, from, false));
        }
        match out.last_mut() {
            Some(last) if last.2 && last.1 == from => last.1 = to,
            _ => out.push((from, to, true)),
        }
        *at = to;
    };
    known(0, floor, &mut out, &mut at);
    for &(from, to) in beyond.into_iter().flatten() {
        known(from, to, &mut out, &mut at);
    }
    if at < end {
        out.push((at, end, false));
    }
    out
}

/// Les unités UTF-16 `[from, to)` d'une chaîne. Une paire de substitution coupée par une borne
/// rend U+FFFD pour la moitié gardée, comme Yjs et yrs.
fn utf16_slice(text: &str, from: u32, to: u32) -> String {
    let mut out = String::new();
    let mut at = 0u32;
    for ch in text.chars() {
        let (start, end) = (at, at + ch.len_utf16() as u32);
        at = end;
        if end <= from {
            continue;
        }
        if start >= to {
            break;
        }
        if start >= from && end <= to {
            out.push(ch);
        } else {
            out.push('\u{FFFD}');
        }
    }
    out
}

/// Le contenu d'un élément, coupé aux unités `[from, to)`. `content` : ses octets, sans
/// l'en-tête.
fn sliced(
    kind: u8,
    content: &[u8],
    from: u32,
    to: u32,
    out: &mut Vec<u8>,
) -> Result<(), CrdtError> {
    let mut c = Cursor::new(content);
    match kind {
        BLOCK_ITEM_DELETED_REF_NUMBER => var(out, u64::from(to - from)),
        BLOCK_ITEM_STRING_REF_NUMBER => {
            let text = utf16_slice(c.read_string().map_err(decode_error)?, from, to);
            var(out, text.len() as u64);
            out.extend(text.as_bytes());
        }
        BLOCK_ITEM_ANY_REF_NUMBER => {
            let len: u32 = c.read_var().map_err(decode_error)?;
            let (mut first, mut last) = (c.next, c.next);
            for n in 0..len.min(to) {
                if n == from {
                    first = c.next;
                }
                skip_any(&mut c).map_err(decode_error)?;
                last = c.next;
            }
            var(out, u64::from(to - from));
            out.extend(&content[first..last]);
        }
        // Du JSON à plusieurs valeurs : yrs et Yjs n'en lisent pas le même nombre.
        _ => return Err(CrdtError::Structure(ASTRIDE.to_string())),
    }
    Ok(())
}

/// La mise à jour (v1) récrite pour qu'aucun de ses éléments ne soit à cheval sur ce que la page
/// connaît : `floor`, son vecteur d'état (par client, le début de son premier trou), et `beyond`,
/// ce qu'elle connaît au-delà. `None` s'il n'y a rien à récrire. Le jeu de suppressions est
/// recopié tel quel.
pub(crate) fn cut(
    update: &[u8],
    floor: &StateVector,
    beyond: &Beyond,
) -> Result<Option<Vec<u8>>, CrdtError> {
    let mut c = Cursor::new(update);
    let mut out = Vec::with_capacity(update.len() + 16);
    let mut changed = false;
    let n_clients: u32 = c.read_var().map_err(decode_error)?;
    var(&mut out, u64::from(n_clients));
    for _ in 0..n_clients {
        let n_structs: u32 = c.read_var().map_err(decode_error)?;
        let client: u64 = c.read_var().map_err(decode_error)?;
        if client > MAX_CLIENT {
            return Err(decode_error("identifiant de client hors de 53 bits"));
        }
        let first: u32 = c.read_var().map_err(decode_error)?;
        let known = floor.get(&ClientID::new(client));
        let holes = beyond.get(&client);
        let mut clock = first;
        let mut written: u64 = 0;
        let mut structs = Vec::new();
        for _ in 0..n_structs {
            let start = c.next;
            let info = c.read_u8().map_err(decode_error)?;
            written += 1;
            if matches!(info, BLOCK_SKIP_REF_NUMBER | BLOCK_GC_REF_NUMBER) {
                let len: u32 = c.read_var().map_err(decode_error)?;
                let end = clock
                    .checked_add(len)
                    .ok_or_else(|| decode_error("horloge hors limites"))?;
                // Une plage ramassée n'a ni parent ni voisin, mais yrs la coupe mal quand la page
                // en connaît le milieu (`Block::splice` ne raccourcit pas la moitié gauche) : il
                // écrit alors par-dessus un trou plus court qu'elle. Coupée ici de même.
                let parts = if info == BLOCK_GC_REF_NUMBER && holes.is_some() {
                    parts(known, holes, clock, end)
                } else {
                    Vec::new()
                };
                clock = end;
                if parts.len() < 2 {
                    structs.extend(&update[start..c.next]);
                    continue;
                }
                changed = true;
                written += parts.len() as u64 - 1;
                for (from, to, is_known) in parts {
                    structs.push(if is_known {
                        BLOCK_SKIP_REF_NUMBER
                    } else {
                        BLOCK_GC_REF_NUMBER
                    });
                    var(&mut structs, u64::from(to - from));
                }
                continue;
            }
            let origin_id = (info & HAS_ORIGIN != 0)
                .then(|| read_id(&mut c))
                .transpose()
                .map_err(decode_error)?;
            let origin = origin_id.is_some();
            let mut right = (info & HAS_RIGHT_ORIGIN != 0)
                .then(|| read_id(&mut c))
                .transpose()
                .map_err(decode_error)?;
            // Ruling R802 : un élément dont l'origine droite est son origine (Yjs en écrit : un
            // rétablissement qui retrouve le même voisin des deux côtés). Yjs comme yrs le
            // rangent selon que ce voisin est fondu ou non avec les siens ; la page vivante
            // n'était plus son journal. L'origine droite est retirée : l'élément est rangé par
            // son origine seule, partout pareil.
            let mut info = info;
            let mut same_origins = None;
            if let Some((origin_client, origin_clock)) = origin_id
                && right == origin_id
            {
                info &= !HAS_RIGHT_ORIGIN;
                right = None;
                let mut rewritten = vec![info];
                var(&mut rewritten, origin_client);
                var(&mut rewritten, u64::from(origin_clock));
                same_origins = Some(rewritten);
            }
            if !origin && right.is_none() {
                if c.read_var::<u32>().map_err(decode_error)? == 1 {
                    c.read_string().map_err(decode_error)?;
                } else {
                    read_id(&mut c).map_err(decode_error)?;
                }
                if info & yrs::block::HAS_PARENT_SUB != 0 {
                    c.read_string().map_err(decode_error)?;
                }
            }
            let header = c.next;
            let len = read_content(&mut c, info, false, JsonCount::AsRead)
                .map_err(decode_error)?
                .len;
            let end = clock
                .checked_add(len)
                .ok_or_else(|| decode_error("horloge hors limites"))?;
            let mut parts = if len > 1 {
                parts(known, holes, clock, end)
            } else {
                Vec::new()
            };
            // Ruling R801 : une entrée de map de plusieurs valeurs est récrite en autant
            // d'entrées d'une valeur, chacune d'origine la précédente (ce que le client aurait
            // envoyé sans les fondre) : yrs ne garde alors aucune entrée de map qui se coupe.
            if len > 1
                && info & 0b1111 == BLOCK_ITEM_ANY_REF_NUMBER
                && info & yrs::block::HAS_PARENT_SUB != 0
            {
                parts = parts
                    .into_iter()
                    .flat_map(|(from, to, is_known)| {
                        if is_known {
                            vec![(from, to, true)]
                        } else {
                            (from..to).map(|unit| (unit, unit + 1, false)).collect()
                        }
                    })
                    .collect();
            }
            if parts.len() < 2 {
                // Tout neuf, ou tout connu : yrs le prend ou l'ignore en entier.
                match &same_origins {
                    Some(head) => {
                        changed = true;
                        structs.extend(head);
                        structs.extend(&update[header..c.next]);
                    }
                    None => structs.extend(&update[start..c.next]),
                }
                clock = end;
                continue;
            }
            changed = true;
            written += parts.len() as u64 - 1;
            let content = &update[header..c.next];
            for (from, to, is_known) in parts {
                if is_known {
                    structs.push(BLOCK_SKIP_REF_NUMBER);
                    var(&mut structs, u64::from(to - from));
                    continue;
                }
                if from == clock {
                    match &same_origins {
                        Some(head) => structs.extend(head),
                        None => structs.extend(&update[start..header]),
                    }
                } else {
                    // `Item.write(encoder, offset)` de Yjs : l'origine est l'unité d'avant, le
                    // parent et la clé ne sont pas écrits.
                    structs.push(info | HAS_ORIGIN);
                    var(&mut structs, client);
                    var(&mut structs, u64::from(from - 1));
                    if let Some((right_client, right_clock)) = right {
                        var(&mut structs, right_client);
                        var(&mut structs, u64::from(right_clock));
                    }
                }
                sliced(
                    info & 0b1111,
                    content,
                    from - clock,
                    to - clock,
                    &mut structs,
                )?;
            }
            clock = end;
        }
        var(&mut out, written);
        var(&mut out, client);
        var(&mut out, u64::from(first));
        out.extend(structs);
    }
    if !changed {
        return Ok(None);
    }
    out.extend(&update[c.next..]);
    Ok(Some(out))
}

/// La mise à jour (v1) où chacun des éléments de `flagged` (l'identifiant de son début) perd son
/// origine droite (règle N3, Ruling R825), comme N2 la retire à un élément dont l'origine droite
/// est son origine. Chacun a une origine : ses octets ne nomment ni parent ni clé, avec ou sans
/// l'origine droite. Le reste est recopié tel quel.
pub(crate) fn unright(
    update: &[u8],
    flagged: &std::collections::HashSet<(u64, u32)>,
) -> Result<Vec<u8>, CrdtError> {
    let mut c = Cursor::new(update);
    let mut out = Vec::with_capacity(update.len());
    let n_clients: u32 = c.read_var().map_err(decode_error)?;
    var(&mut out, u64::from(n_clients));
    for _ in 0..n_clients {
        let n_structs: u32 = c.read_var().map_err(decode_error)?;
        let client: u64 = c.read_var().map_err(decode_error)?;
        let first: u32 = c.read_var().map_err(decode_error)?;
        var(&mut out, u64::from(n_structs));
        var(&mut out, client);
        var(&mut out, u64::from(first));
        let mut clock = first;
        for _ in 0..n_structs {
            let start = c.next;
            let info = c.read_u8().map_err(decode_error)?;
            if matches!(info, BLOCK_SKIP_REF_NUMBER | BLOCK_GC_REF_NUMBER) {
                let len: u32 = c.read_var().map_err(decode_error)?;
                clock = clock
                    .checked_add(len)
                    .ok_or_else(|| decode_error("horloge hors limites"))?;
                out.extend(&update[start..c.next]);
                continue;
            }
            let origin = (info & HAS_ORIGIN != 0)
                .then(|| read_id(&mut c))
                .transpose()
                .map_err(decode_error)?;
            let after_origin = c.next;
            let right = (info & HAS_RIGHT_ORIGIN != 0)
                .then(|| read_id(&mut c))
                .transpose()
                .map_err(decode_error)?;
            let after_right = c.next;
            if origin.is_none() && right.is_none() {
                if c.read_var::<u32>().map_err(decode_error)? == 1 {
                    c.read_string().map_err(decode_error)?;
                } else {
                    read_id(&mut c).map_err(decode_error)?;
                }
                if info & yrs::block::HAS_PARENT_SUB != 0 {
                    c.read_string().map_err(decode_error)?;
                }
            }
            let len = read_content(&mut c, info, false, JsonCount::AsRead)
                .map_err(decode_error)?
                .len;
            if origin.is_some() && right.is_some() && flagged.contains(&(client, clock)) {
                out.push(info & !HAS_RIGHT_ORIGIN);
                out.extend(&update[start + 1..after_origin]);
                out.extend(&update[after_right..c.next]);
            } else {
                out.extend(&update[start..c.next]);
            }
            clock = clock
                .checked_add(len)
                .ok_or_else(|| decode_error("horloge hors limites"))?;
        }
    }
    out.extend(&update[c.next..]);
    Ok(out)
}

/// La mise à jour (v1) où chacun des éléments de `collected` (l'identifiant de son début) devient
/// une plage ramassée de même longueur (règle G, Ruling R827) : ce que Yjs en fait
/// (`Item.getMissing`, puis `Item.integrate` sans parent). Le reste est recopié tel quel.
pub(crate) fn collect(
    update: &[u8],
    collected: &std::collections::HashSet<(u64, u32)>,
) -> Result<Vec<u8>, CrdtError> {
    let mut c = Cursor::new(update);
    let mut out = Vec::with_capacity(update.len());
    let n_clients: u32 = c.read_var().map_err(decode_error)?;
    var(&mut out, u64::from(n_clients));
    for _ in 0..n_clients {
        let n_structs: u32 = c.read_var().map_err(decode_error)?;
        let client: u64 = c.read_var().map_err(decode_error)?;
        let first: u32 = c.read_var().map_err(decode_error)?;
        var(&mut out, u64::from(n_structs));
        var(&mut out, client);
        var(&mut out, u64::from(first));
        let mut clock = first;
        for _ in 0..n_structs {
            let start = c.next;
            let info = c.read_u8().map_err(decode_error)?;
            if matches!(info, BLOCK_SKIP_REF_NUMBER | BLOCK_GC_REF_NUMBER) {
                let len: u32 = c.read_var().map_err(decode_error)?;
                clock = clock
                    .checked_add(len)
                    .ok_or_else(|| decode_error("horloge hors limites"))?;
                out.extend(&update[start..c.next]);
                continue;
            }
            let origin = info & HAS_ORIGIN != 0;
            let right = info & HAS_RIGHT_ORIGIN != 0;
            if origin {
                read_id(&mut c).map_err(decode_error)?;
            }
            if right {
                read_id(&mut c).map_err(decode_error)?;
            }
            if !origin && !right {
                if c.read_var::<u32>().map_err(decode_error)? == 1 {
                    c.read_string().map_err(decode_error)?;
                } else {
                    read_id(&mut c).map_err(decode_error)?;
                }
                if info & yrs::block::HAS_PARENT_SUB != 0 {
                    c.read_string().map_err(decode_error)?;
                }
            }
            let len = read_content(&mut c, info, false, JsonCount::AsRead)
                .map_err(decode_error)?
                .len;
            if len > 0 && collected.contains(&(client, clock)) {
                out.push(BLOCK_GC_REF_NUMBER);
                var(&mut out, u64::from(len));
            } else {
                out.extend(&update[start..c.next]);
            }
            clock = clock
                .checked_add(len)
                .ok_or_else(|| decode_error("horloge hors limites"))?;
        }
    }
    out.extend(&update[c.next..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_follow_what_the_page_knows() {
        assert_eq!(parts(3, None, 1, 6), [(1, 3, true), (3, 6, false)]);
        assert_eq!(parts(3, None, 3, 6), [(3, 6, false)]);
        assert_eq!(parts(3, None, 0, 3), [(0, 3, true)]);
        let beyond = vec![(5, 7), (9, 12)];
        assert_eq!(
            parts(3, Some(&beyond), 2, 11),
            [
                (2, 3, true),
                (3, 5, false),
                (5, 7, true),
                (7, 9, false),
                (9, 11, true)
            ]
        );
        // Une plage qui touche le plancher le prolonge.
        assert_eq!(
            parts(3, Some(&vec![(3, 5)]), 2, 6),
            [(2, 5, true), (5, 6, false)]
        );
    }

    #[test]
    fn a_surrogate_pair_cut_in_two_becomes_replacement_characters() {
        // « a😀b » : a = 1 unité, 😀 = 2, b = 1.
        assert_eq!(utf16_slice("a😀b", 2, 4), "\u{FFFD}b");
        assert_eq!(utf16_slice("a😀b", 0, 2), "a\u{FFFD}");
        assert_eq!(utf16_slice("a😀b", 1, 3), "😀");
        assert_eq!(utf16_slice("a😀b", 3, 4), "b");
    }
}
