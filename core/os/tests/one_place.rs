//! PLAT-2: code that differs between operating systems lives in `m2-os`. Outside it, code other
//! than tests names no OS: no `cfg` on the OS, no `libc`, no `std::os`, no `extern "C"` block, no
//! `$HOME`. This reads every `.rs` file under `core/`, `crates/` and `apps/m2/`, skipping
//! `core/os/`, `tests/` folders, test files (`tests.rs`, `*_tests.rs`, and modules declared under
//! `#[cfg(test)]`) and `#[cfg(test)]` items, matched by braces. Comments don't count.
//!
//! The desktop shell (`apps/music2-desktop/src`) is outside what it reads, and G11's fault
//! injection (`refused` in the gates' `server.rs`) is allowed. `NOT_YET` lists the files whose OS
//! code a later slice of the platform step moves into `m2-os`; each slice removes its own lines, and
//! the list is empty after S8.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const NAMES_AN_OS: [&str; 11] = [
    "target_os",
    "cfg(unix)",
    "cfg!(unix)",
    "cfg(not(unix))",
    "cfg(windows)",
    "cfg!(windows)",
    "libc::",
    "std::os::",
    "extern \"C\" {",
    "var_os(\"HOME\")",
    "var(\"HOME\")",
];

/// Files whose OS code a later slice moves into `m2-os`, and that slice.
const NOT_YET: &[(&str, &str)] = &[
    ("core/song/src/hash.rs", "S6"),
    ("core/session/src/watch.rs", "S6"),
    ("core/session/src/lock.rs", "S6"),
    ("core/session/src/tx/mod.rs", "S6"),
    ("core/session/src/tx/stage.rs", "S6"),
    ("crates/editor/src/render.rs", "S6"),
    ("crates/server/src/project.rs", "S6"),
    ("crates/server/src/resources.rs", "S6"),
    ("crates/server/src/graph.rs", "S8"),
    ("crates/server/src/graph/fsevents.rs", "S8"),
    ("crates/server/src/graph/watcher.rs", "S8"),
];

/// The one function allowed to name an OS outside `m2-os`: G11's refused write, a measuring tool's
/// fault (PLAT-2).
const ALLOWED_FN: (&str, &str) = ("crates/gates/src/server.rs", "fn refused(");

#[test]
fn os_code_lives_in_m2_os() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let mut files = vec![];
    for dir in ["core", "crates", "apps/m2"] {
        rust_files(&root.join(dir), &mut files);
    }
    files.retain(|f| !f.starts_with(root.join("core/os")));
    let sources: Vec<(PathBuf, Source)> = files.into_iter().map(|f| (f.clone(), Source::read(&f))).collect();
    let test_modules: BTreeSet<PathBuf> = sources.iter().flat_map(|(f, s)| s.test_modules(f)).collect();
    let mut found = vec![];
    let mut named = BTreeSet::new();
    for (file, src) in &sources {
        if test_modules.contains(file) {
            continue;
        }
        let rel = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        let mut skip = src.test_items();
        if rel == ALLOWED_FN.0 {
            skip.extend(src.item_at(ALLOWED_FN.1));
        }
        for (at, pattern) in src.hits() {
            if skip.iter().any(|r| r.contains(&at)) {
                continue;
            }
            named.insert(rel.clone());
            if !NOT_YET.iter().any(|(f, _)| *f == rel) {
                found.push(format!("{rel}:{}: {pattern}: {}", src.line_of(at), src.line_text(at).trim()));
            }
        }
    }
    assert!(found.is_empty(), "OS code outside m2-os (PLAT-2): move it into core/os, or name the OS only in a test:\n{}", found.join("\n"));
    let moved: Vec<_> = NOT_YET.iter().filter(|(f, _)| !named.contains(*f)).map(|(f, s)| format!("{f} ({s})")).collect();
    assert!(moved.is_empty(), "these files name no OS any more: remove them from NOT_YET in core/os/tests/one_place.rs: {moved:?}");
}

/// Every `.rs` file under `dir`, outside `tests/` folders and test files.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if p.is_dir() {
            if name != "tests" && name != "target" {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            out.push(p);
        }
    }
}

/// A source file, and the same text with comments blanked (`code`) and with comments, strings and
/// character literals blanked (`bare`), so braces can be counted. Indices are chars; newlines stay.
struct Source {
    code: Vec<char>,
    bare: Vec<char>,
}

impl Source {
    fn read(path: &Path) -> Source {
        let text: Vec<char> = std::fs::read_to_string(path).unwrap().chars().collect();
        let (mut code, mut bare) = (text.clone(), text.clone());
        let blank = |v: &mut Vec<char>, from: usize, to: usize| {
            for c in &mut v[from..to.min(text.len())] {
                if *c != '\n' {
                    *c = ' ';
                }
            }
        };
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        let at = |i: usize| text.get(i).copied().unwrap_or('\0');
        let mut i = 0;
        while i < text.len() {
            let c = text[i];
            if c == '/' && at(i + 1) == '/' {
                let end = (i..text.len()).find(|&j| text[j] == '\n').unwrap_or(text.len());
                blank(&mut code, i, end);
                blank(&mut bare, i, end);
                i = end;
            } else if c == '/' && at(i + 1) == '*' {
                let (mut depth, mut j) = (1, i + 2);
                while j < text.len() && depth > 0 {
                    if text[j] == '/' && at(j + 1) == '*' {
                        depth += 1;
                        j += 1;
                    } else if text[j] == '*' && at(j + 1) == '/' {
                        depth -= 1;
                        j += 1;
                    }
                    j += 1;
                }
                blank(&mut code, i, j);
                blank(&mut bare, i, j);
                i = j;
            } else if c == 'r' && (i == 0 || !ident(text[i - 1]) || (text[i - 1] == 'b' && (i < 2 || !ident(text[i - 2])))) && {
                let h = (i + 1..text.len()).find(|&j| text[j] != '#').unwrap_or(text.len());
                at(h) == '"'
            } {
                // A raw string: r"…", r#"…"#, br"…".
                let open = (i + 1..text.len()).find(|&j| text[j] != '#').unwrap();
                let hashes = open - i - 1;
                let mut j = open + 1;
                while j < text.len() && !(text[j] == '"' && (1..=hashes).all(|k| at(j + k) == '#')) {
                    j += 1;
                }
                blank(&mut bare, open + 1, j);
                i = j + 1 + hashes;
            } else if c == '"' {
                let mut j = i + 1;
                while j < text.len() && text[j] != '"' {
                    j += if text[j] == '\\' { 2 } else { 1 };
                }
                blank(&mut bare, i + 1, j);
                i = j + 1;
            } else if c == '\'' && (at(i + 1) == '\\' || at(i + 2) == '\'') {
                // A character literal ('x', '\n', '\u{7b}'), not a lifetime ('a).
                let mut j = i + 1;
                while j < text.len() && text[j] != '\'' {
                    j += if text[j] == '\\' { 2 } else { 1 };
                }
                blank(&mut bare, i + 1, j);
                i = j + 1;
            } else {
                i += 1;
            }
        }
        Source { code, bare }
    }

    /// Where each pattern occurs in the code.
    fn hits(&self) -> Vec<(usize, &'static str)> {
        let code: String = self.code.iter().collect();
        let mut v = vec![];
        for p in NAMES_AN_OS {
            for (byte, _) in code.match_indices(p) {
                v.push((code[..byte].chars().count(), p));
            }
        }
        v.sort();
        v
    }

    /// The items under `#[cfg(test)]`, attribute included.
    fn test_items(&self) -> Vec<std::ops::Range<usize>> {
        self.find("#[cfg(test)]").into_iter().map(|at| at..self.item_end(at + "#[cfg(test)]".len())).collect()
    }

    /// The item that starts with `start`, if the file has it.
    fn item_at(&self, start: &str) -> Option<std::ops::Range<usize>> {
        self.find(start).first().map(|&at| at..self.item_end(at))
    }

    /// The files of modules declared under `#[cfg(test)]` (`mod props;`).
    fn test_modules(&self, file: &Path) -> Vec<PathBuf> {
        let dir = match file.file_name().unwrap().to_str().unwrap() {
            "lib.rs" | "main.rs" | "mod.rs" => file.parent().unwrap().to_path_buf(),
            name => file.parent().unwrap().join(name.trim_end_matches(".rs")),
        };
        let mut v = vec![];
        for r in self.test_items() {
            let item: String = self.bare[r].iter().collect();
            let words: Vec<&str> = item.split(|c: char| c.is_whitespace() || c == ';').filter(|w| !w.is_empty()).collect();
            if item.trim_end().ends_with(';')
                && let [.., "mod", name] = words.as_slice()
            {
                v.push(dir.join(format!("{name}.rs")));
                v.push(dir.join(name).join("mod.rs"));
            }
        }
        v
    }

    fn find(&self, what: &str) -> Vec<usize> {
        let w: Vec<char> = what.chars().collect();
        (0..self.bare.len()).filter(|&i| self.bare[i..].starts_with(&w)).collect()
    }

    /// Where the item from `from` ends: past its closing brace, or its `;` (a field's `,`) when it
    /// has no body.
    fn item_end(&self, from: usize) -> usize {
        let (mut nest, mut i) = (0i32, from);
        while i < self.bare.len() {
            match self.bare[i] {
                '(' | '[' => nest += 1,
                ')' | ']' | '}' if nest == 0 => return i,
                ')' | ']' => nest -= 1,
                ';' | ',' if nest == 0 => return i + 1,
                '{' if nest == 0 => {
                    let mut depth = 0;
                    for (j, &c) in self.bare.iter().enumerate().skip(i) {
                        depth += (c == '{') as i32 - (c == '}') as i32;
                        if depth == 0 {
                            return j + 1;
                        }
                    }
                    return self.bare.len();
                }
                _ => {}
            }
            i += 1;
        }
        self.bare.len()
    }

    fn line_of(&self, at: usize) -> usize {
        1 + self.code[..at].iter().filter(|&&c| c == '\n').count()
    }

    fn line_text(&self, at: usize) -> String {
        let start = self.code[..at].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1);
        self.code[start..].iter().take_while(|&&c| c != '\n').collect()
    }
}

#[test]
fn the_reader_sees_through_strings_comments_and_test_items() {
    let dir = std::env::temp_dir().join(format!("m2-one-place-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("lib.rs");
    std::fs::write(
        &file,
        concat!(
            "// libc:: in a comment\n",
            "/* std::os:: in /* a nested */ block */\n",
            "#[cfg(test)]\n",
            "mod t { fn f() -> [u8; 2] { let s = \"}\"; let c = '}'; let r = r#\"}\"#; let _ = libc::getpid(); [0; 2] } }\n",
            "#[cfg(test)]\n",
            "mod props;\n",
            "struct S { #[cfg(test)] a: u8, b: u8 }\n",
            "fn b<'a>(x: &'a str) { std::env::var_os(\"HOME\"); }\n",
        ),
    )
    .unwrap();
    let src = Source::read(&file);
    let hits = src.hits();
    let skip = src.test_items();
    let seen: Vec<_> = hits.iter().filter(|(at, _)| !skip.iter().any(|r| r.contains(at))).map(|(at, p)| (src.line_of(*at), *p)).collect();
    assert_eq!(seen, [(8, "var_os(\"HOME\")")], "only the code outside the test module counts");
    assert!(src.test_modules(&file).contains(&dir.join("props.rs")));
    let _ = std::fs::remove_dir_all(&dir);
}
