//! A sampler's file (track ab): a WAV dropped on a sampler's card in Device plays in it.
//!
//! The file comes in first through [`crate::import::audio`] (a WAV only, create-only, never
//! deleted). Then [`set_sample`] commits one `TextEdit` Tx confined to the call: it writes
//! `sample: "<name>"` into the call's options, or replaces the name written there. One undo step,
//! which takes the code back; the file stays in `assets/` (music2 never deletes an asset).
//!
//! The planner asserts against the graph: every `std:sampler` kernel the call makes reads the new
//! file, and nothing outside the call's track changes (a new file makes its voices new, D10). A
//! sampler whose code gives `zones`, or sets its file by code, is refused in words.

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

/// The component a sampler is.
pub const SAMPLER: &str = "std:sampler";

/// What a sampler's card shows: the files its voices play (none yet, one, or one per zone).
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SamplerCard {
    pub files: Vec<String>,
}

/// The files the `std:sampler` kernels call `site` of `m` makes read, as the first of them reads
/// them; and how many kernels it makes.
fn files_of(m: &Module, site: u32) -> (Vec<String>, usize) {
    let mut files = None;
    let mut kernels = 0;
    walk(&m.nodes, &mut |n| {
        if let Op::Kernel { kernel, assets, .. } = &n.op
            && kernel == SAMPLER
            && calls_of(m, n).any(|f| f == site)
        {
            kernels += 1;
            files.get_or_insert_with(|| assets.clone());
        }
    });
    (files.unwrap_or_default(), kernels)
}

/// The card of call `site` of `compiled`, when it is a sampler.
pub fn card(compiled: &Compiled, site: u32) -> Option<SamplerCard> {
    let m = compiled.ir.modules.iter().find(|m| m.frames.iter().any(|f| f.site == site && f.component.as_deref() == Some(SAMPLER)))?;
    Some(SamplerCard { files: files_of(m, site).0 })
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
    let in_code = |why: &str| EditError::Rejected(format!("Sampler's file can't be set here: {why}. Write `sample: {quoted}` in its code."));
    let options = options_of(m, site.id).map_err(in_code)?;
    let props = options.and_then(|i| match site.args.get(i) {
        Some(Arg::Object { props, .. }) => Some(props),
        _ => None,
    });
    if props.is_some_and(|p| p.iter().any(|(k, _)| k == "zones")) {
        return Err(EditError::Rejected("This Sampler plays zones, a file for each range of notes: change them in its code.".into()));
    }
    let edit = match props.and_then(|p| p.iter().find(|(k, _)| k == "sample")) {
        Some((_, v)) => {
            let (a, b) = v.span();
            let old = text.get(a as usize..b as usize).unwrap_or("");
            // Only a name written there is replaced: anything else is code that picks the file.
            if !(old.len() >= 2 && (old.starts_with('"') && old.ends_with('"') || old.starts_with('\'') && old.ends_with('\''))) || old.contains('\\') {
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
        let others = delta.added.iter().chain(&delta.removed).map(|n| &n.name).chain(delta.rewired.iter().map(|w| &w.node)).chain(delta.values.iter().map(|v| &v.node));
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
