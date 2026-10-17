//! Automate a knob (HOME-2): the value becomes a control lane's fallback, read where the value
//! was written, with the lane's empty file beside the track's other data. The lane has no clips,
//! so it plays the fallback and the song sounds the same; the timeline shows a lane to draw on.
//!
//! One Tx, confined to the value's expression, the function's parameter list and the new lane
//! (WRITE-3's example): `pan: 0.3` becomes `pan: lane.control("hats-pan", "bipolar", 0.3)`, and
//! `() =>` becomes `({ lane }) =>` when the function didn't take its lanes yet. Undo takes the
//! lane file away and writes the value back, in one step.

use std::collections::BTreeSet;
use std::path::Path;

use m2_ir::{Kind, Module};
use m2_script::sites::Site;
use m2_session::Session;
use m2_song::tx::{Confined, Declared as Ranges, Expect, Op, Payload, Range, TxPath};
use m2_song::{DataPath, Key, LaneAddr, LaneData, ROOT};

use crate::cable::{consts_of, descriptor_default, spelled};
use crate::controls::{Control, resolve};
use crate::plan::{self, path};
use crate::rename::track_call;
use crate::sites::{FnParams, Lane, Params, fn_params, import_insertion, imported_names};
use crate::{EditError, Edited, keys, ready};

/// An automated knob: the lane its value now comes from, and what the knob is called (`hats
/// pan`), as the app says it.
#[derive(Clone, Debug, PartialEq)]
pub struct Automated {
    pub lane: LaneAddr,
    pub name: String,
    pub edited: Edited,
}

/// What `c`, a value of module `m`, is called, in words, and what goes in front of that when its
/// lane's key clashes (D6, M7): a declared control by its label (`decay`), in front of which goes
/// the call's binding, else its component (`pad decay`); a dictionary entry's control by its entry
/// and control (`channels.hats.pan` is `hats pan`). A value no control declares is named by its
/// call's binding (or callee) and its argument.
fn name_of(c: &Control, m: &Module, sites: &[Site]) -> (String, Option<String>) {
    let site_of = |id: u32| sites.iter().find(|s| s.id == id);
    let Some(slot) = c.origin.control.as_ref() else {
        let call = site_of(c.origin.site);
        let by = call.and_then(|s| s.binding.clone()).or_else(|| call.map(|s| s.callee.replace('.', " "))).unwrap_or_default();
        let last = c.origin.arg.last().filter(|a| a.parse::<usize>().is_err());
        return ([Some(by), last.cloned()].into_iter().flatten().collect::<Vec<_>>().join(" "), None);
    };
    if let [_, entry, control] = slot.slot.split('.').collect::<Vec<_>>()[..] {
        return (format!("{entry} {control}"), None);
    }
    let frame = m.frames.get(slot.frame as usize);
    let binding = frame.and_then(|f| site_of(f.site)).and_then(|s| s.binding.clone());
    let component = frame.and_then(|f| f.component.as_deref()).map(|c| c.rsplit(':').next().unwrap_or(c).to_string());
    let label = c.declared.iter().find(|d| d.slot == slot.slot).map_or(slot.slot.as_str(), |d| d.label.as_str());
    (label.to_string(), binding.or(component))
}

/// `words` as a lane shows them: the first letter a capital (`Decay`, `Hats pan`).
fn sentence(words: &str) -> String {
    let mut chars = words.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The call that made module `m`: its `track()`, or for the root its `song()`.
fn module_call<'a>(s: &'a Session, m: &Module) -> Option<&'a Site> {
    let rev = s.accepted();
    if m.key != ROOT {
        return track_call(&rev.compiled.sites, &rev.snapshot.files, m);
    }
    let file = m.src.as_deref().and_then(|s| s.rsplitn(3, ':').nth(2))?;
    let mut calls = rev.compiled.sites.iter().filter(|x| x.in_project && x.file == file && x.callee == "song");
    let call = calls.next()?;
    calls.next().is_none().then_some(call)
}

/// Where automating a control writes: its module, its call, and how the module's function
/// reaches `lane`.
pub(crate) struct Place<'a> {
    module: &'a Module,
    site: &'a Site,
    params: FnParams,
}

/// Where automating `c`, whose constants are in `modules`, would write, or why it can't, as far as
/// that is known without building: what the knob's menu says (the view) and what automate checks
/// first. `params` reads a function's parameters (memoized by the view).
pub(crate) fn place<'a>(
    s: &'a Session,
    c: &Control,
    modules: &BTreeSet<&str>,
    params: &mut dyn FnMut(&str, (u32, u32)) -> Params,
) -> Result<Place<'a>, EditError> {
    let rev = s.accepted();
    if c.unit.is_none() {
        return Err(EditError::Rejected(
            "this value has no unit, so a lane couldn't say what it holds; give it one first (`hz(500)`, `db(-6)`)".into(),
        ));
    }
    let m = match modules.iter().collect::<Vec<_>>()[..] {
        [] => return Err(EditError::Rejected("the value is no longer in the graph".into())),
        [one] => rev.compiled.ir.modules.iter().find(|m| m.key == *one).expect("its module"),
        _ => return Err(EditError::Rejected("this knob's call is shared by several tracks; automate it where it is called".into())),
    };
    let site = rev.compiled.sites.iter().find(|x| x.id == c.origin.site);
    let site = site.ok_or_else(|| EditError::Rejected("the value is no longer in the graph".into()))?;
    // `lane` is the module's: the call must be inside the function its track() is given.
    let outside = || {
        EditError::Rejected(format!("this knob's call isn't inside {}'s function, where its lanes are; automate it in the code", m.label()))
    };
    let call = module_call(s, m).ok_or_else(outside)?;
    let fn_span = call.args.last().map(|a| a.span()).filter(|_| call.file == c.file).ok_or_else(outside)?;
    if !(fn_span.0 <= site.start && site.end <= fn_span.1) {
        return Err(outside());
    }
    let params = params(&c.file, fn_span).map_err(|why| EditError::Rejected(format!("{why}; automate it in the code")))?;
    Ok(Place { module: m, site, params })
}

/// Automate control `id`, as named at revision `at`: a number written in the code, or a declared
/// control a call leaves out (written into the call as the lane, its default the fallback, as
/// `disconnect` spells it). Refused for a value with no unit, one shared by several tracks, or one
/// whose call isn't inside its track's function.
pub fn automate(s: &mut Session, at: u64, id: &str) -> Result<Automated, EditError> {
    ready(s)?;
    let c = resolve(s, at, id)?;
    let rev = s.accepted();
    let consts = consts_of(&rev.compiled, &c.origin);
    let modules: BTreeSet<&str> = consts.iter().map(|(m, ..)| m.as_str()).collect();
    let text = rev.snapshot.files[Path::new(&c.file)].clone();
    let Place { module: m, site, params } = place(s, &c, &modules, &mut |_, span| fn_params(&text, span))?;
    let (site, unit) = (site.clone(), c.unit.expect("checked"));
    let lane_of = match &params.lane {
        Lane::Named(name) => name.clone(),
        Lane::Added { .. } => "lane".to_string(),
    };

    // The lane's key: from the knob's name, free among the module's lanes and its files in any
    // capitals (ID-2). On a clash, the call's binding goes in front, then a number (D6).
    let (mut name, prefix) = name_of(&c, m, &rev.compiled.sites);
    let module = Key::new(&m.key).map_err(EditError::Rejected)?;
    let taken = |k: &str| {
        let Ok(lane) = Key::new(k) else { return true };
        m.lanes.iter().any(|l| l.key == k)
            || rev.snapshot.taken(&DataPath::lane(&LaneAddr { module: module.clone(), lane }).path()).is_some()
    };
    let own = keys::propose(&name, |_| false);
    let key = match prefix {
        _ if !own.is_empty() && !taken(&own) => own,
        Some(p) => {
            name = format!("{p} {name}");
            keys::propose(&name, taken)
        }
        None => keys::propose(&name, taken),
    };
    let key = if key.is_empty() { keys::propose("lane", taken) } else { key };
    let addr = LaneAddr { module: module.clone(), lane: Key::new(&key).map_err(EditError::Rejected)? };
    // The lane shows its key as words (`decay` as "Decay"): the code names it only when that isn't
    // what the knob is called.
    let shown = sentence(&name);
    let named = match sentence(&key.replace('-', " ")) == shown {
        true => String::new(),
        false => format!(", {{ name: {} }}", json(&shown)),
    };

    // The value's expression becomes the lane's fallback, as written; an inherited control's
    // default is written into its call as `disconnect` spells it.
    let read = |fallback: &str| format!("{lane_of}.control({}, {}, {fallback}{named})", json(&key), json(unit.name()));
    let mut edits: Vec<(u32, u32, String)> = vec![];
    let mut ranges: Vec<(u32, u32)> = vec![];
    let expression = match c.expression() {
        Some((start, end)) => {
            let written = text[start as usize..end as usize].to_string();
            edits.push((start, end, read(&written)));
            ranges.push((start, end));
            written
        }
        None => {
            let (written, import) = match descriptor_default(&rev.compiled, &consts) {
                Some(d) => spelled(&d, &imported_names(&text, "@m2/core"), &rev.compiled.units),
                None => (number(c.value), None),
            };
            let (start, end, insert) = c.planned(&read(&written)).ok_or_else(|| EditError::NotFound(id.to_string()))?;
            edits.push((start, end, insert));
            // The call makes an inherited control (PKG-3): its range is what changes.
            ranges.push((site.start, site.end));
            if let Some((at, line)) = import.and_then(|name| import_insertion(&text, "@m2/core", &name)) {
                edits.push((at, at, line));
                ranges.push((at, at));
            }
            written
        }
    };
    if let Lane::Added { start, end, text: with } = &params.lane {
        edits.push((*start, *end, with.clone()));
    }
    ranges.push((params.start, params.end));
    edits.sort_by_key(|e| e.0);

    let file = path(&c.file)?;
    let lane_file = TxPath::lane(&addr);
    let empty = m2_song::canonical(&LaneData::Control { unit: Some(unit.name().to_string()), clips: vec![] });
    let ops = vec![plan::text_edit(&file, &text, &edits), Op::Write { file: lane_file, bytes: Payload::Text(empty.into()) }];
    let declared = Ranges::Ranges(ranges.iter().map(|&(a, b)| Range::bytes(a, b)).collect());
    let expect = Expect::Confined(Confined { ranges: [(file, declared)].into(), lanes: vec![addr.clone()], modules: vec![] });
    let label = (format!("{name} {expression}"), format!("{name} automated"));
    let tx = plan::planned(s, label.clone(), ops, expect);
    let (value, at_file) = (c.value, c.file.clone());
    let said = |why: &str| EditError::Rejected(format!("automating {name} would change more than this value ({why})"));
    let done = plan::run_as(s, tx, said, |s, checked| {
        plan::unmoved(s, checked)?;
        // The planner's assertion: the module declares the lane, of the value's unit, falling
        // back to the value, so it plays what played (HOME-2).
        let candidate = &checked.candidate().compiled;
        let decl = candidate.ir.modules.iter().find(|x| x.key == addr.module.as_str()).and_then(|x| x.lanes.iter().find(|l| l.key == key));
        match decl {
            Some(l) if l.kind == Kind::Control(Some(unit)) && l.default == Some(value) => Ok(()),
            _ => Err(EditError::Rejected(format!("the lane {key} wouldn't play {name} as it is now"))),
        }
    })?;
    let edited = Edited { revision: done.number, file: at_file, before: label.0, after: label.1 };
    Ok(Automated { lane: addr, name, edited })
}

/// `s` as a JSON string, which is a TypeScript string.
fn json(s: &str) -> String {
    serde_json::to_string(s).expect("a string serialises")
}

/// A base-unit value as a plain number.
fn number(x: f64) -> String {
    if x == 0.0 { "0".to_string() } else { x.to_string() }
}

#[cfg(test)]
#[path = "automate_tests.rs"]
mod tests;
