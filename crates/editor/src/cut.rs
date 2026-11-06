//! Cut by (KIT-27): a Drum cut by another Drum track's hits, written as TypeScript over what is
//! there, an export and an import. The Drum's call gets `cutBy: <track>.<lane>`; the file imports
//! the track when it doesn't; the track returns its notes lane, by its key, beside its sound when it
//! doesn't. None takes `cutBy` out, and leaves the import and the export, which change no sound.
//!
//! One Tx, one undo step, confined to the bytes it writes (WRITE-10). What it means is asserted on
//! the checked candidate (WRITE-9): the Drum's voices then read the cut of that track's lane, or
//! none.

use std::path::Path;

use m2_ir::{Module, Node, Op, split_ref};
use m2_script::sites::{Arg, Site};
use m2_session::{Compiled, Revision, Session};
use m2_song::tx::{Confined, Declared as Ranges, Expect, Range};
use serde::Serialize;

use crate::controls::walk;
use crate::drums::DRUM;
use crate::kits::drum;
use crate::plan::{self, path, unmoved};
use crate::sites::{Lane, camel, fn_params, fn_result, free_binding, identifier, imports_of, properties};
use crate::tracks::import_edit;
use crate::{EditError, Edited, ready};

/// Bytes `start..end` of a file, and what replaces them.
type Edit = (u32, u32, String);

/// The Drum's kernel that a cut reaches (`@m2/drums:cut`), and the input it reads the cut at.
const CUT_KERNEL: &str = "@m2/drums:cut";

/// What a Drum card's Cut by row shows (KIT-27): the track whose hits cut it, the tracks it could be
/// cut by, each its key and name, by name; or why it can't be changed here.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CutBy {
    /// The track the Drum is on, by its key: what the route names.
    pub track: String,
    /// The track whose hits cut it, by its key; none for None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    pub choices: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// The Drum call of Drum track `track` ([`drum`]): its module and its site.
fn drum_call<'a>(rev: &'a Revision, track: &str) -> Option<(&'a Module, &'a Site)> {
    drum(rev, track)?;
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == track)?;
    let frame = m.frames.iter().find(|f| f.component.as_deref() == Some(DRUM))?;
    let site = rev.compiled.sites.iter().find(|s| s.id == frame.site)?;
    Some((m, site))
}

/// The node of `m` that `r` names, inside a poly's voice too.
fn node<'a>(m: &'a Module, r: &str) -> Option<&'a Node> {
    fn find<'a>(ns: &'a [Node], id: &str) -> Option<&'a Node> {
        ns.iter().find_map(|n| match &n.op {
            _ if n.id == id => Some(n),
            Op::Poly { body, .. } => find(&body.nodes, id),
            _ => None,
        })
    }
    find(&m.nodes, split_ref(r).0)
}

/// What cuts the Drum of track `track` in `compiled` (KIT-27): None for nothing; the track whose
/// export of its lane it reads, by key; `Some(None)` for a cut the code works out.
pub(crate) fn cut_of(compiled: &Compiled, track: &str) -> Option<Option<String>> {
    let m = compiled.ir.modules.iter().find(|m| m.key == track)?;
    let mut cut = None;
    walk(&m.nodes, &mut |n| {
        if let Op::Kernel { kernel, inputs, .. } = &n.op
            && kernel == CUT_KERNEL
        {
            cut = inputs.get("cut").cloned();
        }
    });
    let marks = cut?;
    let events = match node(m, &marks).map(|n| &n.op) {
        Some(Op::Poly { events, .. }) => events.clone(),
        _ => return Some(None),
    };
    Some(match node(m, &events).map(|n| &n.op) {
        Some(Op::Import { module, export }) => {
            let from = compiled.ir.modules.iter().find(|o| o.key == *module);
            let lane = from.and_then(|o| o.exports.get(export)).and_then(|r| node(from.unwrap(), r)).map(|n| &n.op);
            matches!(lane, Some(Op::Lane { lane }) if lane == export).then(|| module.clone())
        }
        _ => None,
    })
}

/// The file a module's code is in (`src` is `file:line:column`).
fn file_of(m: &Module) -> Option<&str> {
    m.src.as_deref().and_then(|s| s.rsplitn(3, ':').nth(2))
}

/// What Drum call `site`'s Cut by row shows; none when it isn't a Drum track's Drum.
pub(crate) fn row(s: &Session, site: &Site) -> Option<CutBy> {
    let rev = s.accepted();
    let m = rev
        .compiled
        .ir
        .modules
        .iter()
        .find(|m| m.track && m.frames.iter().any(|f| f.site == site.id && f.component.as_deref() == Some(DRUM)))?;
    let track = m.key.clone();
    if drum_call(rev, &track).is_none_or(|(_, at)| at.id != site.id) {
        return None;
    }
    let by = cut_of(&rev.compiled, &track);
    let refused = match (&by, site.in_project) {
        (_, false) => Some(format!("{}'s Drum is written in a pack. Change it in code.", m.label())),
        (Some(None), _) => Some(format!("{}'s Drum is cut by what its code works out. Change it in code.", m.label())),
        _ => None,
    };
    let mut choices: Vec<(String, String)> = rev
        .compiled
        .ir
        .modules
        .iter()
        .filter(|o| o.track && o.key != track && drum(rev, &o.key).is_some())
        .map(|o| (o.key.clone(), o.label().to_string()))
        .collect();
    // By name, as a list a person reads.
    choices.sort_by(|a, b| (a.1.to_lowercase(), &a.0).cmp(&(b.1.to_lowercase(), &b.0)));
    Some(CutBy { track, by: by.flatten(), choices, refused })
}

/// The `track(…)` call of module `key` in `rev`: its site, in the file its code is in.
fn track_call<'a>(rev: &'a Revision, key: &str, file: &str, text: &str) -> Option<&'a Site> {
    let quoted = [format!("\"{key}\""), format!("'{key}'")];
    rev.compiled.sites.iter().find(|x| {
        x.file == file
            && x.callee == "track"
            && x.args.first().is_some_and(|a| {
                let (a, b) = a.span();
                quoted.iter().any(|q| text.get(a as usize..b as usize) == Some(q.as_str()))
            })
    })
}

/// How code reads export `export` of a value named `binding`: `hats.hits`, or `hats["hits-2"]`.
fn member(binding: &str, export: &str) -> String {
    match identifier(export) {
        true => format!("{binding}.{export}"),
        false => format!("{binding}[{}]", serde_json::to_string(export).expect("a string")),
    }
}

/// The edits that make track `key` (whose code is in `file`, `text`) return its notes lane `lane`
/// by its key beside its sound: none when it does already; refused in words when it can't be sure.
fn export_edits(rev: &Revision, key: &str, lane: &str, file: &str, text: &str) -> Result<Vec<Edit>, EditError> {
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == key).ok_or_else(|| EditError::NotFound(key.to_string()))?;
    let change = |why: &str| EditError::Rejected(format!("{} can't give its hits from here: {why}. Change it in code.", m.label()));
    if let Some(r) = m.exports.get(lane) {
        return match node(m, r).map(|n| &n.op) {
            Some(Op::Lane { lane: l }) if l == lane => Ok(vec![]),
            _ => Err(change(&format!("it returns something else as {lane}"))),
        };
    }
    let call = track_call(rev, key, file, text).ok_or_else(|| change("its track() call isn't where it is expected"))?;
    let span = call.args.last().map(Arg::span).ok_or_else(|| change("its track() call has no function"))?;
    let params = fn_params(text, span).map_err(change)?;
    let result = fn_result(text, span).ok_or_else(|| change("what its function returns isn't one value written last in it"))?;
    let (reach, mut edits) = match params.lane {
        Lane::Named(name) => (name, vec![]),
        Lane::Added { start, end, text } => ("lane".to_string(), vec![(start, end, text)]),
    };
    let key_text = if identifier(lane) { lane.to_string() } else { serde_json::to_string(lane).expect("a string") };
    let read = format!("{key_text}: {reach}.notes({})", serde_json::to_string(lane).expect("a string"));
    match result.object {
        Some(object) => {
            let props = properties(text, object).ok_or_else(|| change("it returns an object written in a form Cut by can't follow"))?;
            match props.last() {
                Some(last) => edits.push((last.end, last.end, format!(", {read}"))),
                None => edits.push((object.0 + 1, object.1 - 1, format!(" {read} "))),
            }
        }
        None => {
            let was = &text[result.start as usize..result.end as usize];
            let made = format!("{{ out: {was}, {read} }}");
            edits.push((result.start, result.end, if result.arrow { format!("({made})") } else { made }));
        }
    }
    Ok(edits)
}

/// Cut the Drum of Drum track `track` by the hits of Drum track `by`, or by none (KIT-27): one Tx,
/// one undo step. Refused in words, writing nothing, where the code isn't in a shape it can be sure of.
pub fn cut_by(s: &mut Session, track: &str, by: Option<&str>) -> Result<Edited, EditError> {
    ready(s)?;
    let rev = s.accepted();
    let name = |key: &str| rev.compiled.ir.modules.iter().find(|m| m.key == key).map_or(key.to_string(), |m| m.label().to_string());
    let (m, site) = drum_call(rev, track).ok_or_else(|| {
        EditError::Rejected(format!(
            "{} plays no Drum from a lane of its own, so Cut by isn't offered there. Change it in code.",
            name(track)
        ))
    })?;
    let site = site.clone();
    if !site.in_project {
        return Err(EditError::Rejected(format!("{}'s Drum is written in a pack. Change it in code.", m.label())));
    }
    if cut_of(&rev.compiled, track) == Some(None) {
        return Err(EditError::Rejected(format!("{}'s Drum is cut by what its code works out. Change it in code.", m.label())));
    }
    let file = site.file.clone();
    let text = rev.snapshot.files.get(Path::new(&file)).ok_or_else(|| EditError::NotFound(file.clone()))?.clone();
    let options = match site.args.get(1) {
        None => None,
        Some(Arg::Object { start, end, .. }) => Some((*start, *end)),
        Some(_) => {
            return Err(EditError::Rejected(format!(
                "{}'s Drum takes its options in a form Cut by can't follow. Change it in code.",
                m.label()
            )));
        }
    };
    let props = match options {
        Some(o) => properties(&text, o)
            .ok_or_else(|| EditError::Rejected(format!("{}'s Drum's options aren't plain. Change it in code.", m.label())))?,
        None => vec![],
    };
    let at = props.iter().position(|p| p.key == "cutBy");
    let was = at.map_or("no cut".to_string(), |i| text[props[i].value.0 as usize..props[i].value.1 as usize].to_string());
    // Each file's edits: the Drum's, and the cutting track's when it is another file.
    let mut edits: Vec<(String, Vec<Edit>)> = vec![(file.clone(), vec![])];
    let now = match by {
        None => {
            let i = at.ok_or_else(|| EditError::Rejected(format!("{} isn't cut by any track.", m.label())))?;
            let p = &props[i];
            let cut = match (i.checked_sub(1).map(|j| &props[j]), props.get(i + 1)) {
                (Some(before), _) => (before.end, p.end, String::new()),
                (None, Some(after)) => (p.start, after.start, String::new()),
                // Its only option: the options go, with the comma before them.
                (None, None) => (site.args[0].span().1, options.expect("its options").1, String::new()),
            };
            edits[0].1.push(cut);
            "no cut".to_string()
        }
        Some(by) => {
            if by == track {
                return Err(EditError::Rejected(format!(
                    "{} can't be cut by its own hits: its lane cuts it already where its drum chokes.",
                    m.label()
                )));
            }
            let row = drum(rev, by).ok_or_else(|| {
                EditError::Rejected(format!("{} isn't a Drum track: a Drum is cut by another Drum track's hits.", name(by)))
            })?;
            let other = rev.compiled.ir.modules.iter().find(|o| o.key == by).expect("a Drum track's module");
            let other_file = file_of(other)
                .ok_or_else(|| EditError::Rejected(format!("{}'s code isn't in a file of the song.", other.label())))?
                .to_string();
            let other_text = rev.snapshot.files.get(Path::new(&other_file)).ok_or_else(|| EditError::NotFound(other_file.clone()))?.clone();
            // The name the Drum's file reads the track by: its import, or its `const` in one file.
            let binding = match other_file == file {
                true => track_call(rev, by, &file, &text).filter(|c| c.constant).and_then(|c| c.binding.clone()).ok_or_else(|| {
                    EditError::Rejected(format!(
                        "{} isn't named by a const in {file}, so {} can't read its hits. Change it in code.",
                        other.label(),
                        m.label()
                    ))
                })?,
                false => match imports_of(&text, &file, &other_file).into_iter().find_map(|i| i.default) {
                    Some(b) => b,
                    None => {
                        let b = free_binding(&text, &camel(by));
                        let (at, line) = import_edit(&text, &file, &other_file, &b);
                        edits[0].1.push((at, at, line));
                        b
                    }
                },
            };
            let value = member(&binding, &row.lane);
            match (at, options, props.last()) {
                (Some(i), ..) => edits[0].1.push((props[i].value.0, props[i].value.1, value.clone())),
                (None, Some(_), Some(last)) => edits[0].1.push((last.end, last.end, format!(", cutBy: {value}"))),
                (None, Some((a, b)), None) => edits[0].1.push((a, b, format!("{{ cutBy: {value} }}"))),
                (None, None, _) => edits[0].1.push((site.args[0].span().1, site.args[0].span().1, format!(", {{ cutBy: {value} }}"))),
            }
            if was == value {
                return Err(EditError::Rejected(format!("{} is cut by {} already.", m.label(), other.label())));
            }
            let exporting = export_edits(rev, by, &row.lane, &other_file, &other_text)?;
            match other_file == file {
                true => edits[0].1.extend(exporting),
                false if exporting.is_empty() => {}
                false => edits.push((other_file, exporting)),
            }
            value
        }
    };
    // What may change: the Drum's whole call (what its voices are made of changes inside it), and
    // each other edit's bytes.
    let mut ops = vec![];
    let mut ranges = std::collections::BTreeMap::new();
    for (f, e) in &edits {
        let t = if *f == file { &text } else { &rev.snapshot.files[Path::new(f)] };
        let sorted = crate::tracks::sorted(e.clone())?;
        let p = path(f)?;
        ops.push(plan::text_edit(&p, t, &sorted));
        let mut declared: Vec<Range> = sorted.iter().map(|(a, b, _)| Range::bytes(*a, *b)).collect();
        if *f == file {
            declared.push(Range::bytes(site.start, site.end));
        }
        ranges.insert(p, Ranges::Ranges(declared));
    }
    let expect = Expect::Confined(Confined { ranges, ..Confined::default() });
    let label = (was, now);
    let tx = plan::planned(s, label.clone(), ops, expect);
    let (track_name, by_key) = (m.label().to_string(), by.map(str::to_string));
    let said = |why: &str| EditError::Rejected(format!("cutting {track_name} would change more than that ({why})"));
    let track_key = track.to_string();
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        match cut_of(&checked.candidate().compiled, &track_key) {
            got if got == by_key.clone().map(Some) => Ok(()),
            _ => Err(EditError::Rejected(format!("cutting {} would change more than that", track_key))),
        }
    })?;
    Ok(Edited { revision: done.number, file, before: label.0, after: label.1 })
}

#[cfg(test)]
#[path = "cut_tests.rs"]
mod tests;
