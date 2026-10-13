//! std's knobs written while a song plays (step 5): a control whose kernel reads it every sample,
//! unsmoothed, is declared smooth, so a write ramps over one block instead of jumping the output
//! (PKG-14, RENDER-22a).

mod common;

use common::*;
use m2_runtime::CellId;

/// A copy of the empty template that plays `song`.
fn project(song: &str) -> std::path::PathBuf {
    let dir = temp("knobs");
    let template = repo().join("templates/empty");
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::copy(template.join("music2.toml"), dir.join("music2.toml")).unwrap();
    std::fs::copy(template.join("data/arrangement.json"), dir.join("data/arrangement.json")).unwrap();
    std::fs::write(dir.join("song.ts"), song).unwrap();
    dir
}

// PKG-14, RENDER-22a: the usual tremolo, an LFO through `range` as a gain. Turning the range's
// `lo` (or `xrange`'s) from 0.2 to 1 while the LFO is low changes the gain by about 0.8: as a
// step that is a jump of half the sine's height in one sample; ramped over the block, no sample
// moves further than the loudest sine itself does from one sample to the next (2π · 110 / 48000).
#[test]
fn a_range_bound_ramps() {
    for range in ["range", "xrange"] {
        let song = compile(project(&format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ lfo, {range} }} from \"std/control\";\n\
             import {{ osc }} from \"std/dsp\";\nimport {{ scale }} from \"std/math\";\n\
             const t = track(\"t\", {{ name: \"T\" }}, ({{ time }}) =>\n  \
             scale(osc.sine(110), {range}(lfo(time.beats, {{ period: 4 }}), 0.2, 1)));\n\
             export default song(() => t.out);\n"
        )));
        let cells = m2_runtime::cells::cells(&song.flat);
        let lo = cells.iter().position(|c| c.origin.slot() == Some("lo")).expect("the bound is a cell");
        assert!(cells[lo].smooth, "{range}: lo is declared smooth");
        let mut r = song.renderer();
        let block = 128.0 / f64::from(SR);
        let before = r.render(20.0 * block).unwrap();
        r.cell_slots().set(&[CellId(lo as u32)], 1.0, 1);
        let after = r.render(4.0 * block).unwrap();
        let joined: Vec<[f32; 2]> = before[before.len() - 256..].iter().chain(&after).copied().collect();
        let sine = (std::f32::consts::TAU * 110.0 / SR as f32) * 1.05;
        assert!(max_step(&joined) < sine, "{range}: the write ramps, at most {sine} a sample, not {}", max_step(&joined));
        assert!(peak(&after[after.len() - 256..]) > 0.9, "{range}: and arrives at a gain of about 1");
    }
}
