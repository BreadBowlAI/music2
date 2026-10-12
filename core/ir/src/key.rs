//! Keys: the saved identity a person writes in code (ID-1, ID-2).
//!
//! A key names a track, a lane, a plugin's state or a `key()` frame, and saved data is
//! filed under it. Keys are validated, never normalised: [`suggest`] only proposes a
//! spelling for an error message, and nothing derives a key from a label by itself.

/// The root module's key, reserved for it (ID-3).
pub const ROOT: &str = "song";

/// The longest key.
pub const MAX_LEN: usize = 32;

/// Whether `s` is a key: `^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?$` (ID-1).
pub fn is_key(s: &str) -> bool {
    let b = s.as_bytes();
    let edge = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    (1..=MAX_LEN).contains(&b.len()) && b.first().is_some_and(edge) && b.last().is_some_and(edge) && b.iter().all(|c| edge(c) || *c == b'-')
}

/// A valid spelling close to `s`, for the message of `key.invalid` (ID-2): folded to ASCII,
/// lowercased, every run of other characters turned into one `-`, and trimmed to the key
/// rule. `None` when nothing of `s` survives, or when `s` is already a key.
pub fn suggest(s: &str) -> Option<String> {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        let folded = fold(c);
        for f in folded.chars() {
            if f.is_ascii_lowercase() || f.is_ascii_digit() {
                out.push(f);
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let trimmed: String = out.trim_matches('-').chars().take(MAX_LEN).collect();
    let k = trimmed.trim_end_matches('-').to_string();
    (is_key(&k) && k != s).then_some(k)
}

/// Why `s` is not a valid key, as the message of `key.invalid`; `None` when it is one. `what`
/// names the key's place, such as "the track key".
pub fn invalid(what: &str, s: &str) -> Option<String> {
    if is_key(s) {
        return None;
    }
    let rule = "a key is 1 to 32 lowercase letters, digits and hyphens, starting and ending with a letter or digit";
    let quoted = serde_json::to_string(s).unwrap_or_else(|_| s.to_string());
    Some(match suggest(s) {
        Some(k) => format!("{what} {quoted} is not a valid key: {rule}. Write \"{k}\" instead"),
        None => format!("{what} {quoted} is not a valid key: {rule}"),
    })
}

/// Latin letters with marks, as ASCII. Anything else that isn't ASCII becomes a separator.
fn fold(c: char) -> String {
    const FOLD: &[(&str, &str)] = &[
        ("àáâãäåāăą", "a"),
        ("æ", "ae"),
        ("çćĉċč", "c"),
        ("ďđð", "d"),
        ("èéêëēĕėęě", "e"),
        ("ĝğġģ", "g"),
        ("ĥħ", "h"),
        ("ìíîïĩīĭįı", "i"),
        ("ĵ", "j"),
        ("ķ", "k"),
        ("ĺļľŀł", "l"),
        ("ñńņňŋ", "n"),
        ("òóôõöøōŏő", "o"),
        ("œ", "oe"),
        ("ŕŗř", "r"),
        ("śŝşš", "s"),
        ("ß", "ss"),
        ("ţťŧ", "t"),
        ("þ", "th"),
        ("ùúûüũūŭůűų", "u"),
        ("ŵ", "w"),
        ("ýÿŷ", "y"),
        ("źżž", "z"),
    ];
    if c.is_ascii() {
        return c.to_string();
    }
    FOLD.iter().find(|(from, _)| from.contains(c)).map_or_else(|| "-".to_string(), |(_, to)| to.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ID-1: the key rule.
    #[test]
    fn the_key_rule() {
        for ok in ["kick", "lead-vox", "808", "a", "a1", "a--b", "x".repeat(32).as_str(), "aux", "con"] {
            assert!(is_key(ok), "{ok}");
        }
        for bad in ["", "Kick", "lead vox", "-a", "a-", "-", "lead_vox", "é", "x".repeat(33).as_str(), "song.ts", "a/b", "a\\b", " a"] {
            assert!(!is_key(bad), "{bad}");
        }
    }

    // ID-2: an invalid key is reported with a valid spelling, never normalised silently.
    #[test]
    fn suggestions_fold_to_the_key_rule() {
        assert_eq!(suggest("Lead Vox").as_deref(), Some("lead-vox"));
        assert_eq!(suggest("  Café  Bar!! ").as_deref(), Some("cafe-bar"));
        assert_eq!(suggest("Straße_2").as_deref(), Some("strasse-2"));
        assert_eq!(suggest("--808--").as_deref(), Some("808"));
        assert_eq!(suggest(&"Abc ".repeat(20)).map(|k| k.len()), Some(31), "cut to 32, then trimmed to the rule");
        assert_eq!(suggest("ベース"), None);
        assert_eq!(suggest("kick"), None, "already a key");
        let m = invalid("the track key", "Lead Vox").unwrap();
        assert!(m.contains("\"Lead Vox\"") && m.contains("Write \"lead-vox\""), "{m}");
        assert!(invalid("the lane key", "hook").is_none());
        assert!(!invalid("the track key", "ベース").unwrap().contains("Write"));
    }
}
