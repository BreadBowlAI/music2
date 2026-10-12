use std::time::Duration;

use m2_ir::Op;

use super::*;

/// A project's code, as the snapshot gives it: paths in the project to texts.
fn code(files: &[(&str, &str)]) -> BTreeMap<String, String> {
    files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
}

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

fn options(files: &[(&str, &str)]) -> Options {
    let mut o = Options::new(code(files), "song.ts");
    o.packages.push(("std".into(), std_dir()));
    o.dependencies.push("std".into());
    o.time_limit = Duration::from_secs(2);
    o
}

fn build_of(files: &[(&str, &str)]) -> Result<Ir, Vec<Diagnostic>> {
    build(&options(files))
}

/// The one diagnostic a build fails with.
fn error_of(files: &[(&str, &str)]) -> Diagnostic {
    let mut e = build_of(files).expect_err("the build fails");
    assert_eq!(e.len(), 1, "{e:?}");
    e.remove(0)
}

/// A song that plays the track exported as `out` by `tracks/t.ts`.
const PLAY_T: &str = "import { song } from \"@m2/core\";\nimport t from \"./tracks/t\";\nexport default song(() => t.out);\n";

#[test]
fn builds_tracks_imports_and_poly_with_source_locations() {
    let ir = build_of(&[
        (
            "tracks/bass.ts",
            r#"import { track, poly, kernel, hz, type Audio } from "@m2/core";

export default track("bass", { name: "Bass" }, ({ lane }) => {
  const notes = lane.notes("notes");
  const cutoff = lane.control("cutoff", "hz", hz(800));
  const out: Audio = poly(notes, (n) => {
    const f = kernel("std:mtof", { pitch: n.pitch });
    return kernel("std:saw", { freq: f });
  });
  return { out: kernel("std:svf", { in: out, cutoff }).port("lp"), env: kernel("std:follow", { in: out }) };
});
"#,
        ),
        (
            "song.ts",
            r#"import { song, kernel } from "@m2/core";
import bass from "./tracks/bass";

export default song(() => kernel("std:scale", { in: bass.out, by: 0.5 }));
"#,
        ),
    ])
    .unwrap_or_else(|e| panic!("{e:?}"));
    let keys: Vec<&str> = ir.modules.iter().map(|m| m.key.as_str()).collect();
    assert_eq!(keys, ["song", "bass"]);
    let bass = &ir.modules[1];
    assert!(bass.track);
    assert_eq!(bass.lanes.len(), 2);
    assert_eq!(bass.exports.keys().collect::<Vec<_>>(), ["env", "out"]);
    // ID-11: the song reads bass through an import of its key, not its label.
    assert!(ir.modules[0].nodes.iter().any(|n| matches!(&n.op, Op::Import { module, export } if module == "bass" && export == "out")));
    // Source locations point at the TypeScript lines.
    let svf = bass.nodes.iter().find(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == "std:svf")).unwrap();
    assert!(svf.src.as_deref().unwrap().starts_with("tracks/bass.ts:10:"), "{:?}", svf.src);
    let Some(Op::Poly { body, .. }) = bass.nodes.iter().map(|n| &n.op).find(|op| matches!(op, Op::Poly { .. })) else { panic!() };
    let saw = body.nodes.iter().find(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == "std:saw")).unwrap();
    assert!(saw.src.as_deref().unwrap().starts_with("tracks/bass.ts:8:"), "{:?}", saw.src);
}

#[test]
fn errors_point_at_the_users_line() {
    let e = error_of(&[(
        "song.ts",
        r#"import { song, kernel } from "@m2/core";

export default song(() => {
  const t = Date.now();
  return kernel("std:sine");
});
"#,
    )]);
    assert!(e.message.contains("Date"), "{e}");
    assert_eq!(e.code, "code.build");
    assert!(e.src.as_deref().unwrap_or("").starts_with("song.ts:4:"), "{:?}", e.src);
}

#[test]
fn randomness_and_outside_files_are_refused() {
    let song = "import { song, kernel } from \"@m2/core\";\nconst r = Math.random();\nexport default song(() => kernel(\"std:sine\"));\n";
    let e = error_of(&[("song.ts", song)]);
    assert!(e.message.contains("Math.random"), "{e}");

    // FMT-6a: a path outside the project is never read.
    let e = error_of(&[("song.ts", "import x from \"../../../../../../../../etc/passwd\";\nexport default x;\n")]);
    assert_eq!(e.code, "import.outside-code", "{e}");
    assert!(e.message.contains("outside the project"), "{e}");
    let e = error_of(&[("song.ts", "import x from \"/etc/passwd\";\nexport default x;\n")]);
    assert_eq!(e.code, "import.outside-code", "{e}");

    // FMT-3: a package the manifest doesn't list can't be imported, installed or not.
    let e = error_of(&[("song.ts", "import fs from \"fs\";\nexport default fs;\n")]);
    assert_eq!(e.code, "pkg.undeclared", "{e}");
    assert!(e.message.contains("`fs`") && e.message.contains("[dependencies]"), "{e}");
}

#[test]
fn syntax_errors_and_endless_loops() {
    let e = error_of(&[("song.ts", "import { song } from \"@m2/core\";\nexport default song(() => {\n  const x: number = ;\n});\n")]);
    assert!(e.src.as_deref().unwrap_or("").starts_with("song.ts:3:"), "{e}");
    assert_eq!(e.code, "code.build");

    let t = Instant::now();
    let e = error_of(&[("song.ts", "import { song } from \"@m2/core\";\nexport default song(() => { while (true) {} });\n")]);
    assert!(e.message.contains("too long"), "{e}");
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn std_modules_load() {
    let b = build_full(&options(&[(
        "song.ts",
        r#"import { song } from "@m2/core";
import { osc } from "std/dsp";
export default song(() => osc.sine(440));
"#,
    )]))
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(b.ir.modules[0].nodes.iter().any(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == "std:sine")));
    // FMT-6a, RENDER-2: every package file the build read is recorded with its hash.
    let dsp = b.package_files.iter().find(|f| f.path == "std/src/dsp.ts").expect("std/src/dsp.ts was read");
    let bytes = std::fs::read(std_dir().join("src/dsp.ts")).unwrap();
    assert_eq!(dsp.sha256, format!("{:x}", Sha256::digest(&bytes)));
    assert!(b.package_files.iter().any(|f| f.path == "std/src/math.ts"), "dsp imports math: {:?}", b.package_files);
}

#[test]
fn instrumentation_keeps_javascript_semantics() {
    // Every expression here would behave differently if instrumentation changed
    // receivers, short-circuiting or evaluation order; the song throws if any does.
    build_of(&[(
        "song.ts",
        r#"import { song, kernel } from "@m2/core";

class A { v = 1; get() { return this.v; } get2(x: number) { return this.v + x; } }
class B extends A { get() { return super.get() + 1; } get2(x: number) { return super.get2(x) * 10; } }
const a = new A();
const b = new B();
const none: any = null;
const order: string[] = [];
const log = (s: string) => { order.push(s); return s; };
function three(x: string, y: string, z: string) { return x + y + z; }

const checks: [string, unknown, unknown][] = [
  ["parenthesised callee", (a.get)(), 1],
  ["non-null callee", a.get!(), 1],
  ["super call", b.get(), 2],
  ["super call with args", b.get2(1), 20],
  ["optional chain, no args", none?.inner.method(), undefined],
  ["optional call, no args", none?.(), undefined],
  ["optional chain, args", none?.inner.method(log("never")), undefined],
  ["member call with args", a.get2(4), 5],
  ["spread args", three(...["x", "y", "z"]), "xyz"],
  ["evaluation order", three(log("1"), log("2"), log("3")), "123"],
];
for (const [what, got, want] of checks) {
  if (got !== want) throw new Error(`${what}: got ${String(got)}, want ${String(want)}`);
}
if (order.join("") !== "123") throw new Error(`evaluation order: ${order.join("")}`);

export default song(() => kernel("std:sine"));
"#,
    )])
    .unwrap_or_else(|e| panic!("{e:?}"));
}

// ---------------------------------------------------------------- keys and labels

/// A song that adds the `out` exports of `tracks/a.ts` and `tracks/b.ts`.
const PLAY_AB: &str = r#"import { song, kernel } from "@m2/core";
import a from "./tracks/a";
import b from "./tracks/b";
export default song(() => kernel("std:add", { a: a.out, b: b.out }));
"#;

// ID-4, ID-11: the key comes first and is the module's address; the label is display text.
#[test]
fn tracks_have_a_key_and_a_label() {
    let ir = build_of(&[
        (
            "tracks/a.ts",
            r#"import { track, kernel } from "@m2/core";
export default track("lead-vox", { name: "Lead Vox" }, () => kernel("std:sine"));"#,
        ),
        ("tracks/b.ts", "import { track, kernel } from \"@m2/core\";\nexport default track(\"808\", () => kernel(\"std:sine\"));\n"),
        ("song.ts", PLAY_AB),
    ])
    .unwrap_or_else(|e| panic!("{e:?}"));
    let m = |k: &str| ir.modules.iter().find(|m| m.key == k).unwrap_or_else(|| panic!("no module {k}"));
    assert_eq!((m("lead-vox").label.as_deref(), m("lead-vox").label()), (Some("Lead Vox"), "Lead Vox"));
    assert_eq!((m("808").label.as_deref(), m("808").label()), (None, "808"), "the label defaults to the key");
    assert_eq!(m("song").label(), "song");
    assert!(ir.modules[0].nodes.iter().any(|n| matches!(&n.op, Op::Import { module, .. } if module == "lead-vox")));
}

// ID-10: no property of the track's context, and nothing reachable from it or from any
// handle, returns the label. The song searches everything it can reach for it.
#[test]
fn code_cannot_observe_a_label() {
    let search = r#"
export function reach(from: unknown[], label: string): string[] {
  const seen = new Set<unknown>();
  const found: string[] = [];
  const walk = (v: unknown, path: string, depth: number) => {
    if (typeof v === "string") { if (v.includes(label)) found.push(path); return; }
    if ((typeof v !== "object" && typeof v !== "function") || v === null || seen.has(v) || depth > 12) return;
    seen.add(v);
    const props = new Set<string>([...Object.getOwnPropertyNames(v)]);
    for (let p = Object.getPrototypeOf(v); p && p !== Object.prototype && p !== Function.prototype; p = Object.getPrototypeOf(p)) {
      for (const k of Object.getOwnPropertyNames(p)) props.add(k);
    }
    for (const k of props) {
      if (k === "caller" || k === "callee" || k === "arguments") continue;
      let x: unknown;
      try { x = (v as any)[k]; } catch { continue; }
      walk(x, `${path}.${k}`, depth + 1);
    }
  };
  from.forEach((v, i) => walk(v, `#${i}`, 0));
  return found;
}
"#;
    let ir = build_of(&[
        ("lib/reach.ts", search),
        (
            "tracks/t.ts",
            r#"import { track, kernel, plugin, key } from "@m2/core";
import { reach } from "../lib/reach";
export default track("lead-vox", { name: "Lead Vox" }, (ctx) => {
  if (ctx.key !== "lead-vox") throw new Error(`ctx.key is ${ctx.key}`);
  if ("name" in ctx || "label" in ctx) throw new Error("the context has a name or label");
  const hook = ctx.lane.notes("hook", { name: "Hook Lane" });
  const out = key("chain", () => kernel("std:sine"));
  const found = reach([ctx, hook, out, ctx.time.beats, JSON.stringify(Object.keys(ctx))], "Lead Vox");
  found.push(...reach([ctx, hook, out], "Hook Lane"));
  if (found.length) throw new Error(`the label is reachable at ${found.join(", ")}`);
  return out;
});
"#,
        ),
        (
            "song.ts",
            r#"import { song } from "@m2/core";
import t from "./tracks/t";
import { reach } from "./lib/reach";
export default song(() => {
  const found = reach([t, Object.keys(t), (t as any).name, (t as any).label], "Lead Vox");
  if (found.length) throw new Error(`the label is reachable from the handle at ${found.join(", ")}`);
  return t.out;
});
"#,
        ),
    ])
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(ir.modules[1].label.as_deref(), Some("Lead Vox"));
    assert_eq!(ir.modules[1].lanes[0].label.as_deref(), Some("Hook Lane"));
}

// ID-10: `__build` returns every label, so song code calling it fails the build.
#[test]
fn song_code_cannot_call_the_build() {
    let e = error_of(&[(
        "song.ts",
        r#"import { song, kernel, __build } from "@m2/core";
const s = song(() => kernel("std:sine"));
const peek = __build(s);
export default s;
"#,
    )]);
    assert!(e.message.contains("__build"), "{e}");
}

// ID-10 and step 4's gate: changing a label changes only the label, so no node and no sound.
#[test]
fn a_label_change_changes_only_the_label() {
    let with = |name: &str| {
        let t = format!(
            r#"import {{ track }} from "@m2/core";
import {{ kick }} from "std/instruments";
export default track("kick", {{ name: "{name}" }}, ({{ lane }}) => kick(lane.notes("hits", {{ name: "{name} hits" }})));
"#
        );
        build_of(&[("tracks/t.ts", &t), ("song.ts", PLAY_T)]).unwrap_or_else(|e| panic!("{e:?}"))
    };
    let (mut a, mut b) = (with("Kick"), with("Big Kick"));
    assert_ne!(a, b);
    // Positions after the label move with it; they are not identity.
    fn unplace(nodes: &mut [m2_ir::Node]) {
        for n in nodes {
            n.src = None;
            if let Op::Poly { body, .. } = &mut n.op {
                unplace(&mut body.nodes);
            }
        }
    }
    for ir in [&mut a, &mut b] {
        for m in &mut ir.modules {
            (m.label, m.src) = (None, None);
            m.lanes.iter_mut().for_each(|l| l.label = None);
            unplace(&mut m.nodes);
        }
    }
    assert_eq!(a, b);
}

/// Build a project whose track `tracks/t.ts` has `body` as its function's body, from line 3.
fn with_track(key: &str, body: &str) -> Result<Ir, Vec<Diagnostic>> {
    let t = format!(
        r#"import {{ track, kernel, plugin, key, hz }} from "@m2/core";
export default track({key}, ({{ lane }}) => {{
{body}
}});"#
    );
    build_of(&[("tracks/t.ts", &t), ("song.ts", PLAY_T)])
}

// ID-1, ID-2, ID-3: every key is checked against the rule, never normalised, and the error
// suggests a valid spelling.
#[test]
fn invalid_keys_are_errors_with_a_suggestion() {
    let sine = "return kernel(\"std:sine\");";
    assert!(with_track("\"808\"", sine).is_ok(), "a leading digit is allowed");
    for (key, body, want) in [
        ("\"Lead Vox\"", sine, "Write \"lead-vox\""),
        ("\"kick-\"", sine, "Write \"kick\""),
        ("\"a-very-long-track-key-that-goes-past-32\"", sine, "Write \"a-very-long-track-key-that-goes\""),
        ("\"kick\"", "lane.notes(\"Hits\");\nreturn kernel(\"std:sine\");", "the lane key \"Hits\""),
        ("\"kick\"", "return plugin(\"clap:x\", { key: \"Lead\" });", "Write \"lead\""),
        ("\"kick\"", "return key(\"Chain L\", () => kernel(\"std:sine\"));", "Write \"chain-l\""),
        ("\"song\"", sine, "reserved for the root module"),
        ("42", sine, "must be a string"),
    ] {
        let e = with_track(key, body).expect_err(key);
        assert_eq!(e.len(), 1, "{e:?}");
        assert_eq!(e[0].code, "key.invalid", "{key}: {}", e[0]);
        assert!(e[0].message.contains(want), "{key}: {}", e[0]);
        assert!(e[0].src.as_deref().unwrap_or("").starts_with("tracks/t.ts:"), "{:?}", e[0].src);
    }
}

// ID-5: two tracks with one key are an error that names both evaluations: the call site,
// and for a track() in a helper, the stack that ran it.
#[test]
fn duplicate_track_keys_name_both_evaluations() {
    let e = error_of(&[
        ("tracks/a.ts", "import { track, kernel } from \"@m2/core\";\nexport default track(\"kick\", () => kernel(\"std:sine\"));\n"),
        ("tracks/b.ts", "import { track, kernel } from \"@m2/core\";\n\nexport default track(\"kick\", () => kernel(\"std:sine\"));\n"),
        ("song.ts", PLAY_AB),
    ]);
    assert_eq!(e.code, "key.duplicate", "{e}");
    assert!(e.message.contains("tracks/a.ts:2:16") && e.message.contains("tracks/b.ts:3:16"), "{e}");
    assert_eq!(e.module.as_deref(), Some("kick"));

    // One site that runs twice: the stacks tell the evaluations apart.
    let make = r#"import { track, kernel } from "@m2/core";
export function make(k: string) {
  return track(k, () => kernel("std:sine"));
}
"#;
    let song = r#"import { song } from "@m2/core";
import { make } from "./lib/make";
const a = make("pad");
const b = make("pad");
export default song(() => a.out);
"#;
    let e = error_of(&[("lib/make.ts", make), ("song.ts", song)]);
    assert_eq!(e.code, "key.duplicate", "{e}");
    assert!(e.message.contains("lib/make.ts:3:10"), "the call site: {e}");
    assert!(e.message.contains("called from song.ts:3:") && e.message.contains("called from song.ts:4:"), "both stacks: {e}");
}

// ID-5: a key error fails the build even when song code catches it.
#[test]
fn key_errors_cannot_be_caught_away() {
    let song = r#"import { song, track, kernel, key } from "@m2/core";
import a from "./tracks/a";
try { track("kick", () => kernel("std:sine")); } catch {}
export default song(() => { try { key("Bad Key", () => 1); } catch {} return a.out; });
"#;
    let a = "import { track, kernel } from \"@m2/core\";\nexport default track(\"kick\", () => kernel(\"std:sine\"));\n";
    let e = error_of(&[("tracks/a.ts", a), ("song.ts", song)]);
    assert_eq!(e.code, "key.duplicate", "{e}");
    let song = r#"import { song, kernel, key } from "@m2/core";
export default song(() => { try { key("Bad Key", () => 1); } catch {} return kernel("std:sine"); });
"#;
    let e = error_of(&[("song.ts", song)]);
    assert_eq!(e.code, "key.invalid", "{e}");
    assert!(e.src.as_deref().unwrap_or("").starts_with("song.ts:2:"), "the error points where it was raised: {:?}", e.src);
}

// ID-5, ID-7, ID-8: key() frames are unique in their parent, compose plugin state paths,
// and a plugin key is unique in its frame.
#[test]
fn key_frames_and_plugin_keys() {
    let t = |body: &str| with_track("\"vox\"", body);
    let ir = t(r#"const x = kernel("std:sine");
const chain = (y: any) => plugin("clap:fx", { in: plugin("clap:fx", { in: y, key: "tune" }) });
const l = key("chain-l", () => chain(x));
const r = key("chain-r", () => key("inner", () => chain(l)));
return plugin("clap:fx", { in: r, key: "tune" });"#)
    .unwrap_or_else(|e| panic!("{e:?}"));
    let plugins: Vec<(Option<&str>, Vec<&str>)> = ir.modules[1]
        .nodes
        .iter()
        .filter_map(|n| match &n.op {
            Op::Plugin { key, frames, .. } => Some((key.as_deref(), frames.iter().map(String::as_str).collect())),
            _ => None,
        })
        .collect();
    assert_eq!(plugins, [
        (Some("tune"), vec!["chain-l"]),
        (None, vec!["chain-l"]),
        (Some("tune"), vec!["chain-r", "inner"]),
        (None, vec!["chain-r", "inner"]),
        (Some("tune"), vec![]),
    ]);
    let plugin_nodes = ir.modules[1].nodes.iter().filter(|n| matches!(n.op, Op::Plugin { .. }));
    let paths: Vec<Option<Vec<String>>> = plugin_nodes.map(|n| n.op.state_path()).collect();
    assert_eq!(paths[0].as_deref(), Some(&["chain-l".to_string(), "tune".to_string()][..]));
    assert_eq!(paths[1].as_deref(), Some(&["chain-l".to_string()][..]), "no key: the innermost frame's key names the state");

    for (body, code, want) in [
        (
            r#"key("a", () => 1);
key("a", () => 2);
return kernel("std:sine");"#,
            "key.duplicate",
            r#"key("a") is used twice in the top level"#,
        ),
        (
            r#"key("a", () => key("b", () => 1));
key("a2", () => key("b", () => 1));
key("a", () => 1);
return kernel("std:sine");"#,
            "key.duplicate",
            "tracks/t.ts:3:1",
        ),
        (
            r#"plugin("clap:fx", { key: "tune" });
return plugin("clap:fx", { key: "tune" });"#,
            "key.duplicate",
            r#"two plugins have the key "tune" in "vox""#,
        ),
        (
            r#"return key("x", () => { plugin("clap:fx", { key: "t" }); return plugin("clap:fx", { key: "t" }); });"#,
            "key.duplicate",
            r#"the frame key("x")"#,
        ),
        (r#"return plugin("clap:fx", { state: "lead" });"#, "code.build", "was replaced by `key"),
    ] {
        let e = t(body).expect_err(body);
        assert_eq!((e[0].code, e.len()), (code, 1), "{body}: {}", e[0]);
        assert!(e[0].message.contains(want), "{body}: {}", e[0]);
    }
    // The same frame key in two tracks, or under two parents, is fine.
    let a = r#"import { track, kernel, key } from "@m2/core";
export default track("a", () => key("x", () => key("y", () => kernel("std:sine"))));
"#;
    let b = r#"import { track, kernel, key } from "@m2/core";
export default track("b", () => { key("y", () => 1); return key("x", () => kernel("std:sine")); });
"#;
    build_of(&[("tracks/a.ts", a), ("tracks/b.ts", b), ("song.ts", PLAY_AB)]).unwrap_or_else(|e| panic!("{e:?}"));
}

// ID-6: a lane declared again the same way is the same lane; otherwise `lane.conflict`.
// Declared from two key() frame chains it is `lane.shared`, unless every declaration says so.
#[test]
fn lane_declarations() {
    let t = |body: &str| with_track("\"bass\"", &format!("{body}\nreturn kernel(\"std:sine\");"));
    let ir = t(r#"lane.control("cutoff", "hz", hz(800), { name: "Cutoff" });
lane.control("cutoff", "hz", 800);
lane.mask("drop");
lane.control("drop", "unit01", 0);"#)
    .unwrap_or_else(|e| panic!("{e:?}"));
    let lanes: Vec<(&str, String, Option<f64>, &str)> =
        ir.modules[1].lanes.iter().map(|l| (l.key.as_str(), l.kind.to_string(), l.default, l.label())).collect();
    assert_eq!(lanes, [("cutoff", "control<hz>".into(), Some(800.0), "Cutoff"), ("drop", "control<unit01>".into(), Some(0.0), "drop")]);

    for (body, code, want) in [
        ("lane.notes(\"hook\");\nlane.audio(\"hook\");", "lane.conflict", "declared as events<note> at tracks/t.ts:3:1"),
        ("lane.control(\"c\", \"hz\", 800);\nlane.control(\"c\", \"hz\", 900);", "lane.conflict", "control<hz> with fallback 900"),
        ("lane.notes(\"n\", { name: \"A\" });\nlane.notes(\"n\", { name: \"B\" });", "lane.conflict", "named \"B\""),
        ("lane.notes(\"n\", { label: \"A\" });", "code.build", "unknown option \"label\""),
        (
            r#"key("l", () => lane.control("pan", "bipolar"));
key("r", () => lane.control("pan", "bipolar"));"#,
            "lane.shared",
            r#"key("l")"#,
        ),
        (
            r#"lane.control("pan", "bipolar");
key("r", () => lane.control("pan", "bipolar", 0, { shared: true }));"#,
            "lane.shared",
            "the top level",
        ),
    ] {
        let e = t(body).expect_err(body);
        assert_eq!((e[0].code, e.len()), (code, 1), "{body}: {}", e[0]);
        assert!(e[0].message.contains(want), "{body}: {}", e[0]);
    }
    // Shared on purpose: every declaration says so. The lane is still addressed by track and lane key only.
    let ir = t(r#"key("l", () => lane.control("pan", "bipolar", 0, { shared: true }));
key("r", () => lane.control("pan", "bipolar", 0, { shared: true }));"#)
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(ir.modules[1].lanes.iter().map(|l| l.key.as_str()).collect::<Vec<_>>(), ["pan"]);
}

// ---------------------------------------------------------------- imports

// FMT-6a: project imports resolve only against the snapshot's code files, byte for byte:
// the path, then with `.ts`, then `/index.ts`. Nothing else is read, on any disk.
#[test]
fn imports_resolve_only_against_the_snapshot() {
    let lib = "export const n = 1;\n";
    // The import is used, or TypeScript would drop it.
    let song = |import: &str| {
        format!(
            r#"import {{ song, kernel }} from "@m2/core";
import {{ n }} from "{import}";
export default song(() => kernel("std:sine", {{ freq: n }}));"#
        )
    };
    for import in ["./lib/util", "./lib/util.ts", "./lib", "./lib/../lib/util"] {
        let s = song(import);
        build_of(&[("lib/util.ts", lib), ("lib/index.ts", lib), ("song.ts", &s)]).unwrap_or_else(|e| panic!("{import}: {e:?}"));
    }
    for (import, want) in [
        ("./Lib/Util", "spelled as the file is named, `lib/util.ts`"),
        ("./lib/util.js", "isn't TypeScript"),
        ("./lib/other", "names no code file"),
        ("./data/song/notes.json", "names no code file"),
        ("../outside", "outside the project"),
    ] {
        let s = song(import);
        let e = error_of(&[("lib/util.ts", lib), ("song.ts", &s)]);
        assert_eq!(e.code, "import.outside-code", "{import}: {e}");
        assert!(e.message.contains(want) && e.message.contains(import), "{import}: {e}");
        assert_eq!(e.src.as_deref(), Some("song.ts"), "{import}");
    }
    // A file beside the project on disk is invisible: the build reads only the snapshot.
    let s = song("./Cargo.toml");
    let e = error_of(&[("song.ts", &s)]);
    assert_eq!(e.code, "import.outside-code", "{e}");
}

// FMT-3, FMT-6a: package imports need the package listed, and are spelled as on disk.
#[test]
fn package_imports() {
    let song = |import: &str| {
        format!("import {{ song }} from \"@m2/core\";\nimport {{ osc }} from \"{import}\";\nexport default song(() => osc.sine(440));\n")
    };
    let s = song("std/dsp");
    let mut o = options(&[("song.ts", &s)]);
    o.dependencies.clear();
    let e = build(&o).unwrap_err();
    assert_eq!(e[0].code, "pkg.undeclared", "{}", e[0]);
    assert!(e[0].message.contains("`std`"), "{}", e[0]);

    // FMT-6a: other capitals, or a path out of the package, is import.outside-code like a
    // project import's; a file the package doesn't have stays a build error.
    let s = song("std/DSP");
    let e = error_of(&[("song.ts", &s)]);
    assert!(e.message.contains("same capitals"), "{e}");
    assert_eq!(e.code, "import.outside-code", "{e}");
    let s = song("std/../../../../../../etc/hosts");
    let e = error_of(&[("song.ts", &s)]);
    assert_eq!(e.code, "import.outside-code", "{e}");
    assert!(e.message.contains("own project or from installed packages"), "{e}");
    let s = song("std/no-such-file");
    assert_eq!(error_of(&[("song.ts", &s)]).code, "code.build");
}

// DIAG-2: an error the SDK throws keeps its code only when it is a build code.
#[test]
fn song_code_cannot_invent_codes() {
    let e = error_of(&[(
        "song.ts",
        r#"import { song } from "@m2/core";
const e: any = new Error("mine");
e.code = "tx.interrupted";
throw e;
export default song(() => 0 as any);
"#,
    )]);
    assert_eq!(e.code, "code.build");
}
