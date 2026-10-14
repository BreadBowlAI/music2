//! The expectations a Tx is judged by (WRITE-6): a closed set, each judged on the delta from the
//! baseline to the candidate, which core computed itself. None knows a gesture (WRITE-9).
//!
//! Ranges and anchors are in each file's base coordinates. Whether something lies inside a
//! declared range is judged by spans (the step 6 design's 2.4, mapping 3): inclusive of the Tx's
//! own edits at a range's ends, strict for everything else.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use m2_song::tx::{Anchor, Confined, Declared, Expect, ModuleChange, Pos, Range as TxRange};
use m2_song::{DataPath, Sha};

use super::delta::{At, Facts, Graph, GraphDelta, Name};
use crate::text::{Coords, Point};

/// What judging needs besides the delta: both graphs, every touched file's coordinates and base
/// text, the Tx's own edits by file (in base coordinates), and the paths its ops wrote (whole or
/// by edits), removed or renamed.
pub struct Judge<'a> {
    pub delta: &'a GraphDelta,
    pub hash: &'a Sha,
    pub old: &'a Graph,
    pub new: &'a Graph,
    pub coords: &'a BTreeMap<String, Coords>,
    pub bases: &'a BTreeMap<String, String>,
    pub edits: &'a BTreeMap<String, Vec<Range<usize>>>,
    pub written: &'a BTreeSet<String>,
}

/// A position of a base text, as bytes: a byte offset, or a line and a UTF-16 column counted from
/// 0 (the step 6 design's C5). Why not, when it is off the text or inside a character.
pub fn resolve(text: &str, p: &Pos) -> Result<usize, String> {
    match *p {
        Pos::Byte(b) => {
            let b = b as usize;
            if b > text.len() || !text.is_char_boundary(b) {
                return Err(format!(
                    "byte {b} isn't a place in the file (it has {} bytes, and a position falls between characters)",
                    text.len()
                ));
            }
            Ok(b)
        }
        Pos::LineCol { line, utf16_col } => {
            let start = if line == 0 {
                0
            } else {
                text.match_indices('\n')
                    .nth(line as usize - 1)
                    .map(|(i, _)| i + 1)
                    .ok_or_else(|| format!("the file has no line {line} (from 0)"))?
            };
            let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
            let mut units = 0u32;
            for (i, c) in text[start..end].char_indices() {
                if units == utf16_col {
                    return Ok(start + i);
                }
                units += c.len_utf16() as u32;
                if units > utf16_col {
                    return Err(format!("line {line}, column {utf16_col} falls inside a character"));
                }
            }
            if units == utf16_col { Ok(end) } else { Err(format!("line {line} has {units} columns, not {utf16_col}")) }
        }
    }
}

/// A Tx's range in base bytes.
pub fn bytes(text: &str, r: &TxRange) -> Result<Range<usize>, String> {
    let (s, e) = (resolve(text, &r.start)?, resolve(text, &r.end)?);
    if s > e {
        return Err(format!("a range ends ({e}) before it starts ({s})"));
    }
    Ok(s..e)
}

impl Judge<'_> {
    fn coords(&self, file: &str) -> Coords {
        self.coords.get(file).cloned().unwrap_or_else(Coords::same)
    }

    /// The base range a place spans (inclusive of the Tx's own edits), or None when part of it
    /// is an outside change.
    fn span(&self, at: &At) -> Option<Range<usize>> {
        let c = self.coords(&at.file);
        let (s, e) = (c.span(&at.start)?, c.span(&at.end)?);
        Some(s.start.min(e.start)..s.end.max(e.end))
    }

    /// The base range a place's characters are exactly, when they are unchanged, or exactly one
    /// own edit's new text.
    fn exact(&self, at: &At) -> Option<Range<usize>> {
        let c = self.coords(&at.file);
        match (at.start, at.end) {
            (Point::Base(a), Point::Base(b)) => Some(a..b + 1),
            (Point::Own { edit: i, at: 0 }, Point::Own { edit: j, at: k })
                if i == j && c.own.0.get(i).is_some_and(|h| h.new.len() == k + 1) =>
            {
                c.own_base.get(i).cloned().flatten()
            }
            _ => None,
        }
    }

    /// The declared ranges of `file`, in base bytes; None for the whole of it.
    fn declared(&self, c: &Confined, file: &str) -> Result<Option<Vec<Range<usize>>>, String> {
        let Some((_, d)) = c.ranges.iter().find(|(p, _)| p.as_str() == file) else { return Ok(Some(vec![])) };
        match d {
            Declared::Whole => Ok(None),
            Declared::Ranges(rs) => {
                let text = self.bases.get(file).ok_or_else(|| format!("{file}'s base text isn't known, so its ranges can't be read"))?;
                rs.iter().map(|r| bytes(text, r)).collect::<Result<Vec<_>, _>>().map(Some)
            }
        }
    }

    /// Whether a node lies inside a declared range: one of its places does.
    fn inside(&self, c: &Confined, f: &Facts) -> bool {
        f.spans.iter().any(|at| match self.declared(c, &at.file) {
            Ok(None) => true,
            Ok(Some(ranges)) => self.span(at).is_some_and(|s| ranges.iter().any(|r| r.start <= s.start && s.end <= r.end.max(r.start))),
            Err(_) => false,
        })
    }
}

/// Short, readable: `kick/@song.ts:12-40#0/osc@…`.
fn shown(n: &Name) -> String {
    n.join("/")
}

/// Why the delta breaks `expect`, or nothing.
pub fn judge(expect: &Expect, j: &Judge) -> Result<(), String> {
    let d = j.delta;
    match expect {
        Expect::Identical => match d.is_empty() {
            true => Ok(()),
            false => Err(format!("it was expected to change nothing that plays, but it changes {}", summary(d))),
        },
        Expect::Seen(h) => match h == j.hash {
            true => Ok(()),
            false => Err(format!("its delta is {}, not the {} it was checked as", j.hash.prefixed(), h.prefixed())),
        },
        Expect::Journal(_) => Ok(()),
        Expect::Values(values) => {
            let other = GraphDelta { values: vec![], ..d.clone() };
            if !other.is_empty() {
                return Err(format!("it was expected to change only values, but it changes {}", summary(&other)));
            }
            let mut named: BTreeSet<&Name> = BTreeSet::new();
            for (anchor, v) in values {
                let (file, range) = match anchor {
                    Anchor::Written { file, range } | Anchor::Slot { site: m2_song::Spot { file, range }, .. } => (file, range),
                };
                let text = j.bases.get(file.as_str()).ok_or_else(|| format!("{file}'s base text isn't known"))?;
                let r = bytes(text, range)?;
                let mut found = 0;
                for (name, f) in &j.new.nodes {
                    let here = match anchor {
                        Anchor::Written { .. } => {
                            f.written.as_ref().is_some_and(|at| at.file == file.as_str() && j.exact(at) == Some(r.clone()))
                        }
                        Anchor::Slot { slot, .. } => {
                            f.slot.as_ref().is_some_and(|(at, s)| s == slot && at.file == file.as_str() && j.exact(at) == Some(r.clone()))
                        }
                    };
                    if !here {
                        continue;
                    }
                    found += 1;
                    if f.value != Some(*v) {
                        return Err(format!("{} would be {}, not {v}", shown(name), f.value.unwrap_or(f64::NAN)));
                    }
                    named.insert(name);
                }
                if found == 0 {
                    return Err(format!("the value at {file} {}..{} is no longer in the graph", r.start, r.end));
                }
            }
            match d.values.iter().find(|v| !named.contains(&v.node)) {
                Some(v) => Err(format!("{} changes too, from {} to {}, though no anchor names it", shown(&v.node), v.from, v.to)),
                None => Ok(()),
            }
        }
        Expect::Confined(c) => confined(c, j),
    }
}

fn confined(c: &Confined, j: &Judge) -> Result<(), String> {
    let d = j.delta;
    // 1. Nodes: every node that differs was made inside a declared range.
    let mut moved: BTreeSet<&Name> = BTreeSet::new();
    for n in &d.added {
        match j.new.nodes.get(&n.name) {
            Some(f) if j.inside(c, f) => {
                moved.insert(&n.name);
            }
            _ => return Err(format!("{} is added outside the ranges it may change", shown(&n.name))),
        }
    }
    for n in &d.removed {
        match j.old.nodes.get(&n.name) {
            Some(f) if j.inside(c, f) => {
                moved.insert(&n.name);
            }
            _ => return Err(format!("{} is removed outside the ranges it may change", shown(&n.name))),
        }
    }
    for w in &d.rewired {
        let here = j.new.nodes.get(&w.node).or(j.old.nodes.get(&w.node)).is_some_and(|f| j.inside(c, f));
        let to_moved = |r: &Option<super::delta::Ref>| r.as_ref().is_some_and(|r| moved.contains(&r.node));
        if !here && !to_moved(&w.from) && !to_moved(&w.to) {
            return Err(format!("{}'s `{}` is rewired outside the ranges it may change", shown(&w.node), w.input));
        }
    }
    for v in &d.values {
        if !j.new.nodes.get(&v.node).is_some_and(|f| j.inside(c, f)) {
            return Err(format!("{} changes from {} to {} outside the ranges it may change", shown(&v.node), v.from, v.to));
        }
    }
    // 2. Lanes: only the declared ones.
    if let Some(l) = d.lanes.iter().find(|l| !c.lanes.contains(l)) {
        return Err(format!("the data of {} › {} changes, which isn't declared", l.module, l.lane));
    }
    // 3. Modules: exactly as declared.
    let (want, got): (BTreeSet<&ModuleChange>, BTreeSet<&ModuleChange>) = (c.modules.iter().collect(), d.modules.iter().collect());
    if want != got {
        return Err(format!("its modules change as {got:?}, not as declared ({want:?})"));
    }
    // 4. Bytes: every edit inside a declared range of its file.
    for (file, ranges) in j.edits {
        let declared = j.declared(c, file)?;
        for r in ranges {
            let ok = declared.as_ref().is_none_or(|ds| ds.iter().any(|x| x.start <= r.start && r.end <= x.end));
            if !ok {
                return Err(format!("it edits {file} {}..{}, outside the ranges it declares", r.start, r.end));
            }
        }
    }
    // 5. A state, an asset or the arrangement changes only where the Tx's own ops wrote its file
    //    (the step 6 design's C1).
    let wrote = |p: &str| j.written.iter().any(|w| std::path::Path::new(p).starts_with(w));
    for s in &d.states {
        let mut parts = s.split('/');
        let module = parts.next().unwrap_or_default();
        let keys: Vec<m2_song::Key> = parts.filter_map(|k| m2_song::Key::new(k).ok()).collect();
        let Ok(module) = m2_song::Key::new(module) else { continue };
        let blob = DataPath::PluginState { module: module.clone(), path: keys.clone(), sidecar: false }.to_string();
        let side = DataPath::PluginState { module, path: keys, sidecar: true }.to_string();
        if !wrote(&blob) && !wrote(&side) {
            return Err(format!("the plugin state {s} changes, though the Tx doesn't write it"));
        }
    }
    if let Some(a) = d.assets.iter().find(|a| !wrote(&format!("assets/{a}"))) {
        return Err(format!("assets/{a} changes, though the Tx doesn't write it"));
    }
    if d.arrangement && !wrote("data/arrangement.json") {
        return Err("the arrangement changes, though the Tx doesn't write it".into());
    }
    Ok(())
}

/// What a delta changes, in a few words.
pub fn summary(d: &GraphDelta) -> String {
    let mut parts = vec![];
    let n = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
    if !d.added.is_empty() {
        parts.push(format!("{} added", n(d.added.len(), "node", "nodes")));
    }
    if !d.removed.is_empty() {
        parts.push(format!("{} removed", n(d.removed.len(), "node", "nodes")));
    }
    if !d.rewired.is_empty() {
        parts.push(format!("{} rewired", n(d.rewired.len(), "input", "inputs")));
    }
    if !d.values.is_empty() {
        parts.push(n(d.values.len(), "value", "values"));
    }
    if !d.lanes.is_empty() {
        parts.push(n(d.lanes.len(), "lane's data", "lanes' data"));
    }
    if !d.states.is_empty() {
        parts.push(n(d.states.len(), "plugin state", "plugin states"));
    }
    if !d.assets.is_empty() {
        parts.push(n(d.assets.len(), "asset", "assets"));
    }
    if d.arrangement {
        parts.push("the arrangement".into());
    }
    if !d.modules.is_empty() {
        parts.push(n(d.modules.len(), "module", "modules"));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // WRITE-3, C5: a line and a UTF-16 column, from 0, resolve against the base text; a column
    // inside a surrogate pair or past the line, or a byte inside a character, is refused.
    #[test]
    fn positions_resolve_against_the_base_text() {
        let t = "é = 1;\n😀 x\nend";
        assert_eq!(resolve(t, &Pos::Byte(2)), Ok(2));
        assert!(resolve(t, &Pos::Byte(1)).is_err(), "inside é");
        assert!(resolve(t, &Pos::Byte(99)).is_err());
        assert_eq!(resolve(t, &Pos::LineCol { line: 0, utf16_col: 1 }), Ok(2));
        assert_eq!(resolve(t, &Pos::LineCol { line: 1, utf16_col: 2 }), Ok(t.find('😀').unwrap() + 4));
        assert!(resolve(t, &Pos::LineCol { line: 1, utf16_col: 1 }).is_err(), "inside a surrogate pair");
        assert_eq!(resolve(t, &Pos::LineCol { line: 2, utf16_col: 3 }), Ok(t.len()));
        assert!(resolve(t, &Pos::LineCol { line: 2, utf16_col: 4 }).is_err());
        assert!(resolve(t, &Pos::LineCol { line: 3, utf16_col: 0 }).is_err());
        assert!(bytes(t, &TxRange::bytes(4, 2)).is_err());
    }
}
