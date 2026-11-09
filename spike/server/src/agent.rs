//! L'agent écrit comme un pair CRDT côté serveur : une transaction Yjs par appel, d'origine
//! `agent:{id}:{run}`, appliquée aux positions UTF-16 du texte, avec contrôle de révision.

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use spike_mmd::ydoc::{block_text, find_block_xml, insert_blocks_after, read_blocks, FRAGMENT};
use spike_mmd::{render, Block, RenderOpts};
use yrs::types::text::YChange;
use yrs::types::Attrs;
use yrs::updates::decoder::Decode;
use yrs::{GetString, Out, ReadTxn, StateVector, Text, Transact, TransactionMut, Update, XmlFragment, XmlFragmentRef, XmlOut};

use crate::revisions::{block_markdown, flatten, revision_of, snapshot_of, Snapshot};
use crate::rooms::Room;

#[derive(Deserialize)]
pub struct UpdateRequest {
    pub base_revision: String,
    pub agent: AgentRef,
    pub operations: Vec<Op>,
}

#[derive(Deserialize)]
pub struct AgentRef {
    pub id: String,
    pub run: String,
}

#[derive(Deserialize, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    StrReplace { old: String, new: String },
    Insert { after: String, markdown: String },
}

#[derive(Debug)]
pub enum AgentError {
    /// Un bloc visé a changé depuis `base_revision` : (id, MMD actuel).
    Conflict(Vec<(String, String)>),
    NoMatch(Vec<String>),
    Ambiguous(Vec<String>),
    Invalid(String),
    UnknownRevision,
}

impl IntoResponse for AgentError {
    fn into_response(self) -> Response {
        match self {
            AgentError::Conflict(blocks) => (
                StatusCode::CONFLICT,
                Json(json!({ "error": "conflict", "blocks": blocks.into_iter().map(|(id, md)| json!({ "id": id, "markdown": md })).collect::<Vec<_>>() })),
            )
                .into_response(),
            AgentError::NoMatch(c) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": "no_match", "candidates": c }))).into_response(),
            AgentError::Ambiguous(c) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": "ambiguous_match", "candidates": c }))).into_response(),
            AgentError::Invalid(m) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "error": "invalid", "message": m }))).into_response(),
            AgentError::UnknownRevision => (
                StatusCode::CONFLICT,
                Json(json!({ "error": "unknown_revision", "message": "relire la page (GET) pour obtenir une révision récente" })),
            )
                .into_response(),
        }
    }
}

/// Vue d'une page pour un agent : MMD, révision, ancres ; et l'instantané à mémoriser.
pub fn view<T: ReadTxn>(txn: &T, frag: &XmlFragmentRef, anchors: bool) -> (Value, String, Snapshot) {
    let blocks = read_blocks(txn, frag);
    let with = render(&blocks, RenderOpts { anchors: true });
    let snapshot = snapshot_of(&blocks, &with.anchors);
    let markdown = if anchors { with.markdown.clone() } else { render(&blocks, RenderOpts { anchors: false }).markdown };
    let revision = revision_of(txn);
    let map: serde_json::Map<String, Value> = with.anchors.into_iter().map(|(a, id)| (a, Value::String(id))).collect();
    (json!({ "markdown": markdown, "revision": revision, "anchors": map }), revision, snapshot)
}

pub async fn handle_update(room: &Arc<Room>, req: UpdateRequest) -> Result<Value, AgentError> {
    let snapshot = room.revisions.get(&req.base_revision).ok_or(AgentError::UnknownRevision)?;
    let origin = format!("agent:{}:{}", req.agent.id, req.agent.run);
    let ops = req.operations;
    let outcome = room
        .transact(&origin, |txn, frag| {
            // Répétition à blanc sur une copie : l'appel est atomique, rien n'est écrit si une opération échoue.
            let state = txn.encode_state_as_update_v1(&StateVector::default());
            let scratch = crate::store::new_doc();
            let sfrag = scratch.get_or_insert_xml_fragment(FRAGMENT);
            {
                let mut stx = scratch.transact_mut();
                stx.apply_update(Update::decode_v1(&state).map_err(|e| AgentError::Invalid(e.to_string()))?)
                    .map_err(|e| AgentError::Invalid(e.to_string()))?;
                run_ops(&mut stx, &sfrag, &ops, &snapshot)?;
            }
            run_ops(txn, frag, &ops, &snapshot)
        })
        .await
        .map_err(|e| AgentError::Invalid(e.to_string()))?;
    let touched = outcome?;

    let (_, revision, snap) = room.read(|txn, frag| view(txn, frag, false)).await;
    room.revisions.record(revision.clone(), snap);
    let touched_markdown: Vec<String> = room
        .read(|txn, frag| {
            let mut all = Vec::new();
            flatten(&read_blocks(txn, frag), &mut all);
            touched.iter().filter_map(|id| all.iter().find(|b| &b.id == id).map(block_markdown)).collect()
        })
        .await;
    Ok(json!({ "revision": revision, "touched_markdown": touched_markdown, "warnings": [] }))
}

fn run_ops(txn: &mut TransactionMut, frag: &XmlFragmentRef, ops: &[Op], snapshot: &Snapshot) -> Result<Vec<String>, AgentError> {
    let mut touched: Vec<String> = Vec::new();
    for op in ops {
        match op {
            Op::StrReplace { old, new } => {
                if old.is_empty() {
                    return Err(AgentError::Invalid("`old` ne peut pas être vide".into()));
                }
                let mut blocks = Vec::new();
                flatten(&read_blocks(&*txn, frag), &mut blocks);
                let mut hits: Vec<(&Block, usize)> = Vec::new();
                for b in &blocks {
                    let text = block_text(b);
                    for (byte, _) in text.match_indices(old.as_str()) {
                        hits.push((b, text[..byte].chars().count()));
                    }
                }
                match hits.len() {
                    0 => return Err(AgentError::NoMatch(candidates(&blocks, old))),
                    1 => {}
                    _ => {
                        let mut c: Vec<String> = hits.iter().map(|(b, _)| block_markdown(b)).collect();
                        c.dedup();
                        return Err(AgentError::Ambiguous(c));
                    }
                }
                let (b, start) = hits[0];
                // Rebase seulement si le bloc visé n'a pas changé depuis la révision lue.
                if !touched.contains(&b.id) && snapshot.hashes.get(&b.id) != Some(&crate::revisions::block_hash(b)) {
                    return Err(AgentError::Conflict(vec![(b.id.clone(), block_markdown(b))]));
                }
                let id = b.id.clone();
                replace_in_block(txn, frag, &id, start, old.chars().count(), new)?;
                touched.push(id);
            }
            Op::Insert { after, markdown } => {
                let target = snapshot.aliases.get(after).ok_or_else(|| AgentError::Invalid(format!("ancre inconnue : {after}")))?;
                let blocks = spike_mmd::parse(markdown).map_err(|e| AgentError::Invalid(format!("MMD ligne {} : {}", e.line, e.message)))?;
                if !insert_blocks_after(txn, frag, target, &blocks) {
                    return Err(AgentError::Conflict(vec![(target.clone(), String::new())]));
                }
                touched.extend(blocks.iter().map(|b| b.id.clone()));
            }
        }
    }
    Ok(touched)
}

/// Les 3 blocs les plus proches du texte cherché (mots en commun), pour aider l'agent à corriger.
fn candidates(blocks: &[Block], old: &str) -> Vec<String> {
    let words: Vec<String> = old.to_lowercase().split_whitespace().map(str::to_string).collect();
    let mut scored: Vec<(usize, &Block)> = blocks
        .iter()
        .map(|b| {
            let t = block_text(b).to_lowercase();
            (words.iter().filter(|w| t.contains(w.as_str())).count(), b)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out: Vec<String> = scored.iter().take(3).map(|(_, b)| block_markdown(b)).collect();
    if out.is_empty() {
        out = blocks.iter().take(3).map(block_markdown).collect();
    }
    out
}

/// Attributs de formatage du caractère à la position UTF-16 `at`.
fn attrs_at<T: ReadTxn>(text: &yrs::XmlTextRef, txn: &T, at: u32) -> Attrs {
    let mut acc = 0u32;
    for d in text.diff(txn, YChange::identity) {
        let Out::Any(yrs::Any::String(s)) = &d.insert else { continue };
        let len = s.encode_utf16().count() as u32;
        if at < acc + len {
            return d.attributes.map(|a| *a).unwrap_or_default();
        }
        acc += len;
    }
    Attrs::new()
}

/// Remplace `len` caractères à partir du caractère `start` du texte du bloc, aux positions UTF-16 de Yjs.
fn replace_in_block(txn: &mut TransactionMut, frag: &XmlFragmentRef, id: &str, start: usize, len: usize, new: &str) -> Result<(), AgentError> {
    let el = find_block_xml(&*txn, frag, id).ok_or_else(|| AgentError::Conflict(vec![(id.to_string(), String::new())]))?;
    let children: Vec<XmlOut> = el.children(&*txn).collect();
    let mut pos = 0usize;
    for child in children {
        match child {
            XmlOut::Text(t) => {
                let s = t.get_string(&*txn);
                let n = s.chars().count();
                if start >= pos && start < pos + n {
                    let local = start - pos;
                    if local + len > n {
                        return Err(AgentError::NoMatch(vec!["le texte cherché chevauche une mention ou une formule".into()]));
                    }
                    let off16: u32 = s.chars().take(local).map(|c| c.len_utf16() as u32).sum();
                    let len16: u32 = s.chars().skip(local).take(len).map(|c| c.len_utf16() as u32).sum();
                    let attrs = attrs_at(&t, &*txn, off16);
                    t.remove_range(txn, off16, len16);
                    t.insert_with_attributes(txn, off16, new, attrs);
                    return Ok(());
                }
                pos += n;
            }
            XmlOut::Element(_) => pos += 1,
            XmlOut::Fragment(_) => {}
        }
    }
    Err(AgentError::NoMatch(vec![]))
}
