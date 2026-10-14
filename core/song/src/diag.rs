//! The diagnostic model (DIAG-1): every problem has a severity, a stable code, a message in
//! plain words, the file it is about, and the fixes that apply, the likeliest first.
//!
//! A fix is a ready-to-commit [`Tx`]: its ops, the base of every file it touches, and what it
//! may change. Nothing applies one by itself (DIAG-5); applying it is committing its Tx, which
//! is checked, journaled and undone like any edit, and a stale base is a conflict.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

use crate::tx::{Base, Expect, Op, Tx, TxPath};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Notice,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// A code from the contract's table, such as `lane.invalid`.
    pub code: &'static str,
    /// What is wrong and what to do, naming the file, the field and the fix.
    pub message: String,
    /// The file it is about, relative to the project and `/`-separated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    /// The key of the module it is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    pub fn new(severity: Severity, code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic { severity, code, message: message.into(), src: None, module: None, fixes: vec![] }
    }

    pub fn error(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Error, code, message)
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Warning, code, message)
    }

    pub fn notice(code: &'static str, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Notice, code, message)
    }

    pub fn file(mut self, src: impl fmt::Display) -> Diagnostic {
        self.src = Some(src.to_string());
        self
    }

    pub fn module(mut self, key: impl fmt::Display) -> Diagnostic {
        self.module = Some(key.to_string());
        self
    }

    /// Offer `fix`. A fix built without an id gets one derived from where it was raised and its
    /// Tx (its ops and the bases it was raised on, R19), so it is the same in the app, the CLI
    /// and an agent's read of the same files, one fix offered by two problems is one Tx, and the
    /// same fix on other contents is another.
    pub fn fix(mut self, mut fix: Fix) -> Diagnostic {
        if fix.tx.id.is_empty() {
            let parts = [self.src.clone().unwrap_or_default(), fix.code.to_string(), fix.tx.hash().hex()];
            fix.tx.id = crate::tx::derived_id("fix", &parts);
        }
        self.fixes.push(fix);
        self
    }
}

/// The message and its code, after the file when the message doesn't name it already.
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(src) = self.src.as_ref().filter(|s| !self.message.contains(s.as_str())) {
            write!(f, "{src}: ")?;
        }
        write!(f, "{} [{}]", self.message, self.code)
    }
}

/// One way to fix a problem, applied only when a person or an agent asks (DIAG-1, DIAG-5).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fix {
    /// From the closed vocabulary: `relink`, `delete-data`, `repin-asset`, `fix-asset-name`,
    /// `retag`, `assign-clip-ids`, `renumber-clips`, `finish-tx`, `discard-tx`, …
    pub code: &'static str,
    pub title: String,
    /// The one fix the problem plainly calls for, offered in one click.
    pub likely: bool,
    /// The fix as a ready-to-commit Tx, bases included: applying it is committing it.
    pub tx: Tx,
}

impl Fix {
    /// A fix of `ops` on files at `base`, judged by `expect`, labelled for history as `label`.
    /// Its Tx's id is derived when it joins its problem ([`Diagnostic::fix`]).
    pub fn new(
        code: &'static str,
        title: impl Into<String>,
        likely: bool,
        label: (String, String),
        base: BTreeMap<TxPath, Base>,
        ops: Vec<Op>,
        expect: Expect,
    ) -> Fix {
        let mut tx = Tx::new("", label, ops, expect);
        tx.base = base;
        Fix { code, title: title.into(), likely, tx }
    }
}
