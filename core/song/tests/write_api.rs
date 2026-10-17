//! The write API stays step 6's (design-8b §13.1, gate 8: "each producer feature lands without
//! changing core's write API"): every op, expectation, base and declaration Tx JSON can say, matched
//! with no wildcard, so a new one fails to compile here until this test and the contract change
//! (WRITE-6: a new kind of expectation needs a design review); and one recorded Tx per op and per
//! expectation that parses and serialises back byte for byte, so no field is renamed or dropped.

use m2_song::tx::{Anchor, Base, Declared, Expect, ModuleChange, Op, Payload, Pos, Tx};

/// Every variant of the write API's enums, by name: no `_` arm, on purpose.
fn names(tx: &Tx) -> Vec<&'static str> {
    let mut out = vec![];
    for b in tx.base.values() {
        out.push(match b {
            Base::Sha(_) => "base sha",
            Base::Absent => "base absent",
            Base::WithText { .. } => "base with text",
        });
    }
    let pos = |p: &Pos| match p {
        Pos::Byte(_) => "byte",
        Pos::LineCol { .. } => "line and column",
    };
    for op in &tx.ops {
        let name = match op {
            Op::TextEdit { edits, .. } => {
                out.extend(edits.iter().map(|e| pos(&e.start)));
                "text edit"
            }
            Op::Write { bytes, .. } => match bytes {
                Payload::Text(_) => "write text",
                Payload::Bytes(_) => "write bytes",
                Payload::Local { .. } => "write local",
            },
            Op::Remove { .. } => "remove",
            Op::Rename { .. } => "rename",
            Op::Clips { .. } => "clips",
        };
        out.push(name);
    }
    let name = match &tx.expect {
        Expect::Identical => "identical",
        Expect::Values(values) => {
            for (a, _) in values {
                out.push(match a {
                    Anchor::Written { .. } => "anchor written",
                    Anchor::Slot { .. } => "anchor slot",
                });
            }
            "values"
        }
        Expect::Confined(c) => {
            for d in c.ranges.values() {
                out.push(match d {
                    Declared::Whole => "declared whole",
                    Declared::Ranges(_) => "declared ranges",
                });
            }
            for m in &c.modules {
                out.push(match m {
                    ModuleChange::Added(_) => "added",
                    ModuleChange::Removed(_) => "removed",
                    ModuleChange::Rekeyed { .. } => "rekeyed",
                    ModuleChange::Muted { .. } => "muted",
                });
            }
            "confined"
        }
        Expect::Seen(_) => "seen",
        Expect::Journal(_) => "journal",
    };
    out.push(name);
    out
}

/// One Tx per op and per expectation, as recorded at step 6 (compact JSON, keys sorted).
const RECORDED: &[(&str, &str)] = &[
    (
        "text_edit",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","data/song/hats-pan.json":null,"song.ts":{"sha":"sha256:abababababababababababababababababababababababababababababababab","text":"x"}},"expect":"identical","hint":7,"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"edits":[{"end":4,"expected":"x","start":3,"text":"y"},{"end":{"line":0,"utf16_col":1},"expected":"x","start":{"line":0,"utf16_col":0},"text":"z"}],"file":"song.ts","op":"text_edit"}]}"#,
    ),
    (
        "write text",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","data/song/hats-pan.json":null,"song.ts":{"sha":"sha256:abababababababababababababababababababababababababababababababab","text":"x"}},"expect":"identical","id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"file":"data/song/hats-pan.json","op":"write","text":"{\"clips\": []}\n"}]}"#,
    ),
    (
        "write bytes",
        r#"{"base":{"data/lead/lead.state":null},"expect":"identical","id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"base64":"AAEC/w==","file":"data/lead/lead.state","op":"write"}]}"#,
    ),
    (
        "write local",
        r#"{"base":{"data/lead/lead.state":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"},"expect":{"journal":"1760351234567-4121-3"},"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"file":"data/lead/lead.state","local":{"path":".music2/tx/blobs/ab","sha":"sha256:abababababababababababababababababababababababababababababababab"},"op":"write"}]}"#,
    ),
    (
        "remove",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"},"expect":"identical","id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"file":"data/kick/hits.json","op":"remove"}]}"#,
    ),
    (
        "rename",
        r#"{"base":{"data/hat":"sha256:abababababababababababababababababababababababababababababababab","data/hats":null},"expect":{"confined":{"lanes":[["hats","hits"]],"modules":[],"ranges":{}}},"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"from":"data/hat","op":"rename","to":"data/hats"}]}"#,
    ),
    (
        "clips",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"},"expect":{"confined":{"lanes":[["kick","hits"]],"modules":[],"ranges":{}}},"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"delete":["q7m2c8xa"],"lane":["kick","hits"],"op":"clips","put":[{"id":"k3f9q2zt","length":16,"notes":[],"start":0}]}]}"#,
    ),
    (
        "values",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","data/song/hats-pan.json":null,"song.ts":{"sha":"sha256:abababababababababababababababababababababababababababababababab","text":"x"}},"expect":{"values":[[{"written":{"file":"song.ts","range":[1,2]}},0.45],[{"slot":{"site":{"file":"song.ts","range":[0,9]},"slot":"q"}},0.7]]},"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"edits":[{"end":4,"expected":"x","start":3,"text":"y"},{"end":{"line":0,"utf16_col":1},"expected":"x","start":{"line":0,"utf16_col":0},"text":"z"}],"file":"song.ts","op":"text_edit"}]}"#,
    ),
    (
        "confined",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","data/song/hats-pan.json":null,"song.ts":{"sha":"sha256:abababababababababababababababababababababababababababababababab","text":"x"}},"expect":{"confined":{"lanes":[["pluck","notes"]],"modules":[{"added":"pluck"},{"removed":"kick"},{"rekeyed":["bass","low"]},{"muted":["hats",true]}],"ranges":{"song.ts":[[3,4],[{"line":0,"utf16_col":0},{"line":0,"utf16_col":1}]],"tracks/pluck.ts":"whole"}}},"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"edits":[{"end":4,"expected":"x","start":3,"text":"y"},{"end":{"line":0,"utf16_col":1},"expected":"x","start":{"line":0,"utf16_col":0},"text":"z"}],"file":"song.ts","op":"text_edit"}]}"#,
    ),
    (
        "seen",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","data/song/hats-pan.json":null,"song.ts":{"sha":"sha256:abababababababababababababababababababababababababababababababab","text":"x"}},"expect":{"seen":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"},"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"edits":[{"end":4,"expected":"x","start":3,"text":"y"},{"end":{"line":0,"utf16_col":1},"expected":"x","start":{"line":0,"utf16_col":0},"text":"z"}],"file":"song.ts","op":"text_edit"}]}"#,
    ),
    (
        "journal",
        r#"{"base":{"data/kick/hits.json":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd","data/song/hats-pan.json":null,"song.ts":{"sha":"sha256:abababababababababababababababababababababababababababababababab","text":"x"}},"expect":{"journal":"1760351234567-4121-3"},"id":"tx-7k2m9q4c","label":["before","after"],"ops":[{"edits":[{"end":4,"expected":"x","start":3,"text":"y"},{"end":{"line":0,"utf16_col":1},"expected":"x","start":{"line":0,"utf16_col":0},"text":"z"}],"file":"song.ts","op":"text_edit"}]}"#,
    ),
];

// Gate 8, design-8b §13.1: the five ops, the five expectations, the three bases, both
// declarations, the four module changes, both anchors and both positions parse, and serialise
// back to the same bytes.
#[test]
fn the_write_api_is_step_6s() {
    let mut seen = std::collections::BTreeSet::new();
    for (what, json) in RECORDED {
        let v: serde_json::Value = serde_json::from_str(json).unwrap_or_else(|e| panic!("{what}: {e}"));
        let tx = Tx::from_json(&v).unwrap_or_else(|d| panic!("{what}: {d:?}"));
        assert_eq!(serde_json::to_string(&tx.to_json()).unwrap(), *json, "{what} serialises back byte for byte");
        seen.extend(names(&tx));
    }
    let every = [
        "base sha",
        "base absent",
        "base with text",
        "byte",
        "line and column",
        "text edit",
        "write text",
        "write bytes",
        "write local",
        "remove",
        "rename",
        "clips",
        "identical",
        "values",
        "anchor written",
        "anchor slot",
        "confined",
        "declared whole",
        "declared ranges",
        "added",
        "removed",
        "rekeyed",
        "muted",
        "seen",
        "journal",
    ];
    for name in every {
        assert!(seen.contains(name), "no recorded Tx says {name}");
    }
    assert_eq!(seen.len(), every.len(), "{seen:?}");
}
