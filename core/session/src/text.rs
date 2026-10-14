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

/// Lines a line diff may find different (about 2 MB of work at worst); a change that differs in
/// more is one hunk between the common prefix and suffix.
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

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The change from `old` to `new`: Myers' diff over whole lines, each hunk then trimmed by
    /// the bytes its old and new text share at both ends, so a changed literal is a hunk of the
    /// literal alone. Past [`DIFF_LINES`] different lines, one hunk between the common prefix and
    /// suffix.
    pub fn between(old: &str, new: &str) -> Edits {
        if old == new {
            return Edits::default();
        }
        let (a, b) = (lines(old), lines(new));
        let (at, bt): (Vec<&str>, Vec<&str>) = (a.iter().map(|l| l.1).collect(), b.iter().map(|l| l.1).collect());
        let Some(kept) = kept_lines(&at, &bt) else {
            let p = prefix(old, new);
            let s = suffix(&old[p..], &new[p..]);
            return Edits(vec![Hunk { old: p..old.len() - s, new: p..new.len() - s }]);
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
            let p = prefix(&old[os..oe], &new[ns..ne]);
            let s = suffix(&old[os + p..oe], &new[ns + p..ne]);
            if os + p < oe - s || ns + p < ne - s {
                hunks.push(Hunk { old: os + p..oe - s, new: ns + p..ne - s });
            }
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
