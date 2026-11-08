//! Each node's key (CARD-25): what a window keeps its own state by, such as which nodes are
//! expanded. A call's id is its place among its file's calls (`song.ts#3`), so an edit above it
//! gives it another; its key stays, found again through each edit as ID-13 names a call (its file
//! and the `(` of its argument list), as a scope follows its call (TAP-9). A call no view had gets
//! a key no node of the song has had. A node that isn't a call keys by its id, which names it
//! already (`import:bass.out`, `export:out`). Kept in memory for the open song only.
//!
//! Two edits move a call's `(` so the diff loses it (review Z4): ⋯ ▸ Move swaps two calls, and
//! an undo brings a removed call back as new text. So a call the diff didn't find keeps its key
//! when one call of its callee went missing from its file and one came; and a revision whose code
//! the song had before gets that revision's keys back.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use m2_editor::GraphView;
use m2_session::Revision;
use m2_session::text::Edits;

/// The keys given so far, for the open song.
#[derive(Default)]
pub(super) struct Keys {
    /// The revision the keys were last given for, and each call's key by its id there.
    was: Option<(Arc<Revision>, HashMap<String, String>)>,
    /// How many keys were made: the next is one more.
    made: u64,
    /// The keys each code the song had was shown with, by its code's hash, newest last: what an
    /// undo or a redo brings back. As many as an undo stack goes back, about.
    seen: VecDeque<(u64, HashMap<String, String>)>,
}

/// How many codes' keys are kept for undo and redo.
const SEEN: usize = 256;

/// The hash of `rev`'s code: its source files, by path and text.
fn code_hash(rev: &Revision) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (path, text) in &rev.snapshot.files {
        if path.extension().is_some_and(|e| e == "ts" || e == "tsx" || e == "js" || e == "mjs") {
            path.hash(&mut h);
            text.hash(&mut h);
        }
    }
    h.finish()
}

impl Keys {
    /// Give each node of `view`, the view of `now`, its key.
    pub(super) fn stamp(&mut self, now: &Arc<Revision>, view: &mut GraphView) {
        let hash = code_hash(now);
        let carried = match &self.was {
            Some((was, keys)) if Arc::ptr_eq(was.compiled.code(), now.compiled.code()) => keys.clone(),
            // Code the song had before (an undo, a redo): its keys then.
            _ if let Some((_, keys)) = self.seen.iter().rev().find(|(h, _)| *h == hash) => keys.clone(),
            Some((was, keys)) => carry(was, now, keys),
            None => HashMap::new(),
        };
        let mut given: HashMap<String, String> = HashMap::new();
        for n in view.modules.iter_mut().flat_map(|m| m.nodes.iter_mut()) {
            if n.kind != "call" {
                n.key = n.id.clone();
                continue;
            }
            // One call drawn in two modules (a helper's) is one call: one key.
            let key = match given.get(&n.id).or_else(|| carried.get(&n.id)) {
                Some(k) => k.clone(),
                None => {
                    self.made += 1;
                    format!("k{}", self.made)
                }
            };
            given.insert(n.id.clone(), key.clone());
            n.key = key;
        }
        self.seen.retain(|(h, _)| *h != hash);
        self.seen.push_back((hash, given.clone()));
        while self.seen.len() > SEEN {
            self.seen.pop_front();
        }
        self.was = Some((now.clone(), given));
    }
}

/// `keys`, by call id in `was`, by the ids their calls have in `now`: a file that didn't change
/// keeps its ids; in one that did, each call is found by its `(`, carried through the file's edits.
/// A call the edit deleted has no id in `now`, and its key goes.
fn carry(was: &Revision, now: &Revision, keys: &HashMap<String, String>) -> HashMap<String, String> {
    let mut edits: HashMap<&str, Option<Edits>> = HashMap::new();
    let mut out = HashMap::new();
    for (id, key) in keys {
        let Some((file, ordinal)) = id.rsplit_once('#') else { continue };
        let Ok(ordinal) = ordinal.parse::<u32>() else { continue };
        let Some(site) = was.compiled.sites.iter().find(|s| s.file == file && s.ordinal == ordinal) else { continue };
        let found = edits.entry(file).or_insert_with(|| {
            let path = Path::new(file);
            let (before, after) = (was.snapshot.files.get(path)?, now.snapshot.files.get(path)?);
            Some(Edits::between(before, after))
        });
        let Some(e) = found else { continue };
        let open = site.open as usize;
        let Some(to) = e.carry(open..open + 1) else { continue };
        let to = to.start as u32;
        if let Some(s) = now.compiled.sites.iter().find(|s| s.file == file && s.open == to) {
            out.entry(format!("{}#{}", s.file, s.ordinal)).or_insert_with(|| key.clone());
        }
    }
    // A call the diff didn't find (⋯ ▸ Move's swap, two lines swapped): its key goes to the one
    // call of its callee that came in its file, when exactly one went and one came.
    let lost: Vec<(&str, &str)> = keys
        .iter()
        .filter(|(_, key)| !out.values().any(|k| k == *key))
        .filter_map(|(id, key)| {
            let (file, ordinal) = id.rsplit_once('#')?;
            let ordinal: u32 = ordinal.parse().ok()?;
            was.compiled.sites.iter().find(|s| s.file == file && s.ordinal == ordinal).map(|s| (key.as_str(), s.callee.as_str()))
        })
        .collect();
    let lost_files: HashMap<&str, &str> = keys.iter().filter_map(|(id, key)| Some((key.as_str(), id.rsplit_once('#')?.0))).collect();
    for (key, callee) in &lost {
        let file = lost_files[key];
        let went = lost.iter().filter(|(k, c)| c == callee && lost_files[k] == file).count();
        let came: Vec<String> = now
            .compiled
            .sites
            .iter()
            .filter(|s| s.file == file && s.callee == *callee)
            .map(|s| format!("{}#{}", s.file, s.ordinal))
            .filter(|id| !out.contains_key(id))
            .collect();
        if went == 1 && came.len() == 1 {
            out.insert(came[0].clone(), key.to_string());
        }
    }
    out
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
