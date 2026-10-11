use std::time::Duration;

use m2_ir::Op;

use super::*;

/// A project folder with the given files, in a fresh temp dir.
fn project(files: &[(&str, &str)]) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("m2-script-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn build_in(dir: &Path) -> Result<Ir, Vec<Diagnostic>> {
    let mut o = Options::new(dir, &dir.join("song.ts"));
    o.packages.push(("std".into(), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    o.time_limit = Duration::from_secs(2);
    build(&o)
}

#[test]
fn builds_tracks_imports_and_poly_with_source_locations() {
    let dir = project(&[
        (
            "tracks/bass.ts",
            r#"import { track, poly, kernel, hz, type Audio } from "@m2/core";

export default track("Bass", ({ lane }) => {
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
    ]);
    let ir = build_in(&dir).unwrap_or_else(|e| panic!("{e:?}"));
    let names: Vec<&str> = ir.modules.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["song", "Bass"]);
    let bass = &ir.modules[1];
    assert!(bass.track);
    assert_eq!(bass.lanes.len(), 2);
    assert_eq!(bass.exports.keys().collect::<Vec<_>>(), ["env", "out"]);
    // The song reads Bass through an import.
    assert!(ir.modules[0].nodes.iter().any(|n| matches!(&n.op, Op::Import { module, export } if module == "Bass" && export == "out")));
    // Source locations point at the TypeScript lines.
    let svf = bass.nodes.iter().find(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == "std:svf")).unwrap();
    assert!(svf.src.as_deref().unwrap().starts_with("tracks/bass.ts:10:"), "{:?}", svf.src);
    let Some(Op::Poly { body, .. }) = bass.nodes.iter().map(|n| &n.op).find(|op| matches!(op, Op::Poly { .. })) else { panic!() };
    let saw = body.nodes.iter().find(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == "std:saw")).unwrap();
    assert!(saw.src.as_deref().unwrap().starts_with("tracks/bass.ts:8:"), "{:?}", saw.src);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn errors_point_at_the_users_line() {
    let dir = project(&[(
        "song.ts",
        r#"import { song, kernel } from "@m2/core";

export default song(() => {
  const t = Date.now();
  return kernel("std:sine");
});
"#,
    )]);
    let e = build_in(&dir).unwrap_err();
    assert_eq!(e.len(), 1);
    assert!(e[0].message.contains("Date"), "{}", e[0]);
    assert!(e[0].src.as_deref().unwrap_or("").starts_with("song.ts:4:"), "{:?}", e[0].src);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn randomness_and_outside_files_are_refused() {
    let dir = project(&[("song.ts", "import { song, kernel } from \"@m2/core\";\nconst r = Math.random();\nexport default song(() => kernel(\"std:sine\"));\n")]);
    let e = build_in(&dir).unwrap_err();
    assert!(e[0].message.contains("Math.random"), "{}", e[0]);
    std::fs::remove_dir_all(dir).ok();

    let dir = project(&[("song.ts", "import x from \"../../../../../../../../etc/passwd\";\nexport default x;\n")]);
    let e = build_in(&dir).unwrap_err();
    assert!(e[0].message.contains("not found") || e[0].message.contains("only import"), "{}", e[0]);
    std::fs::remove_dir_all(dir).ok();

    let dir = project(&[("song.ts", "import fs from \"fs\";\nexport default fs;\n")]);
    let e = build_in(&dir).unwrap_err();
    assert!(e[0].message.contains("no package `fs`"), "{}", e[0]);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn syntax_errors_and_endless_loops() {
    let dir = project(&[("song.ts", "import { song } from \"@m2/core\";\nexport default song(() => {\n  const x: number = ;\n});\n")]);
    let e = build_in(&dir).unwrap_err();
    assert!(e[0].message.contains("song.ts:3:"), "{}", e[0]);
    std::fs::remove_dir_all(dir).ok();

    let dir = project(&[("song.ts", "import { song } from \"@m2/core\";\nexport default song(() => { while (true) {} });\n")]);
    let t = Instant::now();
    let e = build_in(&dir).unwrap_err();
    assert!(e[0].message.contains("too long"), "{}", e[0]);
    assert!(t.elapsed() < Duration::from_secs(5));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn std_modules_load() {
    let dir = project(&[(
        "song.ts",
        r#"import { song } from "@m2/core";
import { osc } from "std/dsp";
export default song(() => osc.sine(440));
"#,
    )]);
    let ir = build_in(&dir).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(ir.modules[0].nodes.iter().any(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == "std:sine")));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn instrumentation_keeps_javascript_semantics() {
    // Every expression here would behave differently if instrumentation changed
    // receivers, short-circuiting or evaluation order; the song throws if any does.
    let dir = project(&[(
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
    )]);
    build_in(&dir).unwrap_or_else(|e| panic!("{e:?}"));
    std::fs::remove_dir_all(dir).ok();
}
