//! `m2 edit` and `m2 write` (AGENT-1, the agents design's 5.3): a change to a song's text file as a
//! Tx with a base, which the app can refuse, name and let the agent undo while the song is open, and
//! which `m2` writes itself while it is closed. What an agent's own Edit and Write tools do, as Tx.

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

/// `m2 edit`: replace `--old`, found exactly once (or every time, with `--all`), with `--new`, as
/// one Tx whose edits each guard their place with the text they replace, on the base of the text
/// it read. A file that changed since it was read is read again, and the edit tried once more.
pub fn edit(args: &[String]) -> Result<()> {
    let [_, dir, file, rest @ ..] = args else { bail!("{EDIT}") };
    let (mut old, mut new, mut all) = (None, None, false);
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--old" => old = rest.get(i + 1).cloned(),
            "--new" => new = rest.get(i + 1).cloned(),
            "--all" => {
                all = true;
                i += 1;
                continue;
            }
            other => bail!("m2 edit doesn't know `{other}`: {EDIT}"),
        }
        i += 2;
    }
    let (Some(old), Some(new)) = (old, new) else { bail!("say what to replace and with what: {EDIT}") };
    if old.is_empty() {
        bail!("--old can't be empty: give the text to replace");
    }
    let dir = Path::new(dir);
    let path = text_file(dir, file, "edit")?;
    let mut s = super::session(dir)?;
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
        match commit(&mut s, &path, tx) {
            Ok(c) => {
                let n = at.len();
                println!("{path}: replaced {n} place{} (revision {})", if n == 1 { "" } else { "s" }, c.number);
                return Ok(());
            }
            // "Replace this text" means the same on the file as it is now.
            Err(Refusal::Conflict { .. }) if !again => {
                s.refresh();
            }
            Err(r) => bail!("nothing was changed: {r}"),
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
    let mut s = super::session(dir)?;
    let base = match std::fs::read_to_string(dir.join(path.path())) {
        Ok(t) => Base::text(&t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Base::Absent,
        Err(e) => bail!("{path} can't be read ({e}): nothing was changed"),
    };
    let made = base == Base::Absent;
    let label = (if made { "nothing".into() } else { path.to_string() }, path.to_string());
    let mut tx = Tx::new(
        &m2_song::tx::random_id(),
        label,
        vec![Op::Write { file: path.clone(), bytes: Payload::Text(text.as_str().into()) }],
        Expect::Identical,
    );
    tx.base.insert(path.clone(), base);
    let c = commit(&mut s, &path, tx).map_err(|r| anyhow!("nothing was changed: {r}"))?;
    println!("{path}: {} {} bytes (revision {})", if made { "made, with" } else { "written," }, text.len(), c.number);
    Ok(())
}
