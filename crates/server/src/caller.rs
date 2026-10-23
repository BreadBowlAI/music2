//! Who is asking (AGENT-10): the page, by its own key, or an agent, by the tools key that
//! `~/.music2/session.json` publishes, named by what it says it is. The server's `auth` decides it
//! by the key alone, and serves the request as that caller: every job it starts on the session's
//! actor carries it ([`crate::graph::Graph`]), so a step's source and the refusals of AGENT-3 and
//! AGENT-4 go by the key, never by what a request says about itself.

use std::cell::RefCell;
use std::future::Future;

/// The longest an agent's words are shown (AGENT-10): its name, why it made a change.
pub const WORDS: usize = 80;

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
        let name = name.map(words).filter(|n| !n.is_empty()).unwrap_or_else(|| "agent".into());
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

/// Plain text an agent gives to be shown (AGENT-10): no control characters, spaces as they are
/// at either end trimmed, and at most [`WORDS`] characters, the last of them "…" when it is cut.
pub fn words(s: &str) -> String {
    let plain: String = s.chars().filter(|c| !c.is_control()).collect();
    let plain = plain.trim();
    if plain.chars().count() <= WORDS {
        return plain.to_string();
    }
    let mut cut: String = plain.chars().take(WORDS - 1).collect();
    cut.push('…');
    cut
}

tokio::task_local! {
    /// The caller of the request this task serves.
    static ASKING: Caller;
}

thread_local! {
    /// The caller of the work this thread does now: a route's blocking work, or a test's.
    static ON_THREAD: RefCell<Option<Caller>> = const { RefCell::new(None) };
}

/// Who asks for what is served now: the request's caller on its task, or on the thread its
/// blocking work runs on; else the page (the app's own work).
pub fn now() -> Caller {
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
    let caller = now();
    tokio::task::spawn_blocking(move || as_caller(caller, f)).await
}

/// Serve `fut` as `caller`: what `auth` does with each request.
pub async fn serving<F: Future>(caller: Caller, fut: F) -> F::Output {
    ASKING.scope(caller, fut).await
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
}
