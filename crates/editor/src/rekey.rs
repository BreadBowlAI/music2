//! Change a track's ID (polish A5, design-8b §11.1, ID-17): the key its `track()` call is given,
//! which names it in code and files its data (ID-1). One Tx, so one undo step:
//!
//! - the key string in its `track()` call;
//! - its mixer channel's key, when it is the track's key (ID-18), so `bass: …` reads `low: …`;
//! - `data/<old>/` renamed to `data/<new>/`, when the folder is there;
//! - `layout.json`'s `order` and `tracks` (its colour), when the file reads (R15).
//!
//! The track's file keeps its name, and the binding that imports it keeps its own: neither is the
//! track's identity, and renaming the file would read as a removed and an added module (§11.1 step
//! 3). Its sounds restart: what plays is matched by key (Q21; keeping them waits for the engine).

use std::collections::BTreeMap;
use std::path::Path;

use m2_session::Session;
use m2_song::tx::{Confined, Declared, Expect, ModuleChange, Op, Range, TxPath};
use m2_song::{DataPath, Key, LaneAddr};

use crate::layout::{self, Read};
use crate::mix::{self, Mixing};
use crate::plan::{self, path, unmoved};
use crate::rename::track_call;
use crate::sites::{imports_of, references, whole_lines};
use crate::tracks::{channel_keys, prop_key, taken};
use crate::{EditError, Edited, keys, ready};

/// `key`, if it can be a track's new ID in `s`'s accepted revision (a track's own is free to it),
/// else why not, in ID words: what the dialog says as you type.
pub fn check(s: &Session, module: &str, key: &str) -> Result<(), EditError> {
    let rev = s.accepted();
    let taken = taken(rev);
    if key == module {
        return Ok(());
    }
    if !Key::valid(key) {
        let like = keys::propose(key, |k| k != module && taken(k));
        let like: String = like.chars().take(32).collect::<String>().trim_matches('-').to_string();
        return Err(EditError::Rejected(format!("{key} isn't an ID: IDs are lowercase letters, digits and hyphens, such as {like}")));
    }
    // The track's own channel follows it, so its key is free to it.
    let own = match mix::mixer(rev) {
        Mixing::One(m) => m.channel_of(module).filter(|c| c.key == module).map(|c| c.key.clone()),
        _ => None,
    };
    let channel = channel_keys(rev).into_iter().any(|c| c.eq_ignore_ascii_case(key) && Some(&c) != own.as_ref());
    let track = rev.compiled.ir.modules.iter().any(|m| m.key.eq_ignore_ascii_case(key) && m.key != module);
    let data = rev.snapshot.taken(&DataPath::Module { module: Key::new(key).map_err(EditError::Rejected)? }.path()).is_some();
    match () {
        _ if channel && !track => Err(EditError::Rejected(format!("The mixer already has a channel called {key}: pick another ID"))),
        _ if track || data || key == m2_song::ROOT => {
            Err(EditError::Rejected(format!("{key} is taken: another track or its data has that ID")))
        }
        _ => Ok(()),
    }
}

/// Change track `module`'s ID to `key`. One undo step, labelled `bass → low` (and "and its mixer
/// channel" when that followed).
pub fn rekey(s: &mut Session, module: &str, key: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module && m.track).ok_or_else(|| EditError::NotFound(module.to_string()))?;
    let label = m.label().to_string();
    if key == module {
        return Err(EditError::Rejected(format!("{label}'s ID is {key} already.")));
    }
    check(s, module, key)?;
    let site = track_call(&rev.compiled.sites, &rev.snapshot.files, m)
        .ok_or_else(|| EditError::Rejected(format!("{label} is made by code that makes several tracks, so change its ID in code.")))?;
    let file = site.file.clone();
    let text = rev.snapshot.files.get(Path::new(&file)).cloned().ok_or_else(|| EditError::NotFound(file.clone()))?;
    let (from, to) = (Key::new(module).map_err(EditError::Rejected)?, Key::new(key).map_err(EditError::Rejected)?);

    // The key string, in the quotes it is written with.
    let (a, b) = site.args[0].span();
    let quote = &text[a as usize..a as usize + 1];
    let mut edits: Vec<(String, (u32, u32, String))> = vec![(file.clone(), (a, b, format!("{quote}{key}{quote}")))];
    let mut ranges: BTreeMap<String, Vec<Range>> = BTreeMap::new();
    // Where other files read it (`kick.env` in a duck): an import of another module is named by its
    // key, so each read changes with it. The innermost call around each, else its line.
    for (p, t) in rev.snapshot.files.iter() {
        let Some(f) = p.to_str().filter(|f| *f != file && matches!(DataPath::parse(p), DataPath::Code(_))) else { continue };
        for b in imports_of(t, f, &file).into_iter().filter_map(|i| i.default) {
            for at in references(t, &b) {
                let call = rev
                    .compiled
                    .sites
                    .iter()
                    .filter(|x| x.in_project && x.file == f && x.start <= at && at < x.end)
                    .min_by_key(|x| x.end - x.start);
                let (a, b) = call.map_or_else(|| whole_lines(t, at, at + b.len() as u32), |x| (x.start, x.end));
                ranges.entry(f.to_string()).or_default().push(Range::bytes(a, b));
            }
        }
    }
    // The channel, when it is called as the track is.
    let mut follows = false;
    if let Mixing::One(mx) = mix::mixer(rev)
        && let Some(c) = mx.channel_of(module).filter(|c| c.key == module)
    {
        let t = rev.snapshot.files.get(Path::new(&mx.file)).cloned().ok_or_else(|| EditError::NotFound(mx.file.clone()))?;
        let p = &c.property;
        let colon = t[p.start as usize..p.value.0 as usize].find(':').ok_or_else(|| {
            EditError::Rejected(format!("{label}'s mixer channel is written as {module} alone, so change its ID in code."))
        })? as u32;
        let named = t[p.start as usize..(p.start + colon) as usize].trim_end().len() as u32;
        edits.push((mx.file.clone(), (p.start, p.start + named, prop_key(key))));
        if mx.file != file {
            ranges.entry(mx.file.clone()).or_default().push(Range::bytes(mx.call.0, mx.call.1));
        }
        follows = true;
    }
    let mut ops = vec![];
    for f in edits.iter().map(|(f, _)| f.clone()).collect::<std::collections::BTreeSet<_>>() {
        let t = rev.snapshot.files[Path::new(&f)].clone();
        let mut mine: Vec<(u32, u32, String)> = edits.iter().filter(|(x, _)| *x == f).map(|(_, e)| e.clone()).collect();
        mine.sort_by_key(|e| e.0);
        ops.push(plan::text_edit(&path(&f)?, &t, &mine));
    }
    // Its data folder, when it has one.
    let folder = DataPath::Module { module: from.clone() };
    if !rev.snapshot.under(&folder.path()).is_empty() {
        let new = DataPath::Module { module: to.clone() };
        ops.push(Op::Rename {
            from: TxPath::of(&folder).map_err(EditError::Rejected)?,
            to: TxPath::of(&new).map_err(EditError::Rejected)?,
        });
    }
    // `layout.json`, when it reads: the key renamed in its order and its tracks.
    let layout = match layout::read_disk(&rev.project.dir) {
        Read::Valid(mut l) => {
            l.order.iter_mut().filter(|k| *k == module).for_each(|k| *k = key.to_string());
            l.tracks.iter_mut().filter(|(k, _)| k == module).for_each(|(k, _)| *k = key.to_string());
            Some(layout::write_op(s, l.canonical()))
        }
        _ => None,
    };
    let lanes: Vec<LaneAddr> = m
        .lanes
        .iter()
        .filter_map(|l| Key::new(&l.key).ok())
        .flat_map(|l| [LaneAddr { module: from.clone(), lane: l.clone() }, LaneAddr { module: to.clone(), lane: l }])
        .collect();
    // Confined (design-8b §11.1 step 4) when every file whose nodes change is one the Tx edits: the
    // track's file whole, and where others read it. A file that reads it but isn't edited (a channel
    // with a name of its own) can't declare ranges (core: "ranges are declared in song.ts, which
    // the Tx doesn't touch"), though its imports of the track change with the key: then the delta is
    // checked first, asserted, and committed only as seen (WRITE-11).
    let edited: std::collections::BTreeSet<&String> = edits.iter().map(|(f, _)| f).collect();
    let confined = ranges.keys().all(|f| edited.contains(f));
    let mut declared = BTreeMap::from([(path(&file)?, Declared::Whole)]);
    for (f, r) in ranges {
        declared.insert(path(&f)?, Declared::Ranges(r));
    }
    let rekeyed = ModuleChange::Rekeyed { from: from.clone(), to: to.clone() };
    let expect = Expect::Confined(Confined { ranges: declared, lanes, modules: vec![rekeyed.clone()] });
    let after = if follows { format!("{key} (and its mixer channel)") } else { key.to_string() };
    if let Some((op, _, _)) = &layout {
        ops.push(op.clone());
    }
    let planned = |s: &Session, expect: Expect| {
        let mut tx = plan::planned(s, (module.to_string(), after.clone()), ops.clone(), expect);
        if let Some((_, f, base)) = &layout {
            tx.base.insert(f.clone(), base.clone());
        }
        tx
    };
    let said = |why: &str| EditError::Rejected(format!("changing {label}'s ID would change more than its ID ({why})"));
    let data = rev.compiled.data.clone();
    let lane_keys: Vec<String> = m.lanes.iter().map(|l| l.key.clone()).collect();
    let (old, new) = (module.to_string(), key.to_string());
    // The planner's assertion: the delta's modules are exactly the re-key; every node that changes
    // is the track's, another module's import of it, or (when it followed) its mixer channel's; no
    // value changes; and every lane plays the same clips under the new key.
    let assert = |checked: &m2_session::Checked| -> Result<(), EditError> {
        let d = &checked.delta;
        if d.modules != [rekeyed.clone()] {
            return Err(said(&format!("its modules change as {:?}", d.modules)));
        }
        let channel = |seg: &String| follows && (seg.contains(&format!("channels.{old}.")) || seg.contains(&format!("channels.{new}.")));
        let its = |n: &Vec<String>| {
            n.first().is_some_and(|m| *m == old || *m == new)
                || n.get(1).is_some_and(|x| x.starts_with(&format!("import:{old}.")) || x.starts_with(&format!("import:{new}.")))
                || n.iter().any(channel)
        };
        let refs = |r: &Option<m2_session::tx::delta::Ref>| r.as_ref().is_some_and(|r| its(&r.node));
        if let Some(n) = d.added.iter().chain(&d.removed).map(|n| &n.name).find(|n| !its(n)) {
            return Err(said(&format!("{} changes", n.join("/"))));
        }
        if let Some(w) = d.rewired.iter().find(|w| !its(&w.node) && !refs(&w.from) && !refs(&w.to)) {
            return Err(said(&format!("{}'s {} is rewired", w.node.join("/"), w.input)));
        }
        if let Some(v) = d.values.first() {
            return Err(said(&format!("{} changes value", v.node.join("/"))));
        }
        let now = &checked.candidate().compiled.data;
        for l in &lane_keys {
            if data.lane(&old, l) != now.lane(&new, l) {
                return Err(said(&format!("its {l} lane would play other clips")));
            }
        }
        Ok(())
    };
    let tx = match confined {
        true => planned(s, expect),
        false => {
            // R26: a check on `Seen` never refuses on its expectation.
            let probe = planned(s, Expect::Seen(m2_song::Sha::of(b"probe")));
            let checked = s.check(&probe).map_err(|r| crate::refused(s, r))?;
            unmoved(s, &checked)?;
            assert(&checked)?;
            planned(s, Expect::Seen(checked.hash))
        }
    };
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        assert(checked)
    })?;
    Ok(Edited { revision: done.number, file, before: module.to_string(), after })
}

#[cfg(test)]
#[path = "rekey_tests.rs"]
mod tests;
