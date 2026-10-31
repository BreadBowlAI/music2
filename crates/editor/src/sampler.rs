//! A sampler's file (track ab): a WAV dropped on a sampler's card in Device plays in it.
//!
//! The file comes in first through [`crate::import::audio`] (a WAV only, create-only, never
//! deleted). Then [`set_sample`] writes it as any recording is written (CARD-5,
//! [`crate::settings::set_recording`]): one `TextEdit` Tx confined to the call, which writes
//! `sample: "<name>"` into the call's options, or replaces the name written there. One undo step,
//! which takes the code back; the file stays in `assets/` (music2 never deletes an asset).
//!
//! The planner asserts against the graph that a kernel the call makes reads the new file, and the
//! Tx's `Confined` that nothing outside the call changes. A sampler whose code gives `zones` or
//! `regions`, or sets its file by code, is refused in words.
//!
//! A sampler that plays regions (PKG-16) says on its card what it plays, in words: "5 recordings, 2
//! layers, round robin of 2, release sounds, loops", read from its `std:regions` kernel.

use m2_ir::{Module, Op};
use m2_script::sites::{Arg, Site};
use m2_session::{Compiled, Session};
use serde::Serialize;

use crate::cable::current_site;
use crate::controls::{options_of, walk};
use crate::effect::calls_of;
use crate::import::Imported;
use crate::{EditError, Edited, ready};

/// The component a sampler is, and the kernel its voices are: `std:sampler` for one file or zones.
pub const SAMPLER: &str = "std:sampler";

/// The kernel a sampler's voices are when it plays regions.
const REGIONS: &str = "std:regions";

/// What a sampler plays: the files its voices play (none yet, one, one per zone, or each its regions
/// name once), and for regions what it plays, in words. Its card says these through its settings
/// (CARD-1): its keymap's and its regions' words.
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

/// Why a Sampler's file is set only in its code: its call's options (`options`, as written) give it
/// zones or regions, which it plays instead (PKG-16, rule 7).
pub(crate) fn plays_others(options: Option<&Arg>) -> Option<&'static str> {
    let Some(Arg::Object { props, .. }) = options else { return None };
    if props.iter().any(|(k, _)| k == "zones") {
        return Some("This Sampler plays zones, a file for each range of notes: change them in its code.");
    }
    props
        .iter()
        .any(|(k, _)| k == "regions")
        .then_some("This Sampler plays regions, recordings each set to its notes and velocities: change them in its code.")
}

/// Play `file` (just imported) in sampler call `call` of revision `at`: its recording `sample`, set
/// as any recording is (CARD-5, [`crate::settings::set_recording`]): `sample: "<name>"` written into
/// its options, or the name written there replaced. One undo step, "Sampler plays kick.wav".
pub fn set_sample(s: &mut Session, at: u64, call: &str, file: &Imported) -> Result<Edited, EditError> {
    can_take(s, at, call)?;
    crate::settings::set_recording(s, at, &format!("{call}/@sample"), file)
}

/// Whether sampler call `call` of revision `at` can take a file, before one is brought in (CARD-5):
/// it is a Sampler, playing no zones or regions, whose file the card can change.
pub fn can_take(s: &Session, at: u64, call: &str) -> Result<(), EditError> {
    ready(s)?;
    let site: Site = current_site(s, at, call)?.clone();
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.frames.iter().any(|f| f.site == site.id && f.component.as_deref() == Some(SAMPLER)));
    let m = m.ok_or_else(|| EditError::Rejected(format!("`{}` isn't a sampler: drop a WAV on a Sampler's card.", site.callee)))?;
    if let Some(why) = options_of(m, site.id).ok().flatten().and_then(|i| plays_others(site.args.get(i))) {
        return Err(EditError::Rejected(why.into()));
    }
    crate::settings::can_take(s, at, &format!("{call}/@sample")).map(drop)
}

#[cfg(test)]
#[path = "sampler_tests.rs"]
mod tests;
