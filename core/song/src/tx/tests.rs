use serde_json::json;

use super::*;

fn p(s: &str) -> TxPath {
    TxPath::new(s).unwrap_or_else(|e| panic!("{e}"))
}

fn sha(s: &str) -> Sha {
    Sha::of(s.as_bytes())
}

/// WRITE-3's example: automate the hats pan.
fn automate() -> Value {
    json!({"id": "tx-7k2m9q4c",
           "base": {"song.ts": sha("song").prefixed(), "data/song/hats-pan.json": null},
           "label": ["hats pan 0.3", "hats pan automated"],
           "ops": [{"op": "text_edit", "file": "song.ts",
                    "edits": [{"start": 327, "end": 329, "expected": "()", "text": "({ lane })"},
                              {"start": {"line": 14, "utf16_col": 37}, "end": {"line": 14, "utf16_col": 40}, "expected": "0.3",
                               "text": "lane.control(\"hats-pan\", \"bipolar\", 0.3)"}]},
                   {"op": "write", "file": "data/song/hats-pan.json", "text": "{\"unit\": \"bipolar\", \"clips\": []}\n"}],
           "expect": {"confined": {"ranges": {"song.ts": [[327, 329], [422, 425]]}, "lanes": [["song", "hats-pan"]], "modules": []}}})
}

fn codes(e: &[Diagnostic]) -> Vec<&'static str> {
    e.iter().map(|d| d.code).collect()
}

// WRITE-3: the JSON reads and writes back the same, every op and expectation included.
#[test]
fn the_json_round_trips() {
    let tx = Tx::from_json(&automate()).unwrap();
    assert_eq!(tx.validate(), Ok(()));
    assert_eq!(Tx::from_json(&tx.to_json()).unwrap(), tx);
    let Op::TextEdit { edits, .. } = &tx.ops[0] else { panic!() };
    assert_eq!(edits[1].start, Pos::LineCol { line: 14, utf16_col: 37 });

    let every = json!({"id": "x", "hint": 4,
        "base": {"data/lead/lead.state": {"sha": sha("old").prefixed(), "text": "old"}, "data/kick/old.json": sha("o").prefixed(),
                 "data/hat": sha("d").prefixed(), "data/hats": null, "data/kick/hits.json": sha("h").prefixed(),
                 "data/lead/lead.state.json": null},
        "ops": [{"op": "write", "file": "data/lead/lead.state", "base64": "AAEC/w=="},
                {"op": "write", "file": "data/lead/lead.state.json", "local": {"path": ".music2/tx/blobs/ab", "sha": sha("x").prefixed()}},
                {"op": "remove", "file": "data/kick/old.json"},
                {"op": "rename", "from": "data/hat", "to": "data/hats"},
                {"op": "clips", "lane": ["kick", "hits"], "put": [{"id": "k3f9q2zt", "start": 0, "length": 16, "notes": []}],
                 "delete": ["q7m2c8xa"]}],
        "expect": {"values": [[{"written": {"file": "data/kick/hits.json", "range": [1, 2]}}, 0.45],
                              [{"slot": {"site": {"file": "data/kick/hits.json", "range": [0, 9]}, "slot": "q"}}, 0.7]]}});
    let tx = Tx::from_json(&every).unwrap();
    assert_eq!(tx.validate(), Ok(()));
    let Op::Write { bytes: Payload::Bytes(b), .. } = &tx.ops[0] else { panic!() };
    assert_eq!(&**b, &[0, 1, 2, 255]);
    assert_eq!(Tx::from_json(&tx.to_json()).unwrap(), tx);
    assert_eq!(tx.hint, Some(4));
    for e in [
        json!("identical"),
        json!({"seen": sha("d").prefixed()}),
        json!({"journal": "1760351234567-4121-3"}),
        json!({"confined": {"ranges": {"song.ts": "whole"},
                            "modules": [{"added": "probe"}, {"rekeyed": ["kick", "kicks"]}, {"muted": ["kick", true]}]}}),
    ] {
        let mut v = automate();
        v["expect"] = e.clone();
        let tx = Tx::from_json(&v).unwrap_or_else(|d| panic!("{e}: {d:?}"));
        assert_eq!(Tx::from_json(&tx.to_json()).unwrap(), tx, "{e}");
    }
    // The canonical form hashes the same whatever the key order or spacing.
    let spaced: Value = serde_json::from_str(&serde_json::to_string_pretty(&automate()).unwrap()).unwrap();
    assert_eq!(Tx::from_json(&spaced).unwrap().hash(), Tx::from_json(&automate()).unwrap().hash());
}

// FMT-8 applied to Tx: an unknown field, two payloads, a bad hash or a bad position is
// `tx.invalid`, never a panic and never dropped.
#[test]
fn malformed_json_is_refused() {
    let with = |f: &dyn Fn(&mut Value)| {
        let mut v = automate();
        f(&mut v);
        Tx::from_json(&v).map(|_| ()).map_err(|e| codes(&e))
    };
    let invalid = Err(vec!["tx.invalid"]);
    assert_eq!(with(&|v| v["colour"] = json!("red")), invalid);
    assert_eq!(with(&|v| v["ops"][1]["base64"] = json!("AA==")), invalid, "two payloads");
    assert_eq!(with(&|v| v["ops"][1]["text"] = json!(1)), invalid);
    assert_eq!(with(&|v| v["ops"][0]["edits"][0]["start"] = json!(-1)), invalid);
    assert_eq!(with(&|v| v["ops"][0]["edits"][0]["start"] = json!(1.5)), invalid);
    assert_eq!(with(&|v| v["ops"][0]["edits"][0]["start"] = json!({"line": 1})), invalid);
    assert_eq!(with(&|v| v["ops"][0]["edits"][0]["colour"] = json!(1)), invalid);
    assert_eq!(with(&|v| v["base"]["song.ts"] = json!("sha256:XYZ")), invalid);
    assert_eq!(with(&|v| v["base"]["song.ts"] = json!(sha("x").hex())), invalid, "the prefix is required");
    assert_eq!(with(&|v| v["ops"][0]["op"] = json!("patch")), invalid);
    assert_eq!(with(&|v| v["expect"] = json!("anything")), invalid);
    assert_eq!(with(&|v| v["expect"] = json!({"seen": "x", "journal": "y"})), invalid);
    assert_eq!(with(&|v| v["expect"]["confined"]["ranges"]["song.ts"] = json!("all")), invalid);
    assert_eq!(with(&|v| v["ops"][1]["base64"] = json!("AAA")), Err(vec!["tx.invalid"]));
    assert_eq!(with(&|v| v["label"] = json!("one")), invalid);
    assert!(with(&|v| v.as_object_mut().unwrap().remove("label").map(drop).unwrap()).is_ok(), "label is optional");
    assert!(serde_json::from_value::<Tx>(json!({"id": "x"})).is_err());
}

// WRITE-3a: every path is relative, `/`-separated, plainly spelled, and one a Tx may write.
#[test]
fn hostile_paths_are_refused() {
    for bad in [
        "",
        "../song.ts",
        "/abs/song.ts",
        "C:\\song.ts",
        "a\\b.ts",
        "data//kick/hits.json",
        "./song.ts",
        "song.ts/",
        ".",
        ".music2/lock",
        ".music2/tx/x/song.ts",
        "node_modules/x.ts",
        "data/aux/x.json",
        "song\0.ts",
        "song\u{202e}st.ts",
        "so\u{200b}ng.ts",
        "song.js",
        "README.md",
        "data/Kick/hits.json",
        "assets/a/../b.wav",
        "assets/.hidden.wav",
        "lib/.x/y.ts",
    ] {
        let e = TxPath::new(bad).err().unwrap_or_else(|| panic!("{bad:?} was admitted"));
        assert!(e.contains("can't be named by a Tx"), "{bad:?}: {e}");
        let mut v = automate();
        v["base"][bad] = json!(null);
        assert_eq!(Tx::from_json(&v).map(|_| ()).map_err(|e| codes(&e)), Err(vec!["tx.path"]), "{bad:?}");
    }
    for good in [
        "song.ts",
        "music2.toml",
        "tracks/bass.ts",
        "data/arrangement.json",
        "data/kick",
        "data/aux-/x.json",
        "data/kick/hits.json",
        "data/vox/chain-l/tune.state",
        "data/vox/chain-l/tune.state.json",
        "assets/vox/lead.wav",
        "layout.json",
    ] {
        assert_eq!(p(good).as_str(), good);
    }
    assert!(valid_local(".music2/tx/blobs/ab"));
    for bad in [".music2/tx/", ".music2/lock", "song.ts", ".music2/tx/../lock", ".music2/tx//x", "/.music2/tx/x"] {
        assert!(!valid_local(bad), "{bad}");
    }
}

/// A Tx with `ops`, each touched path given a base, and `expect`.
fn tx(ops: Vec<Op>, expect: Expect) -> Tx {
    let mut t = Tx::new("tx-1", (String::new(), String::new()), ops, expect);
    for p in t.touched() {
        t.base.insert(p, Base::Sha(sha("x")));
    }
    t
}

fn messages(t: &Tx) -> Vec<String> {
    t.validate().err().unwrap_or_default().into_iter().map(|d| format!("{}: {}", d.code, d.message)).collect()
}

// WRITE-3, WRITE-3a: the shape rules that need no disk.
#[test]
fn the_shape_rules() {
    let write = |f: &str| Op::Write { file: p(f), bytes: Payload::Text("x".into()) };
    let ok = tx(vec![write("song.ts")], Expect::Identical);
    assert_eq!(messages(&ok), Vec::<String>::new());

    let mut t = ok.clone();
    t.id = "Tx 1".into();
    assert!(messages(&t)[0].contains("isn't a Tx id"));
    let mut t = ok.clone();
    t.base.clear();
    assert!(messages(&t)[0].contains("has no base"));
    let mut t = ok.clone();
    t.base.insert(p("tracks/bass.ts"), Base::Absent);
    assert!(messages(&t)[0].contains("no op touches it"));

    // R15: no two op paths overlap.
    let rename = |a: &str, b: &str| Op::Rename { from: p(a), to: p(b) };
    for ops in [
        vec![rename("data/kick", "data/kicks"), write("data/kick/hits.json")],
        vec![rename("data/kick", "data/kicks"), Op::Remove { file: p("data/kicks/hits.json") }],
        vec![Op::Clips { lane: LaneAddr::new("kick", "hits").unwrap(), put: vec![], delete: vec![] }, write("data/kick/hits.json")],
        vec![write("song.ts"), write("song.ts")],
    ] {
        let mut t = tx(ops, Expect::Identical);
        for b in t.base.values_mut() {
            *b = Base::Absent;
        }
        assert!(messages(&t).iter().any(|m| m.contains("overlap")), "{:?}", messages(&t));
    }

    // A rename keeps a path's kind, into an absent name.
    let mut t = tx(vec![rename("data/kick/hits.json", "data/kick/a.state")], Expect::Identical);
    t.base.insert(p("data/kick/a.state"), Base::Absent);
    assert!(messages(&t)[0].contains("keeps a file or folder what it is"));
    let t = tx(vec![rename("data/kick", "data/kicks")], Expect::Identical);
    assert!(messages(&t)[0].contains("must be absent"));
    let t = tx(vec![Op::Remove { file: p("data/kick") }], Expect::Identical);
    assert!(messages(&t)[0].contains("only a `rename` moves one"));

    // FMT-14: assets are created, never overwritten.
    let t = tx(vec![write("assets/kick.wav")], Expect::Identical);
    assert!(messages(&t)[0].contains("never overwritten"));

    // FMT-11: a put clip has an id the builder chose; ids don't repeat or clash.
    let lane = LaneAddr::new("kick", "hits").unwrap();
    let clips = |put: Vec<Value>, delete: Vec<&str>| {
        tx(vec![Op::Clips { lane: lane.clone(), put, delete: delete.into_iter().map(str::to_string).collect() }], Expect::Identical)
    };
    assert!(messages(&clips(vec![json!({"start": 0})], vec![]))[0].contains("has no valid id"));
    assert!(messages(&clips(vec![json!({"id": "A"})], vec![]))[0].contains("has no valid id"));
    assert!(messages(&clips(vec![json!({"id": "a"}), json!({"id": "a"})], vec![]))[0].contains("twice"));
    assert!(messages(&clips(vec![json!({"id": "a"})], vec!["a"]))[0].contains("both puts and deletes"));
    assert!(messages(&clips(vec![], vec!["A B"]))[0].contains("isn't a clip id"));
    assert_eq!(messages(&clips(vec![json!({"id": "a"})], vec!["b"])), Vec::<String>::new());

    // Values are finite and anchored in touched files.
    let anchor = |f: &str| Anchor::Written { file: p(f), range: Range::bytes(0, 1) };
    let t = tx(vec![write("song.ts")], Expect::Values(vec![(anchor("song.ts"), f64::NAN)]));
    assert!(messages(&t)[0].contains("isn't a number"));
    let t = tx(vec![write("song.ts")], Expect::Values(vec![(anchor("tracks/bass.ts"), 1.0)]));
    assert!(messages(&t)[0].contains("doesn't touch"));

    // R23: `[]` is no bytes, and a whole-file op on code says "whole".
    let confined = |d: Declared| {
        let mut c = Confined::default();
        c.ranges.insert(p("song.ts"), d);
        Expect::Confined(c)
    };
    assert!(messages(&tx(vec![write("song.ts")], confined(Declared::Ranges(vec![]))))[0].contains("declares no bytes"));
    assert!(messages(&tx(vec![write("song.ts")], Expect::Confined(Confined::default())))[0].contains("declares it \"whole\""));
    assert_eq!(messages(&tx(vec![write("song.ts")], confined(Declared::Whole))), Vec::<String>::new());
    assert!(messages(&tx(vec![write("song.ts")], Expect::Journal("x".into())))[0].contains("isn't a journal's name"));
}

#[test]
fn base64_round_trips() {
    for bytes in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 128, 7, 9]] {
        assert_eq!(unbase64(&base64(bytes)).as_deref(), Some(bytes));
    }
    assert_eq!(base64(b"Man"), "TWFu");
    assert_eq!(base64(b"Ma"), "TWE=");
    for bad in ["TWE", "TW==x===", "T===", "TW=E", "@@@@"] {
        assert_eq!(unbase64(bad), None, "{bad}");
    }
}
