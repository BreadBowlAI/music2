//! PLAT-4, RENDER-2: host code that makes audio computes every transcendental function with the
//! `libm` crate, never the OS's math library. glibc and Apple's libm round some results differently
//! in the last bit, and a release build turns `2f64.powf(x)` into `exp2` where a debug build calls
//! `pow`. This finds the float methods that reach the OS's library in the code `m2-runtime` and
//! `m2-play` compile, outside `#[cfg(test)]` items. `sqrt`, `floor` and arithmetic are exact on
//! every OS and stay. `powi` is banned too: it is not correctly rounded, and a release build may
//! expand it into other multiplications than a debug build's call. `asinh`, `acosh` and `atanh`
//! call the OS's `asinh`, `acosh` and `log1p`.

use std::path::{Path, PathBuf};

const NAMES: [&str; 26] = [
    "sin", "cos", "tan", "sin_cos", "powf", "exp", "exp2", "exp_m1", "ln", "ln_1p", "log", "log2", "log10", "tanh", "sinh", "cosh", "asin",
    "acos", "atan", "atan2", "asinh", "acosh", "atanh", "cbrt", "hypot", "powi",
];

#[test]
fn host_math_goes_through_libm() {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = vec![];
    for src in ["runtime/src", "play/src"] {
        let (root, mut files, mut every) = (core.join(src), vec![], vec![]);
        tree(&root.join("lib.rs"), &root, false, &mut files);
        // Every `.rs` file must be in the tree with its tests, so none goes unread because of a
        // `mod` line this can't parse.
        tree(&root.join("lib.rs"), &root, true, &mut every);
        let mut on_disk = vec![];
        rs_files(&root, &mut on_disk);
        let unread: Vec<_> =
            on_disk.iter().filter(|f| !every.iter().any(|(p, _)| p == *f)).map(|f| f.strip_prefix(&core).unwrap_or(f)).collect();
        assert!(unread.is_empty(), "not reached by a `mod name;` line from {src}/lib.rs, so not checked: {unread:#?}");
        for (file, code) in files {
            for (i, line) in code.lines().enumerate() {
                let line = line.trim_start();
                if line.starts_with("//") {
                    continue;
                }
                let at = |what: &str| format!("{}:{}: {what}", file.strip_prefix(&core).unwrap_or(&file).display(), i + 1);
                // The method with its bracket, so `.expect(` never matches.
                for n in NAMES {
                    found.extend(line.matches(&format!(".{n}(")).map(at));
                }
                // The function by its path, called or passed: `f32::sin(x)`, `.map(f64::sin)`.
                for ty in ["f32::", "f64::"] {
                    for (p, _) in line.match_indices(ty) {
                        let name = line[p + ty.len()..].split(|c: char| !c.is_ascii_alphanumeric() && c != '_').next().unwrap_or("");
                        if NAMES.contains(&name) {
                            found.push(at(&format!("{ty}{name}")));
                        }
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "host math from the OS's library (PLAT-4); call libm's function of the same name: {found:#?}");
}

/// The files of a module tree from `file`, whose child modules live in `dir`; unless `tests`, each
/// with its `#[cfg(test)]` items blanked, so a `#[cfg(test)] mod tests;` and its files are left out.
fn tree(file: &Path, dir: &Path, tests: bool, out: &mut Vec<(PathBuf, String)>) {
    let code = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let code = if tests { code } else { without_tests(&code) };
    for line in code.lines() {
        // `mod name;`, with any visibility and a comment after it.
        let words: Vec<&str> = line.split("//").next().unwrap_or("").split_whitespace().collect();
        if let [vis @ .., "mod", name] = &words[..]
            && vis.first().is_none_or(|v| v.starts_with("pub"))
            && let Some(name) = name.strip_suffix(';')
        {
            let flat = dir.join(format!("{name}.rs"));
            let child = if flat.exists() { flat } else { dir.join(name).join("mod.rs") };
            tree(&child, &dir.join(name), tests, out);
        }
    }
    out.push((file.to_path_buf(), code));
}

/// Every `.rs` file under `dir`.
fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `text` with every `#[cfg(test)]` item blanked (line breaks kept): the attribute, then up to
/// the item's first `;` or through its braces.
fn without_tests(text: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let mut out = text.as_bytes().to_vec();
    let mut from = 0;
    while let Some(at) = text[from..].find(ATTR).map(|i| i + from) {
        // An attribute starts its line; elsewhere it is only mentioned.
        if !text[..at].rsplit('\n').next().unwrap_or("").trim().is_empty() {
            from = at + ATTR.len();
            continue;
        }
        let end = item_end(text.as_bytes(), at + ATTR.len());
        out[at..end].iter_mut().filter(|c| **c != b'\n').for_each(|c| *c = b' ');
        from = end;
    }
    String::from_utf8(out).expect("whole characters were blanked")
}

/// Where the item from `i` ends: after its first `;`, or after the brace that closes its first `{`.
/// Braces inside comments, strings and character literals don't count.
fn item_end(b: &[u8], mut i: usize) -> usize {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => i += b[i..].iter().position(|&c| c == b'\n').unwrap_or(b.len() - i),
            b'/' if b.get(i + 1) == Some(&b'*') => i += b[i..].windows(2).position(|w| w == b"*/").map_or(b.len() - i, |p| p + 1),
            // A raw string, `r"…"` or `r#"…"#`: no escapes, and it ends at a quote with as many `#`.
            b'r' if (i == 0 || !ident(b[i - 1]) || b[i - 1] == b'b') && matches!(b.get(i + 1), Some(b'"' | b'#')) => {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    let close: Vec<u8> = std::iter::once(b'"').chain(std::iter::repeat_n(b'#', hashes)).collect();
                    let body = i + 2 + hashes;
                    i = b[body..].windows(close.len()).position(|w| w == close.as_slice()).map_or(b.len(), |p| body + p + close.len() - 1);
                }
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            // A character literal (`'{'`, `'\''`, `'é'`), never a lifetime (`'a`).
            b'\'' => {
                let len = match b.get(i + 1) {
                    Some(b'\\') => b.get(i + 3..).and_then(|r| r.iter().position(|&c| c == b'\'')).map_or(0, |p| p + 2),
                    Some(&c) => (c.leading_ones() as usize).max(1),
                    None => 0,
                };
                if b.get(i + 1 + len) == Some(&b'\'') {
                    i += 1 + len;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            b';' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    b.len()
}
