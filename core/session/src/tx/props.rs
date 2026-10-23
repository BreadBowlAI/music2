//! Property tests over Tx (the step 6 gate): bytes outside the declared ranges are unchanged; the
//! inverse restores the original; hostile paths and ids are refused; `check` then `commit` write
//! the same bytes with equal delta hashes, even across an unrelated edit between them.
//!
//! Seeded, so a failure names the case that found it.

use std::collections::BTreeMap;

use m2_song::tx::{Base, Confined, Declared, Edit, Expect, Op, Payload, Pos, Range, Tx, TxPath};
use m2_song::{LaneAddr, Sha};
use serde_json::json;

use super::Refusal;
use super::expect::resolve;
use super::tests::{Dir, based, p};
use crate::Session;
use crate::text::Edits;

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// `(line, UTF-16 column)` from 0 of byte `at`.
fn line_col(text: &str, at: usize) -> Pos {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    Pos::LineCol { line: text[..start].matches('\n').count() as u32, utf16_col: text[start..at].encode_utf16().count() as u32 }
}

// WRITE-3, the step 6 gate (a): random texts of many scripts and line ends, with random sorted
// disjoint edits given as bytes or as lines and UTF-16 columns: both forms resolve to the same
// bytes, and every byte outside the edits is where the mapping says.
#[test]
fn bytes_outside_the_edits_are_unchanged() {
    let pieces = ["a", "bc", " ", "\n", "\r\n", "é", "ベース", "😀", "(", ")", "0.3", "// c\n"];
    for case in 0..500u64 {
        let mut rng = Rng(0x5eed_0000 + case);
        let text: String = (0..rng.below(40)).map(|_| *rng.pick(&pieces)).collect();
        let bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).chain([text.len()]).collect();
        let mut cuts: Vec<usize> = (0..rng.below(6) * 2).map(|_| *rng.pick(&bounds)).collect();
        cuts.sort();
        cuts.dedup();
        let edits: Vec<(usize, usize, String)> = cuts
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (c[0], c[1], (0..rng.below(3)).map(|_| *rng.pick(&pieces)).collect()))
            .collect();
        for e in &edits {
            assert_eq!(resolve(&text, &Pos::Byte(e.0 as u32)), Ok(e.0), "case {case}");
            assert_eq!(resolve(&text, &line_col(&text, e.0)), Ok(e.0), "case {case}: {:?} at {}", text, e.0);
            assert_eq!(resolve(&text, &line_col(&text, e.1)), Ok(e.1), "case {case}");
        }
        let (own, new) = Edits::of(&text, &edits);
        // Outside the edits, every byte is where the hunks put it.
        let mut at = 0;
        for h in &own.0 {
            let unchanged = &text[at..h.old.start];
            let to = h.new.start - unchanged.len();
            assert_eq!(&new[to..h.new.start], unchanged, "case {case}");
            at = h.old.end;
        }
        assert!(new.ends_with(&text[at..]), "case {case}");
    }
}

/// Every number literal written in a project file of `s` that a constant of the graph reads, as
/// (file, byte range): what a knob writes.
fn literals(s: &Session) -> Vec<(String, std::ops::Range<usize>)> {
    fn consts<'a>(nodes: &'a [m2_ir::Node], out: &mut Vec<(&'a m2_ir::Origin, f64)>) {
        for n in nodes {
            if let (m2_ir::Op::Const { value, .. }, Some(o)) = (&n.op, &n.origin) {
                out.push((o, *value));
            }
            if let m2_ir::Op::Poly { body, .. } = &n.op {
                consts(&body.nodes, out);
            }
        }
    }
    let rev = s.accepted();
    let mut origins = vec![];
    for m in &rev.compiled.ir.modules {
        consts(&m.nodes, &mut origins);
    }
    let mut out = vec![];
    for (o, v) in origins.into_iter().filter(|(o, _)| !o.arg.is_empty()) {
        let Some(site) = rev.compiled.sites.iter().find(|x| x.id == o.site && x.in_project) else { continue };
        // A number the constant is, times its scale (`khz(2.5)`): what a knob reads it as.
        let scale = o.scale.unwrap_or(1.0);
        if let Some(m2_script::sites::Arg::Number { start, end, value }) = site.arg(&o.arg)
            && scale.is_finite()
            && scale > 0.0
            && value * scale == v
        {
            out.push((site.file.clone(), *start as usize..*end as usize));
        }
    }
    out.sort_by(|a, b| (&a.0, a.1.start).cmp(&(&b.0, b.1.start)));
    out.dedup();
    out
}

/// One random Tx on the project at `d` (a first-song copy with unread data), of a kind `rng`
/// picks: a literal (Values), a comment (Identical), clips (Confined to the lane), a new lane file,
/// a removed one, or a folder renamed.
fn random(rng: &mut Rng, s: &Session, d: &Dir, n: usize) -> Tx {
    let kick = LaneAddr::new("kick", "hits").unwrap();
    match rng.below(6) {
        0 => {
            let lits = literals(s);
            let (file, r) = rng.pick(&lits).clone();
            let text = d.read(&file);
            let old = &text[r.clone()];
            let value: f64 = (rng.below(900) as f64 + 1.0) / 10.0;
            let written = if value == old.parse::<f64>().unwrap_or(f64::NAN) { value + 0.5 } else { value };
            let op = Op::TextEdit {
                file: p(&file),
                edits: vec![Edit {
                    start: Pos::Byte(r.start as u32),
                    end: Pos::Byte(r.end as u32),
                    expected: old.into(),
                    text: written.to_string(),
                }],
            };
            // The constant is the number times its scale (`khz(2.5)`): Seen, as an agent learns it.
            based(d, vec![op], Expect::Seen(Sha::of(b"?")))
        }
        1 => {
            let files = ["song.ts", "tracks/bass.ts", "tracks/kick.ts", "tracks/keys.ts", "tracks/hats.ts"];
            let file = *rng.pick(&files);
            let text = d.read(file);
            let starts: Vec<usize> =
                std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).filter(|i| *i < text.len()).collect();
            let at = *rng.pick(&starts) as u32;
            let op = Op::TextEdit {
                file: p(file),
                edits: vec![Edit { start: Pos::Byte(at), end: Pos::Byte(at), expected: String::new(), text: format!("// note {n}\n") }],
            };
            let mut c = Confined::default();
            c.ranges.insert(p(file), Declared::Ranges(vec![Range::bytes(at, at)]));
            based(d, vec![op], Expect::Confined(c))
        }
        2 => {
            let now: serde_json::Value = serde_json::from_str(&d.read("data/kick/hits.json")).unwrap();
            let ids: Vec<String> = now["clips"].as_array().unwrap().iter().filter_map(|c| c["id"].as_str().map(str::to_string)).collect();
            let mut put = vec![];
            let mut delete = vec![];
            match rng.below(3) {
                0 => put.push(json!({"id": format!("p{n}"), "start": 64 + rng.below(64), "length": 4,
                                     "notes": [{"pitch": "C1", "start": rng.below(4), "length": 0.5, "velocity": 0.8}]})),
                1 if !ids.is_empty() => {
                    let id = rng.pick(&ids).clone();
                    let mut c = now["clips"].as_array().unwrap().iter().find(|c| c["id"] == json!(id)).unwrap().clone();
                    c["length"] = json!(c["length"].as_f64().unwrap() + 1.0);
                    put.push(c);
                }
                _ if !ids.is_empty() => delete.push(rng.pick(&ids).clone()),
                _ => put.push(json!({"id": format!("q{n}"), "start": 0, "length": 1, "notes": []})),
            }
            based(
                d,
                vec![Op::Clips { lane: kick, put, delete }],
                Expect::Confined(Confined { lanes: vec![LaneAddr::new("kick", "hits").unwrap()], ..Confined::default() }),
            )
        }
        3 => {
            let ops = vec![Op::Write { file: p(&format!("data/kick/unread{n}.json")), bytes: Payload::Text("{\"clips\": []}\n".into()) }];
            based(d, ops, Expect::Confined(Confined::default()))
        }
        4 => {
            let unread: Vec<String> = std::fs::read_dir(d.0.join("data/kick"))
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|f| f.starts_with("unread"))
                .collect();
            match unread.first() {
                Some(f) => based(d, vec![Op::Remove { file: p(&format!("data/kick/{f}")) }], Expect::Identical),
                None => based(
                    d,
                    vec![Op::Write { file: p("data/kick/unread.json"), bytes: Payload::Text("{\"clips\": []}\n".into()) }],
                    Expect::Confined(Confined::default()),
                ),
            }
        }
        _ => {
            let from = if d.0.join("data/old").exists() { "data/old" } else { "data/older" };
            let to = if from == "data/old" { "data/older" } else { "data/old" };
            based(d, vec![Op::Rename { from: p(from), to: p(to) }], Expect::Confined(Confined::default()))
        }
    }
}

/// A first-song copy with data no lane reads.
fn start() -> Dir {
    let d = Dir::first_song();
    d.write("data/old/hits.json", "{\"clips\": [{\"id\": \"o\", \"start\": 0, \"length\": 1, \"notes\": []}]}\n");
    d.write("data/kick/unread0.json", "{\"clips\": []}\n");
    d
}

// WRITE-21, WRITE-4, WRITE-5, the step 6 gate (b) and (d): random Tx, each checked then committed:
// the hashes agree, the files on disk are the candidate's, a literal or comment changes its file
// only inside what it replaced; undo restores every file and redo makes them again.
#[test]
fn the_inverse_restores_and_check_matches_commit() {
    let d = start();
    let mut s = d.open();
    let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
    for case in 0..500usize {
        let mut rng = Rng(0xc0ffee + case as u64);
        let tx = random(&mut rng, &s, &d, case);
        let kind = match (&tx.ops[0], &tx.expect) {
            (Op::TextEdit { .. }, Expect::Seen(_)) => "literal",
            (Op::TextEdit { .. }, _) => "comment",
            (Op::Clips { .. }, _) => "clips",
            (Op::Write { .. }, _) => "write",
            (Op::Remove { .. }, _) => "remove",
            (Op::Rename { .. }, _) => "rename",
        };
        *kinds.entry(kind).or_default() += 1;
        let before = d.tree();
        let checked = match s.check(&tx) {
            Ok(c) => c,
            Err(e) => panic!("case {case}: {e}\n{}", tx.to_json()),
        };
        let mut tx = tx;
        if matches!(tx.expect, Expect::Seen(_)) {
            tx.expect = Expect::Seen(checked.hash);
        }
        let candidate: BTreeMap<String, String> =
            checked.candidate().snapshot.files.iter().map(|(k, v)| (m2_song::path::slashed(k), v.to_string())).collect();
        let done = s.commit(tx.clone()).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert_eq!(done.hash, checked.hash, "case {case}: check then commit, one delta");
        let after = d.tree();
        for (path, text) in &candidate {
            assert_eq!(
                after.get(path).map(|b| String::from_utf8_lossy(b).into_owned()).as_ref(),
                Some(text),
                "case {case}: {path} is the candidate's"
            );
        }
        // A text edit changes its file only inside the bytes it replaced.
        for op in &tx.ops {
            if let Op::TextEdit { file, edits } = op {
                let (old, new) = (
                    String::from_utf8_lossy(&before[file.as_str()]).into_owned(),
                    String::from_utf8_lossy(&after[file.as_str()]).into_owned(),
                );
                let Pos::Byte(s0) = edits[0].start else { unreachable!() };
                let Pos::Byte(e0) = edits[0].end else { unreachable!() };
                let (s0, e0) = (s0 as usize, e0 as usize);
                assert_eq!(&new[..s0], &old[..s0], "case {case}: before the edit");
                assert_eq!(&new[new.len() - (old.len() - e0)..], &old[e0..], "case {case}: after the edit");
            }
        }
        s.undo().unwrap_or_else(|e| panic!("case {case}: undo: {e}"));
        assert_eq!(d.tree(), before, "case {case}: undo restores every file");
        s.redo().unwrap_or_else(|e| panic!("case {case}: redo: {e}"));
        assert_eq!(d.tree(), after, "case {case}: redo makes them again");
    }
    assert_eq!(kinds.len(), 6, "every kind of Tx was made: {kinds:?}");
}

// WRITE-4, WRITE-5, WRITE-21 (the G11 fix's A.1): one `commit_with` is `check`, the assertion,
// then `commit`: over random Tx on two copies of one project, the planner sees the same candidate,
// the commits say the same (revision, delta, patches, written paths), the disks are the same, and
// so are the undo and redo after.
#[test]
fn commit_with_is_check_then_commit() {
    let (a, b) = (start(), start());
    let (mut sa, mut sb) = (a.open(), b.open());
    let (mut committed, mut refused) = (0, 0);
    for case in 0..300usize {
        let mut rng = Rng(0xa1_0000 + case as u64);
        let mut tx = random(&mut rng, &sa, &a, case);
        let checked = sa.check(&tx);
        if let (Ok(c), Expect::Seen(_)) = (&checked, &tx.expect) {
            // As an agent commits what it learned; a few keep a hash that is wrong.
            if case % 7 != 0 {
                tx.expect = Expect::Seen(c.hash);
            }
        }
        let seen = checked.as_ref().map(|c| (c.hash, c.delta.clone(), format!("{:?}", c.diagnostics))).map_err(|e| e.to_string());
        // Some planners say no: then nothing is committed, as when `run` stops after `check`.
        let says_no = case % 11 == 3;
        let mut asserted = None;
        let one = sb.commit_with(tx.clone(), |_, c: &super::Checked| {
            asserted = Some((c.hash, c.delta.clone(), format!("{:?}", c.diagnostics)));
            if says_no { Err(()) } else { Ok(()) }
        });
        let two = match checked {
            Ok(_) if says_no => Err("the planner said no".to_string()),
            Ok(_) => sa.commit(tx.clone()).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };
        match (&one, &two) {
            (Ok(x), Ok(y)) => {
                assert_eq!(asserted.as_ref().map(|a| (a.0, &a.1, &a.2)), seen.as_ref().ok().map(|s| (s.0, &s.1, &s.2)), "case {case}");
                assert_eq!((x.number, x.code_rev, x.data_rev, x.hash), (y.number, y.code_rev, y.data_rev, y.hash), "case {case}");
                assert_eq!((&x.delta, &x.patches), (&y.delta, &y.patches), "case {case}");
                let paths = |c: &super::Committed| c.wrote.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();
                assert_eq!(paths(x), paths(y), "case {case}");
                committed += 1;
            }
            (Err(super::Uncommitted::Refused(x)), Err(y)) => {
                assert_eq!(&x.to_string(), y, "case {case}");
                refused += 1;
            }
            (Err(super::Uncommitted::Asserted(())), Err(_)) if says_no && seen.is_ok() => {
                assert_eq!(asserted.as_ref().map(|a| a.0), seen.as_ref().ok().map(|s| s.0), "case {case}");
                refused += 1;
            }
            _ => panic!(
                "case {case}: one committed and one didn't
{one:?}
{two:?}"
            ),
        }
        assert_eq!(a.tree(), b.tree(), "case {case}: the same files");
        assert_eq!((sa.can_undo(), sa.can_redo()), (sb.can_undo(), sb.can_redo()), "case {case}");
        if case % 5 == 4 {
            let (x, y) = (sa.undo().map(|s| s.committed.hash).map_err(|e| e.to_string()), sb.undo().map(|s| s.committed.hash));
            assert_eq!(x, y.map_err(|e| e.to_string()), "case {case}: undo");
            assert_eq!(a.tree(), b.tree(), "case {case}: the same files after undo");
        }
    }
    assert!(committed > 100 && refused > 20, "{committed} committed, {refused} refused");
}

// R17, the step 6 gate (d): a comment typed into a touched file between check and commit, outside
// what the Tx changes, leaves the delta's hash as check said.
#[test]
fn an_unrelated_edit_between_check_and_commit_keeps_the_hash() {
    let d = start();
    let mut s = d.open();
    for case in 0..100usize {
        let mut rng = Rng(0xbeef + case as u64);
        let mut tx = random(&mut rng, &s, &d, case);
        while !matches!(tx.ops[0], Op::TextEdit { .. }) {
            tx = random(&mut rng, &s, &d, case);
        }
        let Op::TextEdit { file, edits } = &tx.ops[0] else { unreachable!() };
        let file = file.clone();
        let Pos::Byte(at) = edits[0].start else { unreachable!() };
        if matches!(tx.expect, Expect::Seen(_)) {
            tx.expect = Expect::Seen(Sha::of(b"?"));
        }
        let hash = s.check(&tx).unwrap().hash;
        if matches!(tx.expect, Expect::Seen(_)) {
            tx.expect = Expect::Seen(hash);
        }
        // Typed at the top of the file, or at its end: never inside the edit.
        let text = d.read(file.as_str());
        let typed = if at > 0 { format!("// typed {case}\n{text}") } else { format!("{text}// typed {case}\n") };
        d.write(file.as_str(), &typed);
        let done = s.commit(tx).unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert_eq!(done.hash, hash, "case {case}: the same delta");
        assert!(d.read(file.as_str()).contains(&format!("// typed {case}")));
    }
}

// WRITE-3a, the step 6 gate (c): hostile paths, ids, payloads and overlaps are refused, and the
// project is unchanged.
#[test]
fn hostile_paths_and_ids_are_refused() {
    let d = start();
    let mut s = d.open();
    let tree = d.tree();
    let parts = [
        "..",
        ".",
        "",
        "data",
        "kick",
        "aux",
        "Kick",
        ".music2",
        "lock",
        "node_modules",
        "x.ts",
        "a\\b",
        "\u{202e}",
        "\0",
        "C:",
        "hits.json",
    ];
    let mut refused = 0;
    for case in 0..500u64 {
        let mut rng = Rng(0xbad + case);
        let path: String = (0..1 + rng.below(4)).map(|_| *rng.pick(&parts)).collect::<Vec<_>>().join("/");
        let path = if rng.below(4) == 0 { format!("/{path}") } else { path };
        match TxPath::new(&path) {
            Err(_) => {
                let ops = json!([{"op": "write", "file": path, "text": "x"}]);
                let v = json!({"id": "tx-h", "base": {path.clone(): null}, "ops": ops, "expect": "identical"});
                let e = Tx::from_json(&v).err().unwrap_or_else(|| panic!("case {case}: {path:?} was admitted"));
                assert!(e.iter().all(|d| d.code == "tx.path" || d.code == "tx.invalid"), "case {case}");
                refused += 1;
            }
            Ok(tp) => {
                // A path that parses is one the table admits, spelled exactly.
                assert_eq!(tp.as_str(), path, "case {case}");
                assert!(!path.starts_with(".music2") && !path.contains(".."), "case {case}: {path}");
            }
        }
    }
    assert!(refused > 300, "{refused}");
    // Ids, payloads, overlaps and the empty range list, through a session: refused, nothing written.
    let write = |f: &str| Op::Write { file: p(f), bytes: Payload::Text("{\"clips\": []}\n".into()) };
    let cases: Vec<Tx> = vec![
        {
            let mut t = based(&d, vec![write("data/kick/new.json")], Expect::Identical);
            t.id = "Tx!".into();
            t
        },
        {
            let mut t = based(&d, vec![write("data/kick/new.json")], Expect::Identical);
            t.id = "x".repeat(33);
            t
        },
        based(
            &d,
            vec![Op::Write { file: p("data/kick/new.json"), bytes: Payload::Local { path: "song.ts".into(), sha: Sha::of(b"") } }],
            Expect::Identical,
        ),
        based(
            &d,
            vec![Op::Write {
                file: p("data/kick/new.json"),
                bytes: Payload::Local { path: ".music2/tx/blobs/none".into(), sha: Sha::of(b"x") },
            }],
            Expect::Identical,
        ),
        based(&d, vec![Op::Rename { from: p("data/old"), to: p("data/older") }, write("data/old/hits.json")], Expect::Identical),
        based(
            &d,
            vec![Op::Clips { lane: LaneAddr::new("kick", "hits").unwrap(), put: vec![], delete: vec![] }, write("data/kick/hits.json")],
            Expect::Identical,
        ),
        based(
            &d,
            vec![write("song.ts")],
            Expect::Confined(Confined { ranges: [(p("song.ts"), Declared::Ranges(vec![]))].into(), ..Confined::default() }),
        ),
        {
            let mut t = based(&d, vec![write("data/old/hits.json")], Expect::Identical);
            t.base.insert(p("data/old/hits.json"), Base::Sha(Sha::of(b"another")));
            t
        },
        {
            let mut t = based(&d, vec![Op::Rename { from: p("data/old"), to: p("data/kick") }], Expect::Identical);
            t.base.insert(p("data/kick"), Base::Absent);
            t
        },
    ];
    for (i, tx) in cases.into_iter().enumerate() {
        match s.commit(tx) {
            Err(Refusal::Rejected { diagnostics, .. }) => {
                assert!(diagnostics.iter().all(|d| d.code.starts_with("tx.")), "{i}: {diagnostics:?}")
            }
            Err(Refusal::Conflict { .. }) => {}
            other => panic!("{i}: {other:?}"),
        }
        assert_eq!(d.tree(), tree, "{i}: nothing written");
    }
}
