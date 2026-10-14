//! The fast lane reader and writer against the old ones ([`super::oracle`]): every lane file in
//! the repository, every invalid case the lane tests name, and generated files written in any key
//! order, with repeated keys, odd ids, `-0`, named and bad pitches and the wrong shapes.

use std::path::{Path, PathBuf};

use super::{LaneData, LaneKind, canonical, oracle};

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }

    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

/// A JSON object written as given: keys in this order, repeats kept.
fn object(fields: Vec<(&str, String)>) -> String {
    let fields: Vec<String> = fields.into_iter().map(|(k, v)| format!("\"{k}\": {v}")).collect();
    format!("{{{}}}", fields.join(", "))
}

fn shuffle<T>(rng: &mut Rng, xs: &mut [T]) {
    for i in (1..xs.len()).rev() {
        xs.swap(i, rng.below(i + 1));
    }
}

const NUMBERS: [&str; 9] = ["0", "-0.0", "0.5", "4", "1e-7", "16.25", "3", "-0", "0.30000000000000004"];

/// One clip, mostly of `kind`'s shape, sometimes not.
fn clip(rng: &mut Rng, kind: &LaneKind) -> String {
    let mut f: Vec<(&str, String)> = vec![];
    match rng.below(9) {
        0 => {}
        1 => f.push(("id", "5".into())),
        2 => f.push(("id", rng.pick(&["true", "null", "1.5", "[1]", "{\"a\": 1}"]).into())),
        3 => f.push(("id", "\"A\"".into())),
        _ => f.push(("id", format!("\"{}\"", rng.pick(&["a", "b", "k3f9q2zt", "z1"])))),
    }
    f.push(("start", rng.pick(&NUMBERS).into()));
    f.push(("length", rng.pick(&["4", "1", "0.25", "0", "-1", "16"]).into()));
    let shape = if rng.one_in(10) { [LaneKind::Notes, LaneKind::Control { unit: None }, LaneKind::Audio][rng.below(3)].clone() } else { kind.clone() };
    match shape {
        LaneKind::Notes => {
            let notes: Vec<String> = (0..rng.below(5)).map(|_| note(rng)).collect();
            f.push(("notes", format!("[{}]", notes.join(", "))));
        }
        LaneKind::Control { .. } => {
            let points: Vec<String> = (0..rng.below(4))
                .map(|_| match rng.below(12) {
                    0 => "[1]".into(),
                    1 => "\"x\"".into(),
                    _ => format!("[{}, {}]", rng.pick(&["0", "8", "4", "-0.0", "4"]), rng.pick(&NUMBERS)),
                })
                .collect();
            f.push(("points", format!("[{}]", points.join(", "))));
        }
        LaneKind::Audio => {
            f.push(("asset", rng.pick(&["\"a.wav\"", "\"vox/lead \\\"take\\\".wav\"", "\"b\\u00e9.wav\""]).into()));
            if rng.one_in(2) {
                f.push(("hash", "\"9f2c3a1be07d44c1\"".into()));
            }
            for key in ["offset", "gain_db", "fade_in", "fade_out", "pitch"] {
                if rng.one_in(3) {
                    f.push((key, rng.pick(&["0", "-1", "0.01", "-0.0", "-12"]).into()));
                }
            }
            if rng.one_in(3) {
                f.push(("reverse", rng.pick(&["true", "false", "1"]).into()));
            }
            if rng.one_in(2) {
                f.push(("warp", rng.pick(&["[[1.0, 1], [0.0, 0]]", "[[0, 0], [1, 0]]", "[]", "[[0, 0]]"]).into()));
            }
        }
    }
    match rng.below(14) {
        0 => f.push(("colour", "1".into())),
        1 => f.push(("start", rng.pick(&NUMBERS).into())),
        2 => f.push(("length", "\"long\"".into())),
        3 => f.retain(|(k, _)| *k != "length"),
        _ => {}
    }
    shuffle(rng, &mut f);
    object(f)
}

fn note(rng: &mut Rng) -> String {
    let mut f: Vec<(&str, String)> = vec![];
    let pitches = ["60", "\"C4\"", "\"A#1\"", "\"Bb3\"", "60.5", "-0.0", "127", "128", "\" c4 \"", "33", "0"];
    let bad = ["\"H2\"", "true", "null", "[60]", "{}", "\"\""];
    let pitch = if rng.one_in(12) { rng.pick(&bad) } else { rng.pick(&pitches) };
    f.push(("pitch", pitch.into()));
    f.push(("start", rng.pick(&["0", "0.25", "-0.0", "1", "0.5"]).into()));
    f.push(("length", rng.pick(&["0.5", "1", "0.25"]).into()));
    if !rng.one_in(3) {
        f.push(("velocity", rng.pick(&["0.9", "0", "1", "0.8", "-0.0"]).into()));
    }
    match rng.below(16) {
        0 => f.push(("vel", "1".into())),
        1 => f.retain(|(k, _)| *k != "start"),
        2 => f.push(("pitch", rng.pick(&pitches).into())),
        3 => f.push(("length", "\"x\"".into())),
        _ => {}
    }
    shuffle(rng, &mut f);
    object(f)
}

/// A whole lane file for `kind`, sometimes broken at the top or as JSON.
fn file(rng: &mut Rng, kind: &LaneKind) -> String {
    let clips: Vec<String> = (0..rng.below(6)).map(|_| clip(rng, kind)).collect();
    let mut f: Vec<(&str, String)> = vec![("clips", format!("[{}]", clips.join(", ")))];
    match rng.below(10) {
        0 => f.push(("unit", "\"hz\"".into())),
        1 => f.push(("unit", rng.pick(&["\"db\"", "5"]).into())),
        2 => f.push(("kind", "\"notes\"".into())),
        3 => f.push(("clips", "[]".into())),
        _ => {}
    }
    if matches!(kind, LaneKind::Control { unit: Some(_) }) && rng.below(3) > 0 {
        f.push(("unit", "\"hz\"".into()));
    }
    shuffle(rng, &mut f);
    let text = object(f);
    if rng.one_in(30) { text[..text.len() / 2].to_string() } else { text }
}

fn kinds() -> [LaneKind; 4] {
    [LaneKind::Notes, LaneKind::Control { unit: Some("hz".into()) }, LaneKind::Control { unit: None }, LaneKind::Audio]
}

/// Every lane file under the repository's projects and fixtures.
fn repo_lanes() -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(d) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() && !name.starts_with('.') && name != "node_modules" && name != "target" {
                walk(&p, out);
            } else if name.ends_with(".json") && p.components().any(|c| c.as_os_str() == "data") && !name.ends_with(".state.json") {
                out.push(p);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = vec![];
    for d in ["examples", "templates", "core/session/tests", "crates", "apps"] {
        walk(&root.join(d), &mut out);
    }
    out
}

/// Equal, every number to the bit (`-0` is not `0`): what a read gives is what plays.
fn same(a: &LaneData, b: &LaneData) -> bool {
    format!("{a:?}") == format!("{b:?}")
}

/// Both readers on `text` as `kind`: the same data or the same error, and the same canonical bytes.
fn both(text: &str, kind: &LaneKind, what: &str) -> bool {
    let (new, old) = (LaneData::read(text, kind), oracle::read(text, kind));
    match (&new, &old) {
        (Ok(a), Ok(b)) => {
            assert!(same(a, b), "{what}: read differs\n{text}\nnew {a:?}\nold {b:?}");
            assert_eq!(canonical(a), oracle::canonical(b), "{what}: canonical differs\n{text}");
            assert_eq!(a.clip_lines(), oracle::clip_lines(b), "{what}: lines differ\n{text}");
            true
        }
        (Err(a), Err(b)) => {
            assert_eq!(a, b, "{what}: errors differ\n{text}");
            false
        }
        _ => panic!("{what}: one reads and one doesn't\n{text}\nnew {new:?}\nold {old:?}"),
    }
}

// FMT-8, FMT-10, FMT-12: the fast read is the old read, data and error messages alike, on every
// lane file in the repository and on generated files of every shape.
#[test]
fn the_fast_read_is_the_old_read() {
    let files = repo_lanes();
    assert!(files.len() > 20, "the repository's lane files: {}", files.len());
    for p in &files {
        let text = std::fs::read_to_string(p).unwrap();
        for kind in kinds() {
            both(&text, &kind, &p.display().to_string());
        }
    }
    let (mut read, mut refused) = (0, 0);
    for case in 0..20_000u64 {
        let mut rng = Rng(0x0a9_0000 + case);
        let kind = kinds()[rng.below(4)].clone();
        let text = file(&mut rng, &kind);
        if both(&text, &kind, &format!("case {case}")) { read += 1 } else { refused += 1 }
    }
    // Both kinds of outcome are well covered.
    assert!(read > 3000 && refused > 3000, "{read} read, {refused} refused");
}

// FMT-8a: the one-buffer writer writes the old writer's bytes, and the sort that makes lines
// only for ties gives the old order, over generated lanes full of ties, anonymous clips, ids that
// aren't strings and `-0`.
#[test]
fn canonical_bytes_are_unchanged() {
    for case in 0..20_000u64 {
        let mut rng = Rng(0xa3_0000 + case);
        let kind = kinds()[rng.below(4)].clone();
        let text = file(&mut rng, &kind);
        let Ok(mut data) = oracle::read(&text, &kind) else { continue };
        // Shuffled out of order, so both sorts have work to do.
        match &mut data {
            LaneData::Notes(c) => {
                shuffle(&mut rng, c);
                c.iter_mut().for_each(|c| shuffle(&mut rng, &mut c.notes));
            }
            LaneData::Control { clips, .. } => shuffle(&mut rng, clips),
            LaneData::Audio(c) => shuffle(&mut rng, c),
        }
        assert_eq!(canonical(&data), oracle::canonical(&data), "case {case}\n{text}");
        let mut sorted = data.clone();
        sorted.sort();
        assert!(same(&sorted, &oracle::sort(data)), "case {case}: the sort differs\n{text}");
    }
}

// FMT-8a, PLANE-1 (the G11 fix's A.2): data read from a file, or made from it by `put_clips`,
// reads back from its canonical text bit for bit exactly when `reads_back` says so, so a commit's
// candidate may play it as made instead of parsing what it wrote. What doesn't: a `-0` (written
// `0`), and a control lane's unit that a read as its kind wouldn't keep.
#[test]
fn the_seed_is_what_a_read_gives() {
    let (mut seeded, mut not) = (0, 0);
    for case in 0..20_000u64 {
        let mut rng = Rng(0x5eed_0000 + case);
        let kind = kinds()[rng.below(4)].clone();
        let data = if rng.one_in(8) { Ok(LaneData::empty(&kind)) } else { LaneData::read(&file(&mut rng, &kind), &kind) };
        let Ok(mut data) = data else { continue };
        let put: Vec<serde_json::Value> = (0..rng.below(3)).filter_map(|_| serde_json::from_str(&clip(&mut rng, &kind)).ok()).collect();
        let delete: Vec<String> = (0..rng.below(2)).map(|_| rng.pick(&["a", "b", "zz"]).to_string()).collect();
        if data.put_clips(&put, &delete).is_err() {
            continue;
        }
        let back = LaneData::read(&canonical(&data), &kind);
        let exact = back.as_ref().is_ok_and(|b| same(b, &data));
        assert_eq!(data.reads_back(&kind), exact, "case {case}: {data:?}\nread back {back:?}");
        if exact { seeded += 1 } else { not += 1 }
    }
    assert!(seeded > 2000 && not > 300, "{seeded} read back, {not} didn't");
}
