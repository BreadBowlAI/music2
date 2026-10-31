//! Automate a knob (HOME-2): the value becomes a control lane's fallback, read where the value
//! was written, with the lane's empty file beside the track's other data. The lane has no clips,
//! so it plays the fallback and the song sounds the same; the timeline shows a lane to draw on.
//!
//! One Tx, confined to the value's expression, the function's parameter list and the new lane
//! (WRITE-3's example): `pan: 0.3` becomes `pan: lane.control("hats-pan", "bipolar", 0.3)` (the lane
//! Hats pan), a choice's `shape: "square"` a lane of names, `lane.control("shape", "choice", "square")`
//! (CARD-14), and
//! `() =>` becomes `({ lane }) =>` when the function didn't take its lanes yet. Undo takes the
//! lane file away and writes the value back, in one step.

use std::collections::BTreeSet;
use std::path::Path;

use m2_ir::{Kind, Module, Unit};
use m2_script::sites::{Arg, Site};
use m2_session::Session;
use m2_song::tx::{Confined, Declared as Ranges, Expect, Op, Payload, Range, TxPath};
use m2_song::{DataPath, Key, LaneAddr, LaneData, ROOT};

use crate::cable::{consts_of, descriptor_default, spelled};
use crate::controls::{Control, resolve};
use crate::mix::{self, Mixing};
use crate::plan::{self, path};
use crate::rename::track_call;
use crate::sites::{FnParams, Lane, Params, fn_params, import_insertion, imported_names};
use crate::{EditError, Edited, keys, ready};

/// An automated knob: the lane its value now comes from, and what the knob is called (`Hats
/// pan`), as the app says it.
#[derive(Clone, Debug, PartialEq)]
pub struct Automated {
    pub lane: LaneAddr,
    pub name: String,
    pub edited: Edited,
}

/// What `c`, a value of module `m`, is called, in words, and what goes in front of that when its
/// lane's key clashes (D6, M7): a declared control by its label (`decay`), in front of which goes
/// the call's binding, else its component (`pad decay`); a channel of the song's mixer by its
/// tracks and the control's word, as the Mix view says it (`channels.hats.db` is `Hats volume`,
/// `channels.hats.pan` is `Hats pan`; taste.md's words); another dictionary entry's control by its
/// entry and control. A value no control declares is named by its call's binding (or callee) and
/// its argument.
fn name_of(c: &Control, m: &Module, sites: &[Site], channel: &dyn Fn(u32, &str) -> Option<String>) -> (String, Option<String>) {
    let site_of = |id: u32| sites.iter().find(|s| s.id == id);
    let Some(slot) = c.origin.control.as_ref() else {
        let call = site_of(c.origin.site);
        let by = call.and_then(|s| s.binding.clone()).or_else(|| call.map(|s| s.callee.replace('.', " "))).unwrap_or_default();
        let last = c.origin.arg.last().filter(|a| a.parse::<usize>().is_err());
        return ([Some(by), last.cloned()].into_iter().flatten().collect::<Vec<_>>().join(" "), None);
    };
    // A send's level (`channels.hats.sends.verb.db`): Hats send to Reverb (RENDER-34).
    if let [_, entry, "sends", to, _] = slot.slot.split('.').collect::<Vec<_>>()[..] {
        let named = |k: &str| channel(slot.frame, k).unwrap_or_else(|| k.to_string());
        return (format!("{} send to {}", named(entry), named(to)), None);
    }
    if let [_, entry, control] = slot.slot.split('.').collect::<Vec<_>>()[..] {
        return match channel(slot.frame, entry) {
            // A level is Volume, never "db", which reads as a note (graph.ts' `controlLabel`).
            Some(tracks) => (format!("{tracks} {}", if control == "db" { "volume" } else { control }), None),
            None => (format!("{entry} {control}"), None),
        };
    }
    let frame = m.frames.get(slot.frame as usize);
    let binding = frame.and_then(|f| site_of(f.site)).and_then(|s| s.binding.clone());
    let component = frame.and_then(|f| f.component.as_deref()).map(|c| m2_song::pack::split_id(c).map_or(c, |(_, id)| id).to_string());
    let label = c.declared.iter().find(|d| d.slot == slot.slot).map_or(slot.slot.as_str(), |d| d.label.as_str());
    // A level is Volume, never "db", which reads as a note (graph.ts' `controlLabel`).
    let label = if label == "db" { "volume" } else { label };
    // The song's master, as the Mix view names its fader: Master volume.
    if m.key == ROOT && frame.and_then(|f| f.component.as_deref()) == Some("std:master") {
        return (format!("Master {label}"), None);
    }
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
    // The song's mixer's channels, by the tracks they play: what the Mix view calls each strip.
    let mixer = match (m.key == ROOT).then(|| mix::mixer(rev)) {
        Some(Mixing::One(mx)) => Some(mx),
        _ => None,
    };
    let channel = |frame: u32, entry: &str| {
        let mx = mixer.as_ref().filter(|mx| mx.frame == frame)?;
        let ch = mx.channels.iter().find(|ch| ch.key == entry && !ch.tracks.is_empty())?;
        let label = |t: &String| rev.compiled.ir.modules.iter().find(|x| &x.key == t).map_or(t.as_str(), |x| x.label()).to_string();
        Some(ch.tracks.iter().map(label).collect::<Vec<_>>().join(" + "))
    };
    let (mut name, prefix) = name_of(&c, m, &rev.compiled.sites, &channel);
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
            let (written, import) = match (written_as(unit, c.value), descriptor_default(&rev.compiled, &consts)) {
                (Some(name), _) => (name, None),
                (None, Some(d)) => spelled(&d, &imported_names(&text, "@m2/core"), &rev.compiled.units),
                (None, None) => (number(c.value), None),
            };
            edits.extend(c.planned(&read(&written)).ok_or_else(|| EditError::NotFound(id.to_string()))?);
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
    // An automation lane records its unit; a lane of names, `choice` or `switch` (CARD-15).
    let empty = m2_song::canonical(&LaneData::empty(&m2_runtime::lane_kind(Kind::Control(Some(unit)))));
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

/// Remove automation, the inverse of [`automate`] (HOME-2): the read of control lane `lane` of
/// `module` (`lane.control("cutoff", "hz", hz(500))`) becomes a literal of what the lane plays at
/// `beat`, the playhead (its fallback, as written, when none), spelled as its fallback is; and the
/// lane's file goes. One `Confined` Tx on the read's range and the lane; undo puts both back. The
/// track function keeps its `lane` parameter. Refused for a lane that isn't automation, or that
/// isn't read in exactly one place in the project's own code.
pub fn remove(s: &mut Session, module: &str, lane: &str, beat: Option<f64>) -> Result<Edited, EditError> {
    ready(s)?;
    if beat.is_some_and(|b| !b.is_finite()) {
        return Err(EditError::Rejected("The playhead's beat must be a number.".into()));
    }
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module);
    let m = m.ok_or_else(|| EditError::Missing(format!("There's no track `{module}`.")))?;
    let decl = m.lanes.iter().find(|l| l.key == lane).ok_or_else(|| EditError::Missing(format!("{} has no lane `{lane}`.", m.label())))?;
    let name = decl.label().to_string();
    let Kind::Control(unit) = decl.kind else {
        return Err(EditError::Rejected(format!("{}'s {name} lane isn't an automation lane, so it has no value to put back.", m.label())));
    };
    // Where the code reads it: one call, in the project's own files.
    let mut at = BTreeSet::new();
    crate::controls::walk(&m.nodes, &mut |n| {
        if let (m2_ir::Op::Lane { lane: l }, Some(o)) = (&n.op, &n.origin)
            && l == lane
        {
            at.insert(o.site);
        }
    });
    let calls: Vec<&Site> = at.iter().filter_map(|id| rev.compiled.sites.iter().find(|x| x.id == *id)).collect();
    let site = match calls[..] {
        [one] if one.in_project => one.clone(),
        [] | [_] => {
            return Err(EditError::Rejected(format!(
                "{name} isn't read in this song's own code; take the automation out where it is read."
            )));
        }
        _ => return Err(EditError::Rejected(format!("{name} is read in {} places; take the automation out in the code.", calls.len()))),
    };
    let text = rev.snapshot.files[Path::new(&site.file)].clone();
    let fallback = site.args.get(2).filter(|_| site.callee.ends_with("control")).map(|a| a.span());
    let units = &rev.compiled.units;
    let ctor_of = |span: (u32, u32)| {
        let call = rev.compiled.sites.iter().find(|x| x.file == site.file && (x.start, x.end) == span)?;
        units.iter().find(|u| u.name == call.callee && Some(u.unit) == unit)
    };
    // What it plays there, in the fallback's spelling (`hz(1250)`, `khz(1.25)`, `0.3`), else in
    // the unit's own, imported when it isn't yet.
    let (literal, import) = match (beat, fallback) {
        (None, Some((a, b))) => (text[a as usize..b as usize].to_string(), None),
        (beat, fallback) => {
            let v = match beat {
                Some(b) => rev.compiled.data.control(module, lane).value_at(b, decl.default.unwrap_or(0.0)),
                None => decl.default.unwrap_or(0.0),
            };
            match fallback.and_then(ctor_of) {
                // A choice's place is written as its name, a switch's as true or false.
                _ if unit.is_some_and(|u| !u.is_number()) => (unit.and_then(|u| written_as(u, v)).unwrap_or_else(|| number(v)), None),
                Some(c) => (format!("{}({})", c.name, number(v / c.scale)), None),
                None if fallback.is_some_and(|(a, b)| text[a as usize..b as usize].parse::<f64>().is_ok()) => (number(v), None),
                None => match unit.and_then(|u| units.iter().find(|c| c.unit == u && c.canonical)) {
                    Some(c) => {
                        let d = m2_script::component::Spelled { value: v, spelled: Some(c.name.clone()), written: v / c.scale };
                        spelled(&d, &imported_names(&text, "@m2/core"), units)
                    }
                    None => (number(v), None),
                },
            }
        }
    };
    let mut edits = vec![(site.start, site.end, literal.clone())];
    let mut ranges = vec![(site.start, site.end)];
    // A read bound to a `const` (`const drop = lane.mask("drop")`) is used by its name, and a plain
    // number written in its place is made where a call reads that name (PKG-3): those names are
    // where the graph may change too. Nothing is written there.
    if let Some(b) = site.binding.as_ref().filter(|_| site.constant) {
        fn uses(a: &Arg, name: &str, out: &mut Vec<(u32, u32)>) {
            match a {
                Arg::Name { start, end, name: n } if n == name => out.push((*start, *end)),
                Arg::Object { props, .. } => props.iter().for_each(|(_, a)| uses(a, name, out)),
                _ => {}
            }
        }
        for x in rev.compiled.sites.iter().filter(|x| x.file == site.file) {
            x.args.iter().for_each(|a| uses(a, b, &mut ranges));
        }
    }
    if let Some((at, line)) = import.and_then(|n| import_insertion(&text, "@m2/core", &n)) {
        edits.push((at, at, line));
        ranges.push((at, at));
    }
    edits.sort_by_key(|e| e.0);
    let addr = LaneAddr::new(module, lane).map_err(EditError::Rejected)?;
    let file = path(&site.file)?;
    let mut ops = vec![plan::text_edit(&file, &text, &edits)];
    if rev.snapshot.files.contains_key(&DataPath::lane(&addr).path()) {
        ops.push(Op::Remove { file: TxPath::lane(&addr) });
    }
    let declared = Ranges::Ranges(ranges.iter().map(|&(a, b)| Range::bytes(a, b)).collect());
    let expect = Expect::Confined(Confined { ranges: [(file, declared)].into(), lanes: vec![addr.clone()], modules: vec![] });
    let label = (format!("{name} automated"), format!("{name} {literal}"));
    let tx = plan::planned(s, label.clone(), ops, expect);
    let said = |why: &str| EditError::Rejected(format!("taking out the {name} automation would change more than its lane ({why})"));
    let done = plan::run_as(s, tx, said, |s, checked| {
        plan::unmoved(s, checked)?;
        // The planner's assertion: the module no longer declares the lane.
        let candidate = &checked.candidate().compiled;
        match candidate.ir.modules.iter().find(|x| x.key == module).is_some_and(|x| x.lanes.iter().any(|l| l.key == lane)) {
            true => Err(EditError::Rejected(format!("the {name} lane would still be read"))),
            false => Ok(()),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file.clone(), before: label.0, after: label.1 })
}

/// How a choice's place, or a switch's 1 or 0, is written in code (CARD-10, CARD-11): its name, or
/// true or false. None for a number.
fn written_as(unit: Unit, value: f64) -> Option<String> {
    match unit {
        Unit::Switch => Some((value != 0.0).to_string()),
        Unit::Choice(o) => o.name(value as usize).filter(|_| value >= 0.0 && value.fract() == 0.0).map(json),
        _ => None,
    }
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
