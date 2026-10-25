//! AGENT-12: `m2 docs` prints the guide built into this `m2`, the pages made from code are what
//! `m2` makes, a command `m2` doesn't know is refused, and the guide names only commands and
//! routes there are. (The command table's own tests, one table and every flag shown, are in
//! `src/commands.rs`: they read its consts.) FMT-23: `m2 new` gives a song AGENTS.md, naming this
//! `m2`, and CLAUDE.md.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A scratch HOME, removed after.
struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("m2-docs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn m2_in(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", home).env("M2_AUDIO", "none").env("M2_MIDI", "none").args(args).output().unwrap()
}

fn m2(args: &[&str]) -> Output {
    let home = Home::new(&args.join("-").replace(['/', ' '], "_"));
    m2_in(&home.0, args)
}

fn stdout(args: &[&str]) -> String {
    let out = m2(args);
    assert!(out.status.success(), "m2 {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// The pages `docs/guide/SUMMARY.md` names, in order: their topics.
fn summary() -> Vec<String> {
    let text = std::fs::read_to_string(root().join("docs/guide/SUMMARY.md")).unwrap();
    text.lines()
        .filter_map(|l| l.split_once("](").map(|(_, rest)| rest.trim_end().trim_end_matches(')').trim_end_matches(".md").to_string()))
        .collect()
}

/// A page of the guide with each whole-line `{{#include}}` filled in, as mdBook shows it.
fn expanded(topic: &str) -> String {
    let path = root().join("docs/guide").join(format!("{topic}.md"));
    let mut out = String::new();
    for line in std::fs::read_to_string(&path).unwrap().split_inclusive('\n') {
        match line.trim().strip_prefix("{{#include ").and_then(|r| r.strip_suffix("}}")) {
            Some(file) => out += &std::fs::read_to_string(path.parent().unwrap().join(file)).unwrap(),
            None => out += line,
        }
    }
    out
}

/// Pages `m2 docs` makes from code rather than the committed copy, and the agents' page, which
/// names this m2.
const MADE: &[&str] = &["commands", "routes"];

// AGENT-12: a page made from code that differs from the committed one fails, saying how to make
// it again.
#[test]
fn the_pages_made_from_code_are_fresh() {
    for page in MADE {
        let made = stdout(&["docs", page]);
        let committed = std::fs::read_to_string(root().join(format!("docs/guide/{page}.md"))).unwrap();
        assert!(made == committed, "docs/guide/{page}.md isn't what `m2 docs {page}` makes now: run ci/guide.sh gen, and commit the page");
    }
}

// AGENT-12: `m2 docs` lists every page of SUMMARY.md, in its order, with its title.
#[test]
fn m2_docs_lists_every_page_in_the_menus_order() {
    let list = stdout(&["docs"]);
    let listed: Vec<String> =
        list.lines().filter(|l| l.starts_with("  ")).map(|l| l.split_whitespace().next().unwrap().to_string()).collect();
    assert_eq!(listed, summary(), "{list}");
    for topic in summary() {
        let title = expanded(&topic).lines().find_map(|l| l.strip_prefix("# ").map(str::to_string)).unwrap();
        assert!(list.lines().any(|l| l.trim_start().starts_with(&topic) && l.ends_with(&title)), "{topic}: {title}\n{list}");
    }
}

// AGENT-12: each page prints as its Markdown, includes filled in; `all` prints them all, in order.
#[test]
fn each_page_prints_as_its_markdown_with_includes_filled_in() {
    let mut all = vec![];
    for topic in summary() {
        let printed = stdout(&["docs", &topic]);
        if !MADE.contains(&topic.as_str()) && topic != "agents" {
            assert!(printed == expanded(&topic), "m2 docs {topic} isn't docs/guide/{topic}.md with its includes filled in");
        }
        assert!(!printed.contains("{{#"), "m2 docs {topic} prints an mdBook command");
        all.push(printed);
    }
    assert!(stdout(&["docs", "all"]) == all.join("\n"), "m2 docs all isn't every page in the menu's order");
    // A link's own spelling works too.
    assert!(stdout(&["docs", "window.md"]) == expanded("window"));
}

/// templates/song-agents.md as a song made by this m2 gets it: its m2 line naming this m2.
fn agents_md() -> String {
    let template = std::fs::read_to_string(root().join("templates/song-agents.md")).unwrap();
    let me = Path::new(env!("CARGO_BIN_EXE_m2")).canonicalize().unwrap();
    let line = format!("m2 for this song: `{}`", me.display());
    let lines: Vec<&str> = template.lines().map(|l| if l.starts_with("m2 for this song: ") { line.as_str() } else { l }).collect();
    assert!(lines.contains(&line.as_str()), "templates/song-agents.md has no m2 line");
    lines.join("\n") + "\n"
}

// The song's AGENTS.md text is the guide's agents page (the owner's D3); `m2 docs agents` prints
// it as a song this m2 makes gets it, naming this m2 (AGENT-12, FMT-23).
#[test]
fn the_agents_page_is_the_songs_agents_md_naming_this_m2() {
    let template = std::fs::read_to_string(root().join("templates/song-agents.md")).unwrap();
    assert_eq!(expanded("agents"), template);
    assert_eq!(stdout(&["docs", "agents"]), agents_md());
}

// FMT-23: `m2 new` makes a song with AGENTS.md, naming this m2, and CLAUDE.md, which imports it.
#[test]
fn m2_new_gives_the_song_agents_md_and_claude_md() {
    let home = Home::new("new");
    let song = home.0.join("songs/Night drive");
    let out = m2_in(&home.0, &["new", song.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(song.join("AGENTS.md")).unwrap(), agents_md());
    assert_eq!(std::fs::read_to_string(song.join("CLAUDE.md")).unwrap(), "@AGENTS.md\n");
}

// AGENT-12: an unknown page is said, exit 1; a command m2 doesn't know is refused, exit 2. Neither
// writes anything, nor does `m2 docs`.
#[test]
fn unknown_pages_and_commands_are_refused_and_nothing_is_written() {
    let home = Home::new("refused");
    let out = m2_in(&home.0, &["docs", "nope"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("there's no page nope: m2 docs lists them"));
    let out = m2_in(&home.0, &["nosuch", "song"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("m2 has no command `nosuch`"), "{out:?}");
    assert!(out.stdout.is_empty());
    // --as or --why put first isn't an unknown command: m2 says where they go (the review's R5).
    for first in ["--as", "--why"] {
        let out = m2_in(&home.0, &[first, "Claude", "set", "song", "id", "1"]);
        let said = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{said}");
        assert!(said.contains("--as and --why go at the end, after the command's own arguments"), "{said}");
    }
    for args in [&["docs"][..], &["docs", "all"], &["docs", "commands"]] {
        assert!(m2_in(&home.0, args).status.success());
    }
    let left: Vec<_> = std::fs::read_dir(&home.0).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "m2 docs wrote into HOME: {left:?}");
    // The usage, with no command or asked for.
    for args in [&[][..], &["help"], &["--help"]] {
        let out = m2_in(&home.0, args);
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("  m2 docs [<page> | all]"));
    }
}

/// The words a Markdown text shows as code: its code spans and the lines of its code blocks.
fn code(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut block = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            block = !block;
            continue;
        }
        if block {
            out.push(line.to_string());
        } else {
            out.extend(line.split('`').skip(1).step_by(2).map(String::from));
        }
    }
    out
}

// AGENT-12: the guide names only commands m2 has, and routes the route table has.
#[test]
fn the_guide_names_only_real_commands_and_routes() {
    let commands: BTreeSet<String> = stdout(&["docs", "commands"])
        .lines()
        .filter_map(|l| l.strip_prefix("## m2 ").map(str::to_string))
        // check's other name, and the word that prints the usage.
        .chain(["build".to_string(), "help".to_string()])
        .collect();
    assert!(commands.contains("listen") && commands.contains("docs"), "{commands:?}");
    let routes: serde_json::Value = serde_json::from_str(&stdout(&["routes", "--json"])).unwrap();
    let paths: BTreeSet<String> = routes.as_array().unwrap().iter().map(|r| r["path"].as_str().unwrap().to_string()).collect();
    let mut pages: Vec<(String, String)> = summary().iter().map(|t| (format!("docs/guide/{t}.md"), expanded(t))).collect();
    pages.push(("README.md".into(), std::fs::read_to_string(root().join("README.md")).unwrap()));
    let mut bad = vec![];
    for (file, text) in &pages {
        for c in code(text) {
            let words: Vec<&str> = c.split_whitespace().collect();
            for w in words.windows(2) {
                let program = w[0].rsplit('/').next().unwrap();
                if program == "m2" && w[1].chars().next().is_some_and(|ch| ch.is_ascii_lowercase()) && !commands.contains(w[1]) {
                    bad.push(format!("{file}: `m2 {}`, which is no command", w[1]));
                }
            }
            for (i, _) in c.match_indices("/api/") {
                let path: String = c[i..].chars().take_while(|ch| ch.is_ascii_alphanumeric() || "/-_".contains(*ch)).collect();
                let path = path.trim_end_matches('/');
                if !paths.contains(path) {
                    bad.push(format!("{file}: `{path}`, which the route table hasn't got"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

// AGENT-12, the review's R2: the made commands page keeps what the manual's table said, which
// agents parse: m2 watch's lines, m2 focus's fields, m2 routes --json's shape, and what m2 call
// and m2 point say.
#[test]
fn the_commands_page_keeps_what_the_manual_said() {
    let page = stdout(&["docs", "commands"]);
    let section = |name: &str| {
        let start = page.find(&format!("\n## m2 {name}\n")).unwrap_or_else(|| panic!("no m2 {name}"));
        let rest = &page[start + 1..];
        rest[..rest[3..].find("\n## ").map_or(rest.len(), |e| e + 3)].to_string()
    };
    for (name, said) in [
        (
            "watch",
            &[
                "{\"t\":\"watch\",\"session\":…}",
                "`transport`",
                "every quarter second",
                "{\"t\":\"closed\",\"why\":\"app\"}",
                "exits 0",
                "{\"t\":\"closed\",\"why\":\"song\"}",
                "1,024 lines",
                "never takes a message",
            ][..],
        ),
        ("focus", &["`{track, lane, id}`", "`range`", "`pitch`, `start` and `length`", "`seen`", "`comparing`", "`midi`", "null"]),
        ("routes", &["`[{method, path, limit, writes, agents, words, example}]`", "`limit` being the largest body"]),
        ("call", &["On standard error it says the undo step a write made", "a body bigger than the route takes"]),
        ("point", &["so does their OK", "saying how many bars it has"]),
    ] {
        let text = section(name);
        for s in said {
            assert!(text.contains(s), "m2 {name} on the commands page doesn't say {s}:\n{text}");
        }
    }
    // Said once: the clips page's json sentence (R3).
    assert_eq!(section("clips").matches("`json` takes any gesture").count(), 1, "{}", section("clips"));
}
