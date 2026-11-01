//! Who is asking (AGENT-10): the page, by its own key, or an agent, by the tools key that
//! `~/.music2/session.json` publishes, named by what it says it is. The server's `auth` decides it
//! by the key alone, and serves the request as that caller: every job it starts on the session's
//! actor carries it ([`crate::graph::Graph`]), so a step's source and the refusals of AGENT-3 and
//! AGENT-4 go by the key, never by what a request says about itself.

use std::cell::RefCell;
use std::future::Future;

/// The longest an agent's words are shown (AGENT-10): its name, why it made a change.
pub const WORDS: usize = 80;

/// The name of the session's actor's thread, where [`now`] is never read.
pub(crate) const ACTOR: &str = "m2-session";

/// Who sent a request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Caller {
    /// The app's page (and the app's own work: its watcher, its timers).
    #[default]
    Page,
    /// An agent, by the name it gives (`agent` when it gives none), and why it made its change.
    Agent { name: String, why: Option<String> },
}

impl Caller {
    /// An agent that calls itself `name` (else `agent`), saying `why`: both as plain text, at most
    /// [`WORDS`] characters.
    pub fn agent(name: Option<&str>, why: Option<&str>) -> Caller {
        let name = name.map(words).filter(|n| !n.is_empty() && !the_apps(n)).unwrap_or_else(|| "agent".into());
        let why = why.map(words).filter(|w| !w.is_empty());
        Caller::Agent { name, why }
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, Caller::Agent { .. })
    }

    /// This caller, with the name and words a request's body gives, when it is an agent's: the
    /// body says them for `POST /api/tx/commit` (`as`, `why`), else the headers did.
    pub fn saying(self, name: Option<&str>, why: Option<&str>) -> Caller {
        match self {
            Caller::Agent { name: was, why: said } => Caller::agent(Some(name.unwrap_or(&was)), why.or(said.as_deref())),
            page => page,
        }
    }

    /// Who the steps it makes are by (AGENT-2).
    pub fn source(&self) -> m2_session::Source {
        match self {
            Caller::Page => m2_session::Source::Page,
            Caller::Agent { name, .. } => m2_session::Source::Agent(name.clone()),
        }
    }

    /// Why it made its change, as its step is named.
    pub fn why(&self) -> Option<String> {
        match self {
            Caller::Agent { why, .. } => why.clone(),
            Caller::Page => None,
        }
    }
}

/// Plain text an agent gives to be shown (AGENT-10): no control or invisible characters (bidi
/// controls, zero-width marks: what a Tx's path may not hold either), a line break or a tab read as
/// a space (one space between words), no spaces at either end, and at most [`WORDS`] characters,
/// the last of them "…" when it is cut.
pub fn words(s: &str) -> String {
    let spaced: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).filter(|c| !m2_song::tx::hidden(*c)).collect();
    let plain = spaced.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let plain = plain.trim();
    if plain.chars().count() <= WORDS {
        return plain.to_string();
    }
    let mut cut: String = plain.chars().take(WORDS - 1).collect();
    cut.push('…');
    cut
}

/// An agent's words as a header carries them (AGENT-10): UTF-8, percent-encoded, so any name fits
/// in one, whatever its letters. `m2` sends `x-m2-as` and `x-m2-why` so, and a GET's keys in its
/// query, which is encoded the same way.
pub fn encoded(words: &str) -> String {
    words
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// A header's words (AGENT-10): each `%XX` the byte it names, the rest as it is, read as UTF-8. A
/// plain name sent as it is reads the same.
pub fn decoded(said: &str) -> String {
    let (b, mut out, mut i) = (said.as_bytes(), vec![], 0);
    let hex = |c: Option<&u8>| c.and_then(|&c| (c as char).to_digit(16));
    while i < b.len() {
        match (b[i], hex(b.get(i + 1)), hex(b.get(i + 2))) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether an agent's name reads as the app's own words for a change it didn't make: the notice's
/// "Changed outside the app", the Edit menu's "outside change" (AGENT-10). Such a name is `agent`,
/// so no agent's change passes for an outside one.
fn the_apps(name: &str) -> bool {
    let letters: String = name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    letters.starts_with("changedoutside") || letters.starts_with("outsidechange")
}

tokio::task_local! {
    /// The caller of the request this task serves.
    static ASKING: Caller;
    /// The page's window it came from (`x-m2-window`), if the page said (AGENT-2).
    static WINDOW: Option<String>;
}

thread_local! {
    /// The caller of the work this thread does now: a route's blocking work, or a test's.
    static ON_THREAD: RefCell<Option<Caller>> = const { RefCell::new(None) };
    /// The window of the work this thread does now, as [`window`] says it.
    static WINDOW_ON_THREAD: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The page's window the request served now came from (AGENT-2): none for an agent, or the app.
pub fn window() -> Option<String> {
    if let Some(w) = WINDOW_ON_THREAD.with(|w| w.borrow().clone()) {
        return Some(w);
    }
    WINDOW.try_with(Option::clone).ok().flatten()
}

/// Who asks for what is served now: the request's caller on its task, or on the thread its
/// blocking work runs on; else the page (the app's own work). Read where a request is served, never
/// on the session's actor, whose jobs are given their caller ([`crate::graph::Graph`]'s `call`).
pub fn now() -> Caller {
    debug_assert_ne!(std::thread::current().name(), Some(ACTOR), "the session's actor is given its job's caller; it never reads one");
    if let Some(c) = ON_THREAD.with(|c| c.borrow().clone()) {
        return c;
    }
    ASKING.try_with(Caller::clone).unwrap_or_default()
}

/// Run `f` on this thread as `caller`.
pub fn as_caller<T>(caller: Caller, f: impl FnOnce() -> T) -> T {
    let was = ON_THREAD.with(|c| c.replace(Some(caller)));
    struct Back(Option<Caller>);
    impl Drop for Back {
        fn drop(&mut self) {
            let was = self.0.take();
            ON_THREAD.with(|c| *c.borrow_mut() = was);
        }
    }
    let _back = Back(was);
    f()
}

/// Run `f` off the async threads, as the request's caller.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, tokio::task::JoinError> {
    let (caller, window) = (now(), window());
    tokio::task::spawn_blocking(move || {
        let was = WINDOW_ON_THREAD.with(|w| w.replace(window));
        let out = as_caller(caller, f);
        WINDOW_ON_THREAD.with(|w| *w.borrow_mut() = was);
        out
    })
    .await
}

/// Serve `fut` as `caller`: what `auth` does with each request.
pub async fn serving<F: Future>(caller: Caller, fut: F) -> F::Output {
    serving_in(caller, None, fut).await
}

/// [`serving`], from the page's window `window` (AGENT-2).
pub async fn serving_in<F: Future>(caller: Caller, window: Option<String>, fut: F) -> F::Output {
    ASKING.scope(caller, WINDOW.scope(window, fut)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // AGENT-10: an agent's words are plain text, at most 80 characters.
    #[test]
    fn an_agents_words_are_plain_and_short() {
        assert_eq!(words("  brighter\u{7} hats\n"), "brighter hats");
        let long = words(&"x".repeat(100));
        assert_eq!((long.chars().count(), long.ends_with('…')), (80, true));
        assert_eq!(Caller::agent(None, Some("")), Caller::Agent { name: "agent".into(), why: None });
        let named = Caller::agent(Some("Claude"), None).saying(None, Some("warmer"));
        assert_eq!(named, Caller::Agent { name: "Claude".into(), why: Some("warmer".into()) });
        assert_eq!(Caller::Page.saying(Some("Claude"), Some("x")), Caller::Page, "the page is the page");
        assert_eq!(as_caller(Caller::agent(Some("A"), None), now).source(), m2_session::Source::Agent("A".into()));
        assert_eq!(now(), Caller::Page);
    }

    // AGENT-10 (the review's F7): invisible characters go too (bidi controls, zero-width marks), and
    // a name that reads as the app's own words for a change made outside it is `agent`.
    #[test]
    fn an_agents_words_hide_nothing_and_its_name_is_its_own() {
        assert_eq!(words("the code \u{202E}stah\u{200B} \u{2066}x\u{2069}\u{feff}"), "the code stah x");
        let name = |n: &str| match Caller::agent(Some(n), None) {
            Caller::Agent { name, .. } => name,
            Caller::Page => unreachable!(),
        };
        for fake in ["Changed outside the app", "changed outside the app: song.ts", "Outside change", "outside\u{200B} change"] {
            assert_eq!(name(fake), "agent", "{fake:?}");
        }
        assert_eq!(name("Outsider"), "Outsider");
        assert_eq!(name("Claude"), "Claude");
    }

    // AGENT-10 (track 4): words cross a header whole, percent-encoded, whatever their letters; a
    // plain name sent as it is reads the same, and a stray `%` is kept.
    #[test]
    fn an_agents_words_cross_a_header_whole() {
        for words in ["Clåude", "mark the drop: 100% «louder» 🎹", "Claude 2", ""] {
            let sent = encoded(words);
            assert!(sent.bytes().all(|b| b.is_ascii_graphic()), "{sent}");
            assert_eq!(decoded(&sent), words);
        }
        assert_eq!(decoded("Codex"), "Codex");
        assert_eq!(decoded("100% sure %4"), "100% sure %4");
    }

    // AGENT-10 (the review of ag-routes' F9): a line break or a tab in an agent's words reads as a
    // space, so what it sent on two lines doesn't run together.
    #[test]
    fn a_line_break_in_an_agents_words_reads_as_a_space() {
        assert_eq!(words("Evil\r\nx-m2-token: nope\r\nX: y"), "Evil x-m2-token: nope X: y");
        assert_eq!(words("warmer\tkeys"), "warmer keys");
        assert_eq!(Caller::agent(Some("Claude\n2"), None), Caller::Agent { name: "Claude 2".into(), why: None });
    }
}
