//! `m2 edit` and `m2 write` (AGENT-1, the agents design's 5.3): a change to a song's text file as a
//! Tx with a base, which the app can refuse, name and let the agent undo while the song is open, and
//! which `m2` writes itself while it is closed. What an agent's own Edit and Write tools do, as Tx.
//!
//! While the song's code on disk doesn't build, `m2` can't plan a Tx; then they save a code file as
//! the app's code editor does (WRITE-2), in the app while it is open, written only when the song
//! takes it in as written, so it builds again: the fix, as one undo step, the agent's.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use m2_session::{Committed, Refusal, Session};
use m2_song::DataPath;
use m2_song::tx::{Base, Confined, Edit, Expect, Op, Payload, Pos, Tx, TxPath};

const EDIT: &str = "m2 edit <song> <file> --old <text> --new <text> [--all]";
const WRITE: &str = "m2 write <song> <file> [<from> | -]";

/// The file `file` names in the song at `dir`, as a Tx names it: a text file, or why not.
fn text_file(dir: &Path, file: &str, verb: &str) -> Result<TxPath> {
    let rel = m2_song::path::slashed(&super::in_project(dir, file));
    let path = TxPath::new(&rel).map_err(|e| anyhow!("{e}"))?;
    if !path.is_text() {
        match path.data() {
            DataPath::Asset(_) => bail!("{rel} is audio: m2 import brings audio in, and m2 {verb} changes text files"),
            DataPath::PluginState { .. } => bail!("{rel} is a plugin's state: m2 plugin-state writes it, and m2 {verb} changes text files"),
            _ => bail!("{rel} isn't a text file of the song: m2 {verb} changes code, lanes, the arrangement, the manifest and the layout"),
        }
    }
    Ok(path)
}

/// Commit `tx`, which writes text file `path`: a lane's data confined to that lane, so undoing it
/// is judged and heard at once (PLANE-5); anything else on the delta its check shows (WRITE-11),
/// as `m2 tx --seen` does, since an edit of code may change anything it reaches.
fn commit(s: &mut Session, path: &TxPath, mut tx: Tx) -> Result<Committed, Refusal> {
    if let DataPath::Lane { module, lane } = path.data() {
        let lane = m2_song::LaneAddr { module: module.clone(), lane: lane.clone() };
        tx.expect = Expect::Confined(Confined { lanes: vec![lane], ..Confined::default() });
        return s.commit(tx);
    }
    // A check on `Seen` never refuses for its expectation: it says the delta's hash.
    tx.expect = Expect::Seen(m2_song::Sha::of(b""));
    let checked = s.check(&tx)?;
    tx.expect = Expect::Seen(checked.hash);
    s.commit(tx)
}

/// A session to write the song at `dir` with, and whether its code on disk doesn't build: then
/// only a code file can be changed, and it is saved (the fix), not committed.
fn session(dir: &Path, path: &TxPath, verb: &str) -> Result<(Session, bool)> {
    let s = super::locked(dir, "")?;
    if s.accepted().has_code() {
        super::attention(dir, s.problems());
        return Ok((s, false));
    }
    if !matches!(path.data(), DataPath::Code(_)) {
        let e = super::broken(dir, &s);
        bail!("{e}: the song's code doesn't build, so m2 {verb} changes only its code now, to fix it; {path} waits for that");
    }
    Ok((s, true))
}

/// Save `text` over `base` at `path`, in a song whose code doesn't build: written only when the
/// song takes it in as written (AGENT-1). Its revision, or why nothing was written.
fn save(s: &mut Session, path: &TxPath, base: Base, text: &str) -> Result<u64, Refusal> {
    s.save_built(&path.path(), base, text)?;
    Ok(s.landed().filter(|l| l.number > 0).map_or(s.accepted().number, |l| l.number))
}

/// Why a change wasn't made, in words: a conflict or a rejection changed nothing; the app's and the
/// disk's own words say what happened, which may be otherwise.
fn refused(r: Refusal, dir: &Path, path: &TxPath, fixing: bool) -> anyhow::Error {
    match r {
        Refusal::Io { message } => anyhow!("{message}"),
        Refusal::Rejected { diagnostics, .. } if fixing => {
            super::attention(dir, &diagnostics);
            anyhow!("nothing was changed: the song's code wouldn't build with {path} so (above), and while it doesn't, it isn't written")
        }
        r => anyhow!("nothing was changed: {r}"),
    }
}

/// `m2 edit`: replace `--old`, found exactly once (or every time, with `--all`), with `--new`, as
/// one Tx whose edits each guard their place with the text they replace, on the base of the text
/// it read; or, while the code doesn't build, as a save of the file. A file that changed since it
/// was read is read again, and the edit tried once more.
pub fn edit(args: &[String]) -> Result<()> {
    // Its usage took them (AGENT-13): each `<text>` is the word after its flag, whatever it is.
    let [dir, file] = &crate::words::places(args)[..] else { unreachable!("its usage takes two words") };
    let (Some(old), Some(new)) = (crate::words::value(args, "--old"), crate::words::value(args, "--new")) else {
        unreachable!("its usage needs both: {EDIT}")
    };
    let all = crate::words::on(args, "--all");
    if old.is_empty() {
        bail!("--old can't be empty: give the text to replace");
    }
    let dir = Path::new(dir);
    let path = text_file(dir, file, "edit")?;
    let (mut s, fixing) = session(dir, &path, "edit")?;
    for again in [false, true] {
        let text = std::fs::read_to_string(dir.join(path.path())).with_context(|| format!("reading {path}"))?;
        let at: Vec<usize> = text.match_indices(old.as_str()).map(|(i, _)| i).collect();
        match at.len() {
            0 => bail!("`{old}` isn't in {path}: nothing was changed"),
            n if n > 1 && !all => {
                bail!(
                    "`{old}` is found {n} times in {path}: give more of the text around it, or add --all to replace each one; nothing was changed"
                )
            }
            _ => {}
        }
        let edits = at
            .iter()
            .map(|&i| Edit { start: Pos::Byte(i as u32), end: Pos::Byte((i + old.len()) as u32), expected: old.clone(), text: new.clone() })
            .collect();
        let mut tx = Tx::new(
            &m2_song::tx::random_id(),
            (old.clone(), new.clone()),
            vec![Op::TextEdit { file: path.clone(), edits }],
            Expect::Identical,
        );
        tx.base.insert(path.clone(), Base::text(&text));
        let done = match fixing {
            false => commit(&mut s, &path, tx).map(|c| c.number),
            true => save(&mut s, &path, Base::text(&text), &text.replace(old.as_str(), &new)),
        };
        match done {
            Ok(number) => {
                let n = at.len();
                let builds = if fixing { "; the song builds again" } else { "" };
                println!("{path}: replaced {n} place{} (revision {number}){builds}", if n == 1 { "" } else { "s" });
                return Ok(());
            }
            // "Replace this text" means the same on the file as it is now.
            Err(Refusal::Conflict { .. }) if !again => {
                s.refresh();
            }
            Err(r) => return Err(refused(r, dir, &path, fixing)),
        }
    }
    unreachable!("the second try returns")
}

/// `m2 write`: write a whole text file, from a file or standard input, as one Tx on the base it read
/// (nothing, for a new file).
pub fn write(args: &[String]) -> Result<()> {
    let (dir, file, from) = match args {
        [_, dir, file] => (dir, file, "-"),
        [_, dir, file, from] => (dir, file, from.as_str()),
        _ => bail!("{WRITE}"),
    };
    let dir = Path::new(dir);
    let path = text_file(dir, file, "write")?;
    let text = match from {
        "-" => std::io::read_to_string(std::io::stdin()).context("reading the text from standard input")?,
        from => std::fs::read_to_string(from).with_context(|| format!("reading {from} (m2 write writes text, in UTF-8)"))?,
    };
    let (mut s, fixing) = session(dir, &path, "write")?;
    let base = match std::fs::read_to_string(dir.join(path.path())) {
        Ok(t) => Base::text(&t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Base::Absent,
        Err(e) => bail!("{path} can't be read ({e}): nothing was changed"),
    };
    let made = base == Base::Absent;
    if made && fixing {
        bail!(
            "nothing was changed: the song's code doesn't build, so m2 write fixes a file that is there; {path} isn't. Fix the code first"
        );
    }
    let number = match fixing {
        true => save(&mut s, &path, base, &text),
        false => {
            let label = (if made { "nothing".into() } else { path.to_string() }, path.to_string());
            let mut tx = Tx::new(
                &m2_song::tx::random_id(),
                label,
                vec![Op::Write { file: path.clone(), bytes: Payload::Text(text.as_str().into()) }],
                Expect::Identical,
            );
            tx.base.insert(path.clone(), base);
            commit(&mut s, &path, tx).map(|c| c.number)
        }
    }
    .map_err(|r| refused(r, dir, &path, fixing))?;
    let builds = if fixing { "; the song builds again" } else { "" };
    println!("{path}: {} {} bytes (revision {number}){builds}", if made { "made, with" } else { "written," }, text.len());
    Ok(())
}
