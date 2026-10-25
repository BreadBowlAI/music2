//! Builds the guide into `m2` for `m2 docs` (AGENT-12): every page `docs/guide/SUMMARY.md` names,
//! in its order, with its part, its topic (the file's name) and its title (its first `# ` line),
//! and its Markdown with each whole-line `{{#include <path>}}` filled in. Any other `{{#…}}` fails
//! the build, so no mdBook command reaches the terminal. So a page added to SUMMARY.md is in
//! `m2 docs` with nothing else to change. Writes `$OUT_DIR/guide.rs`, which `src/docs.rs` includes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let guide = root.join("docs/guide");
    // A folder: cargo builds again when anything in it changes.
    println!("cargo:rerun-if-changed={}", guide.display());
    let summary = read(&guide.join("SUMMARY.md"));
    let mut part = String::new();
    let mut out = String::from("pub const PAGES: &[Page] = &[\n");
    for line in summary.lines() {
        let line = line.trim();
        if let Some(heading) = line.strip_prefix("# ") {
            // The first heading is the menu's own ("Summary"); each one after it starts a part.
            if heading != "Summary" {
                part = heading.to_string();
            }
            continue;
        }
        let Some(file) = link(line) else { continue };
        let path = guide.join(file);
        assert!(
            path.parent() == Some(guide.as_path()) && file.ends_with(".md"),
            "docs/guide/SUMMARY.md: {file}: the guide is flat, every page a .md file in docs/guide/"
        );
        let text = expand(&path);
        let title = text.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or_else(|| panic!("docs/guide/{file} has no `# ` title")).trim();
        let topic = file.trim_end_matches(".md");
        writeln!(out, "    Page {{ part: {part:?}, topic: {topic:?}, title: {title:?}, text: {text:?} }},").unwrap();
    }
    out += "];\n";
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("guide.rs"), out).unwrap();
}

/// The file a menu line links: `[Title](file.md)` or `- [Title](file.md)`.
fn link(line: &str) -> Option<&str> {
    let line = line.strip_prefix("- ").unwrap_or(line);
    let rest = line.strip_prefix('[')?;
    let (_, after) = rest.split_once("](")?;
    after.strip_suffix(')')
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The page at `path` with each whole-line `{{#include <path>}}` filled in, as mdBook does.
fn expand(path: &Path) -> String {
    let mut out = String::new();
    for line in read(path).split_inclusive('\n') {
        let t = line.trim();
        match t.strip_prefix("{{#include ").and_then(|r| r.strip_suffix("}}")) {
            Some(file) if !file.contains(char::is_whitespace) => {
                let included = path.parent().unwrap().join(file);
                println!("cargo:rerun-if-changed={}", included.display());
                let text = read(&included);
                out += &text;
                if line.ends_with('\n') && !text.ends_with('\n') {
                    out.push('\n');
                }
            }
            _ if line.contains("{{#") => {
                panic!("{}: m2 docs fills in only a whole-line {{{{#include <path>}}}}, not: {}", path.display(), line.trim_end())
            }
            _ => out += line,
        }
    }
    out
}
