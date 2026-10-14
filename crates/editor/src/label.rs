//! The labels history shows (the step 6 design's 13.5, P10): what an edit said before and says
//! after, made by the editor for each Tx's `label`. Core stores and returns them; it never
//! makes one.

use std::path::Path;

use m2_script::sites::{Arg, Site};
use m2_session::Session;

/// What a whole-file change did, to show it and its undo: the one line it changed, or how
/// many. `pan: 0.3,` → `pan: 0.45,`; `3 lines` → `5 lines`.
pub fn summarize(old: &str, new: &str) -> (String, String) {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    let side = |lines: &[&str]| match lines {
        [] => "nothing".to_string(),
        [one] => {
            let one = one.trim();
            if one.chars().count() > 48 { format!("{}…", one.chars().take(47).collect::<String>()) } else { one.to_string() }
        }
        many => format!("{} lines", many.len()),
    };
    (side(&a[head..a.len() - tail]), side(&b[head..b.len() - tail]))
}

/// Call `call` of `old` before and after bytes `start..end` inside it become `text`, as a message
/// shows them: whole when both fit on a line; otherwise with each long argument the edit doesn't
/// touch (and each long property of an options object it edits) cut to `…` and white space
/// folded, so the change stays in view (`master(…)` → `master(…, { ceiling: 0 })`).
pub fn shown_call(call: &Site, old: &str, start: u32, end: u32, text: &str) -> (String, String) {
    const LINE: usize = 60;
    const SHORT: usize = 24;
    let (a, d) = (call.start as usize, call.end as usize);
    let before = &old[a..d];
    let after = format!("{}{text}{}", &old[a..start as usize], &old[end as usize..d]);
    let fits = |t: &str| !t.contains('\n') && t.chars().count() <= LINE;
    if fits(before) && fits(&after) {
        return (before.to_string(), after);
    }
    // Touched: the replaced bytes overlap it, or an insertion goes inside it.
    let untouched = |(s, e): (u32, u32)| if start < end { e <= start || end <= s } else { e <= start || start <= s };
    let long = |(s, e): (u32, u32)| !fits(&old[s as usize..e as usize]) || old[s as usize..e as usize].chars().count() > SHORT;
    let mut cut: Vec<(u32, u32)> = vec![];
    for arg in &call.args {
        let span = arg.span();
        if untouched(span) {
            cut.extend(Some(span).filter(|&x| long(x)));
        } else if let Arg::Object { props, .. } = arg {
            cut.extend(props.iter().map(|(_, v)| v.span()).filter(|&x| untouched(x) && long(x)));
        }
    }
    // The same cuts in both: those after the edit move by what it adds.
    let grown = text.len() as i64 - i64::from(end - start);
    let render = |t: &str, shift: bool| {
        let mut out = String::new();
        let mut at = 0;
        for &(s, e) in &cut {
            let moved = |x: u32| (i64::from(x) - a as i64 + if shift && s >= end { grown } else { 0 }) as usize;
            out += &t[at..moved(s)];
            out += "…";
            at = moved(e);
        }
        out += &t[at..];
        fold(&out)
    };
    (render(before, false), render(&after, true))
}

/// `code` on one line: without line comments (which would run on to the end), each run of white
/// space one space, none just inside brackets, and no comma before a closing one.
pub fn fold(code: &str) -> String {
    let mut bare = String::new();
    let mut quote = None;
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                bare.push(c);
                if c == '\\' {
                    bare.extend(chars.next());
                } else if c == q {
                    quote = None;
                }
            }
            None if c == '/' && chars.peek() == Some(&'/') => while chars.next_if(|&n| n != '\n').is_some() {},
            None if c == '/' && chars.peek() == Some(&'*') => {
                // A block comment stays, `//` inside it included.
                bare.push(c);
                bare.extend(chars.next());
                let mut star = false;
                for n in chars.by_ref() {
                    bare.push(n);
                    if star && n == '/' {
                        break;
                    }
                    star = n == '*';
                }
            }
            None => {
                if matches!(c, '"' | '\'' | '`') {
                    quote = Some(c);
                }
                bare.push(c);
            }
        }
    }
    let words: Vec<&str> = bare.split_whitespace().collect();
    let mut out = words.join(" ");
    for (from, to) in [("( ", "("), ("[ ", "["), (" )", ")"), (" ]", "]"), (",)", ")"), (",]", "]")] {
        out = out.replace(from, to);
    }
    out
}

/// How writing `text` over bytes `start..end` inside call `site` of the accepted revision is
/// shown: the call before and after, as an effect edit is (see [`shown_call`]).
pub fn shown(s: &Session, site: u32, start: u32, end: u32, text: &str) -> (String, String) {
    let rev = s.accepted();
    let call = rev.compiled.sites.iter().find(|x| x.id == site).filter(|x| x.start <= start && end <= x.end);
    let source = call.and_then(|x| rev.snapshot.files.get(Path::new(&x.file)));
    match (call, source) {
        (Some(x), Some(old)) => shown_call(x, old, start, end, text),
        _ => (String::new(), text.to_string()),
    }
}
