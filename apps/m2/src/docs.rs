//! `m2 docs` (AGENT-12): the guide, `docs/guide/`, built into this `m2` by `build.rs`, so it
//! matches what this `m2` does and needs no network. It needs no song and no app, and reads and
//! writes nothing: `main` answers it before anything else starts.

use std::io::Write;
use std::process::ExitCode;

/// One page of the guide, as `build.rs` found it through `docs/guide/SUMMARY.md`.
pub struct Page {
    /// Its part of the menu ("Getting started"), or "" before the first.
    pub part: &'static str,
    /// Its file's name: `window` for `window.md`.
    pub topic: &'static str,
    /// Its first `# ` line.
    pub title: &'static str,
    /// Its Markdown, each `{{#include}}` filled in.
    pub text: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/guide.rs"));

/// Where the guide is online: it follows the newest music2, this `m2` its own version.
pub const SITE: &str = "https://breadbowlai.github.io/music2/";

/// `m2 docs [<page> | all]`, `args[0]` being `docs`.
pub fn docs(args: &[String]) -> ExitCode {
    let out = match args.get(1..).unwrap_or_default() {
        [] => list(),
        [all] if all == "all" => PAGES.iter().map(text).collect::<Vec<_>>().join("\n"),
        [topic] => match PAGES.iter().find(|p| p.topic == topic.trim_end_matches(".md")) {
            Some(p) => text(p),
            None => {
                eprintln!("error: there's no page {topic}: m2 docs lists them");
                return ExitCode::FAILURE;
            }
        },
        _ => unreachable!("its usage takes one page at most (AGENT-13)"),
    };
    say(&out)
}

/// The page as `m2 docs` prints it: those made from code are made now, from this `m2`'s own
/// tables, not the committed copies; the agents page names this `m2`.
fn text(p: &Page) -> String {
    match p.topic {
        "commands" => crate::commands::page(),
        "routes" => crate::call::page(),
        // As a song this m2 makes gets it: its m2 line names this m2 (FMT-23).
        "agents" => m2_server::project::agents_text(m2_server::resources::m2_program().as_ref()),
        _ => p.text.to_string(),
    }
}

/// The pages, part by part.
fn list() -> String {
    let mut out = format!(
        "music2's guide, built into this m2 ({}). Print a page: m2 docs <page>. All of it: m2 docs all.\n\
         Online, for the newest music2: {SITE}\n",
        env!("CARGO_PKG_VERSION")
    );
    let mut part = None;
    for p in PAGES {
        if part != Some(p.part) {
            out += "\n";
            if !p.part.is_empty() {
                out += &format!("{}\n", p.part);
            }
            part = Some(p.part);
        }
        out += &format!("  {:<18} {}\n", p.topic, p.title);
    }
    out + "\nA link to another page, such as window.md, is m2 docs window.\n"
}

/// Print `text`; a reader that went away (`m2 docs all | head`) is no failure.
fn say(text: &str) -> ExitCode {
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
        _ => ExitCode::SUCCESS,
    }
}
