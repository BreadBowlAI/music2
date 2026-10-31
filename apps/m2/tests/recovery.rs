//! An interrupted commit and a malformed Tx from the terminal (WRITE-16, DIAG-4, the step 6
//! design's 6a.4 and 1.2): every command that opens a project says an interrupted commit, with the
//! `m2 fix` commands that finish or discard it, whether its code compiles or not, and those
//! commands apply while it doesn't; `m2 tx` answers a Tx that doesn't read in JSON, as it answers
//! every refusal.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use m2_session::tx::journal::{JTarget, Journal};
use m2_song::Sha;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        // Read-only files too: a pack m2 installed in a HOME here (PACK-16).
        let _ = m2_song::pack::remove_tree(&self.0);
    }
}

fn m2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", std::env::temp_dir().join("m2-tests-no-app")).args(args).output().unwrap()
}

fn text(o: &[u8]) -> String {
    String::from_utf8_lossy(o).into_owned()
}

/// A copy of first-song as a crash left it half way through a commit of two code files whose
/// halves don't compile alone: `tracks/hats.ts` renamed its export (reached), `song.ts` the import
/// of it (not yet). Its journal and staged files are as a commit writes them.
fn half_applied(at: &Path, jid: &str) -> (String, String, String, String) {
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), at);
    let (hats, song) = (std::fs::read_to_string(at.join("tracks/hats.ts")).unwrap(), std::fs::read_to_string(at.join("song.ts")).unwrap());
    let hats2 = hats.replacen("export default track(", "export const hatsTrack = track(", 1);
    let song2 = song.replacen("import hats from", "import { hatsTrack as hats } from", 1);
    let target = |path: &str, before: &str, after: &str, k: usize| JTarget::File {
        path: path.into(),
        before: Some(Sha::of(before.as_bytes())),
        after: Some(Sha::of(after.as_bytes())),
        text: Some(before.into()),
        blob: false,
        staged: Some(k),
    };
    let j = Journal {
        id: jid.into(),
        tx: "tx-rename-export".into(),
        host: "here".into(),
        host_id: m2_session::lock::host_id(),
        pid: 1,
        program: "music2".into(),
        label: ("hats".into(), "hatsTrack".into()),
        targets: vec![target("tracks/hats.ts", &hats, &hats2, 0), target("song.ts", &song, &song2, 1)],
    };
    let tx = at.join(".music2/tx");
    std::fs::create_dir_all(tx.join(jid)).unwrap();
    std::fs::write(tx.join(format!("{jid}.json")), j.to_json()).unwrap();
    std::fs::write(tx.join(jid).join("0"), &hats2).unwrap();
    std::fs::write(tx.join(jid).join("1"), &song2).unwrap();
    std::fs::write(at.join("tracks/hats.ts"), &hats2).unwrap();
    (hats, song, hats2, song2)
}

// WRITE-16, DIAG-4, 6a.4, P3: a crash between a commit's renames left code that doesn't compile.
// Reading commands (`check`, `render`, `graph`) say the interrupted commit and its `m2 fix`
// commands before the code's errors; a writing one says them too; `m2 fix … discard-tx` and
// `finish-tx` apply, each leaving a project that compiles.
#[test]
fn an_interrupted_commit_is_said_and_recovered_while_the_code_doesnt_compile() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-recovery-cli-{}", std::process::id())));
    let jid = "1791935974533-64110-0";
    let src = format!(".music2/tx/{jid}.json");
    for (n, fix) in ["discard-tx", "finish-tx"].into_iter().enumerate() {
        let song = temp.0.join(format!("song{n}"));
        let (hats, first, hats2, song2) = half_applied(&song, jid);
        let dir = song.to_str().unwrap();
        let out = temp.0.join("out.wav");
        for args in [vec!["check", dir], vec!["graph", dir], vec!["render", dir, out.to_str().unwrap()], vec!["set", dir, "x", "1"]] {
            let o = m2(&args);
            let err = text(&o.stderr);
            assert!(!o.status.success(), "{args:?}: the code doesn't compile");
            assert!(err.contains("[tx.interrupted]") && err.contains(&format!("m2 fix {dir} {src} {fix}")), "{args:?}: {err}");
            assert!(err.contains("[code.build]") || err.contains("export"), "{args:?}: and the code's errors: {err}");
        }
        let o = m2(&["fix", dir, &src, fix]);
        assert!(o.status.success(), "{fix}: {}{}", text(&o.stdout), text(&o.stderr));
        let now = (std::fs::read_to_string(song.join("tracks/hats.ts")).unwrap(), std::fs::read_to_string(song.join("song.ts")).unwrap());
        let want = if fix == "finish-tx" { (hats2.clone(), song2.clone()) } else { (hats.clone(), first.clone()) };
        assert_eq!(now, want, "{fix}");
        let o = m2(&["check", dir]);
        assert!(o.status.success() && !text(&o.stderr).contains("tx.interrupted"), "{fix}: {}", text(&o.stderr));
        assert!(!song.join(&src).exists(), "{fix}: its journal is gone");
    }
}

/// A Tx naming a file outside the project.
const OUTSIDE: &str =
    r#"{"id": "tx-a", "base": {"../x.json": null}, "ops": [{"op": "remove", "file": "../x.json"}], "expect": "identical"}"#;

// The step 6 design's 1.2 and 18: a Tx that isn't JSON, has a field a Tx doesn't, or names a path
// WRITE-3a refuses is rejected as every other refusal is: JSON on stdout with its `tx.invalid` or
// `tx.path` diagnostic, exit 1, nothing written.
#[test]
fn a_malformed_tx_is_rejected_in_json() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-malformed-tx-{}", std::process::id())));
    let song = temp.0.join("song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    let cases = [
        ("{not json", "tx.invalid"),
        (r#"{"id": "tx-a", "base": {}, "ops": [], "expect": "identical", "extra": 1}"#, "tx.invalid"),
        (OUTSIDE, "tx.path"),
    ];
    for (json, code) in cases {
        let file = temp.0.join("tx.json");
        std::fs::write(&file, json).unwrap();
        for check in [true, false] {
            let mut args = vec!["tx", dir, file.to_str().unwrap()];
            if check {
                args.push("--check");
            }
            let o = m2(&args);
            assert!(!o.status.success(), "{json}");
            let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{json}: {e}: {}", text(&o.stdout)));
            assert_eq!(v["rejected"]["diagnostics"][0]["code"], code, "{json}: {v}");
        }
    }
}
