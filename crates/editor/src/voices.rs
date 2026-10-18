//! How an instrument plays its notes (RENDER-16, track pm): the line its Device card shows under
//! its name, in plain words, made from the `poly` ops its call makes: a std instrument, or a
//! `poly` written in a track. Read-only (D9): editing voices, mode and glide on the card is #35.

use m2_ir::{Alloc, Mode, Module, Node, Op, Steal, split_ref};
use m2_session::Compiled;

use crate::controls::walk;
use crate::effect::calls_of;

/// A poly's glide, as the card says it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Glide {
    /// None, or 0.
    None,
    /// A number or a knob, in seconds.
    Secs(f64),
    /// A signal the code makes.
    Code,
}

/// The card's line for a poly of `voices` given notes as `alloc` says, sliding over `glide`.
pub fn words(voices: u32, alloc: Alloc, glide: Glide) -> String {
    let over = match glide {
        Glide::None => None,
        Glide::Secs(s) if s < 1.0 => Some(format!("over {} ms", (s * 1000.0).round())),
        Glide::Secs(s) => Some(format!("over {} s", (s * 100.0).round() / 100.0)),
        Glide::Code => Some("over the glide set in code".to_string()),
    };
    let one = "Plays one note at a time.";
    match (alloc.mode, over) {
        (Mode::Mono, Some(over)) => format!("{one} Each note starts again, sliding from the last {over}."),
        (Mode::Mono, None) => format!("{one} Each note starts again."),
        (Mode::Legato, Some(over)) => format!("{one} A note played while another is held slides to it {over}, without starting again."),
        (Mode::Legato, None) => format!("{one} A note played while another is held carries on from it, without starting again."),
        (Mode::Poly, _) if voices == 1 => match alloc.steal {
            Steal::None => format!("{one} More are not played until it ends."),
            Steal::Oldest | Steal::Quietest => format!("{one} A new note stops the one sounding."),
        },
        (Mode::Poly, _) => {
            let more = match alloc.steal {
                Steal::Oldest => "One more stops the oldest.",
                Steal::Quietest => "One more stops the quietest.",
                Steal::None => "More are not played until one ends.",
            };
            format!("Plays up to {voices} notes at once. {more}")
        }
    }
}

/// The glide poly `n`'s ref reads in `m`.
fn glide_of(m: &Module, glide: Option<&String>) -> Glide {
    let Some(r) = glide else { return Glide::None };
    let mut found = None;
    walk(&m.nodes, &mut |n| {
        if n.id == split_ref(r).0 {
            found = Some(n.op.clone());
        }
    });
    match found {
        Some(Op::Const { value: 0.0, .. }) => Glide::None,
        Some(Op::Const { value, .. }) => Glide::Secs(value),
        _ => Glide::Code,
    }
}

/// The line on the card of call `site` of `compiled`, when the call plays notes. A poly belongs to
/// the innermost call in the project it was made in, as the Device view groups nodes (a `poly`
/// written in a track is its own call). Of the polys an instrument makes, the last is the one that
/// sounds (a drum kit first marks its hits in another); a call holding several instruments shows
/// the line only when they all say one thing.
pub fn card(compiled: &Compiled, site: u32) -> Option<String> {
    let in_project = |id: u32| compiled.sites.iter().any(|s| s.id == id && s.in_project);
    // By instrument (its innermost call, or the poly's own): the last poly's line.
    let mut lines: Vec<(Option<u32>, String)> = vec![];
    for m in &compiled.ir.modules {
        walk(&m.nodes, &mut |n: &Node| {
            let Op::Poly { voices, alloc, glide, .. } = &n.op else { return };
            let call = calls_of(m, n).find(|&f| in_project(f)).or_else(|| n.origin.as_ref().map(|o| o.site));
            if call != Some(site) {
                return;
            }
            let instrument = calls_of(m, n).next();
            let line = words(*voices, *alloc, glide_of(m, glide.as_ref()));
            match lines.iter_mut().find(|(i, _)| *i == instrument) {
                Some(l) => l.1 = line,
                None => lines.push((instrument, line)),
            }
        });
    }
    let first = lines.first()?.1.clone();
    lines.iter().all(|(_, l)| *l == first).then_some(first)
}

#[cfg(test)]
#[path = "voices_tests.rs"]
mod tests;
