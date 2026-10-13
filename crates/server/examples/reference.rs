//! Write a reference project (p16, p50) that step 3's timing gates are measured on.
//!
//!   cargo run -p m2-server --example reference -- <new-folder> [--tracks 16] [--stems 4] [--repeats 16] [--bpm 127]
//!
//! The folder must be missing or empty. See `m2_session::reference`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use m2_session::reference::{self, Reference};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: reference <new-folder> [--tracks 16] [--stems 0] [--repeats 16] [--bpm 127]";
    let Some(dir) = args.first().filter(|a| !a.starts_with("--")) else { bail!(usage) };
    let mut r = Reference::P16;
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().with_context(|| format!("{flag} needs a value\n{usage}"))?;
        let bad = || format!("{flag} {value}\n{usage}");
        match flag.as_str() {
            "--tracks" => r.tracks = value.parse().with_context(bad)?,
            "--stems" => r.stems = value.parse().with_context(bad)?,
            "--repeats" => r.repeats = value.parse().with_context(bad)?,
            "--bpm" => r.bpm = value.parse().with_context(bad)?,
            _ => bail!("unknown option {flag}\n{usage}"),
        }
    }
    let dir = PathBuf::from(dir);
    reference::write(&dir, &r)?;
    println!("wrote {} ({} tracks, {} stems, {} beats at {} bpm) to {}", r.name(), r.tracks, r.stems, r.end(), r.bpm, dir.display());
    Ok(())
}
