//! The diagnostic model (DIAG-1): every problem has a severity, a stable code, a message in
//! plain words, the file it is about, and the fixes that apply, the likeliest first.
//!
//! A fix built here is an [`Edit`]: the whole files it writes, removes or renames, each
//! with the content hash it expects. Nothing applies one by itself (DIAG-5); the session
//! applies it when asked, as a checked and undoable edit. Step 6 makes it a Tx.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;

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

    pub fn fix(mut self, fix: Fix) -> Diagnostic {
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

/// One way to fix a problem, applied only when a person or an agent asks.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fix {
    /// From the closed vocabulary: `relink`, `delete-data`, `repin-asset`, `fix-asset-name`,
    /// `retag`, `assign-clip-ids`, `renumber-clips`, …
    pub code: &'static str,
    pub title: String,
    /// The one fix the problem plainly calls for, offered in one click.
    pub likely: bool,
    pub edit: Edit,
}

/// What a fix changes: whole files, each guarded by the content it expects (`base`: the
/// SHA-256 of the file's bytes in hex, or `None` for a file that must not exist yet).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Edit {
    pub base: BTreeMap<String, Option<String>>,
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Write { file: String, text: String },
    Remove { file: String },
    Rename { from: String, to: String },
}
