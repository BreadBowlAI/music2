//! The session's store of recent texts, by content hash (WRITE-12): where a Tx's base text is
//! found when the Tx carries only its hash. It is only a cache, bounded and evicted oldest first;
//! a Tx that carries its base texts (`WithText`) has the same outcome in every process.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use m2_song::Sha;

/// At most this much text is kept.
const BYTES: usize = 32 << 20;
/// And at most this many texts.
const TEXTS: usize = 8192;

#[derive(Default)]
pub struct Texts {
    by: HashMap<Sha, Arc<str>>,
    order: VecDeque<Sha>,
    bytes: usize,
}

impl Texts {
    /// Keep `text`, the content of a text file whose bytes are exactly it.
    pub fn put(&mut self, text: &str) -> Sha {
        self.put_hashed(Sha::of(text.as_bytes()), text)
    }

    /// [`Texts::put`] for a text whose hash is known already: `sha` is `text`'s.
    pub fn put_hashed(&mut self, sha: Sha, text: &str) -> Sha {
        self.keep(sha, || text.into())
    }

    /// [`Texts::put`] of a text a snapshot shares: kept as that same text, hashed once
    /// ([`Sha::of_shared`]).
    pub fn put_shared(&mut self, text: &Arc<str>) -> Sha {
        self.keep(Sha::of_shared(text), || text.clone())
    }

    fn keep(&mut self, sha: Sha, text: impl FnOnce() -> Arc<str>) -> Sha {
        if self.by.contains_key(&sha) {
            // Used again: it goes last.
            if let Some(i) = self.order.iter().position(|s| *s == sha) {
                self.order.remove(i);
            }
            self.order.push_back(sha);
            return sha;
        }
        let text = text();
        debug_assert!(Sha::of(text.as_bytes()) == sha, "the hash given is the text's");
        self.bytes += text.len();
        self.by.insert(sha, text);
        self.order.push_back(sha);
        while (self.bytes > BYTES || self.order.len() > TEXTS)
            && let Some(old) = self.order.pop_front()
        {
            if let Some(t) = self.by.remove(&old) {
                self.bytes -= t.len();
            }
        }
        sha
    }

    pub fn get(&self, sha: &Sha) -> Option<Arc<str>> {
        self.by.get(sha).cloned()
    }

    pub fn len(&self) -> usize {
        self.by.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_found_by_hash_and_evicted_oldest_first() {
        let mut t = Texts::default();
        let a = t.put("a");
        assert_eq!(t.get(&a).as_deref(), Some("a"));
        assert_eq!(t.get(&Sha::of(b"b")), None);
        for i in 0..TEXTS {
            t.put(&i.to_string());
        }
        assert_eq!(t.get(&a), None, "the oldest went first");
        assert_eq!(t.len(), TEXTS);
    }
}
