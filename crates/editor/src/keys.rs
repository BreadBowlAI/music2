//! The one key proposer (ID-2): a key made from what something is called, once, when it is
//! created (a lane automate makes now, a track later). The key is then shown and written into the
//! code, where it is the thing's identity; nothing renames it from its label afterwards (ID-10).

use m2_song::Key;

/// A key for `label`, free by `taken`: folded to ASCII (compatibility decomposition, marks
/// dropped), lowercased, every run of other characters one `-`, trimmed to 32 characters and to
/// the key rule, then `-2`, `-3`… while it is taken. Empty when nothing of `label` is left (`"♪"`):
/// the caller has its own fallback (`lane`, `track`), which it proposes the same way.
pub fn propose(label: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = fold(label);
    if base.is_empty() {
        return base;
    }
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| {
            let suffix = format!("-{n}");
            // Shortened to make room for the suffix, and never left ending in a hyphen.
            let head = base[..base.len().min(32 - suffix.len())].trim_end_matches('-');
            format!("{head}{suffix}")
        })
        .find(|k| !taken(k))
        .expect("some suffix is free")
}

/// `label` spelled as a key, or empty.
fn fold(label: &str) -> String {
    let decomposed = icu_normalizer::DecomposingNormalizerBorrowed::new_nfkd().normalize(label);
    let mut out = String::new();
    let mut gap = false;
    for c in decomposed.chars().filter(|&c| !mark(c)) {
        if c.is_ascii_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    let mut key = out[..out.len().min(32)].trim_end_matches('-').to_string();
    // Every character is ASCII here, so the cut is on a boundary; the rule is checked anyway.
    if !Key::valid(&key) {
        key.clear();
    }
    key
}

/// Whether `c` is a combining mark a decomposition leaves after its letter (`é` → `e` + U+0301).
fn mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ID-2: a key is the label folded to the key rule, and never one that is taken.
    #[test]
    fn a_key_is_the_label_folded_and_free() {
        let free = |_: &str| false;
        assert_eq!(propose("hats pan", free), "hats-pan");
        assert_eq!(propose("Lead Vox", free), "lead-vox");
        assert_eq!(propose("  Crème brûlée!! ", free), "creme-brulee");
        assert_eq!(propose("ﬁlter Ⅱ", free), "filter-ii", "compatibility forms fold too");
        assert_eq!(propose("808", free), "808");
        assert_eq!(propose("channels.hats.db", free), "channels-hats-db");
        assert_eq!(propose("♪ ♫", free), "", "nothing left: the caller's fallback");
        let long = "a very long name for a lane that goes on and on";
        assert_eq!(propose(long, free), "a-very-long-name-for-a-lane-that");
        assert!(Key::valid(&propose(long, free)));

        let taken = |k: &str| ["hats-pan", "hats-pan-2"].contains(&k);
        assert_eq!(propose("Hats pan", taken), "hats-pan-3");
        // Shortened for its suffix, and never ending in a hyphen before it.
        let full = |k: &str| k == "a-very-long-name-for-a-lane-that";
        assert_eq!(propose(long, full), "a-very-long-name-for-a-lane-th-2");
        // A cut that would leave a hyphen before the suffix drops it.
        let edge = "abcdefghijklmnopqrstuvwxyz-ab-cd";
        assert_eq!(propose(edge, |k| k == edge), "abcdefghijklmnopqrstuvwxyz-ab-2");
    }
}
