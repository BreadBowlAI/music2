//! A sampler's file (track ab): a WAV dropped on a sampler's card in Device plays in it.
//!
//! The file comes in first through [`crate::import::audio`] (a WAV only, create-only, never
//! deleted). Then [`set_sample`] commits one `TextEdit` Tx confined to the call: it writes
//! `sample: "<name>"` into the call's options, or replaces the name written there. One undo step,
//! which takes the code back; the file stays in `assets/` (music2 never deletes an asset).
//!
//! The planner asserts against the graph: every `std:sampler` kernel the call makes reads the new
//! file, and nothing outside the call's track changes (a new file makes its voices new, D10). A
//! sampler whose code gives `zones` or `regions`, or sets its file by code, is refused in words.
//!
//! A sampler that plays regions (PKG-16) says on its card what it plays, in words: "5 recordings, 2
//! layers, round robin of 2, release sounds, loops", read from its `std:regions` kernel.

use std::path::Path;

use m2_ir::{Module, Op};
use m2_script::sites::{Arg, Site};
use m2_session::{Compiled, Session};
use m2_song::tx::{Confined, Declared, Expect, Range};
use serde::Serialize;

use crate::cable::current_site;
use crate::controls::{options_of, walk};
use crate::effect::calls_of;
use crate::import::{self, Imported};
use crate::label::shown_call;
use crate::lens::Insertion;
use crate::plan::{self, path, unmoved};
use crate::{EditError, Edited, ready};

/// The component a sampler is, and the kernel its voices are: `std:sampler` for one file or zones.
pub const SAMPLER: &str = "std:sampler";

/// The kernel a sampler's voices are when it plays regions.
const REGIONS: &str = "std:regions";

/// What a sampler's card shows: the files its voices play (none yet, one, one per zone, or each its
/// regions name once), and for regions what it plays, in words.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SamplerCard {
    pub files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<String>,
}

/// A voice kernel's name, files (its assets) and values.
type Voice = (String, Vec<String>, Vec<f64>);

/// The settings of the voice kernels call `site` of `m` makes, as the first of them has them; and
/// how many kernels it makes.
fn voices_of(m: &Module, site: u32) -> (Option<Voice>, usize) {
    let mut first = None;
    let mut kernels = 0;
    walk(&m.nodes, &mut |n| {
        if let Op::Kernel { kernel, assets, values, .. } = &n.op
            && (kernel == SAMPLER || kernel == REGIONS)
            && calls_of(m, n).any(|f| f == site)
        {
            kernels += 1;
            first.get_or_insert_with(|| (kernel.clone(), assets.clone(), values.clone()));
        }
    });
    (first, kernels)
}

/// The files the `std:sampler` kernels call `site` of `m` makes read, as the first of them reads
/// them; and how many kernels it makes.
fn files_of(m: &Module, site: u32) -> (Vec<String>, usize) {
    match voices_of(m, site) {
        (Some((kernel, files, _)), n) if kernel == SAMPLER => (files, n),
        _ => (vec![], 0),
    }
}

/// What `std:regions`' values (its layout: `std/m2pkg.toml`) and `files` play, in words: "5
/// recordings, 2 layers, round robin of 2, release sounds, loops". Layers are the most ranges of
/// velocity one note is split into, each with other attack regions to play; the round robin is the
/// most takes a region has.
fn regions_said(files: usize, values: &[f64]) -> String {
    let fields = values.first().map_or(0, |&f| f as usize);
    let regions: Vec<&[f64]> = values.get(1..).filter(|_| fields > 0).map(|v| v.chunks(fields).collect()).unwrap_or_default();
    let field = |r: &[f64], k: usize| r.get(k).copied().unwrap_or(f64::NAN);
    let attack = |r: &&[f64]| field(r, 8) < 0.5;
    let layers = (0..=127)
        .map(|key| {
            let ranges: Vec<(i64, i64)> = regions
                .iter()
                .filter(|r| attack(r) && field(r, 2) <= key as f64 && key as f64 <= field(r, 3))
                .map(|r| (field(r, 4) as i64, field(r, 5) as i64))
                .collect();
            // Where the attack regions that play change, velocity by velocity.
            let mut cuts: Vec<i64> = ranges.iter().flat_map(|&(soft, loud)| [soft, loud + 1]).collect();
            cuts.sort_unstable();
            cuts.dedup();
            cuts.windows(2).filter(|w| ranges.iter().any(|&(soft, loud)| soft <= w[0] && w[0] <= loud)).count()
        })
        .max()
        .unwrap_or(0);
    let takes = regions.iter().map(|r| field(r, 7)).fold(1.0, f64::max);
    let mut said = vec![if files == 1 { "1 recording".to_string() } else { format!("{files} recordings") }];
    if layers > 1 {
        said.push(format!("{layers} layers"));
    }
    if takes > 1.0 {
        said.push(format!("round robin of {takes}"));
    }
    if regions.iter().any(|r| !attack(r)) {
        said.push("release sounds".into());
    }
    if regions.iter().any(|r| field(r, 9) >= 0.0) {
        said.push("loops".into());
    }
    said.join(", ")
}

/// The card of call `site` of `compiled`, when it is a sampler.
pub fn card(compiled: &Compiled, site: u32) -> Option<SamplerCard> {
    let m = compiled.ir.modules.iter().find(|m| m.frames.iter().any(|f| f.site == site && f.component.as_deref() == Some(SAMPLER)))?;
    Some(match voices_of(m, site).0 {
        Some((kernel, files, values)) if kernel == REGIONS => SamplerCard { regions: Some(regions_said(files.len(), &values)), files },
        Some((_, files, _)) => SamplerCard { files, regions: None },
        None => SamplerCard::default(),
    })
}

/// A string literal as TypeScript reads it: the name, quoted. Asset names hold no quote or `\`.
fn literal(name: &str) -> String {
    serde_json::to_string(name).expect("a string")
}

/// Play `file` (just imported) in sampler call `call` of revision `at`: `sample: "<name>"` written
/// into its options, or the name written there replaced. One undo step, "Sampler plays kick.wav".
pub fn set_sample(s: &mut Session, at: u64, call: &str, file: &Imported) -> Result<Edited, EditError> {
    ready(s)?;
    let site: Site = current_site(s, at, call)?.clone();
    let name = file.asset.as_str().to_string();
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.frames.iter().any(|f| f.site == site.id && f.component.as_deref() == Some(SAMPLER)));
    let m = m.ok_or_else(|| EditError::Rejected(format!("`{}` isn't a sampler: drop a WAV on a Sampler's card.", site.callee)))?;
    let key = m.key.clone();
    let d = rev.compiled.components.get(SAMPLER).cloned().ok_or_else(|| EditError::NotFound(SAMPLER.into()))?;
    let text = rev.snapshot.files.get(Path::new(&site.file)).cloned().ok_or_else(|| EditError::NotFound(site.file.clone()))?;
    let quoted = literal(&name);
    let in_code =
        |why: &str| EditError::Rejected(format!("Sampler's file can't be set here: {why}. Write `sample: {quoted}` in its code."));
    let options = options_of(m, site.id).map_err(in_code)?;
    let props = options.and_then(|i| match site.args.get(i) {
        Some(Arg::Object { props, .. }) => Some(props),
        _ => None,
    });
    if props.is_some_and(|p| p.iter().any(|(k, _)| k == "zones")) {
        return Err(EditError::Rejected("This Sampler plays zones, a file for each range of notes: change them in its code.".into()));
    }
    if props.is_some_and(|p| p.iter().any(|(k, _)| k == "regions")) {
        return Err(EditError::Rejected(
            "This Sampler plays regions, recordings each set to its notes and velocities: change them in its code.".into(),
        ));
    }
    let edit = match props.and_then(|p| p.iter().find(|(k, _)| k == "sample")) {
        Some((_, v)) => {
            let (a, b) = v.span();
            let old = text.get(a as usize..b as usize).unwrap_or("");
            // Only a name written there is replaced: anything else is code that picks the file.
            if !(old.len() >= 2 && (old.starts_with('"') && old.ends_with('"') || old.starts_with('\'') && old.ends_with('\'')))
                || old.contains('\\')
            {
                return Err(in_code("its file is picked by code"));
            }
            if old[1..old.len() - 1] == name {
                return Err(EditError::Rejected(format!("This Sampler plays {name} already.")));
            }
            (a, b, quoted.clone())
        }
        None => match Insertion::plan(&site, &d, "sample", &text, options, &rev.compiled.units).map_err(in_code)?.edits(&quoted)[..] {
            [ref one] => one.clone(),
            _ => return Err(in_code("its options can't take it")),
        },
    };
    // Taken in before the Tx names it, so the revision plays it from the start (D1).
    import::take_in(s, std::slice::from_ref(file))?;
    let file_path = path(&site.file)?;
    let label = shown_call(&site, &text, edit.0, edit.1, &edit.2);
    let ops = vec![plan::text_edit(&file_path, &text, std::slice::from_ref(&edit))];
    let expect = Expect::Confined(Confined {
        ranges: [(file_path, Declared::Ranges(vec![Range::bytes(site.start, site.end)]))].into(),
        lanes: vec![],
        modules: vec![],
    });
    let tx = plan::planned(s, label.clone(), ops, expect);
    let said = |why: &str| EditError::Rejected(format!("setting Sampler's file would change more than its file ({why})"));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        // The planner's assertion: every sampler kernel of the call reads the file, and only the
        // track's own nodes change; no lane or track comes or goes.
        let c = &checked.candidate().compiled;
        let new = c.sites.iter().find(|x| x.file == site.file && x.start == site.start && x.callee == site.callee).map(|x| x.id);
        let plays = new.zip(c.ir.modules.iter().find(|x| x.key == key)).map(|(id, m)| files_of(m, id));
        if !plays.is_some_and(|(files, kernels)| kernels > 0 && files == [name.clone()]) {
            return Err(said("its voices don't read the file"));
        }
        let delta = &checked.delta;
        let mine = |n: &Vec<String>| n.first() == Some(&key);
        let others = delta
            .added
            .iter()
            .chain(&delta.removed)
            .map(|n| &n.name)
            .chain(delta.rewired.iter().map(|w| &w.node))
            .chain(delta.values.iter().map(|v| &v.node));
        if let Some(n) = others.into_iter().find(|n| !mine(n)) {
            return Err(said(&format!("{} changes", n.join("/"))));
        }
        match delta.modules.is_empty() && delta.lanes.is_empty() {
            true => Ok(()),
            false => Err(said("its lanes or the tracks change")),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file, before: label.0, after: label.1 })
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
