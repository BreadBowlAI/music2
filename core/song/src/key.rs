//! Saved identity: keys written by a person in code (ID-1), and clip ids (FMT-11).
//!
//! A key is validated, never normalised (ID-2): `"Lead Vox"` is not a key, and nothing here
//! turns it into one.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

/// The root module's key (ID-3).
pub const ROOT: &str = "song";

/// Windows device names: legal keys, but never a bare path segment (ID-9).
const DEVICES: [&str; 22] = [
    "aux", "con", "nul", "prn", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4",
    "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A key: 1 to 32 lowercase letters, digits and inner hyphens (ID-1).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(String);

impl Key {
    pub fn new(s: &str) -> Result<Key, String> {
        if Key::valid(s) {
            Ok(Key(s.to_string()))
        } else {
            Err(format!("`{s}` is not a key: keys are 1 to 32 lowercase letters, digits and inner hyphens, such as `lead-vox` or `808`"))
        }
    }

    /// `^[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?$`
    pub fn valid(s: &str) -> bool {
        let b = s.as_bytes();
        let edge = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
        (1..=32).contains(&b.len()) && edge(b[0]) && edge(b[b.len() - 1]) && b.iter().all(|&c| edge(c) || c == b'-')
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path segment made from this key: the key itself, or with a trailing `-` for a
    /// Windows device name (`aux` → `aux-`). Only `DataPath` uses this (ID-9).
    pub(crate) fn segment(&self) -> String {
        if DEVICES.contains(&self.0.as_str()) { format!("{}-", self.0) } else { self.0.clone() }
    }

    /// Whether `s` is a Windows device name, which a path spells with a hyphen after it.
    pub(crate) fn device(s: &str) -> bool {
        DEVICES.contains(&s)
    }

    /// The key a path segment spells, if it is one `segment` makes. A bare device name is not:
    /// `segment` never writes one, so the mapping stays reversible.
    pub(crate) fn from_segment(seg: &str) -> Option<Key> {
        match seg.strip_suffix('-') {
            Some(k) => DEVICES.contains(&k).then(|| Key(k.to_string())),
            None => (Key::valid(seg) && !DEVICES.contains(&seg)).then(|| Key(seg.to_string())),
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Key {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Key, D::Error> {
        Key::new(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// A lane's address: its module's key and its own (ID-6). Never a frame.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LaneAddr {
    pub module: Key,
    pub lane: Key,
}

impl LaneAddr {
    pub fn new(module: &str, lane: &str) -> Result<LaneAddr, String> {
        Ok(LaneAddr { module: Key::new(module)?, lane: Key::new(lane)? })
    }
}

/// `module/lane`, as messages and clip id derivation spell it.
impl fmt::Display for LaneAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.module, self.lane)
    }
}

/// Lowercase Crockford base32: `0-9a-z` without `i`, `l`, `o`, `u`.
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Clip ids (FMT-11, FMT-12).
pub struct ClipId;

impl ClipId {
    /// What readers accept as an id: `^[0-9a-z]{1,16}$`. Anything else is read as anonymous.
    pub fn valid(s: &str) -> bool {
        (1..=16).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    }

    /// The id a CLI, a sandboxed tool or a fix gives the `n`th clip it creates in `lane`: the
    /// first 8 Crockford characters of SHA-256(`tx_id` ‖ lane address ‖ n), so `tx_id` is the
    /// only random input. The parts are joined by NUL bytes, `n` written in decimal, so no two
    /// inputs run together.
    pub fn derive(tx_id: &str, lane: &LaneAddr, n: u32) -> String {
        let digest = Sha256::new()
            .chain_update(tx_id)
            .chain_update([0])
            .chain_update(lane.to_string())
            .chain_update([0])
            .chain_update(n.to_string());
        crockford8(&digest.finalize())
    }
}

/// The first 8 base32 characters of `bytes` (its first 40 bits, most significant first).
fn crockford8(bytes: &[u8]) -> String {
    let bits = bytes[..5].iter().fold(0u64, |acc, &b| acc << 8 | b as u64);
    (0..8).rev().map(|i| CROCKFORD[(bits >> (i * 5) & 31) as usize] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ID-1
    #[test]
    fn keys_are_lowercase_letters_digits_and_inner_hyphens() {
        for k in ["kick", "lead-vox", "808", "a", "a-b-c", "x2", &"a".repeat(32)] {
            assert!(Key::valid(k), "{k}");
        }
        for k in ["", "Kick", "lead vox", "-a", "a-", "a_b", "a.b", "ä", "song/x", &"a".repeat(33)] {
            assert!(!Key::valid(k), "{k}");
        }
        assert!(Key::new("Lead Vox").unwrap_err().contains("not a key"));
    }

    // ID-2: validated, never normalised.
    #[test]
    fn keys_are_never_normalised() {
        assert!(Key::new("Kick").is_err());
        assert_eq!(Key::new("kick").unwrap().as_str(), "kick");
        assert!(serde_json::from_str::<Key>("\"Kick\"").is_err());
    }

    // ID-9
    #[test]
    fn device_names_get_a_trailing_hyphen_and_the_mapping_reverses() {
        for k in ["aux", "con", "nul", "prn", "com1", "com9", "lpt1", "lpt9"] {
            let key = Key::new(k).unwrap();
            assert_eq!(key.segment(), format!("{k}-"));
            assert_eq!(Key::from_segment(&key.segment()), Some(key));
            assert_eq!(Key::from_segment(k), None, "a bare device name is never a segment");
        }
        for k in ["kick", "com", "com10", "lpt0", "auxiliary"] {
            let key = Key::new(k).unwrap();
            assert_eq!(key.segment(), k);
            assert_eq!(Key::from_segment(k), Some(key));
        }
        assert_eq!(Key::from_segment("kick-"), None, "no key ends in `-`");
    }

    // FMT-11
    #[test]
    fn clip_ids() {
        for id in ["k3f9q2zt", "a", "0123456789abcdef", "ilou"] {
            assert!(ClipId::valid(id), "{id}");
        }
        for id in ["", "K3F9Q2ZT", "k3f9-q2z", "0123456789abcdefg", "ü"] {
            assert!(!ClipId::valid(id), "{id}");
        }
        let lane = LaneAddr::new("bass", "notes").unwrap();
        let a = ClipId::derive("tx-7k2m9q4c", &lane, 0);
        assert_eq!(a, ClipId::derive("tx-7k2m9q4c", &lane, 0), "derived ids are reproducible");
        assert_eq!(a.len(), 8);
        assert!(a.bytes().all(|c| CROCKFORD.contains(&c)), "{a}");
        assert!(ClipId::valid(&a));
        assert_ne!(a, ClipId::derive("tx-7k2m9q4c", &lane, 1));
        assert_ne!(a, ClipId::derive("tx-other", &lane, 0));
        assert_ne!(a, ClipId::derive("tx-7k2m9q4c", &LaneAddr::new("bass", "cutoff").unwrap(), 0));
        // The first 40 bits of the digest, 5 at a time.
        assert_eq!(crockford8(&[0, 0, 0, 0, 0, 9]), "00000000");
        assert_eq!(crockford8(&[0xff, 0xff, 0xff, 0xff, 0xff]), "zzzzzzzz");
        assert_eq!(crockford8(&[0b0000_1000, 0b1000_0000, 0, 0, 0]), "12000000");
    }
}
