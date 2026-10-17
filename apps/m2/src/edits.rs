//! The 8b back ends' commands, each a Tx builder of `crates/editor` taking the project's lock
//! (WRITE-18): mute in song.

use std::path::Path;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};

/// Whether `cmd` is one of these.
pub fn handles(cmd: &str) -> bool {
    matches!(cmd, "mute-in-song")
}

/// Run `cmd`, with the command line's arguments (`args[0]` is `cmd`).
pub fn run(cmd: &str, args: &[String]) -> Result<()> {
    match cmd {
        "mute-in-song" => mute_in_song(args),
        _ => unreachable!("handled: {cmd}"),
    }
}

/// `m2 mute-in-song <project> <track>… on|off`: one Tx, one undo step (MON-5).
fn mute_in_song(args: &[String]) -> Result<()> {
    let usage = "say which tracks, then on or off: m2 mute-in-song <project> <track>… on|off";
    let [_, dir, rest @ .., last] = args else { bail!("{usage}") };
    let muted = match last.as_str() {
        "on" => true,
        "off" => false,
        _ => bail!("{usage}"),
    };
    if rest.is_empty() {
        bail!("{usage}");
    }
    let mut s = super::session(Path::new(dir))?;
    let t = Instant::now();
    let e = m2_editor::mute::mute_in_song(&mut s, rest, muted).map_err(|e| anyhow!("{e}"))?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    match e.before == e.after {
        true => println!("{}: nothing to write", e.after),
        false => println!("{}: {} → {} (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision),
    }
    Ok(())
}
