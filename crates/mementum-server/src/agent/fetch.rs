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
//! **Jamais de coupe silencieuse** (tour de correction de S7, R1402) : une lecture ne reprend qu'à
//! un bloc qui a un alias à lui seul ; un bloc sans identifiant, ou dont l'identifiant est porté par
//! un autre, est rendu dans la tranche, ou, s'il ne tient pas, passé sous la marque `<unloaded
//! blocks=N/>` (ces blocs ne se lisent pas par cette route), et la lecture reprend après lui.
//! `truncated` est vrai dès qu'un bloc après `from` n'est pas rendu en entier dans la tranche ;
//! `next` nul avec `truncated` vrai : la suite ne se lit pas par cette route.
//!
//! **Le coût d'une tranche** (R1401) : le nombre de blocs gardés se trouve en O(log n) rendus, par
//! doublement puis dichotomie, jamais un rendu par bloc retiré (une liste numérotée de 12 000
//! éléments rendait la tranche quadratique : un bloc rendu seul sous-estime son poids dans sa
//! liste). **La révision** de chaque tranche sort de la même commande de l'acteur que ses blocs
//! (`Hub::read_page`), et ses empreintes sont gardées (`revisions.rs`) ; la lecture elle-même
//! (blocs, empreintes, index des ancres) est gardée tant que la page n'est pas écrite
//! (`Hub::page_generation`, R1403) : une tranche suivante sur la même révision ne repasse pas par
//! l'acteur. Avec `since` (la révision de la première tranche), la tranche dit ce qui a changé
//! depuis dans les blocs **déjà servis** (ceux d'avant `from` dans l'ordre du document de `since`).

use std::collections::HashSet;
use std::sync::Arc;

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
/// Octets de la marque des blocs sautés, au plus : `<unloaded blocks=N/>` et sa fin de ligne.
const SKIP_MARK: usize = 40;
/// Octets réservés, au plus, aux marques `<unloaded>` qui suivent un bloc paginé en lui-même :
/// `<unloaded blocks=N/>` puis `<unloaded id="<alias de 53 caractères>"/>`.
const MARKS: usize = SKIP_MARK + 72;

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
    /// L'alias du bloc où reprendre.
    pub(crate) next: Option<String>,
    /// Un bloc après `from` n'est pas rendu en entier dans cette tranche.
    pub(crate) truncated: bool,
}

/// `GET /api/v1/pages/{id}/document`, et l'outil `fetch` d'une page : `read` (à la corbeille
/// aussi). Une page hors de la portée d'un jeton reçoit la réponse d'une page inconnue
/// (`access.rs`). `fetch` n'est jugé qu'une fois l'accès dit (l'ordre des refus : la page, puis
/// l'adresse).
pub(crate) async fn document(
    state: &AppState,
    user: &AuthUser,
    id: Uuid,
    fetch: Result<Fetch, ApiError>,
) -> Result<PageDocument, ApiError> {
    let (node, access, space) = state
        .seen_node(user, id)
        .await?
        .ok_or_else(ApiError::page_not_found)?;
    permit(&access, Action::Read)?;
    let fetch = fetch?;
    // La lecture gardée vaut tant que la page n'est pas écrite (R1403) : le compte est lu avant
    // la lecture de l'acteur, si bien qu'une écriture passée entre les deux fait relire.
    let generation = state.hub.page_generation(id);
    let (read, anchors) = match state.revisions.kept_read(id, generation) {
        Some((read, anchors)) => (read, Some(anchors)),
        None => (Arc::new(state.hub.read_page(id).await?), None),
    };
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
    // changements) se fait hors des fils du serveur, au tour de l'appel (Ruling R383).
    let fetch_off = fetch.clone();
    let kept = read.clone();
    let fresh = anchors.is_none();
    let (slice, changed, anchors, size) = state
        .mmd_reads
        .run(user.party(), move || {
            let anchors = anchors.unwrap_or_else(|| Arc::new(Anchors::of(&kept.blocks)));
            // Le poids d'une lecture neuve, pour la garder (R1403).
            let size = fresh.then(|| crate::agent::revisions::read_bytes(&kept));
            read_slice(
                &kept,
                &anchors,
                &fetch_off,
                before.as_ref().map(Option::as_deref),
            )
            .map(|(slice, changed)| (slice, changed, anchors, size))
        })
        .await??;
    if let Some(size) = size {
        state
            .revisions
            .keep_read(id, generation, read, anchors, size);
    }
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
        truncated: slice.truncated,
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
    anchors: &Anchors,
    fetch: &Fetch,
    before: Option<Option<&BlockHashes>>,
) -> Result<(Slice, Since), ApiError> {
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
        anchors,
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

/// L'alias de `block` s'il le désigne, lui seul : sans identifiant, ou avec un identifiant
/// qu'un autre bloc porte aussi, aucun (une lecture ne reprend pas à lui).
fn resumable(anchors: &Anchors, block: &Block) -> Option<String> {
    let alias = anchors.alias(&block.id)?;
    match anchors.resolve(&alias) {
        Ok(id) if id == block.id => Some(alias),
        _ => None,
    }
}

/// Le premier bloc de `blocks` à partir du rang `start` où une lecture peut reprendre (son
/// alias), et combien de blocs sautés avant lui (aucun alias : ils ne se lisent pas par cette
/// route).
fn resume_after(blocks: &[&Block], start: usize, anchors: &Anchors) -> (Option<String>, usize) {
    for (i, block) in blocks.iter().enumerate().skip(start) {
        if let Some(alias) = resumable(anchors, block) {
            return (Some(alias), i - start);
        }
    }
    (None, blocks.len().saturating_sub(start))
}

/// La marque des blocs sautés (`<unloaded blocks=N/>`), ajoutée à `mmd` : chaque appelant lui a
/// réservé sa place ([`SKIP_MARK`]) ; une marque qui ne tiendrait pas n'est pas écrite (la borne
/// d'abord), et c'est un défaut que les tests voient.
fn mark_skipped(mmd: &mut String, skipped: usize, limit: usize) {
    if skipped == 0 {
        return;
    }
    let mark = format!("<unloaded blocks={skipped}/>\n");
    debug_assert!(mark.len() <= SKIP_MARK && mmd.len() + mark.len() <= limit);
    if mmd.len() + mark.len() <= limit {
        mmd.push_str(&mark);
    }
}

/// Le plus grand `k` de `0..=n` tel que `fits(k)`, `fits(0)` étant vraie et `fits` vraie jusqu'à
/// un rang puis fausse : par doublement, puis par dichotomie, en O(log n) appels (R1401).
fn largest(n: usize, mut fits: impl FnMut(usize) -> bool) -> usize {
    let (mut low, mut high) = (0usize, 1usize);
    loop {
        high = high.min(n);
        if high == low {
            return low;
        }
        if !fits(high) {
            break;
        }
        low = high;
        if high == n {
            return n;
        }
        high *= 2;
    }
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if fits(mid) {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
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
    let n = units.len();
    // Le dernier rendu qui tient, pour ne pas le refaire.
    let mut best: Option<(usize, String)> = None;
    let k = largest(n, |k| {
        let mmd = rendered(&units[..k], anchors, with_anchors);
        let fits = mmd.len() <= limit;
        if fits && best.as_ref().is_none_or(|(at, _)| *at < k) {
            best = Some((k, mmd));
        }
        fits
    });
    let taken = |c: usize, best: Option<(usize, String)>| match best {
        Some((at, mmd)) if at == c => mmd,
        _ => rendered(&units[..c], anchors, with_anchors),
    };
    if k == n {
        return Slice {
            mmd: taken(n, best),
            next: None,
            truncated: false,
        };
    }
    if k > 0 {
        // La tranche reprend au plus tard des blocs non rendus qui a un alias à lui : ceux d'après
        // attendent la tranche suivante (R1402).
        if let Some(c) = (1..=k)
            .rev()
            .find(|&c| resumable(anchors, units[c]).is_some())
        {
            return Slice {
                mmd: taken(c, best),
                next: resumable(anchors, units[c]),
                truncated: true,
            };
        }
        // Aucun bloc où reprendre parmi ceux qui tiennent : les suivants sans alias à eux sont
        // sautés, et la marque qui le dit doit tenir dans la tranche, place réservée.
        let c = largest(k, |c| {
            rendered(&units[..c], anchors, with_anchors).len() + SKIP_MARK <= limit
        });
        if c > 0 {
            let mut mmd = rendered(&units[..c], anchors, with_anchors);
            let (next, skipped) = resume_after(&units, c, anchors);
            mark_skipped(&mut mmd, skipped, limit);
            return Slice {
                mmd,
                next,
                truncated: true,
            };
        }
    }
    // Le premier bloc ne tient pas (ou pas avec la marque des blocs sautés après lui) : il se
    // pagine en lui-même.
    let (mut mmd, resume) = page_inside(units[0], anchors, with_anchors, limit);
    let next = match resume {
        Some(resume) => Some(resume),
        None => {
            let (next, skipped) = resume_after(&units, 1, anchors);
            mark_skipped(&mut mmd, skipped, limit);
            next
        }
    };
    Slice {
        mmd,
        next,
        truncated: true,
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
/// tiennent, puis la marque du premier où reprendre, que rend aussi le second élément ; ou, son
/// texte seul dépassant la borne, sa seule marque, et la reprise à son premier sous-bloc qui a un
/// alias. Des sous-blocs sans alias à eux, là où il faudrait reprendre, sont sautés sous
/// `<unloaded blocks=N/>` ; `None` : aucun sous-bloc où reprendre, la lecture continue après lui.
fn page_inside(
    unit: &Block,
    anchors: &Anchors,
    with_anchors: bool,
    limit: usize,
) -> (String, Option<String>) {
    let children: Vec<&Block> = unit.children.iter().collect();
    let n = children.len();
    let with = |k: usize| {
        let mut b = unit.clone();
        b.children.truncate(k);
        rendered(&[&b], anchors, with_anchors)
    };
    let mark_at = |alias: &str| format!("<unloaded id=\"{alias}\"/>\n");
    let own = with(0);
    if own.len() + MARKS > limit {
        let mut mmd = match resumable(anchors, unit) {
            Some(alias) => format!("<unloaded id=\"{alias}\" bytes={}/>\n", own.len()),
            None => format!("<unloaded bytes={}/>\n", own.len()),
        };
        let (resume, skipped) = resume_after(&children, 0, anchors);
        mark_skipped(&mut mmd, skipped, limit);
        return (mmd, resume);
    }
    // Le plus grand nombre de sous-blocs qui tiennent, la place des marques réservée.
    let mut best: Option<(usize, String)> = Some((0, own));
    let k = largest(n, |k| {
        let mmd = with(k);
        let fits = mmd.len() + MARKS <= limit;
        if fits && best.as_ref().is_none_or(|(at, _)| *at < k) {
            best = Some((k, mmd));
        }
        fits
    });
    let taken = |c: usize, best: Option<(usize, String)>| match best {
        Some((at, mmd)) if at == c => mmd,
        _ => with(c),
    };
    if k == n {
        return (taken(n, best), None);
    }
    match (0..=k)
        .rev()
        .find(|&c| resumable(anchors, children[c]).is_some())
    {
        Some(c) => {
            let alias = resumable(anchors, children[c]).unwrap_or_default();
            let mut mmd = taken(c, best);
            mmd.push_str(&mark_at(&alias));
            (mmd, Some(alias))
        }
        None => {
            let mut mmd = taken(k, best);
            let (resume, skipped) = resume_after(&children, k, anchors);
            mark_skipped(&mut mmd, skipped, limit);
            if let Some(alias) = &resume {
                let mark = mark_at(alias);
                if mmd.len() + mark.len() <= limit {
                    mmd.push_str(&mark);
                }
            }
            (mmd, resume)
        }
    }
}

// Les rendus faits sur ce fil, pour les tests : le coût d'une tranche se compte en rendus,
// jamais en durée.
#[cfg(test)]
thread_local! {
    static RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Le MMD de ces blocs, chacun avec son alias en ancre si `with_anchors`.
fn rendered(blocks: &[&Block], anchors: &Anchors, with_anchors: bool) -> String {
    #[cfg(test)]
    RENDERS.with(|renders| renders.set(renders.get() + 1));
    if !with_anchors {
        let owned: Vec<Block> = blocks.iter().map(|b| (*b).clone()).collect();
        return render(&owned, RenderOpts::default()).markdown;
    }
    fn aliased(block: &Block, anchors: &Anchors) -> Block {
        let mut b = block.clone();
        // Un bloc sans alias à lui n'en porte aucun : une ancre qui désigne deux blocs ne sert à
        // rien qu'à faire refuser une opération (R1402).
        b.id = resumable(anchors, block).unwrap_or_default();
        b.children = block.children.iter().map(|c| aliased(c, anchors)).collect();
        b
    }
    let owned: Vec<Block> = blocks.iter().map(|b| aliased(b, anchors)).collect();
    render_anchored(&owned)
}

/// Le MMD des blocs touchés par une écriture (`UpdateResult::mmd`), avec leurs ancres : chaque
/// bloc touché que la page porte encore, avec ses sous-blocs, sauf ceux qu'un bloc touché au-dessus
/// rend déjà, dans l'ordre du document ; `limit` octets au plus, coupés entre deux blocs, en
/// O(log n) rendus (R1401). Hors des fils du serveur.
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
    let mut best: Option<(usize, String)> = None;
    let k = largest(found.len(), |k| {
        let mmd = rendered(&found[..k], &anchors, true);
        let fits = mmd.len() <= limit;
        if fits && best.as_ref().is_none_or(|(at, _)| *at < k) {
            best = Some((k, mmd));
        }
        fits
    });
    let mmd = match best {
        Some((at, mmd)) if at == k => mmd,
        _ => rendered(&found[..k], &anchors, true),
    };
    (mmd, k < found.len())
}

/// Le MMD de toute la page, chaque bloc portant son alias en ancre (`GET …/mmd?anchors=block`),
/// hors des fils du serveur.
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

    /// Une page tirée au hasard : des paragraphes, des listes à puces imbriquées sur trois
    /// niveaux, de longues listes numérotées et des cases à cocher (des blocs dont le rendu dépend
    /// de leurs voisins : le numéro, la ligne vide entre deux familles), des textes de quelques
    /// octets à plusieurs kilo-octets ; et, une page sur cinq, des blocs sans identifiant ou d'un
    /// identifiant porté deux fois (R1402).
    fn drawn_page(draw: &mut Draw) -> Option<Vec<Block>> {
        // Chaque bloc est lu d'une ligne MMD, puis rangé dans l'arbre : un paragraphe, un élément
        // de liste (à puces, numérotée, case à cocher) qui peut porter des sous-blocs.
        fn level(draw: &mut Draw, depth: usize) -> Option<Vec<Block>> {
            let count = 1 + draw.below(if depth == 0 { 40 } else { 12 });
            let mut out = Vec::new();
            for _ in 0..count {
                if depth == 0 && draw.below(8) == 0 {
                    // Une longue liste numérotée : le numéro de chaque élément dépend de ceux d'avant.
                    let items = 1 + draw.below(400);
                    for _ in 0..items {
                        out.push(parse("1. a\n").ok()?.into_iter().next()?);
                    }
                    continue;
                }
                let long = draw.below(10) == 0;
                let words = 1 + draw.below(if long { 900 } else { 12 });
                let text: Vec<String> = (0..words).map(|w| format!("mot{w}")).collect();
                let kind = draw.below(4);
                let line = match kind {
                    0 => format!("{}\n", text.join(" ")),
                    1 => format!("1. {}\n", text.join(" ")),
                    2 => format!("- [ ] {}\n", text.join(" ")),
                    _ => format!("- {}\n", text.join(" ")),
                };
                let mut block = parse(&line).ok()?.into_iter().next()?;
                if kind != 0 && depth < 2 && draw.below(3) == 0 {
                    block.children = level(draw, depth + 1)?;
                }
                out.push(block);
            }
            Some(out)
        }
        let mut blocks = mementum_md::normalize(level(draw, 0)?);
        let hostile = draw.below(5) == 0;
        fn name(blocks: &mut [Block], next: &mut usize, draw: &mut Draw, hostile: bool) {
            for b in blocks {
                *next += 1;
                b.id = match (hostile, draw.below(6)) {
                    (true, 0) => String::new(),
                    (true, 1) => "jumeau".to_string(),
                    _ => format!("bloc-{next}"),
                };
                name(&mut b.children, next, draw, hostile);
            }
        }
        name(&mut blocks, &mut 0, draw, hostile);
        Some(blocks)
    }

    fn count(blocks: &[Block]) -> usize {
        blocks.iter().map(|b| 1 + count(&b.children)).sum()
    }

    /// Les rendus faits sur ce fil depuis l'appel précédent.
    fn renders_since() -> usize {
        RENDERS.with(|renders| renders.replace(0))
    }

    /// Ruling R1225, R1401, R1402 : aucune tranche ne dépasse sa borne, 48 Kio au plus, sur des
    /// pages tirées au hasard, avec ou sans ancres ; chaque tranche avance ; la lecture finit ; une
    /// tranche coûte O(log n) rendus ; un bloc sans alias à lui n'arrête pas la lecture, et
    /// `truncated` dit toute tranche qui laisse un bloc.
    #[test]
    fn no_read_exceeds_48_kio() {
        let mut draw = Draw(0x9e37_79b9_7f4a_7c15);
        let (mut pages, mut markers, mut skipped, mut most) = (0, 0, 0, 0);
        for case in 0..200 {
            let Some(blocks) = drawn_page(&mut draw) else {
                continue;
            };
            pages += 1;
            let total = count(&blocks);
            let anchors = Anchors::of(&blocks);
            let limit = MIN_LIMIT + draw.below((MAX_LIMIT - MIN_LIMIT + 1) as u64) as usize;
            let with_anchors = draw.below(2) == 0;
            let mut from: Option<String> = None;
            let mut slices = 0;
            renders_since();
            loop {
                let id = from
                    .as_deref()
                    .map(|a| anchors.resolve(a).unwrap().to_string());
                let s = slice(&blocks, &anchors, with_anchors, id.as_deref(), limit);
                let renders = renders_since();
                most = most.max(renders);
                assert!(
                    renders <= 4 * (usize::BITS - total.leading_zeros()) as usize + 8,
                    "cas {case} : {renders} rendus pour une tranche d'une page de {total} blocs"
                );
                assert!(
                    s.mmd.len() <= limit && s.mmd.len() <= MAX_LIMIT,
                    "cas {case} : {} octets pour une borne de {limit}",
                    s.mmd.len()
                );
                assert!(
                    s.next.is_none() || s.truncated,
                    "cas {case} : une reprise sans tranche coupée"
                );
                slices += 1;
                markers += usize::from(s.mmd.contains("<unloaded id"));
                skipped += usize::from(s.mmd.contains("<unloaded blocks"));
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
        // Le générateur tire des pages lisibles, des blocs plus gros que la borne, et des blocs
        // sans alias à eux là où une tranche se coupe.
        assert!(pages > 150, "{pages} pages lisibles sur 200");
        assert!(markers > 0, "aucun bloc paginé en lui-même");
        assert!(skipped > 0, "aucun bloc sauté faute d'alias");
        assert!(most > 0);
    }

    /// Le défaut que la relecture de S7 a trouvé (R1401) : une liste numérotée de 12 000 éléments,
    /// dont chacun pèse plus dans sa liste que rendu seul. Une tranche y coûte O(log n) rendus,
    /// et la lecture entière en compte autant par tranche.
    #[test]
    fn a_long_numbered_list_costs_a_logarithmic_number_of_renders() {
        let text: String = (1..=12_000).map(|i| format!("{i}. a\n")).collect();
        let mut blocks = parse(&text).unwrap();
        for (i, b) in blocks.iter_mut().enumerate() {
            b.id = format!("bloc-{i}");
        }
        let anchors = Anchors::of(&blocks);
        let mut from: Option<String> = None;
        let mut slices = 0;
        renders_since();
        loop {
            let id = from
                .as_deref()
                .map(|a| anchors.resolve(a).unwrap().to_string());
            let s = slice(&blocks, &anchors, false, id.as_deref(), MAX_LIMIT);
            let renders = renders_since();
            assert!(renders <= 40, "{renders} rendus pour une tranche");
            assert!(s.mmd.len() <= MAX_LIMIT);
            slices += 1;
            match s.next {
                Some(next) => from = Some(next),
                None => break,
            }
        }
        assert!(slices > 1, "{slices} tranches");
    }

    /// `touched_mmd` aussi coûte O(log n) rendus, et dit quand il coupe.
    #[test]
    fn touched_blocks_are_rendered_in_a_logarithmic_number_of_renders() {
        let text: String = (1..=12_000).map(|i| format!("{i}. a\n")).collect();
        let mut blocks = parse(&text).unwrap();
        for (i, b) in blocks.iter_mut().enumerate() {
            b.id = format!("bloc-{i}");
        }
        let touched: Vec<String> = blocks.iter().map(|b| b.id.clone()).collect();
        renders_since();
        let (mmd, truncated) = touched_mmd(&blocks, &touched, 4096);
        let renders = renders_since();
        assert!(renders <= 40, "{renders} rendus");
        assert!(mmd.len() <= 4096 && truncated);
    }

    /// R1402 : un bloc sans identifiant, puis deux blocs du même identifiant, là où les tranches se
    /// coupent. La lecture ne s'arrête pas en silence : chaque tranche qui laisse un bloc le dit,
    /// les blocs sans alias à eux sont rendus quand ils tiennent, ou sautés sous une marque, et la
    /// lecture finit ; mis bout à bout, ce qu'elle a rendu et sauté couvre la page.
    #[test]
    fn a_block_without_an_alias_of_its_own_never_stops_a_read_in_silence() {
        let text: String = (0..300)
            .map(|i| format!("Paragraphe {i} : un texte de quelques mots, pour la longueur.\n\n"))
            .collect::<String>()
            .trim_end()
            .to_string()
            + "\n";
        let mut blocks = parse(&text).unwrap();
        // Des blocs sans alias à eux, seuls ou par deux, et une suite de soixante sans
        // identifiant, plus longue qu'une tranche : celle-là ne se lit pas par cette route.
        for (i, b) in blocks.iter_mut().enumerate() {
            b.id = match (i, i % 7) {
                (100..160, _) | (_, 3) => String::new(),
                (_, 5 | 6) => "jumeau".to_string(),
                _ => format!("bloc-{i}"),
            };
        }
        let anchors = Anchors::of(&blocks);
        let (mut seen, mut skipped) = (0usize, 0usize);
        let mut from: Option<String> = None;
        loop {
            let id = from
                .as_deref()
                .map(|a| anchors.resolve(a).unwrap().to_string());
            let s = slice(&blocks, &anchors, true, id.as_deref(), 1024);
            seen += s
                .mmd
                .lines()
                .filter(|l| l.starts_with("Paragraphe"))
                .count();
            skipped += s
                .mmd
                .lines()
                .filter_map(|l| l.strip_prefix("<unloaded blocks="))
                .map(|n| n.trim_end_matches("/>").parse::<usize>().unwrap())
                .sum::<usize>();
            match s.next {
                Some(next) => {
                    assert!(s.truncated);
                    from = Some(next);
                }
                None => break,
            }
        }
        assert_eq!(seen + skipped, 300, "{seen} rendus, {skipped} sautés");
        assert!(seen > 200 && skipped > 0, "{seen} rendus, {skipped} sautés");
    }
}
