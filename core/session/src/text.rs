//! What changed from one text to another, and where a position of one is in the other (WRITE-8,
//! WRITE-12, WRITE-21). Three mappings, each named where it is used, so a boundary never means
//! two things (the step 6 design's 2.4):
//!
//! 1. **Carry** (strict): where a range is after a change, when no change touched its inside. A
//!    Tx's own edits are carried across outside changes this way, and undo's inverse edits.
//! 2. **Points** (strict at the character): what names a node. A range is named by the point of
//!    the character it starts at and of the one it ends after: in unchanged text an offset in the
//!    reference text, inside a change that change and the offset in it. So a call around an
//!    edited literal keeps its name, and a call inserted around another never shares its range.
//! 3. **Spans** (inclusive): whether a node, an edit or an anchor lies inside a declared range. A
//!    point inside one of a Tx's own edits stands for the reference range that edit replaced; an
//!    insertion at `p` stands for `[p, p]`, inside a declared `[s, e]` whenever `s ≤ p ≤ e`.

use std::ops::Range;

/// Lines a line diff, or tokens a token diff, may find different (about 2 MB of work at worst). Past
/// it, the line diff leaves the lines between the common prefix and suffix to the token diff, and
/// the token diff leaves them one hunk.
pub(crate) const DIFF_LINES: usize = 500;

/// One change: bytes `old` of the old text became bytes `new` of the new one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

/// What changed from one text to another: hunks, sorted and disjoint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edits(pub Vec<Hunk>);

/// Which side of an insertion at a position a mapped position goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    /// Before what is inserted there: a range's start, taking an insertion at it in.
    Left,
    /// After it: a range's end, taking an insertion at it in.
    Right,
}

/// Each line with its newline, and where it starts.
fn lines(s: &str) -> Vec<(usize, &str)> {
    let mut at = 0;
    s.split_inclusive('\n')
        .map(|l| {
            at += l.len();
            (at - l.len(), l)
        })
        .collect()
}

/// The longest common prefix of `a` and `b`, in bytes, ending on a character boundary of both.
fn prefix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    while !(a.is_char_boundary(n) && b.is_char_boundary(n)) {
        n -= 1;
    }
    n
}

/// The longest common suffix of `a` and `b`, in bytes, ending on a character boundary of both.
fn suffix(a: &str, b: &str) -> usize {
    let mut n = a.bytes().rev().zip(b.bytes().rev()).take_while(|(x, y)| x == y).count();
    while !(a.is_char_boundary(a.len() - n) && b.is_char_boundary(b.len() - n)) {
        n -= 1;
    }
    n
}

/// The hunk `old[o]` → `new[n]` trimmed by the bytes they share at both ends, when they differ.
fn trimmed(old: &str, new: &str, o: Range<usize>, n: Range<usize>, out: &mut Vec<Hunk>) {
    let p = prefix(&old[o.clone()], &new[n.clone()]);
    let s = suffix(&old[o.start + p..o.end], &new[n.start + p..n.end]);
    if o.start + p < o.end - s || n.start + p < n.end - s {
        out.push(Hunk { old: o.start + p..o.end - s, new: n.start + p..n.end - s });
    }
}

/// The tokens of `s`, which starts at byte `at` of its text, each with where it starts:
/// identifiers and numbers, strings, and single punctuation characters, with whitespace and
/// comments left between them. A guess wrong about a regex or a template only makes a token
/// coarser or finer; the hunks still cover every byte that differs.
fn tokens(s: &str, at: usize) -> Vec<(usize, &str)> {
    token_iter(s, at).collect()
}

/// [`tokens`], one at a time.
fn token_iter(s: &str, at: usize) -> impl Iterator<Item = (usize, &str)> {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let mut i = 0;
    std::iter::from_fn(move || {
        while let Some(c) = s[i..].chars().next() {
            let rest = &s[i..];
            let (len, token) = match c {
                _ if rest.starts_with("//") => (rest.find('\n').unwrap_or(rest.len()), false),
                _ if let Some(comment) = rest.strip_prefix("/*") => (comment.find("*/").map_or(rest.len(), |e| e + 4), false),
                _ if c.is_whitespace() => (c.len_utf8(), false),
                '"' | '\'' | '`' => {
                    // To its closing quote, past escaped ones.
                    let mut escaped = false;
                    let close = rest[1..].char_indices().find(|&(_, d)| {
                        let end = !escaped && d == c;
                        escaped = !escaped && d == '\\';
                        end
                    });
                    (close.map_or(rest.len(), |(e, _)| e + 2), true)
                }
                _ if word(c) => (rest.find(|d: char| !word(d)).unwrap_or(rest.len()), true),
                _ => (c.len_utf8(), true),
            };
            i += len;
            if token {
                return Some((at + i - len, &rest[..len]));
            }
        }
        None
    })
}

fn is_word(t: &str) -> bool {
    t.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$')
}

/// The tokens between the `(` at byte `open` of `s` and the `)` that closes it, and where that `)`
/// is; None when it doesn't close.
fn args(s: &str, open: usize) -> Option<(Vec<(usize, &str)>, usize)> {
    let mut tokens = token_iter(&s[open..], open);
    if tokens.next()?.1 != "(" {
        return None;
    }
    let (mut depth, mut inside) = (1, vec![]);
    for (at, t) in tokens {
        depth += match t {
            "(" => 1,
            ")" => -1,
            _ => 0,
        };
        if depth == 0 {
            return Some((inside, at));
        }
        inside.push((at, t));
    }
    None
}

/// The first argument of a call's arguments: up to the first comma outside brackets.
fn first_arg<'a>(args: &[(usize, &'a str)]) -> Vec<(usize, &'a str)> {
    let mut depth = 0;
    args.iter()
        .take_while(|(_, t)| {
            depth += match *t {
                "(" | "[" | "{" => 1,
                ")" | "]" | "}" => -1,
                _ => 0,
            };
            !(depth == 0 && *t == ",")
        })
        .copied()
        .collect()
}

/// A run of tokens that one side has twice, `first` then `second` (in `two`), and the other side
/// once (`one`, in `one_text`): which copy the one continues, judged by the calls it opens (ID-13).
/// The second when every `(` in it has the same arguments there: a call wrapped in a call of the
/// same callee. The first when, with the call the second copy opens taken out (its first argument
/// left in its place), every `(` has the same arguments there: a call of the same callee put
/// inside it. A run that opens no call goes to the second, as a run starting at a word does.
/// None when neither: both calls are new.
fn pick(two: &str, first: &[(usize, &str)], second: &[(usize, &str)], one_text: &str, one: &[(usize, &str)]) -> Option<bool> {
    fn words<'a>(x: &[(usize, &'a str)]) -> Vec<&'a str> {
        x.iter().map(|t| t.1).collect()
    }
    let (mut in_second, mut in_first) = (true, true);
    for r in (0..one.len()).filter(|&r| one[r].1 == "(") {
        let ((want, _), (there, close), (outer, end)) = (args(one_text, one[r].0)?, args(two, second[r].0)?, args(two, first[r].0)?);
        in_second &= words(&there) == words(&want);
        // The call the second copy opens starts at its callee: the words and dots before its `(`.
        let head = (0..r).rev().take_while(|&h| is_word(second[h].1) || second[h].1 == ".").last().unwrap_or(r);
        let (from, to) = (second[head].0, close);
        let unwrapped: Vec<&str> = match first[r].0 < from && to < end {
            true => {
                let mut x: Vec<&str> = outer.iter().filter(|t| t.0 < from).map(|t| t.1).collect();
                x.extend(words(&first_arg(&there)));
                x.extend(outer.iter().filter(|t| t.0 > to).map(|t| t.1));
                x
            }
            false => words(&outer),
        };
        in_first &= unwrapped == words(&want);
    }
    if in_second {
        Some(true)
    } else if in_first {
        Some(false)
    } else {
        None
    }
}

/// Slides each run of inserted, or deleted, tokens of `kept` (which maps `a`'s tokens to `b`'s,
/// from texts `old` and `new`) up as far as it goes, then back down to the first place where it
/// starts with a word: Myers keeps whatever matches first, so a `plugin(…)` wrapped in another
/// `plugin(` would give its `(` to the wrapper. A wrap is inserted as `name(`, and what it wraps
/// keeps its tokens whole (ID-13). But a call of the same callee put inside another is inserted as
/// `name(` too, and there the old `(` is the outer one: [`pick`] tells the two apart by their
/// arguments, and when it can't, the tokens it would move are kept by neither copy. Those are
/// returned (as `a`'s tokens), for a hunk over them that trimming can't take back: both calls new.
fn slide(kept: &mut [Option<usize>], old: &str, new: &str, a: &[(usize, &str)], b: &[(usize, &str)]) -> Vec<bool> {
    let mut forced = vec![false; kept.len()];
    // What follows a run of changed lines is unchanged, as a kept token is: a call that spans
    // lines is wrapped, or unwrapped, at the end of its first line.
    let next = |kept: &[Option<usize>], i: usize| if i < kept.len() { kept[i] } else { Some(b.len()) };
    // Inserted: `b[j + 1..k]`, between two old tokens side by side.
    for i in 1..=kept.len() {
        let (Some(j), Some(k)) = (kept[i - 1], next(kept, i)) else { continue };
        if k == j + 1 {
            continue;
        }
        let up = (1..=i.min(j + 1)).take_while(|&t| kept[i - t] == Some(j + 1 - t) && b[j + 1 - t].1 == b[k - t].1).count();
        if let Some(d) = (1..=up).rev().find(|&d| is_word(b[j + 1 - d].1)) {
            match pick(new, &b[j + 1 - d..=j], &b[k - d..k], old, &a[i - d..i]) {
                Some(true) => (1..=d).for_each(|t| kept[i - t] = Some(k - t)),
                Some(false) => {}
                None => (1..=d).for_each(|t| (kept[i - t], forced[i - t]) = (None, true)),
            }
        }
    }
    // Deleted: `a[s..e]`, between two new tokens side by side.
    let mut s = 1;
    while s < kept.len() {
        let e = (s..kept.len()).find(|&e| kept[e].is_some()).unwrap_or(kept.len());
        if e > s
            && let Some(j) = kept[s - 1]
            && next(kept, e) == Some(j + 1)
        {
            let up = (1..=s.min(j + 1)).take_while(|&t| kept[s - t] == Some(j + 1 - t) && a[s - t].1 == a[e - t].1).count();
            if let Some(d) = (1..=up).rev().find(|&d| is_word(a[s - d].1)) {
                match pick(old, &a[s - d..s], &a[e - d..e], new, &b[j + 1 - d..=j]) {
                    Some(true) => (1..=d).for_each(|t| (kept[e - t], kept[s - t]) = (kept[s - t], None)),
                    Some(false) => {}
                    None => (1..=d).for_each(|t| (kept[s - t], forced[s - t]) = (None, true)),
                }
            }
        }
        s = e + 1;
    }
    forced
}

/// The hunks from `old[o]` to `new[n]`, a run of whole lines that differ: a Myers diff over their
/// tokens, then one hunk for each gap between kept tokens whose bytes differ, trimmed (but for a
/// gap [`slide`] forced). Past [`DIFF_LINES`] different tokens, the run is one hunk.
fn refine(old: &str, new: &str, o: Range<usize>, n: Range<usize>, out: &mut Vec<Hunk>) {
    let (a, b) = (tokens(&old[o.clone()], o.start), tokens(&new[n.clone()], n.start));
    let (at, bt): (Vec<&str>, Vec<&str>) = (a.iter().map(|t| t.1).collect(), b.iter().map(|t| t.1).collect());
    let mut kept = kept_lines(&at, &bt).unwrap_or_else(|| vec![None; a.len()]);
    let forced = slide(&mut kept, old, new, &a, &b);
    let (mut os, mut ns, mut from) = (o.start, n.start, 0);
    for (i, j) in kept.iter().enumerate().filter_map(|(i, j)| Some((i, (*j)?))) {
        match forced[from..i].contains(&true) {
            true => out.push(Hunk { old: os..a[i].0, new: ns..b[j].0 }),
            false => trimmed(old, new, os..a[i].0, ns..b[j].0, out),
        }
        (os, ns, from) = (a[i].0 + a[i].1.len(), b[j].0 + b[j].1.len(), i + 1);
    }
    match forced[from..].contains(&true) {
        true => out.push(Hunk { old: os..o.end, new: ns..n.end }),
        false => trimmed(old, new, os..o.end, ns..n.end, out),
    }
}

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The change from `old` to `new`: Myers' diff over whole lines, each run of changed lines
    /// then refined by a Myers diff over its tokens ([`refine`]), so two literals edited on
    /// adjacent lines are two hunks, and a wrap keeps every token of what it wraps (ID-13). Past
    /// [`DIFF_LINES`] different lines (a whole file re-indented, say), the token diff refines
    /// every line between the common prefix and suffix; past as many different tokens, they are
    /// one hunk.
    pub fn between(old: &str, new: &str) -> Edits {
        if old == new {
            return Edits::default();
        }
        let (a, b) = (lines(old), lines(new));
        let (at, bt): (Vec<&str>, Vec<&str>) = (a.iter().map(|l| l.1).collect(), b.iter().map(|l| l.1).collect());
        let Some(kept) = kept_lines(&at, &bt) else {
            // Widened to whole lines, so no token is cut.
            let p = old[..prefix(old, new)].rfind('\n').map_or(0, |n| n + 1);
            let s = suffix(&old[p..], &new[p..]);
            let s = old[old.len() - s..].find('\n').map_or(0, |n| s - n - 1);
            let mut hunks = vec![];
            refine(old, new, p..old.len() - s, p..new.len() - s, &mut hunks);
            return Edits(hunks);
        };
        let start = |ls: &[(usize, &str)], i: usize, len: usize| ls.get(i).map_or(len, |l| l.0);
        let mut hunks = vec![];
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && kept[i] == Some(j) {
                (i, j) = (i + 1, j + 1);
                continue;
            }
            let mut k = i;
            while k < a.len() && kept[k].is_none() {
                k += 1;
            }
            let m = if k < a.len() { kept[k].expect("kept") } else { b.len() };
            let (os, oe) = (start(&a, i, old.len()), start(&a, k, old.len()));
            let (ns, ne) = (start(&b, j, new.len()), start(&b, m, new.len()));
            refine(old, new, os..oe, ns..ne, &mut hunks);
            (i, j) = (k, m);
        }
        Edits(hunks)
    }

    /// The hunks that replacing each `(start, end, text)` of `old` makes, and the new text. The
    /// edits must be sorted and disjoint, on character boundaries.
    pub fn of(old: &str, edits: &[(usize, usize, String)]) -> (Edits, String) {
        let mut new = String::with_capacity(old.len());
        let mut hunks = vec![];
        let mut at = 0;
        for (s, e, text) in edits {
            new += &old[at..*s];
            hunks.push(Hunk { old: *s..*e, new: new.len()..new.len() + text.len() });
            new += text;
            at = *e;
        }
        new += &old[at..];
        (Edits(hunks), new)
    }

    /// STRICT: where `r` is after the change, when no hunk touches its inside (WRITE-12). A hunk
    /// that only inserts at `r.start` or `r.end` is outside it; one that inserts at an empty
    /// range's point is a guess, and touches it.
    pub fn carry(&self, r: Range<usize>) -> Option<Range<usize>> {
        let mut shift = 0isize;
        for h in &self.0 {
            let grow = h.new.len() as isize - h.old.len() as isize;
            let (os, oe) = (h.old.start, h.old.end);
            if os == oe {
                if os < r.start || (os == r.start && r.start < r.end) {
                    shift += grow;
                    continue;
                }
                if os > r.end || (os == r.end && r.start < r.end) {
                    break;
                }
                return None;
            }
            if oe <= r.start {
                shift += grow;
                continue;
            }
            if os >= r.end {
                break;
            }
            return None;
        }
        let at = |x: usize| (x as isize + shift) as usize;
        Some(at(r.start)..at(r.end))
    }

    /// Where position `pos` of the old text is in the new one: shifted past the hunks before
    /// it; inside a replaced range, to its new range's start (`Left`) or end (`Right`); at an
    /// insertion, before or after what it inserts.
    pub fn map(&self, pos: usize, bias: Bias) -> usize {
        let mut shift = 0isize;
        for h in &self.0 {
            let grow = h.new.len() as isize - h.old.len() as isize;
            let (os, oe) = (h.old.start, h.old.end);
            if oe < pos || (oe == pos && os < oe) {
                shift += grow;
            } else if os == oe && os == pos {
                if bias == Bias::Right {
                    shift += grow;
                } else {
                    break;
                }
            } else if os < pos && pos < oe {
                return if bias == Bias::Left { h.new.start } else { h.new.end };
            } else {
                break;
            }
        }
        (pos as isize + shift) as usize
    }

    /// The hunk whose new text holds `at`, and where in it.
    fn inside_new(&self, at: usize) -> Option<(usize, usize)> {
        self.0.iter().position(|h| h.new.start <= at && at < h.new.end).map(|i| (i, at - self.0[i].new.start))
    }

    /// Position `at` of the new text, outside every hunk, in the old text.
    fn back(&self, at: usize) -> usize {
        let shift: isize = self.0.iter().filter(|h| h.new.end <= at).map(|h| h.new.len() as isize - h.old.len() as isize).sum();
        (at as isize - shift) as usize
    }
}

/// Where a position lies, in a file's reference coordinates (the step 6 design's 5.2): an offset
/// of the reference text, or a byte of what a change made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Point {
    Base(usize),
    /// Byte `at` of own edit `edit`'s new text.
    Own {
        edit: usize,
        at: usize,
    },
    /// Byte `at` of outside hunk `hunk`'s new text.
    Outside {
        hunk: usize,
        at: usize,
    },
}

/// A file's reference coordinates: its reference text (a Tx's base text, or the old revision's),
/// with the hunks that lead from it to the current text (outside changes) and from the current
/// text to the candidate (the Tx's own edits, or a revision's change).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coords {
    pub outside: Edits,
    pub own: Edits,
    /// Each own edit's range in the reference text (what a span inside it stands for), when it
    /// lies in unchanged text.
    pub own_base: Vec<Option<Range<usize>>>,
    /// How an own edit's points are spelled: `e` for a Tx's own edits, `o` for a revision's.
    pub tag: char,
}

impl Coords {
    /// A file both sides have alike: its own reference.
    pub fn same() -> Coords {
        Coords { tag: 'e', ..Coords::default() }
    }

    /// A Tx's file: `outside` from its base text to the current one, then its own `edits` (sorted,
    /// in current coordinates) with the base range each replaced.
    pub fn tx(outside: Edits, own: Edits, own_base: Vec<Option<Range<usize>>>) -> Coords {
        Coords { outside, own, own_base, tag: 'e' }
    }

    /// Two revisions' file: the old text is the reference, and its change to the new one.
    pub fn revisions(change: Edits) -> Coords {
        let own_base = change.0.iter().map(|h| Some(h.old.clone())).collect();
        Coords { outside: Edits::default(), own: change, own_base, tag: 'o' }
    }

    /// A position of the current text (the delta's old side).
    pub fn current(&self, at: usize) -> Point {
        match self.outside.inside_new(at) {
            Some((hunk, at)) => Point::Outside { hunk, at },
            None => Point::Base(self.outside.back(at)),
        }
    }

    /// A position of the candidate (the delta's new side).
    pub fn candidate(&self, at: usize) -> Point {
        match self.own.inside_new(at) {
            Some((edit, at)) => Point::Own { edit, at },
            None => self.current(self.own.back(at)),
        }
    }

    /// INCLUSIVE: the reference range a point stands for. None inside an outside change, which no
    /// Tx can declare.
    pub fn span(&self, p: &Point) -> Option<Range<usize>> {
        match p {
            Point::Base(n) => Some(*n..*n + 1),
            Point::Own { edit, .. } => self.own_base.get(*edit).cloned().flatten(),
            Point::Outside { .. } => None,
        }
    }

    /// How a point is written in a node's name: `412`, `e0.7`, `o2.3`.
    pub fn spell(&self, p: &Point) -> String {
        match p {
            Point::Base(n) => n.to_string(),
            Point::Own { edit, at } => format!("{}{edit}.{at}", self.tag),
            Point::Outside { hunk, at } => format!("o{hunk}.{at}"),
        }
    }
}

/// For each line of `a`, the line of `b` a shortest edit script keeps it as, or None when it is
/// deleted (Myers' greedy algorithm, O((N+M)·D)). None when more than [`DIFF_LINES`] lines differ.
pub fn kept_lines<T: PartialEq>(a: &[T], b: &[T]) -> Option<Vec<Option<usize>>> {
    let mut kept = vec![None; a.len()];
    // Lines both start and end with are kept as they are; the diff runs between them.
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev()).take_while(|(x, y)| x == y).count();
    for (i, k) in kept.iter_mut().enumerate().take(head) {
        *k = Some(i);
    }
    for i in 0..tail {
        kept[a.len() - 1 - i] = Some(b.len() - 1 - i);
    }
    let (xs, ys) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    let (n, m) = (xs.len() as isize, ys.len() as isize);
    let max = ((n + m) as usize).min(DIFF_LINES) as isize;
    // v[k + max] is the furthest x reached on diagonal k; trace[d] is v after round d.
    let at = |k: isize| (k + max) as usize;
    let mut v = vec![0isize; 2 * max as usize + 2];
    let mut trace: Vec<Vec<isize>> = vec![];
    let mut found = None;
    'rounds: for d in 0..=max {
        for k in (-d..=d).step_by(2) {
            let mut x = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) { v[at(k + 1)] } else { v[at(k - 1)] + 1 };
            let mut y = x - k;
            while x < n && y < m && xs[x as usize] == ys[y as usize] {
                (x, y) = (x + 1, y + 1);
            }
            v[at(k)] = x;
            if x >= n && y >= m {
                found = Some(d);
                trace.push(v.clone());
                break 'rounds;
            }
        }
        trace.push(v.clone());
    }
    // Back from the end: each round's edit, then the lines its snake kept.
    let (mut d, mut x, mut y) = (found?, n, m);
    while d > 0 {
        let prev = &trace[d as usize - 1];
        let k = x - y;
        let down = k == -d || (k != d && prev[at(k - 1)] < prev[at(k + 1)]);
        let pk = if down { k + 1 } else { k - 1 };
        let (px, py) = (prev[at(pk)], prev[at(pk)] - pk);
        let (sx, sy) = if down { (px, py + 1) } else { (px + 1, py) };
        while x > sx && y > sy {
            (x, y) = (x - 1, y - 1);
            kept[head + x as usize] = Some(head + y as usize);
        }
        (x, y, d) = (px, py, d - 1);
    }
    while x > 0 && y > 0 {
        (x, y) = (x - 1, y - 1);
        kept[head + x as usize] = Some(head + y as usize);
    }
    Some(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator.
    pub(crate) struct Xorshift(pub u64);

    impl Xorshift {
        pub fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n.max(1) as u64) as usize
        }
    }

    /// `new` rebuilt from `old` and the hunks: what every hunk list must give.
    fn apply(old: &str, new: &str, e: &Edits) -> String {
        let mut out = String::new();
        let mut at = 0;
        for h in &e.0 {
            out += &old[at..h.old.start];
            out += &new[h.new.clone()];
            at = h.old.end;
        }
        out + &old[at..]
    }

    // WRITE-12: a changed literal is a hunk of the literal alone; the hunks rebuild the new text.
    #[test]
    fn hunks_are_trimmed_to_what_changed() {
        let old = "a\n  pan: 0.3,\nb\n";
        let new = "a\n  pan: 0.45,\nb\n";
        let e = Edits::between(old, new);
        assert_eq!(e.0, [Hunk { old: 11..12, new: 11..13 }]);
        assert_eq!(apply(old, new, &e), new);
        // Inserted lines above and below, and a word replaced on a line.
        let (old, new) = ("x\ny = f(1)\nz\n", "// top\nx\ny = g(1)\nz\n// end\n");
        let e = Edits::between(old, new);
        assert_eq!(apply(old, new, &e), new);
        assert_eq!(e.0.len(), 3);
        assert_eq!(&new[e.0[1].new.clone()], "g");
        // Multi-byte characters never split.
        let (old, new) = ("é ベース x", "é ベーキ x");
        let e = Edits::between(old, new);
        assert_eq!((&old[e.0[0].old.clone()], &new[e.0[0].new.clone()]), ("ス", "キ"));
        assert!(Edits::between(old, old).is_empty());
    }

    // ID-13 (identity note F1): two literals edited on adjacent lines in one save are two hunks,
    // each the literal alone, so the calls between them keep their points; so is a wrap's each
    // insertion, whatever the closing brackets do.
    #[test]
    fn two_literals_on_adjacent_lines_are_two_hunks() {
        let old = "const a = lowpass(x, { cutoff: 800 });\nconst b = delay(a, { mix: 0.3 });\n";
        let new = "const a = lowpass(x, { cutoff: 950 });\nconst b = delay(a, { mix: 0.25 });\n";
        let e = Edits::between(old, new);
        let shown: Vec<(&str, &str)> = e.0.iter().map(|h| (&old[h.old.clone()], &new[h.new.clone()])).collect();
        assert_eq!(shown, [("80", "95"), ("3", "25")]);
        let (old, new) = ("reverb(pad(lane.notes(\"n\")));\n", "reverb(delay(pad(lane.notes(\"n\"))));\n");
        let e = Edits::between(old, new);
        let kept = |at: usize| e.carry(at..at + 1).map(|r| r.start);
        assert_eq!((kept(6), kept(10), kept(21)), (Some(6), Some(16), Some(27)), "the `(` of reverb, pad and notes: {:?}", e.0);
    }

    // ID-13: a call wrapped in a call of the same callee keeps its own `(`, not the wrapper's, and
    // so does the unwrap: the wrap is inserted as `plugin(…`, whatever Myers matched first.
    #[test]
    fn a_wrap_by_the_same_callee_keeps_the_wrapped_paren() {
        let old = "  const p = plugin('probe', { gain: 0.5 });\n";
        let new = "  const p = plugin('outer', { in: plugin('probe', { gain: 0.5 }), key: 'k' });\n";
        let (at, to) = (old.find('(').unwrap(), new.rfind("plugin(").unwrap() + 6);
        assert_eq!(Edits::between(old, new).carry(at..at + 1).map(|r| r.start), Some(to), "{:?}", Edits::between(old, new));
        assert_eq!(Edits::between(new, old).carry(to..to + 1).map(|r| r.start), Some(at), "{:?}", Edits::between(new, old));
        // Where the callees differ, nothing slides: the outer call keeps the outer `(`.
        let (old, new) = ("reverb(pad(x));\n", "reverb(delay(pad(x)));\n");
        assert_eq!(Edits::between(old, new).carry(6..7).map(|r| r.start), Some(6));
    }

    // ID-13: a call of the same callee put inside another, as Add Effect writes it after the
    // other's sound, leaves the old `(` on the outer call, where its arguments are; taking the
    // inner one out again leaves it there too. Taking the outer one out leaves the inner's.
    #[test]
    fn a_call_put_inside_one_of_the_same_callee_leaves_it_the_outer_paren() {
        let one = "  return delay(osc.saw(hz(110)), { time: 0.388 });\n";
        let two = "  return delay(delay(osc.saw(hz(110)), { time: 0.25, mix: 0.3 }), { time: 0.388 });\n";
        let (outer, inner, saw) = (two.find('(').unwrap(), two.rfind("delay(").unwrap() + 5, two.find("saw(").unwrap() + 3);
        let at = |old: &str, new: &str, p: usize| Edits::between(old, new).carry(p..p + 1).map(|r| r.start);
        let (paren, its_saw) = (one.find('(').unwrap(), one.find("saw(").unwrap() + 3);
        assert_eq!((at(one, two, paren), at(one, two, its_saw)), (Some(outer), Some(saw)), "{:?}", Edits::between(one, two));
        assert_eq!((at(two, one, outer), at(two, one, inner)), (Some(paren), None), "{:?}", Edits::between(two, one));
        // The outer one taken out: the inner keeps its `(`.
        let left = "  return delay(osc.saw(hz(110)), { time: 0.25, mix: 0.3 });\n";
        assert_eq!((at(two, left, outer), at(two, left, inner)), (None, Some(paren)), "{:?}", Edits::between(two, left));
        // A lowpass inside a lowpass, its setting written positionally.
        let (one, two) = ("x = lowpass(saw, 6000);\n", "x = lowpass(lowpass(saw, 1000), 6000);\n");
        assert_eq!(at(one, two, 11), Some(11));
        assert_eq!(at(two, one, 11), Some(11));
    }

    // ID-13: the same for a call that spans lines, wrapped or unwrapped at the end of its first
    // line: the wrapped call keeps its `(`, and a call put inside it leaves it the outer one.
    #[test]
    fn a_call_that_spans_lines_keeps_its_paren_through_the_same_callee() {
        let one = "  return reverb(\n    widen(x, { width: 1.3 }),\n    { size: 0.6, mix: 0.2 },\n  );\n";
        let wrapped =
            "  return reverb(reverb(\n    widen(x, { width: 1.3 }),\n    { size: 0.6, mix: 0.2 },\n  ), { size: 0.6, mix: 0.25 });\n";
        let inside =
            "  return reverb(\n    reverb(widen(x, { width: 1.3 }), { size: 0.6, mix: 0.25 }),\n    { size: 0.6, mix: 0.2 },\n  );\n";
        let at = |old: &str, new: &str, p: usize| Edits::between(old, new).carry(p..p + 1).map(|r| r.start);
        let paren = one.find('(').unwrap();
        let (outer, inner) = (wrapped.find('(').unwrap(), wrapped.find("reverb(reverb(").unwrap() + 13);
        assert_eq!(at(one, wrapped, paren), Some(inner), "{:?}", Edits::between(one, wrapped));
        assert_eq!((at(wrapped, one, inner), at(wrapped, one, outer)), (Some(paren), None), "{:?}", Edits::between(wrapped, one));
        assert_eq!(at(one, inside, paren), Some(paren));
        assert_eq!(at(inside, one, paren), Some(paren));
    }

    // ID-13: when neither call of the same callee has the old one's arguments, neither is it: a
    // hunk covers both `(`s, and the old one's carries nowhere. What the calls hold keeps its own.
    #[test]
    fn a_call_of_the_same_callee_that_is_neither_is_new() {
        let one = "  return delay(osc.saw(hz(110)), { time: 0.388 });\n";
        let two = "  return delay(delay(osc.saw(hz(110)), { time: 0.25 }), { time: 0.5 });\n";
        let e = Edits::between(one, two);
        let paren = one.find('(').unwrap();
        assert_eq!(e.carry(paren..paren + 1), None, "{e:?}");
        let c = Coords::revisions(e.clone());
        let opens: Vec<usize> = two.match_indices("delay(").map(|(i, _)| i + 5).collect();
        assert!(opens.iter().all(|&o| matches!(c.candidate(o), Point::Own { .. })), "both are new: {e:?}");
        let saw = one.find("saw(").unwrap() + 3;
        assert_eq!(e.carry(saw..saw + 1).map(|r| r.start), Some(two.find("saw(").unwrap() + 3));
        assert_eq!(apply(one, two, &e), two);
        // The same taking one out.
        let e = Edits::between(two, one);
        assert!(opens.iter().all(|&o| e.carry(o..o + 1).is_none()), "{e:?}");
        assert_eq!(apply(two, one, &e), one);
    }

    // ID-13: a reformat of a whole file too long for the line diff (every line re-indented, or
    // its line ends made CRLF) still changes no token: every call's `(` is carried.
    #[test]
    fn a_long_file_reformatted_keeps_every_paren() {
        let lines: Vec<String> =
            (0..400).map(|k| format!("  const v{k} = delay(pluck(lane.notes(\"n{k}\")), {{ time: 0.{k} }});\n")).collect();
        let old: String = lines.concat();
        let parens: Vec<usize> = old.match_indices('(').map(|(i, _)| i).collect();
        for new in [lines.iter().map(|l| format!("  {l}")).collect::<String>(), old.replace('\n', "\r\n")] {
            let e = Edits::between(&old, &new);
            assert_eq!(apply(&old, &new, &e), new);
            let to: Vec<Option<usize>> = parens.iter().map(|&p| e.carry(p..p + 1).map(|r| r.start)).collect();
            let wrong = to.iter().zip(new.match_indices('(')).filter(|(t, (i, _))| **t != Some(*i)).count();
            assert_eq!(wrong, 0, "of {} `(`s, {wrong} not carried to their place", parens.len());
        }
    }

    // DIFF_LINES: a change of more tokens than the token diff may find different is one hunk
    // between the common prefix and suffix, whatever the line diff did.
    #[test]
    fn a_change_past_the_token_limit_is_one_hunk() {
        let old: String = (0..600).map(|k| format!("  const v{k} = delay(x, {k});\n")).collect();
        let new = old.replace("delay(", "echo(");
        let e = Edits::between(&old, &new);
        assert_eq!(e.0, [Hunk { old: 13..old.len() - 10, new: 13..new.len() - 10 }]);
    }

    // ID-13: a reformat moves whitespace only, so every hunk lies between tokens.
    #[test]
    fn a_reformat_changes_no_token() {
        let old = "export default track(\"t\", ({ lane }) => reverb(pad(lane.notes(\"n\")), { mix: 0.3 }));\n";
        let new = "export default track(\"t\", ({ lane }) =>\n  reverb(\n    pad(lane.notes( \"n\" )),\n    { mix: 0.3 } // wet\n  )\n);\n";
        let e = Edits::between(old, new);
        assert!(!e.is_empty());
        for h in &e.0 {
            assert!(
                tokens(&old[h.old.clone()], 0).is_empty() && tokens(&new[h.new.clone()], 0).is_empty(),
                "{:?} → {:?}",
                &old[h.old.clone()],
                &new[h.new.clone()]
            );
        }
    }

    // ID-13, WRITE-12: on code-like text, the hunks are an edit script: sorted and disjoint, on
    // character boundaries, what lies between them equal on both sides, and applied to the old
    // text they give the new one.
    #[test]
    fn token_hunks_are_an_edit_script() {
        let mut rng = Xorshift(0x5851_f42d_4c95_7f2d);
        let words =
            ["pluck", "(", ")", "lane", ".", "notes", "\"n\"", "800", ",", " ", "\n", "// c\n", "/* d */", "é", "'x\\'y'", "{", "}"];
        for case in 0..500 {
            let old: String = (0..rng.below(40)).map(|_| words[rng.below(words.len())]).collect();
            let mut new = old.clone();
            for _ in 0..1 + rng.below(4) {
                let chars: Vec<usize> = new.char_indices().map(|(i, _)| i).chain([new.len()]).collect();
                let at = chars[rng.below(chars.len())];
                let end = chars.iter().copied().filter(|&c| c >= at).nth(rng.below(3)).unwrap_or(at);
                let text: String = (0..rng.below(4)).map(|_| words[rng.below(words.len())]).collect();
                new.replace_range(at..end, &text);
            }
            let e = Edits::between(&old, &new);
            assert_eq!(apply(&old, &new, &e), new, "case {case}: {old:?} → {new:?}");
            let (mut o, mut n) = (0, 0);
            for h in &e.0 {
                assert!(
                    o <= h.old.start && n <= h.new.start && h.old.start <= h.old.end && h.new.start <= h.new.end,
                    "case {case}: {:?}",
                    e.0
                );
                assert!([h.old.start, h.old.end].iter().all(|&x| old.is_char_boundary(x)), "case {case}");
                assert!([h.new.start, h.new.end].iter().all(|&x| new.is_char_boundary(x)), "case {case}");
                assert_eq!(old[o..h.old.start], new[n..h.new.start], "case {case}: carried bytes are equal");
                (o, n) = (h.old.end, h.new.end);
            }
        }
    }

    // ID-13, WRITE-12: calls of one callee nested and edited at random (wrapped, unwrapped, put
    // inside one another, their arguments changed, unbalanced): the hunks, forced ones included,
    // are still an edit script that rebuilds the new text, and an old `(` carried is a `(`.
    #[test]
    fn nested_calls_of_one_callee_give_an_edit_script() {
        let mut rng = Xorshift(0x9e37_79b9_7f4a_7c15);
        let words = ["f(", "f(", ")", "), 2)", ", ", "x", "1", "{ t: 3 }", "\n", " "];
        let mut forced = 0;
        for case in 0..2000 {
            let old: String = (0..rng.below(24)).map(|_| words[rng.below(words.len())]).collect();
            let mut new = old.clone();
            for _ in 0..1 + rng.below(3) {
                let at = rng.below(new.len() + 1);
                let end = (at + rng.below(4)).min(new.len());
                let text: String = (0..rng.below(3)).map(|_| words[rng.below(words.len())]).collect();
                new.replace_range(at..end, &text);
            }
            let e = Edits::between(&old, &new);
            assert_eq!(apply(&old, &new, &e), new, "case {case}: {old:?} → {new:?}");
            let (mut o, mut n) = (0, 0);
            for h in &e.0 {
                assert!(
                    o <= h.old.start && n <= h.new.start && h.old.start <= h.old.end && h.new.start <= h.new.end,
                    "case {case}: {:?}",
                    e.0
                );
                assert_eq!(old[o..h.old.start], new[n..h.new.start], "case {case}: carried bytes are equal");
                forced += usize::from(old[h.old.clone()].contains('(') && prefix(&old[h.old.clone()], &new[h.new.clone()]) > 0);
                (o, n) = (h.old.end, h.new.end);
            }
            for (p, _) in old.match_indices('(') {
                if let Some(to) = e.carry(p..p + 1) {
                    assert_eq!(&new[to], "(", "case {case}: {old:?} → {new:?}");
                }
            }
        }
        assert!(forced > 0, "some cases force a hunk");
    }

    // WRITE-12: carry is strict. A range is carried past changes elsewhere, its own line included;
    // a change inside it, or one replacing a character of it, stops it; an insertion at either end
    // stays outside it.
    #[test]
    fn carry_is_strict() {
        let e = Edits(vec![Hunk { old: 2..2, new: 2..5 }, Hunk { old: 10..12, new: 13..13 }]);
        assert_eq!(e.carry(5..8), Some(8..11), "past an insertion before it");
        assert_eq!(e.carry(2..8), Some(5..11), "an insertion at its start is before it");
        assert_eq!(e.carry(0..2), Some(0..2), "an insertion at its end is after it");
        assert_eq!(e.carry(0..3), None, "an insertion inside it");
        assert_eq!(e.carry(9..11), None, "a deletion overlapping it");
        assert_eq!(e.carry(12..14), Some(13..15), "past a deletion ending where it starts");
        assert_eq!(e.carry(2..2), None, "an insertion at an empty range's point is a guess");
        assert_eq!(e.carry(10..10), Some(13..13));
        let between = Edits::between("call(a, 0.3) // x\n", "call(a, 0.3) // y\n");
        assert_eq!(between.carry(0..12), Some(0..12), "a change past it on its own line");
    }

    // The three mappings at every boundary: an insertion at a range's start and end, a
    // replacement covering its first character, a wrap.
    #[test]
    fn points_spans_and_maps() {
        let base = "pluck(notes)";
        // A wrap: `delay(` at 0 and `, 0.25)` at 12.
        let (own, cand) = Edits::of(base, &[(0, 0, "delay(".into()), (12, 12, ", 0.25)".into())]);
        assert_eq!(cand, "delay(pluck(notes), 0.25)");
        let c = Coords::tx(Edits::default(), own, vec![Some(0..0), Some(12..12)]);
        // The wrapped call keeps its points; the wrapping one's are inside the insertions.
        assert_eq!((c.candidate(6), c.candidate(17)), (Point::Base(0), Point::Base(11)));
        assert_eq!((c.candidate(0), c.candidate(24)), (Point::Own { edit: 0, at: 0 }, Point::Own { edit: 1, at: 6 }));
        assert_eq!((c.spell(&c.candidate(0)), c.spell(&c.candidate(24))), ("e0.0".to_string(), "e1.6".to_string()));
        // Its span is the wrapped call's: inside a declared [0, 12].
        assert_eq!((c.span(&c.candidate(0)), c.span(&c.candidate(24))), (Some(0..0), Some(12..12)));
        // Maps: a declared range's ends take the insertions at them in.
        assert_eq!((c.own.map(0, Bias::Left), c.own.map(12, Bias::Right)), (0, 25));
        assert_eq!((c.own.map(0, Bias::Right), c.own.map(12, Bias::Left)), (6, 18));
        // A replacement covering a character: the point is inside the edit.
        let (own, _) = Edits::of(base, &[(6, 11, "hits".into())]);
        let c = Coords::tx(Edits::default(), own, vec![Some(6..11)]);
        assert_eq!(c.candidate(6), Point::Own { edit: 0, at: 0 });
        assert_eq!(c.candidate(10), Point::Base(11), "past it, the base offset");
        assert_eq!(c.own.map(8, Bias::Left), 6);
        assert_eq!(c.own.map(8, Bias::Right), 10);
        // An outside change: current positions inside it are its points, the rest base offsets.
        let outside = Edits::between("a(1)\nb(2)\n", "// c\na(1)\nb(2)\n");
        let c = Coords::tx(outside, Edits::default(), vec![]);
        assert_eq!(c.current(0), Point::Outside { hunk: 0, at: 0 });
        assert_eq!(c.current(5), Point::Base(0));
        assert_eq!(c.span(&c.current(0)), None);
        // Revisions spell their change `o`.
        let r = Coords::revisions(Edits::between("x = 1;\n", "x = 22;\n"));
        assert_eq!(r.spell(&r.candidate(4)), "o0.0");
    }

    // WRITE-12: the hunks always rebuild the new text, and a range carries exactly when no hunk
    // touches it, wherever it is.
    #[test]
    fn hunks_rebuild_and_carry_finds_untouched_ranges() {
        let mut rng = Xorshift(0x2545_f491_4f6c_dd1d);
        let alphabet = ["a", "b", "é", "ベ", "\n", " ", "(", ")", "0", "1"];
        for case in 0..500 {
            let old: String = (0..rng.below(60)).map(|_| alphabet[rng.below(alphabet.len())]).collect();
            let mut new = old.clone();
            for _ in 0..1 + rng.below(4) {
                let chars: Vec<(usize, char)> = new.char_indices().collect();
                let at = if chars.is_empty() { 0 } else { chars[rng.below(chars.len())].0 };
                let end = new[at..].char_indices().nth(rng.below(3)).map_or(new.len(), |(i, _)| at + i);
                let text: String = (0..rng.below(4)).map(|_| alphabet[rng.below(alphabet.len())]).collect();
                new.replace_range(at..end, &text);
            }
            let e = Edits::between(&old, &new);
            assert_eq!(apply(&old, &new, &e), new, "case {case}: {old:?} → {new:?}");
            for h in &e.0 {
                assert!(old.is_char_boundary(h.old.start) && old.is_char_boundary(h.old.end), "case {case}");
            }
            // Any range of whole characters: carried exactly when no hunk touches its inside.
            let bounds: Vec<usize> = old.char_indices().map(|(i, _)| i).chain([old.len()]).collect();
            let (a, b) = (bounds[rng.below(bounds.len())], bounds[rng.below(bounds.len())]);
            let r = a.min(b)..a.max(b);
            if r.is_empty() {
                continue;
            }
            match e.carry(r.clone()) {
                Some(to) => assert_eq!(&new[to], &old[r.clone()], "case {case}: carried to the same text"),
                None => assert!(
                    e.0.iter().any(|h| (h.old.start < r.end && r.start < h.old.end)
                        || (h.old.is_empty() && r.start < h.old.start && h.old.start < r.end)),
                    "case {case}: refused though no hunk touches {r:?}: {:?}",
                    e.0
                ),
            }
        }
    }
}
