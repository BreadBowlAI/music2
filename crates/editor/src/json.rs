//! A Tx's outcomes as JSON (WRITE-3, WRITE-5, WRITE-11): the one shape the server's
//! `/api/tx/*`, `m2 tx` and later MCP answer with. The Tx itself is core's JSON
//! (`m2_song::Tx::to_json`).
//!
//! - `check`: `{"checked": {"delta": …, "hash": "sha256:…", "diagnostics": […]}}`; an agent
//!   commits with `"expect": {"seen": hash}`.
//! - `commit`: `{"committed": {"revision", "code_rev", "data_rev", "delta", "hash", "diagnostics",
//!   "patches"}}`, or a refusal: `{"conflict": {"files": [{"file", "current", "clip", "why"}]}}`,
//!   `{"rejected": {"diagnostics": […], "unexpected": delta | null}}`, `{"io": {"message"}}`.

use m2_session::{Checked, Committed, Refusal};
use serde_json::{Value, json};

/// What `check` found.
pub fn checked(c: &Checked) -> Value {
    json!({"checked": {"delta": c.delta.to_json(), "hash": c.hash.prefixed(), "diagnostics": c.diagnostics}})
}

/// What a commit made.
pub fn committed(c: &Committed) -> Value {
    let patches: Vec<Value> = c
        .patches
        .iter()
        .map(|p| json!({"file": p.file, "start": p.start, "end": p.end, "text": p.text, "from": [p.from.0, p.from.1], "to": [p.to.0, p.to.1]}))
        .collect();
    json!({"committed": {
        "revision": c.number,
        "code_rev": c.code_rev,
        "data_rev": c.data_rev,
        "delta": c.delta.to_json(),
        "hash": c.hash.prefixed(),
        "diagnostics": c.diagnostics,
        "patches": patches,
    }})
}

/// Why a Tx wasn't committed.
pub fn refusal(r: &Refusal) -> Value {
    match r {
        Refusal::Conflict { files } => {
            let files: Vec<Value> = files
                .iter()
                .map(|f| json!({"file": f.file, "current": f.current.map(|s| s.prefixed()), "clip": f.clip, "why": f.why}))
                .collect();
            json!({"conflict": {"files": files, "message": r.to_string()}})
        }
        Refusal::Rejected { diagnostics, unexpected } => {
            json!({"rejected": {"diagnostics": diagnostics, "unexpected": unexpected.as_ref().map(|d| d.to_json())}})
        }
        Refusal::Io { message } => json!({"io": {"message": message}}),
    }
}
