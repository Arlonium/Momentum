//! Lire une page pour un agent (lot 5, Task S7 ; Rulings R1223, R1225, R1226) : la route
//! `GET /api/v1/pages/{id}/document` et l'outil `fetch` (M2) appellent [`document`].
//!
//! **Une tranche** rend au plus `limit` octets de MMD (24 Kio par défaut, 48 Kio au plus, jamais
//! plus, Ruling R1225). La lecture suit l'ordre du document à partir d'un bloc (`from`, l'alias
//! `next` de la tranche d'avant) : ce bloc et ses suivants dans sa liste, puis les suivants de
//! chaque bloc parent, chacun rendu avec ses sous-blocs, à la racine de la tranche. Un bloc trop
//! gros pour la borne se pagine en lui-même : il est rendu avec ceux de ses sous-blocs qui
//! tiennent, puis `<unloaded id="<alias>"/>` nomme le premier qui ne tient pas, que `next` désigne
//! aussi ; un bloc dont le texte seul dépasse la borne est rendu `<unloaded id="…" bytes=N/>` et ne
//! se lit pas par cette route (limite LA-o). Les tranches mises bout à bout ne sont donc pas le MMD
//! de la page (sous-blocs remontés à la racine, numérotation des listes reprise à 1, marques
//! `<unloaded>`) : pour récrire une longue page, les opérations ciblées.
//!
//! **La révision** de chaque tranche sort de la même commande de l'acteur que ses blocs
//! (`Hub::read_page`), et ses empreintes sont gardées (`revisions.rs`) ; avec `since` (la révision
//! de la première tranche), la tranche dit ce qui a changé depuis dans les blocs **déjà servis**
//! (ceux d'avant `from` dans l'ordre du document de `since`).

use std::collections::HashSet;

use mementum_acl::Action;
use mementum_crdt::anchors::{AnchorError, Anchors, BlockHashes, changed, id_key};
use mementum_md::{Block, RenderOpts, render, render_anchored};
use mementum_sync::PageRead;
use uuid::Uuid;

use crate::AppState;
use crate::access::permit;
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::types::{PageDocument, SearchAncestor};

/// Octets de MMD d'une tranche, par défaut (Ruling R1225).
pub const DEFAULT_LIMIT: usize = 24 * 1024;
/// Octets de MMD d'une tranche, au plus : le plafond de toute lecture (Ruling R1225).
pub const MAX_LIMIT: usize = 48 * 1024;
/// Octets d'une tranche, au moins : assez pour une marque `<unloaded>`, si bien que chaque
/// tranche avance.
pub const MIN_LIMIT: usize = 1024;
/// Sous-pages nommées au plus par une lecture.
pub const MAX_CHILDREN: usize = 200;

/// Ce qu'une lecture demande.
#[derive(Debug, Clone, Default)]
pub(crate) struct Fetch {
    /// Écrire l'alias de chaque bloc en ancre.
    pub(crate) anchors: bool,
    /// Le bloc d'où lire : un alias, ou l'identifiant d'un bloc.
    pub(crate) from: Option<String>,
    /// La révision de la première tranche.
    pub(crate) since: Option<String>,
    /// Octets au plus, de [`MIN_LIMIT`] à [`MAX_LIMIT`].
    pub(crate) limit: usize,
}

/// Une tranche du MMD d'une page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Slice {
    pub(crate) mmd: String,
    /// L'alias du premier bloc non rendu.
    pub(crate) next: Option<String>,
}

/// `GET /api/v1/pages/{id}/document`, et l'outil `fetch` d'une page : `read` (à la corbeille
/// aussi). Une page hors de la portée d'un jeton reçoit la réponse d'une page inconnue
/// (`access.rs`).
pub(crate) async fn document(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    fetch: Fetch,
) -> Result<PageDocument, ApiError> {
    let (node, access, space) = state
        .seen_node(user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    permit(&access, Action::Read)?;
    let read = state.hub.read_page(id).await?;
    state
        .revisions
        .served(id, &read.revision, read.hashes.clone());
    let before = match &fetch.since {
        Some(since) if *since != read.revision => Some(state.revisions.hashes(id, since)),
        _ => None,
    };
    let revision = read.revision.clone();
    let since_is_current = fetch.since.as_deref() == Some(revision.as_str());
    // Ce qui croît avec la page (l'index des ancres, la recherche de `from`, le rendu, les
    // changements) se fait hors des fils du serveur, au tour du compte (Ruling R383).
    let fetch_off = fetch.clone();
    let (slice, changed) = state
        .mmd_reads
        .run(user.party(), move || {
            read_slice(&read, &fetch_off, before.as_ref().map(Option::as_deref))
        })
        .await??;
    let edited_at = state
        .store
        .page_edits_of(&[id])
        .await?
        .first()
        .map(|edit| edit.edited_at);
    let path = crate::routes::search::ancestors(&space, &access);
    let mut named: Vec<Uuid> = path.clone();
    named.sort_unstable();
    named.dedup();
    let titles = state.store.node_titles(&named).await?;
    let children = state
        .store
        .children(node.space_id, id, MAX_CHILDREN * 5)
        .await?
        .into_iter()
        .filter(|child| space.node(child.id).is_some_and(|seen| !seen.in_trash))
        .take(MAX_CHILDREN)
        .map(|child| SearchAncestor {
            id: child.id.to_string(),
            title: child.title,
        })
        .collect();
    let truncated = slice.next.is_some();
    let (changed_since, changed) = match (&fetch.since, changed) {
        (None, _) => (None, None),
        (Some(_), _) if since_is_current => (Some(false), Some(Some(Vec::new()))),
        (Some(_), None) => (Some(true), Some(None)),
        (Some(_), Some((any, aliases))) => (Some(any), Some(Some(aliases))),
    };
    Ok(PageDocument {
        id: id.to_string(),
        space_id: node.space_id.to_string(),
        title: node.title,
        parent_id: access.visible_parent.map(|p| p.to_string()),
        path: path
            .into_iter()
            .filter_map(|p| {
                Some(SearchAncestor {
                    id: p.to_string(),
                    title: titles.get(&p)?.clone(),
                })
            })
            .collect(),
        level: access.level.into(),
        edited_at,
        revision,
        mmd: slice.mmd,
        truncated,
        next: slice.next,
        children,
        changed_since,
        changed,
    })
}

/// Ce qui a changé depuis `since` dans les blocs déjà servis : quelque chose a-t-il changé, et
/// les alias en cause ; `None` quand les empreintes de `since` ne sont plus gardées.
type Since = Option<(bool, Vec<String>)>;

/// La tranche, et, avec `since`, ce qui a changé (hors des fils du serveur). `before` : `None`
/// sans `since` (ou `since` est la révision lue) ; `Some(None)` quand ses empreintes ne sont plus
/// gardées ; `Some(Some(_))` sinon.
fn read_slice(
    read: &PageRead,
    fetch: &Fetch,
    before: Option<Option<&BlockHashes>>,
) -> Result<(Slice, Since), ApiError> {
    let anchors = Anchors::of(&read.blocks);
    let from = match &fetch.from {
        None => None,
        Some(anchor) => Some(
            anchors
                .resolve(anchor)
                .map_err(|e| ApiError::stale_anchor(e == AnchorError::Ambiguous))?
                .to_string(),
        ),
    };
    let slice = slice(
        &read.blocks,
        &anchors,
        fetch.anchors,
        from.as_deref(),
        fetch.limit,
    );
    let since = match before {
        None | Some(None) => None,
        Some(Some(before)) => Some(served_changes(before, read, from.as_deref())),
    };
    Ok((slice, since))
}

/// Ce qui a changé, depuis les empreintes `before`, dans les blocs servis avant `from` (dans
/// l'ordre du document de `before`) : tout, si `from` n'y était pas (dire trop de blocs ne fait
/// que relire, en dire trop peu ferait écrire sur une lecture périmée).
fn served_changes(
    before: &BlockHashes,
    read: &PageRead,
    from: Option<&str>,
) -> (bool, Vec<String>) {
    let from_rank = from.and_then(|from| {
        let key = id_key(from);
        before.as_slice().iter().position(|h| h.id == key)
    });
    let found = changed(before, &read.blocks, &read.hashes);
    let mut aliases = Vec::new();
    let mut seen = HashSet::new();
    let mut named = false;
    for block in found.blocks {
        named = true;
        let served = match (from_rank, block.rank) {
            (Some(limit), Some(rank)) => rank < limit,
            _ => true,
        };
        if served && seen.insert(block.alias.clone()) {
            aliases.push(block.alias);
        }
    }
    // Un changement qu'aucun alias ne nomme (un bloc arrivé en tête de page) est dit comme tel.
    let any = !aliases.is_empty() || (found.any && !named);
    (any, aliases)
}

/// La tranche de `blocks` qui commence au bloc `from` (son identifiant ; `None` : le début),
/// `limit` octets au plus (voir le module). `with_anchors` : chaque bloc porte son alias.
pub(crate) fn slice(
    blocks: &[Block],
    anchors: &Anchors,
    with_anchors: bool,
    from: Option<&str>,
    limit: usize,
) -> Slice {
    let units = continuation(blocks, from);
    let mut taken: Vec<&Block> = Vec::new();
    let mut estimate = 0usize;
    let mut next: Option<String> = None;
    let mut extra = String::new();
    for (i, unit) in units.iter().enumerate() {
        let alone = rendered(&[unit], anchors, with_anchors).len() + 1;
        if estimate + alone <= limit {
            estimate += alone;
            taken.push(unit);
            continue;
        }
        if !taken.is_empty() {
            next = anchors.alias(&unit.id);
            break;
        }
        // Le premier bloc ne tient pas : il se pagine en lui-même.
        let (inside, resume) = page_inside(unit, anchors, with_anchors, limit);
        extra = inside;
        next = match resume {
            Some(resume) => Some(resume),
            None => units.get(i + 1).and_then(|b| anchors.alias(&b.id)),
        };
        break;
    }
    if !extra.is_empty() {
        return Slice { mmd: extra, next };
    }
    // L'estimation additionne les blocs rendus seuls ; rendus ensemble (numérotation des listes),
    // ils peuvent peser un peu plus : on retire le dernier tant que la borne n'est pas tenue.
    loop {
        let mmd = rendered(&taken, anchors, with_anchors);
        if mmd.len() <= limit || taken.len() <= 1 {
            return Slice { mmd, next };
        }
        let last = taken.pop().expect("au moins deux blocs");
        next = anchors.alias(&last.id);
    }
}

/// Les blocs à lire à partir de `from`, dans l'ordre du document : `from` et ses suivants dans sa
/// liste, puis les suivants de chaque bloc parent, du plus proche au plus haut. Sans `from` (ou
/// un `from` absent), tous les blocs de premier niveau.
fn continuation<'b>(blocks: &'b [Block], from: Option<&str>) -> Vec<&'b Block> {
    fn path(blocks: &[Block], id: &str, into: &mut Vec<usize>) -> bool {
        for (i, b) in blocks.iter().enumerate() {
            into.push(i);
            if b.id == id || path(&b.children, id, into) {
                return true;
            }
            into.pop();
        }
        false
    }
    let Some(from) = from else {
        return blocks.iter().collect();
    };
    let mut at = Vec::new();
    if !path(blocks, from, &mut at) {
        return blocks.iter().collect();
    }
    // Les listes de chaque niveau du chemin, de la racine au bloc.
    let mut lists: Vec<&[Block]> = Vec::with_capacity(at.len());
    let mut list = blocks;
    for &i in &at {
        lists.push(list);
        list = &list[i].children;
    }
    let mut out = Vec::new();
    for depth in (0..at.len()).rev() {
        let start = if depth == at.len() - 1 {
            at[depth]
        } else {
            at[depth] + 1
        };
        out.extend(lists[depth][start..].iter());
    }
    out
}

/// Un bloc trop gros pour la borne, paginé en lui-même : rendu avec ceux de ses sous-blocs qui
/// tiennent, puis la marque du premier qui ne tient pas, que rend aussi le second élément ; ou,
/// son texte seul dépassant la borne, sa seule marque, et la reprise à son premier sous-bloc.
fn page_inside(
    unit: &Block,
    anchors: &Anchors,
    with_anchors: bool,
    limit: usize,
) -> (String, Option<String>) {
    let alias = anchors.alias(&unit.id).unwrap_or_default();
    let with = |k: usize| {
        let mut b = unit.clone();
        b.children.truncate(k);
        rendered(&[&b], anchors, with_anchors)
    };
    let marker = |child: &Block| {
        format!(
            "<unloaded id=\"{}\"/>\n",
            anchors.alias(&child.id).unwrap_or_default()
        )
    };
    let fits = |k: usize| {
        let mmd = with(k);
        let mark = unit.children.get(k).map_or(0, |child| marker(child).len());
        mmd.len() + mark <= limit
    };
    if !fits(0) {
        let own = with(0).len();
        let mark = format!("<unloaded id=\"{alias}\" bytes={own}/>\n");
        let resume = unit.children.first().and_then(|c| anchors.alias(&c.id));
        return (mark, resume);
    }
    // Le plus grand nombre de sous-blocs qui tiennent : par doublement, puis par dichotomie.
    let n = unit.children.len();
    let (mut low, mut high) = (0usize, 1usize);
    while high < n && fits(high) {
        low = high;
        high = (high * 2).min(n);
    }
    if high >= n && fits(n) {
        low = n;
    } else {
        while high - low > 1 {
            let mid = low + (high - low) / 2;
            if fits(mid) {
                low = mid;
            } else {
                high = mid;
            }
        }
    }
    let mut mmd = with(low);
    let resume = unit.children.get(low).map(|child| {
        mmd.push_str(&marker(child));
        anchors.alias(&child.id).unwrap_or_default()
    });
    (mmd, resume.filter(|alias| !alias.is_empty()))
}

/// Le MMD de ces blocs, chacun avec son alias en ancre si `with_anchors`.
fn rendered(blocks: &[&Block], anchors: &Anchors, with_anchors: bool) -> String {
    if !with_anchors {
        let owned: Vec<Block> = blocks.iter().map(|b| (*b).clone()).collect();
        return render(&owned, RenderOpts::default()).markdown;
    }
    fn aliased(block: &Block, anchors: &Anchors) -> Block {
        let mut b = block.clone();
        b.id = anchors.alias(&block.id).unwrap_or_default();
        b.children = block.children.iter().map(|c| aliased(c, anchors)).collect();
        b
    }
    let owned: Vec<Block> = blocks.iter().map(|b| aliased(b, anchors)).collect();
    render_anchored(&owned)
}

/// Le MMD des blocs touchés par une écriture (`UpdateResult::mmd`), avec leurs ancres : chaque
/// bloc touché que la page porte encore, avec ses sous-blocs, sauf ceux qu'un bloc touché au-dessus
/// rend déjà, dans l'ordre du document ; `limit` octets au plus, coupés entre deux blocs.
pub(crate) fn touched_mmd(blocks: &[Block], touched: &[String], limit: usize) -> (String, bool) {
    let wanted: HashSet<&str> = touched.iter().map(String::as_str).collect();
    fn walk<'b>(blocks: &'b [Block], wanted: &HashSet<&str>, into: &mut Vec<&'b Block>) {
        for b in blocks {
            if wanted.contains(b.id.as_str()) {
                into.push(b);
            } else {
                walk(&b.children, wanted, into);
            }
        }
    }
    let mut found = Vec::new();
    walk(blocks, &wanted, &mut found);
    let anchors = Anchors::of(blocks);
    let mut taken: Vec<&Block> = Vec::new();
    let mut size = 0usize;
    let mut truncated = false;
    for block in found {
        let alone = rendered(&[block], &anchors, true).len() + 1;
        if size + alone > limit {
            truncated = true;
            break;
        }
        size += alone;
        taken.push(block);
    }
    loop {
        let mmd = rendered(&taken, &anchors, true);
        if mmd.len() <= limit || taken.is_empty() {
            return (mmd, truncated);
        }
        taken.pop();
        truncated = true;
    }
}

/// Le MMD de toute la page, chaque bloc portant son alias en ancre (`GET …/mmd?anchors=block`).
pub(crate) fn anchored(blocks: &[Block]) -> String {
    let anchors = Anchors::of(blocks);
    let all: Vec<&Block> = blocks.iter().collect();
    rendered(&all, &anchors, true)
}

#[cfg(test)]
mod tests {
    use mementum_md::parse;

    use super::*;

    fn page(text: &str) -> Vec<Block> {
        let mut blocks = parse(text).unwrap();
        fn name(blocks: &mut [Block], next: &mut usize) {
            for b in blocks {
                *next += 1;
                b.id = format!("bloc-{next}");
                name(&mut b.children, next);
            }
        }
        name(&mut blocks, &mut 0);
        blocks
    }

    /// Une tranche s'arrête entre deux blocs de premier niveau, et la suivante reprend au premier
    /// bloc non rendu ; mises bout à bout, elles rendent chaque bloc une fois.
    #[test]
    fn slices_follow_each_other_without_gap() {
        let text: String = (0..60)
            .map(|i| format!("Paragraphe numéro {i} qui porte un peu de texte.\n\n"))
            .collect::<String>()
            .trim_end()
            .to_string()
            + "\n";
        let blocks = page(&text);
        let anchors = Anchors::of(&blocks);
        let mut from: Option<String> = None;
        let mut seen = Vec::new();
        loop {
            let id = from
                .as_deref()
                .map(|a| anchors.resolve(a).unwrap().to_string());
            let s = slice(&blocks, &anchors, false, id.as_deref(), 1024);
            assert!(s.mmd.len() <= 1024, "{}", s.mmd.len());
            seen.extend(s.mmd.lines().filter(|l| !l.is_empty()).map(str::to_string));
            match s.next {
                Some(next) => from = Some(next),
                None => break,
            }
        }
        assert_eq!(seen.len(), 60);
        assert_eq!(seen[59], "Paragraphe numéro 59 qui porte un peu de texte.");
    }

    /// Un générateur déterministe (xorshift), pour tirer des pages sans dépendance de plus.
    struct Draw(u64);

    impl Draw {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    /// Une page tirée au hasard : des paragraphes et des listes à puces imbriquées sur trois
    /// niveaux, des textes de quelques octets à plusieurs kilo-octets.
    fn drawn_page(draw: &mut Draw) -> Option<Vec<Block>> {
        // Chaque bloc est lu d'une ligne MMD, puis rangé dans l'arbre : un paragraphe, ou un
        // élément de liste à puces qui peut porter des sous-blocs.
        fn level(draw: &mut Draw, depth: usize) -> Option<Vec<Block>> {
            let count = 1 + draw.below(if depth == 0 { 40 } else { 12 });
            let mut out = Vec::new();
            for _ in 0..count {
                let long = draw.below(10) == 0;
                let words = 1 + draw.below(if long { 900 } else { 12 });
                let text: Vec<String> = (0..words).map(|w| format!("mot{w}")).collect();
                let bullet = draw.below(3) != 0;
                let line = if bullet {
                    format!("- {}\n", text.join(" "))
                } else {
                    format!("{}\n", text.join(" "))
                };
                let mut block = parse(&line).ok()?.into_iter().next()?;
                if bullet && depth < 2 && draw.below(3) == 0 {
                    block.children = level(draw, depth + 1)?;
                }
                out.push(block);
            }
            Some(out)
        }
        let mut blocks = mementum_md::normalize(level(draw, 0)?);
        fn name(blocks: &mut [Block], next: &mut usize) {
            for b in blocks {
                *next += 1;
                b.id = format!("bloc-{next}");
                name(&mut b.children, next);
            }
        }
        name(&mut blocks, &mut 0);
        Some(blocks)
    }

    /// Ruling R1225 : aucune tranche ne dépasse sa borne, 48 Kio au plus, sur des pages tirées au
    /// hasard, avec ou sans ancres ; chaque tranche avance, et la lecture finit.
    #[test]
    fn no_read_exceeds_48_kio() {
        let mut draw = Draw(0x9e37_79b9_7f4a_7c15);
        let (mut pages, mut markers) = (0, 0);
        for case in 0..200 {
            let Some(blocks) = drawn_page(&mut draw) else {
                continue;
            };
            pages += 1;
            let total = {
                fn count(blocks: &[Block]) -> usize {
                    blocks.iter().map(|b| 1 + count(&b.children)).sum()
                }
                count(&blocks)
            };
            let anchors = Anchors::of(&blocks);
            let limit = MIN_LIMIT + draw.below((MAX_LIMIT - MIN_LIMIT + 1) as u64) as usize;
            let with_anchors = draw.below(2) == 0;
            let mut from: Option<String> = None;
            let mut slices = 0;
            loop {
                let id = from
                    .as_deref()
                    .map(|a| anchors.resolve(a).unwrap().to_string());
                let s = slice(&blocks, &anchors, with_anchors, id.as_deref(), limit);
                assert!(
                    s.mmd.len() <= limit && s.mmd.len() <= MAX_LIMIT,
                    "cas {case} : {} octets pour une borne de {limit}",
                    s.mmd.len()
                );
                slices += 1;
                markers += usize::from(s.mmd.contains("<unloaded"));
                assert!(slices <= total + 1, "cas {case} : la lecture n'avance pas");
                match s.next {
                    Some(next) => {
                        assert_ne!(Some(&next), from.as_ref(), "cas {case} : même reprise");
                        from = Some(next);
                    }
                    None => break,
                }
            }
        }
        // Le générateur tire des pages lisibles, et des blocs plus gros que la borne.
        assert!(pages > 150, "{pages} pages lisibles sur 200");
        assert!(markers > 0, "aucun bloc paginé en lui-même");
    }
}
