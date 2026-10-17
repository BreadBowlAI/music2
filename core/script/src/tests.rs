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

// PLAT-4, RENDER-2: song code's `Math` and `**` compute with libm, so a song builds the same
// constants on every OS. Each input is one where glibc's answer differs from libm's in the last
// bit (measured on Linux x86_64), except log1p's and cbrt's, where none was found nearby; `n = 3`
// is such a note for the `**` form of mtof.
#[test]
fn song_math_is_libm_on_every_os() {
    let js = |x: f64| match x {
        x if x.is_nan() => "NaN".to_string(),
        x if x.is_infinite() => format!("{}Infinity", if x < 0.0 { "-" } else { "" }),
        x => format!("{x:?}"),
    };
    let checks: [(&str, f64); 38] = [
        ("Math.sin(2.5)", libm::sin(2.5)),
        ("Math.cos(2.3)", libm::cos(2.3)),
        ("Math.tan(2.2)", libm::tan(2.2)),
        ("Math.asin(0.32)", libm::asin(0.32)),
        ("Math.acos(0.05)", libm::acos(0.05)),
        ("Math.atan(2.4)", libm::atan(2.4)),
        ("Math.atan2(0.1, 1.7)", libm::atan2(0.1, 1.7)),
        ("Math.sinh(3.69)", libm::sinh(3.69)),
        ("Math.cosh(1.7)", libm::cosh(1.7)),
        ("Math.tanh(0.3)", libm::tanh(0.3)),
        ("Math.asinh(2.6)", libm::asinh(2.6)),
        ("Math.acosh(2.3)", libm::acosh(2.3)),
        ("Math.atanh(0.039)", libm::atanh(0.039)),
        ("Math.exp(1)", libm::exp(1.0)),
        ("Math.expm1(0.92)", libm::expm1(0.92)),
        ("Math.log(1.4)", libm::log(1.4)),
        ("Math.log1p(0.5)", libm::log1p(0.5)),
        ("Math.log2(1.3)", libm::log2(1.3)),
        ("Math.log10(0.6)", libm::log10(0.6)),
        ("Math.cbrt(3)", libm::cbrt(3.0)),
        ("Math.pow(0.1, 0.7)", libm::pow(0.1, 0.7)),
        ("0.1 ** 0.7", libm::pow(0.1, 0.7)),
        ("(() => { let y = 2; y **= -89 / 12; return y; })()", libm::pow(2.0, -89.0 / 12.0)),
        // `**=` on a target in TypeScript's clothes (review det-2 of the platform step).
        ("(() => { let a: number | undefined = 0.1; a! **= 0.7; return a; })()", libm::pow(0.1, 0.7)),
        ("(() => { let b = 0.1; (b as number) **= 0.7; return b; })()", libm::pow(0.1, 0.7)),
        ("(() => { let c = 0.1; (<number>c) **= 0.7; return c; })()", libm::pow(0.1, 0.7)),
        ("(() => { let d = 0.1; (d satisfies number) **= 0.7; return d; })()", libm::pow(0.1, 0.7)),
        ("(() => { const o: { p?: number } = { p: 0.1 }; o.p! **= 0.7; return o.p!; })()", libm::pow(0.1, 0.7)),
        ("(() => { const r = [0.1]; r[0]! **= 0.7; return r[0]; })()", libm::pow(0.1, 0.7)),
        ("(() => { const h = [0.1]; ((h[0] as number)) **= 0.7; return h[0]; })()", libm::pow(0.1, 0.7)),
        ("Math.hypot(0.1, 0.1)", libm::hypot(0.1, 0.1)),
        // As QuickJS's own: folded, |x| alone, 0 for none, Infinity over NaN.
        ("Math.hypot(0.1, 0.1, 0.2)", libm::hypot(libm::hypot(0.1, 0.1), 0.2)),
        ("Math.hypot(-3)", 3.0),
        ("Math.hypot()", 0.0),
        ("Math.hypot(NaN, -Infinity)", f64::INFINITY),
        // JS's pow, not C's: NaN for 1 to an infinite power. Arguments as JS takes them.
        ("Math.pow(1, Infinity)", f64::NAN),
        ("Math.sin()", f64::NAN),
        ("Math.sin(\"2.5\", 1)", libm::sin(2.5)),
    ];
    let rows: String = checks.iter().map(|(e, want)| format!("  [{e:?}, {e}, {}],\n", js(*want))).collect();
    let song = format!(
        r#"import {{ song, kernel }} from "@m2/core";
const checks: [string, number, number][] = [
{rows}];
const wrong = checks.filter(([, got, want]) => !Object.is(got, want)).map(([what, got, want]) => `${{what}}: got ${{got}}, want ${{want}}`);
if (wrong.length > 0) throw new Error(`${{wrong.length}} wrong: ${{wrong.join("; ")}}`);
if (Math.sin.name !== "sin" || Math.sin.length !== 1 || Math.hypot.length !== 2) throw new Error("not like a built-in");
const n = 3;
export default song(() => kernel("std:sine", {{ freq: 440 * 2 ** ((n - 69) / 12) }}));
"#
    );
    let ir = build_of(&[("song.ts", &song)]).unwrap_or_else(|e| panic!("{e:?}"));
    let freq = 440.0 * libm::pow(2.0, (3.0 - 69.0) / 12.0);
    let consts: Vec<f64> =
        ir.modules[0].nodes.iter().filter_map(|n| if let Op::Const { value, .. } = n.op { Some(value) } else { None }).collect();
    assert!(consts.iter().any(|c| c.to_bits() == freq.to_bits()), "{freq:?} in {consts:?}");
}

// RENDER-2, PLAT-4 (review det-3 of the platform step): a build runs on a thread with a large stack,
// so a song recursing far deeper than any real one builds the same whichever compiler built
// QuickJS. With 1 MB, gcc's QuickJS stopped at about 1,300 calls and clang's at about 950. Endless
// recursion still ends, with QuickJS's own message, never overflowing the thread.
#[test]
fn deep_recursion_builds_alike_on_every_os() {
    let deep = "import { song, kernel } from \"@m2/core\";\nconst depth = (i: number): number => (i === 0 ? 0 : 1 + depth(i - 1));\n\
                if (depth(5000) !== 5000) throw new Error(\"wrong depth\");\nexport default song(() => kernel(\"std:sine\", { freq: 440 }));\n";
    build_of(&[("song.ts", deep)]).unwrap_or_else(|e| panic!("{e:?}"));
    let endless = "import { song, kernel } from \"@m2/core\";\nconst f = (): number => 1 + f();\nf();\n\
                   export default song(() => kernel(\"std:sine\", { freq: 440 }));\n";
    let e = error_of(&[("song.ts", endless)]);
    assert!(e.message.contains("stack"), "{e:?}");
}

// RENDER-2 (decision D1 of the platform review): song code can't have an `enum`. It is refused
// at its place, saying what to write instead: oxc's enum transform stopped a debug build's whole
// process (an assertion inside QuickJS's loader, which can't unwind), and gives string members
// wrong reverse mappings in a release build (oxc#21667). In a package module too.
#[test]
fn an_enum_is_refused_at_its_place() {
    for code in ["enum Mode { A, B }", "const enum Mode { A = \"a\", B = \"b\" }"] {
        let song = format!(
            "import {{ song, kernel }} from \"@m2/core\";\n\n  {code}\nexport default song(() => kernel(\"std:sine\", {{ freq: 440 }}));\n"
        );
        let e = error_of(&[("song.ts", &song)]);
        assert_eq!(
            (e.message.as_str(), e.src.as_deref()),
            ("enum isn't available in song code: use a plain object, like const Mode = { A: 0, B: 1 }", Some("song.ts:3:3")),
            "{code}"
        );
    }
    let e = error_of(&[("song.ts", PLAY_T), ("tracks/t.ts", "export enum Kind { Kick }\nexport const out = 1;\n")]);
    assert_eq!(e.src.as_deref(), Some("tracks/t.ts:1:8"), "{e}");
}

// PLAT-4: song code's `**` is JavaScript's on every value, though the build lowers it. Numbers
// compute with libm (above); BigInts exactly; a BigInt with a number, or a negative BigInt
// exponent, throws what JS throws. `**=` evaluates its target once, a `Math` the song declares
// doesn't change `**`, and `Math.pow` stays JS's, numbers only, like QuickJS's own `Math.sqrt`. oxc
// leaves a `**` with a BigInt literal to QuickJS, so `2n ** hundred` is the reference for `two ** hundred`.
#[test]
fn exponents_are_javascripts() {
    build_of(&[(
        "song.ts",
        r#"import { song, kernel } from "@m2/core";
const two = 2n, hundred = 100n, three = 3, half = 0.5;
const thrown = (f: () => unknown) => { try { f(); return "nothing"; } catch (e) { return `${(e as Error).name}: ${(e as Error).message}`; } };
const order: string[] = [];
const key = (k: string) => { order.push(k); return k; };
const o: { [k: string]: bigint } = { v: 3n };
o[key("v")] **= two;
let n = 3n;
n **= hundred;
const checks: [string, unknown, unknown][] = [
  ["BigInts", two ** hundred, 1267650600228229401496703205376n],
  ["BigInts as QuickJS's own", two ** hundred, 2n ** hundred],
  ["**= on a variable", n, 3n ** hundred],
  ["**= on a member", o.v, 9n],
  ["its key evaluated once", order.join(","), "v"],
  ["a negative base", (-two) ** (two + 1n), -8n],
  ["converted as JS converts", ({ valueOf: () => two } as any) ** ({ valueOf: () => two } as any), 4n],
  ["a BigInt and a number", thrown(() => two ** three), thrown(() => 2n ** three)],
  ["a number and a BigInt", thrown(() => three ** two), thrown(() => three ** 2n)],
  ["a negative exponent", thrown(() => two ** -hundred), thrown(() => 2n ** -hundred)],
  ["numbers", three ** half, Math.pow(3, 0.5)],
  ["a song's own Math", (() => { const Math = { pow: () => 42 }; return three ** three + Math.pow(); })(), 69],
  ["Math.pow takes numbers only", thrown(() => Math.pow(two as any, two as any)), thrown(() => Math.sqrt(two as any))],
];
const wrong = checks.filter(([, got, want]) => got !== want).map(([what, got, want]) => `${what}: got ${String(got)}, want ${String(want)}`);
if (wrong.length > 0) throw new Error(`${wrong.length} wrong: ${wrong.join("; ")}`);
const errors = [thrown(() => two ** three), thrown(() => two ** -hundred), thrown(() => Math.pow(two as any, 1))];
if (!errors[0].startsWith("TypeError: ") || !errors[1].startsWith("RangeError: ") || !errors[2].startsWith("TypeError: ")) throw new Error(errors.join("; "));
export default song(() => kernel("std:sine"));
"#,
    )])
    .unwrap_or_else(|e| panic!("{e:?}"));
}

// RENDER-2, PLAT-4: song code can't make code from text while it runs, which would skip the
// build's lowering of `**` and compute it with the OS's `pow`: `eval` and each kind of function's
// constructor throw, saying why, as `Math.random` does. It reads no clock either: `Date` and
// `performance` are gone. Nor does it see when garbage is collected: `WeakRef` and
// `FinalizationRegistry` are gone (QuickJS collects after a number of bytes as the OS's malloc counts
// them, which differs between OSes). Functions, modules, `WeakMap` and `WeakSet` work as before.
#[test]
fn song_code_cannot_make_code_from_text() {
    for (code, what) in [
        ("eval(\"0.1 ** 0.7\")", "eval"),
        ("(0, eval)(\"1\")", "eval"),
        ("globalThis[\"ev\" + \"al\"](\"1\")", "eval"),
        ("new Function(\"return 0.1 ** 0.7\")()", "The Function constructor"),
        ("Function(\"return 1\")", "The Function constructor"),
        ("(() => 1).constructor(\"return 1\")", "The Function constructor"),
        ("Reflect.construct(Object.getPrototypeOf(class {}).constructor, [\"return 1\"])", "The Function constructor"),
        ("(async () => 1).constructor(\"return 1\")", "The AsyncFunction constructor"),
        ("(function* () {}).constructor(\"yield 1\")", "The GeneratorFunction constructor"),
        ("Object.getPrototypeOf(async function* () {}).constructor(\"yield 1\")", "The AsyncGeneratorFunction constructor"),
    ] {
        let song =
            format!("import {{ song, kernel }} from \"@m2/core\";\nconst x = {code};\nexport default song(() => kernel(\"std:sine\"));\n");
        let e = error_of(&[("song.ts", &song)]);
        assert!(e.message.starts_with(&format!("{what} isn't available: songs run only the code in their files")), "{code}: {e}");
        assert!(e.src.as_deref().is_some_and(|s| s.starts_with("song.ts:2:")), "{code}: {e:?}");
    }
    build_full(&options(&[(
        "song.ts",
        r#"import { song } from "@m2/core";
import { osc } from "std/dsp";
const kinds = [function () {}, () => 1, async () => 1, function* () {}, async function* () {}, class {}];
if (!kinds.every((f) => f instanceof Function && typeof f === "function")) throw new Error("not functions");
if (Function.name !== "Function" || (async () => 1).constructor.name !== "AsyncFunction") throw new Error("renamed");
if (typeof Date !== "undefined" || typeof performance !== "undefined") throw new Error("a clock");
if (typeof WeakRef !== "undefined" || typeof FinalizationRegistry !== "undefined") throw new Error("the collector's timing");
if (!new WeakMap([[kinds[0], 1]]).has(kinds[0]) || !new WeakSet([kinds[1]]).has(kinds[1])) throw new Error("no weak collections");
const add = function (a: number, b: number) { return a + b; };
export default song(() => osc.sine(add.call(null, 220, 220)));
"#,
    )]))
    .unwrap_or_else(|e| panic!("{e:?}"));

    // From a package's own code too, placed in it (not in the prelude that throws).
    let pkg = TempPackage::new("eval", &[("index.ts", "export const n = 1;\neval(\"n\");\n")]);
    let problems = describe(&[("pkg".into(), pkg.0.clone())]).problems;
    assert!(problems[0].message.contains("eval isn't available"), "{problems:?}");
    assert!(problems[0].src.as_deref().is_some_and(|s| s.starts_with("pkg/src/index.ts:2:")), "{problems:?}");
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

// DIAG-1: an import of a name its module doesn't export names the module in full and as people
// see it, at the import. QuickJS cuts a module's name to 63 bytes and gives no place.
#[test]
fn a_missing_export_is_named_in_full_at_the_import() {
    let long = "tracks/a-folder-with-a-long-name-for-its-tracks/one-more-long-name-past-63-bytes.ts";
    assert!(long.len() > 63);
    let song = format!(
        "import {{ song }} from \"@m2/core\";\nimport {{ type bar as Bar }} from \"./{long}\";\nimport {{ foo, bar }} from \"./{long}\";\nexport default song(() => [foo, bar as Bar]);\n"
    );
    let e = error_of(&[("song.ts", &song), (long, "export const foo = 1;\n")]);
    assert_eq!(e.message, format!("Could not find export 'bar' in module '{long}'"));
    assert_eq!(e.src.as_deref(), Some("song.ts:3:15"), "{e}");
    // An import only types use is dropped with the types, so it is never the place, though it comes first.
    let a = "export interface Foo { x: number }\n";
    let typed = "import { Foo } from \"../lib/a\";\nexport const v: Foo = { x: 1 };\n";
    let used = "import { Foo } from \"../lib/a\";\nexport const w = Foo;\n";
    let s = "import { song } from \"@m2/core\";\nimport { v } from \"./tracks/a\";\nimport { w } from \"./tracks/b\";\nexport default song(() => [v, w] as any);\n";
    let e = error_of(&[("lib/a.ts", a), ("tracks/a.ts", typed), ("tracks/b.ts", used), ("song.ts", s)]);
    assert_eq!((e.message.as_str(), e.src.as_deref()), ("Could not find export 'Foo' in module 'lib/a.ts'", Some("tracks/b.ts:1:10")));

    // A re-export, a package module, the SDK, and QuickJS's other forms of the message.
    let t = "export { nope } from \"std/dsp\";\nexport const out = 1;\n";
    let e = error_of(&[("song.ts", PLAY_T), ("tracks/t.ts", t)]);
    assert_eq!(
        (e.message.as_str(), e.src.as_deref()),
        ("Could not find export 'nope' in module 'std/src/dsp.ts'", Some("tracks/t.ts:1:10"))
    );
    let s = "import { song, sogn } from \"@m2/core\";\nexport default song(sogn);\n";
    let e = error_of(&[("song.ts", s)]);
    assert_eq!((e.message.as_str(), e.src.as_deref()), ("Could not find export 'sogn' in module '@m2/core'", Some("song.ts:1:16")));
    let s = "import { song } from \"@m2/core\";\nimport { x } from \"./lib/c\";\nexport default song(() => x);\n";
    let c = "export * from \"./a\";\nexport * from \"./b\";\n";
    let e = error_of(&[("lib/a.ts", "export const x = 1;\n"), ("lib/b.ts", "export const x = 2;\n"), ("lib/c.ts", c), ("song.ts", s)]);
    assert_eq!((e.message.as_str(), e.src.as_deref()), ("export 'x' in module 'lib/c.ts' is ambiguous", Some("song.ts:2:10")));
    let (a, b) = ("export { x } from \"./b\";\n", "export { x } from \"./a\";\n");
    let e = error_of(&[("lib/a.ts", a), ("lib/b.ts", b), ("song.ts", s.replace("./lib/c", "./lib/a").as_str())]);
    assert_eq!(
        (e.message.as_str(), e.src.as_deref()),
        ("circular reference when looking for export 'x' in module 'lib/a.ts'", Some("lib/b.ts:1:10"))
    );
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

// ---------------------------------------------------------------- component descriptors

/// Components declared with descriptors, as std will be.
const LIB: &str = r#"import { component, control, port, setting, each, signature, kernel, type Audio, type Value, type Signal, type Params } from "@m2/core";

export const lowpass = component(
  {
    id: "lowpass", label: "Lowpass",
    signature: signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>(),
    inputs: { in: port.audio() },
    controls: { cutoff: control.hz(1000).range(20, 20000).log().smooth(), q: control.scalar(0.707).range(0.3, 20).log().smooth() },
    output: port.audio(),
    positional: ["cutoff", "q"],
    editor: { category: "Filter", description: "Darker", insert: { cutoff: 1000 } },
  },
  (x: Audio, { cutoff, q }: Params) => kernel("std:svf", { in: x, cutoff, q }).port("lp"),
);

export const delay = component(
  {
    id: "delay",
    inputs: { in: port.audio() },
    controls: { time: control.ms(250).range(1, 2000).log().smooth(), feedback: control.frac(0.3), mix: control.frac(0.3) },
    output: port.audio(),
    editor: { category: "Delay & Reverb", insert: { time: 250, mix: 0.3 } },
  },
  (x: Audio, { time, feedback, mix }: Params) => kernel("std:delay", { in: x, time, feedback, mix }),
);

export const follow = component(
  { id: "follow", inputs: { in: port.audio({ sidechain: true }) }, controls: { release: control.secs(0.12) }, output: port.control("unit01") },
  (x: Audio, { release }: Params) => kernel("std:follow", { in: x, release }),
);

export const mix = component(
  {
    id: "mix",
    controls: { channels: each({ in: port.audio(), db: control.db(0).optional(), pan: control.bipolar(0).optional().smooth() }) },
    output: port.audio(),
    positional: ["channels"],
  },
  ({ channels }: Params) => {
    const strips = Object.entries(channels).map(([, c]: [string, any]) => {
      let x = c.in;
      if (c.pan !== undefined) x = kernel("std:pan", { in: x, pan: c.pan });
      if (c.db !== undefined) x = kernel("std:gain", { in: x, db: c.db });
      return x;
    });
    return strips.slice(1).reduce((a, b) => kernel("std:add", { a, b }), strips[0]);
  },
);

export const noise = component(
  { id: "noise", settings: { seed: setting.int(1) }, positional: ["seed"], output: port.audio() },
  ({ seed }: Params) => kernel("std:noise", { seed }),
);

export const sum = component(
  { id: "sum", signature: signature<(...xs: Signal[]) => Signal>(), inputs: { xs: port.any().many() } },
  (xs: Signal[]) => xs.slice(1).reduce((a, b) => kernel("std:add", { a, b }), xs[0]),
);

export const when = component(
  { id: "when", inputs: { x: port.any() }, controls: { mask: control.frac() }, positional: ["mask", "x"] },
  (x: Signal, { mask }: Params) => kernel("std:scale", { in: x, by: mask }),
);

export const gain = component(
  { id: "gain", inputs: { in: port.audio() }, controls: { db: control.db(0).range(-60, 12).strict() }, output: port.audio(), positional: ["db"] },
  (x: Audio, { db }: Params) => kernel("std:gain", { in: x, db }),
);

export const square = component(
  { id: "square", controls: { freq: control.hz(440), width: control.frac(0.5) }, positional: ["freq", "width"], output: port.audio() },
  ({ freq, width }: Params) => kernel("std:square", { freq, width }),
);
"#;

/// The song `described` builds: `body` is its function's body, with every component of
/// `lib/fx.ts` and `@m2/core`'s helpers imported, and a track `kick` to pass where a signal goes.
fn song_src(body: &str) -> String {
    format!(
        r#"import {{ song, track, kernel, key, hz, ms, db, secs, frac }} from "@m2/core";
import {{ lowpass, delay, follow, mix, noise, sum, when, gain, square }} from "./lib/fx";
const kick = track("kick", () => kernel("std:sine"));
export default song(() => {{
{body}
}});
"#
    )
}

fn described(body: &str) -> Result<Build, Vec<Diagnostic>> {
    build_full(&options(&[("lib/fx.ts", LIB), ("song.ts", &song_src(body))]))
}

fn described_ok(body: &str) -> Build {
    described(body).unwrap_or_else(|e| panic!("{e:?}"))
}

/// The message of the one error `body` fails with.
fn described_err(body: &str) -> String {
    let e = described(body).err().unwrap_or_else(|| panic!("{body}: expected an error"));
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0].code, "code.build", "{}", e[0]);
    e[0].message.clone()
}

/// Every node of the song module, voices included.
fn song_nodes(b: &Build) -> Vec<&m2_ir::Node> {
    fn walk<'a>(ns: &'a [m2_ir::Node], out: &mut Vec<&'a m2_ir::Node>) {
        for n in ns {
            out.push(n);
            if let Op::Poly { body, .. } = &n.op {
                walk(&body.nodes, out);
            }
        }
    }
    let mut out = vec![];
    walk(&b.ir.modules.iter().find(|m| m.key == "song").expect("the song module").nodes, &mut out);
    out
}

/// The constants of declared controls: `(frame, slot, value, unit, origin)`.
type ControlConst = (u32, String, f64, Option<m2_ir::Unit>, m2_ir::Origin);

fn controls_of(b: &Build) -> Vec<ControlConst> {
    song_nodes(b)
        .into_iter()
        .filter_map(|n| match (&n.op, &n.origin) {
            (Op::Const { value, unit }, Some(o)) => o.control.as_ref().map(|c| (c.frame, c.slot.clone(), *value, *unit, o.clone())),
            _ => None,
        })
        .collect()
}

/// The one site of `file` whose source text is `text`.
fn site_of<'a>(b: &'a Build, file: &str, src: &str, text: &str) -> &'a sites::Site {
    let found: Vec<&sites::Site> = b.sites.iter().filter(|s| s.file == file && &src[s.start as usize..s.end as usize] == text).collect();
    assert_eq!(found.len(), 1, "{text}: {found:?}");
    found[0]
}

/// The frames of the song module that are instances of `component`.
fn frames_of<'a>(b: &'a Build, component: &str) -> Vec<(usize, &'a m2_ir::Frame)> {
    b.ir.modules[0].frames.iter().enumerate().filter(|(_, f)| f.component.as_deref() == Some(component)).collect()
}

// PKG-3, PKG-4: every declared control that isn't optional and isn't connected is exactly one
// constant made in its instance's frame, written or inherited; the body always gets a signal.
#[test]
fn a_declared_control_is_one_const_per_slot() {
    let body = r#"const x = kernel("std:saw", { freq: 110 });
const a = lowpass(x);
const b = lowpass(a, 800, { q: 0.9 });
const c = lowpass(b, hz(500));
return lowpass(c, x);"#;
    let b = described_ok(body);
    let m = &b.ir.modules[0];
    assert_eq!(frames_of(&b, "lowpass").len(), 4, "{:?}", m.frames);
    let ctl = controls_of(&b);
    let at = |frame: u32| -> Vec<(&str, f64, bool, Vec<&str>)> {
        ctl.iter().filter(|c| c.0 == frame).map(|(_, s, v, _, o)| (s.as_str(), *v, o.inherited(), o.arg.iter().map(String::as_str).collect())).collect()
    };
    // Left out: the descriptor's defaults, inherited, at the call with no argument path.
    assert_eq!(at(0), [("cutoff", 1000.0, true, vec![]), ("q", 0.707, true, vec![])]);
    // Written: where the number is, positionally or in the options object.
    assert_eq!(at(1), [("cutoff", 800.0, false, vec!["1"]), ("q", 0.9, false, vec!["2", "q"])]);
    // A quantity's origin is its own call, as for any quantity.
    assert_eq!(at(2), [("cutoff", 500.0, false, vec!["0"]), ("q", 0.707, true, vec![])]);
    let hz = site_of(&b, "song.ts", &song_src(body), "hz(500)");
    // Connected: no constant; the signal goes straight in.
    assert_eq!(at(3), [("q", 0.707, true, vec![])]);
    let nodes = song_nodes(&b);
    for (frame, slot, _, unit, o) in &ctl {
        // Made in its instance's frame, in its declared unit, smooth as declared.
        let node = nodes.iter().find(|n| n.origin.as_ref() == Some(o)).unwrap();
        assert_eq!(node.frame, Some(*frame), "{slot}");
        assert_eq!(*unit, Some(if slot == "cutoff" { m2_ir::Unit::Hz } else { m2_ir::Unit::Scalar }));
        assert!(o.control.as_ref().unwrap().smooth);
        assert_eq!(o.site, if o.arg == ["0"] { hz.id } else { m.frames[*frame as usize].site }, "{slot}");
    }
    let mut seen = std::collections::HashSet::new();
    assert!(ctl.iter().all(|c| seen.insert((c.0, c.1.clone()))), "one constant per (frame, slot)");
    // The kernel reads them: the body got a signal for every control.
    let svfs: Vec<&BTreeMap<String, String>> = nodes
        .iter()
        .filter_map(|n| match &n.op {
            Op::Kernel { kernel, inputs } if kernel == "std:svf" => Some(inputs),
            _ => None,
        })
        .collect();
    assert_eq!(svfs.len(), 4);
    assert!(svfs.iter().all(|i| i.contains_key("cutoff") && i.contains_key("q")));
}

// PKG-1: positional sugar keeps song syntax (`lowpass(x, 1000)`); `undefined` is "not given";
// one options object names any control or setting; defaults are spelled as declared (PKG-4).
#[test]
fn positional_sugar_options_and_settings() {
    let b = described_ok(
        r#"const x = kernel("std:saw", { freq: 110 });
const a = lowpass(x, undefined, 0.8);
const d = delay(a, { feedback: 0.5 });
const w = when(0.5, d);
const n = noise(7);
return sum(w, n, noise());"#,
    );
    let ctl = controls_of(&b);
    let slot = |s: &str| ctl.iter().filter(|c| c.1 == s).map(|c| (c.2, c.4.inherited(), c.4.arg.clone())).collect::<Vec<_>>();
    assert_eq!(slot("cutoff"), [(1000.0, true, vec![])]);
    assert_eq!(slot("q"), [(0.8, false, vec!["2".to_string()])]);
    assert_eq!(slot("feedback"), [(0.5, false, vec!["1".to_string(), "feedback".to_string()])]);
    assert_eq!(slot("mask"), [(0.5, false, vec!["0".to_string()])]);
    // A default in another spelling is that spelling's constructor's value, bit for bit: ms(250).
    let time = ctl.iter().find(|c| c.1 == "time").unwrap();
    assert_eq!((time.2.to_bits(), time.3, time.4.inherited()), ((250.0f64 * 0.001).to_bits(), Some(m2_ir::Unit::Seconds), true));
    // D3: a setting is a plain value, never a constant of a control: noise's seed goes into the
    // kernel as written, or as its default.
    let nodes = song_nodes(&b);
    let seeds: Vec<f64> = nodes
        .iter()
        .filter_map(|n| match &n.op {
            Op::Kernel { kernel, inputs } if kernel == "std:noise" => {
                let seed = nodes.iter().find(|c| c.id == inputs["seed"]).unwrap();
                assert!(seed.origin.is_none(), "a setting is no knob");
                match seed.op {
                    Op::Const { value, .. } => Some(value),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect();
    assert_eq!(seeds, [7.0, 1.0]);
    assert!(!ctl.iter().any(|c| c.1 == "seed"));
    // A `many` input takes every signal; the spread and zero-argument calls keep their sites.
    assert_eq!(frames_of(&b, "sum").len(), 1);
    assert_eq!(frames_of(&b, "noise").len(), 2);
}

// PKG-5, PKG-6, D2: a dictionary slot keeps the caller's keys, in order; an entry may be a bare
// signal; an optional control left out makes no node.
#[test]
fn dictionary_slots_and_optional_controls() {
    let body = r#"const x = kernel("std:saw", { freq: 110 });
const y = kernel("std:sine");
return mix({ kick: x, hats: { in: y, db: db(-6), pan: 0.3 }, keys: { in: y, pan: -0.25 } });"#;
    let b = described_ok(body);
    let ctl = controls_of(&b);
    let slots: Vec<(&str, f64, Vec<&str>, bool)> = ctl.iter().map(|c| (c.1.as_str(), c.2, c.4.arg.iter().map(String::as_str).collect(), c.4.control.as_ref().unwrap().smooth)).collect();
    assert_eq!(slots, [
        ("channels.hats.db", -6.0, vec!["0"], false),
        ("channels.hats.pan", 0.3, vec!["0", "hats", "pan"], true),
        ("channels.keys.pan", -0.25, vec!["0", "keys", "pan"], true),
    ]);
    let db = site_of(&b, "song.ts", &song_src(body), "db(-6)");
    assert_eq!(ctl[0].4.site, db.id);
    // The body sums the strips in the caller's order: kick, hats, keys.
    let nodes = song_nodes(&b);
    let kinds: Vec<&str> = nodes.iter().filter_map(|n| match &n.op {
        Op::Kernel { kernel, .. } => Some(kernel.as_str()),
        _ => None,
    }).collect();
    assert_eq!(kinds, ["std:saw", "std:sine", "std:pan", "std:gain", "std:pan", "std:add", "std:add"]);
    for (body, want) in [
        ("return mix({ Hats: kernel(\"std:sine\") });", "the channels key \"Hats\" is not a valid key"),
        ("return mix({ hats: { in: kernel(\"std:sine\"), gain: 1 } });", "channels.hats has no \"gain\""),
        ("return mix({ hats: { db: 1 } });", "channels.hats: the input in is missing"),
        ("return mix(kernel(\"std:sine\"));", "channels must be an object of entries"),
        ("return mix();", "channels is missing"),
    ] {
        let e = described_err(body);
        assert!(e.contains(want), "{body}: {e}");
    }
}

// D1, D16: a call that doesn't fit the descriptor is a build error that says what to write.
#[test]
fn calls_that_dont_fit_are_errors() {
    for (body, want) in [
        ("return delay(kernel(\"std:sine\"), { tme: 1 });", "delay(): no option \"tme\" (options: time, feedback, mix)"),
        ("return delay(kernel(\"std:sine\"), { in: 1 });", "\"in\" is an input, given as an argument"),
        ("return lowpass(kernel(\"std:sine\"), 800, { cutoff: 900 });", "\"cutoff\" is given twice"),
        ("return lowpass(kernel(\"std:sine\"), { q: 1 }, 800);", "the options object must come last"),
        ("return lowpass(kernel(\"std:sine\"), 1, 2, 3);", "takes at most 3 arguments"),
        ("return delay();", "the input in is missing"),
        ("return delay({ time: 1 });", "the input in is missing"),
        ("return when(kernel(\"std:sine\"));", "the input x is missing"),
        ("return lowpass(kernel(\"std:sine\"), \"800\");", "cutoff: expected a number, a quantity like hz(440) or a signal, not \"800\""),
        ("return lowpass(kernel(\"std:sine\"), Infinity);", "cutoff: Infinity is not a finite number"),
        ("return noise(\"seven\");", "seed must be a whole number, not \"seven\""),
        ("return noise(1.5);", "seed must be a whole number, not 1.5"),
        ("return delay(\"x\");", "the input in must be a signal, not \"x\""),
        // A track's handle is never a signal: the message names the export to use.
        ("return delay(kick);", "in: `kick` is a track; use `kick.out`"),
        ("return mix({ kick });", "channels.kick: `kick` is a track; use `kick.out`"),
        ("return lowpass(kernel(\"std:sine\"), { q: kick });", "q: `kick` is a track"),
        // Point-free Array.map passes the index and the array too.
        ("return sum(...[kernel(\"std:sine\")].map(delay));", "Write xs.map((x) => delay(x))"),
        ("return lowpass(kernel(\"std:sine\"), [1]);", "not an object"),
        // PKG-8: a strict range is a validation constraint; a value outside it is never clamped.
        ("return gain(kernel(\"std:sine\"), 20);", "db 20 is outside its range -60..12"),
        ("return gain(kernel(\"std:sine\"), db(-70));", "db -70 is outside its range"),
    ] {
        let e = described_err(body);
        assert!(e.contains(want), "{body}: {e}");
    }
    // Checking a track handle reads no export of it: no `kick.in` import is made.
    let b = described_ok(
        r#"let said = "";
try { delay(kick); } catch (e) { said = String(e); }
try { mix({ kick }); } catch (e) { said += String(e); }
if (!said.includes("kick.out")) throw new Error(said);
return kernel("std:sine");"#,
    );
    assert!(!song_nodes(&b).iter().any(|n| matches!(&n.op, Op::Import { .. })), "no import of the track was made");
}

// ID-15, PKG-9: a frame records its call's ComponentId, which is the descriptor's id, never the
// name it is imported or called by; frames nest, `key()` frames carry their key, and a call that
// runs twice in one parent counts its occurrences.
#[test]
fn frames_record_component_ids_occurrences_and_keys() {
    let song = r#"import { song, kernel, key } from "@m2/core";
import { lowpass as echo } from "./lib/fx";
import * as fx from "./lib/fx";
const twice = (x: any) => [0, 1].reduce((y: any) => fx.lowpass(y), x);
export default song(() => key("chain", () => echo(twice(kernel("std:saw")), 900)));
"#;
    let b = build_full(&options(&[("lib/fx.ts", LIB), ("song.ts", song)])).unwrap_or_else(|e| panic!("{e:?}"));
    let m = &b.ir.modules[0];
    let shown: Vec<_> = m
        .frames
        .iter()
        .map(|f| {
            let s = b.sites.iter().find(|s| s.id == f.site).unwrap();
            (f.component.as_deref(), f.key.as_deref(), f.occurrence, f.parent, s.callee.as_str())
        })
        .collect();
    assert_eq!(shown, [
        (None, Some("chain"), 0, None, "key"),
        (Some("lowpass"), None, 0, Some(0), "fx.lowpass"),
        (Some("lowpass"), None, 1, Some(0), "fx.lowpass"),
        (Some("lowpass"), None, 0, Some(0), "echo"),
    ]);
    // The svf each call made is in its frame, inside the key() frame.
    let svf_frames: Vec<Option<u32>> = m.nodes.iter().filter(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == "std:svf")).map(|n| n.frame).collect();
    assert_eq!(svf_frames, [Some(1), Some(2), Some(3)]);
    assert_eq!(m.chain(Some(3)).iter().map(|f| f.site).collect::<Vec<_>>(), [m.frames[0].site, m.frames[3].site]);
    // The component calls a node was made inside are the component frames of its chain.
    let svf = m.nodes.iter().find(|n| n.frame == Some(3) && matches!(n.op, Op::Kernel { .. })).unwrap();
    let calls: Vec<u32> = m.chain(svf.frame).iter().filter(|f| f.component.is_some()).map(|f| f.site).collect();
    assert_eq!(calls, [m.frames[3].site]);
}

// RENDER-7, WRITE-8: `time` from @m2/core reads song time anywhere. Outside any call it is the
// module's one node per reading, the same as `ctx.time`'s. Inside a component call it is one node
// per call, made in its frame, however often the body reads it; a voice inside the call has its own.
#[test]
fn time_is_one_node_per_call() {
    let lib = r#"import { component, kernel, poly, port, signature, time, type Audio, type Events } from "@m2/core";
export const wob = component(
  { id: "wob", signature: signature<(x: Audio) => Audio>(), inputs: { in: port.audio() }, output: port.audio() },
  (x: Audio) => kernel("std:scale", { in: kernel("std:scale", { in: x, by: time.playing }), by: time.playing }),
);
export const voiced = component(
  { id: "voiced", signature: signature<(n: Events) => Audio>(), inputs: { notes: port.notes() }, output: port.audio() },
  (n: Events) => kernel("std:scale", { in: poly(n, () => kernel("std:scale", { in: kernel("std:saw"), by: time.playing })), by: time.playing }),
);
"#;
    let song = r#"import { kernel, song, time, track } from "@m2/core";
import { voiced, wob } from "./lib/wob";
const v = track("v", ({ lane }) => voiced(lane.notes("n")));
export default song((ctx) => {
  const a = wob(wob(kernel("std:saw")));
  const b = kernel("std:scale", { in: a, by: time.playing });
  return kernel("std:add", { a: kernel("std:scale", { in: b, by: ctx.time.playing }), b: kernel("std:scale", { in: v.out, by: time.beats }) });
});
"#;
    let b = build_full(&options(&[("lib/wob.ts", lib), ("song.ts", song)])).unwrap_or_else(|e| panic!("{e:?}"));
    // Every `time` node of a module, voices included: (what it reads, its frame, whether in a voice).
    fn times(ns: &[m2_ir::Node], voice: bool, out: &mut Vec<(String, Option<u32>, bool)>) {
        for n in ns {
            match &n.op {
                Op::Time { of } => out.push((serde_json::to_value(of).unwrap().as_str().unwrap().to_string(), n.frame, voice)),
                Op::Poly { body, .. } => times(&body.nodes, true, out),
                _ => {}
            }
        }
    }
    let of = |key: &str| {
        let m = b.ir.modules.iter().find(|m| m.key == key).unwrap();
        let mut out = vec![];
        times(&m.nodes, false, &mut out);
        (m, out)
    };
    let (m, song_times) = of("song");
    let wobs: Vec<u32> =
        m.frames.iter().enumerate().filter(|(_, f)| f.component.as_deref() == Some("wob")).map(|(i, _)| i as u32).collect();
    assert_eq!(wobs.len(), 2);
    let playing = |f: Option<u32>| ("playing".to_string(), f, false);
    // The inner call runs first; each reads twice, and has one node. `time.playing` and
    // `ctx.time.playing` at the top are one node; `time.beats` is another.
    assert_eq!(song_times, [playing(Some(wobs[0])), playing(Some(wobs[1])), playing(None), ("beats".to_string(), None, false)]);
    // In the track, the call's own node, and the voice's own, both in the call's frame.
    let (m, track_times) = of("v");
    let voiced = m.frames.iter().position(|f| f.component.as_deref() == Some("voiced")).unwrap() as u32;
    assert_eq!(track_times, [("playing".to_string(), Some(voiced), true), ("playing".to_string(), Some(voiced), false)]);
}

// ID-15, D17: a call site belongs only to the call it was set for. A component handed to native
// code (Array.map, Array.from) takes no site, so it makes no frame and no knob; the calls an
// arrow makes take theirs.
#[test]
fn a_native_caller_takes_no_site() {
    let body = r#"const x = kernel("std:saw", { freq: 110 });
const ys = [x].map((y) => lowpass(y));
const z = Array.from({ length: 1 }, square);
const f = [110].map(hz);
return sum(kernel("std:sine", { freq: f[0] }), ...ys, ...z);"#;
    let b = described_ok(body);
    let src = song_src(body);
    let lp = frames_of(&b, "lowpass");
    assert_eq!(lp.len(), 1);
    assert_eq!(lp[0].1.site, site_of(&b, "song.ts", &src, "lowpass(y)").id, "the arrow's call, not map's");
    assert!(frames_of(&b, "square").is_empty(), "Array.from's call is not square's");
    // A call ending in a spread, and one with no arguments, keep their sites.
    let sum = site_of(&b, "song.ts", &src, "sum(kernel(\"std:sine\", { freq: f[0] }), ...ys, ...z)");
    assert_eq!(frames_of(&b, "sum").iter().map(|(_, f)| f.site).collect::<Vec<_>>(), [sum.id]);
    let map = site_of(&b, "song.ts", &src, "[x].map((y) => lowpass(y))");
    assert!(b.ir.modules[0].frames.iter().all(|f| f.site != map.id));
    // Its controls are constants with no origin, as any number a native call passes.
    let nodes = song_nodes(&b);
    let consts = |v: f64| nodes.iter().filter(|n| matches!(n.op, Op::Const { value, .. } if value == v)).map(|n| n.origin.is_some()).collect::<Vec<_>>();
    assert_eq!(consts(440.0), [false], "square's freq, inherited, with no instance to name");
    assert_eq!(consts(110.0).iter().filter(|o| **o).count(), 0, "hz(110) made by map has no site");
    // The old form takes no site from native code either (std no longer uses it, so a project declares one).
    let lib = r#"import { component, kernel, type Value } from "@m2/core";
export const sine = component({ controls: ["0"] }, (f: Value) => kernel("std:sine", { freq: f }));
"#;
    let legacy = r#"import { song } from "@m2/core";
import { sine } from "./lib/sine";
export default song(() => [440].map(sine)[0]);
"#;
    let b = build_full(&options(&[("lib/sine.ts", lib), ("song.ts", legacy)])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(b.ir.modules[0].frames.is_empty(), "{:?}", b.ir.modules[0].frames);
    assert!(b.ir.modules[0].nodes.iter().all(|n| n.frame.is_none() && n.origin.as_ref().is_none_or(|o| o.arg.is_empty())));
}

// PKG-13: what a sidechain input received is recorded on its frame, whatever was called first.
#[test]
fn sidechain_inputs_are_recorded() {
    let b = described_ok(
        r#"const k = kernel("std:saw", { freq: 50 });
const key = lowpass(k, 200);
const env = follow(key);
return when(env, kernel("std:sine"));"#,
    );
    let m = &b.ir.modules[0];
    let lp_out = b.outputs.iter().find(|o| o.site == frames_of(&b, "lowpass")[0].1.site).unwrap();
    let (_, f) = frames_of(&b, "follow")[0];
    assert_eq!(f.sidechain, [m2_ir::Sidechain { input: "in".into(), source: lp_out.port.clone() }]);
    assert!(m.frames.iter().filter(|f| f.component.as_deref() != Some("follow")).all(|f| f.sidechain.is_empty()));
}

// PKG-13, PKG-6: a dictionary entry's sidechain input is recorded too, named by its path as the
// entry's controls are, whether the entry is an object or a bare signal.
#[test]
fn an_entry_sidechain_input_is_recorded() {
    let song = r#"import { component, control, each, kernel, port, song, type Audio, type Params } from "@m2/core";
const duckBy = component(
  {
    id: "duck-by",
    inputs: { in: port.audio(), key: port.audio({ sidechain: true }) },
    controls: { others: each({ side: port.audio({ sidechain: true }), depth: control.db(-8) }) },
    output: port.audio(),
  },
  (x: Audio, key: Audio, { others }: Params) =>
    Object.values(others).reduce(
      (y: any, o: any) => kernel("std:duck", { in: y, side: o.side, depth: o.depth }),
      kernel("std:duck", { in: x, side: key }),
    ),
);
export default song(() => {
  const k = kernel("std:sine", { freq: 50 });
  const h = kernel("std:square", { freq: 3000 });
  const s = kernel("std:tri", { freq: 2000 });
  return duckBy(kernel("std:saw", { freq: 110 }), k, { others: { hats: { side: h }, snare: s } });
});
"#;
    let b = build_full(&options(&[("song.ts", song)])).unwrap_or_else(|e| panic!("{e:?}"));
    let m = &b.ir.modules[0];
    let made = |k: &str| m.nodes.iter().find(|n| matches!(&n.op, Op::Kernel { kernel, .. } if kernel == k)).unwrap().id.clone();
    let (_, f) = frames_of(&b, "duck-by")[0];
    let got: Vec<(&str, &str)> = f.sidechain.iter().map(|s| (s.input.as_str(), s.source.as_str())).collect();
    let (k, h, s) = (made("std:sine"), made("std:square"), made("std:tri"));
    assert_eq!(got, [("key", k.as_str()), ("others.hats.side", h.as_str()), ("others.snare.side", s.as_str())]);
}

// PKG-1, design 1.2 and 1.3: the call type's options are `options?`, so a helper may forward
// options it wasn't given. `undefined` where the options go is no options, after the last
// positional argument or in its place; anywhere else it is still too many arguments. The frame
// records which argument held the options (PKG-3: only the call can tell options from a value).
#[test]
fn options_left_out_may_be_forwarded() {
    let body = r#"const x = kernel("std:saw", { freq: 110 });
const space = (y: any, echo?: { time?: number; mix?: number }) => delay(y, echo);
const tone = (y: any, o?: { q?: number }) => lowpass(y, 800, 0.7, o);
const a = space(x);
const b = tone(a);
const c = lowpass(b, space(b, { mix: 0.5 }));
const d = lowpass(c, undefined);
const e = mix({ d: lowpass(d, { q: 2 }) }, undefined);
return lowpass(e, 900);"#;
    let b = described_ok(body);
    let src = song_src(body);
    let options = |call: &str| {
        let site = site_of(&b, "song.ts", &src, call).id;
        b.ir.modules[0].frames.iter().filter(|f| f.site == site).map(|f| f.options).collect::<Vec<_>>()
    };
    assert_eq!(options("delay(y, echo)"), [Some(1), Some(1)], "an options argument, given or not");
    assert_eq!(options("lowpass(y, 800, 0.7, o)"), [Some(3)]);
    assert_eq!(options("lowpass(b, space(b, { mix: 0.5 }))"), [None], "a signal at the cutoff's place");
    assert_eq!(options("lowpass(c, undefined)"), [None], "`undefined` at the cutoff's place is no cutoff");
    assert_eq!(options("lowpass(d, { q: 2 })"), [Some(1)]);
    assert_eq!(options("mix({ d: lowpass(d, { q: 2 }) }, undefined)"), [Some(1)]);
    assert_eq!(options("lowpass(e, 900)"), [None]);
    let mix = controls_of(&b).into_iter().filter(|c| c.1 == "mix").map(|c| (c.2, c.4.inherited())).collect::<Vec<_>>();
    assert_eq!(mix, [(0.3, true), (0.5, false)]);
    for (body, want) in [
        ("return delay(kernel(\"std:sine\"), undefined, undefined);", "delay(): takes at most 1 argument"),
        ("return lowpass(kernel(\"std:sine\"), 1, 2, undefined, {});", "lowpass(): takes at most 3 arguments"),
        // Nothing to take options, so nothing to leave out.
        ("return sum(kernel(\"std:sine\"), undefined);", "the input xs must be a signal, not undefined"),
    ] {
        let e = described_err(body);
        assert!(e.contains(want), "{body}: {e}");
    }
}

// PKG-8: a strict range holds for the unit it is declared in. A value in a unit the control
// accepts is held to that unit's range, never compared with the control's own.
#[test]
fn a_strict_range_holds_for_the_values_unit() {
    let song = |call: &str| {
        format!(
            r#"import {{ component, control, hz, kernel, khz, port, semis, song, type Params }} from "@m2/core";
const tone = component(
  {{
    id: "tone",
    controls: {{ freq: control.hz(440).range(20, 20000).strict().accepts("semitones", 0, 127) }},
    output: port.audio(),
    positional: ["freq"],
  }},
  ({{ freq }}: Params) => kernel("std:sine", {{ freq: freq.unit === "semitones" ? kernel("std:mtof", {{ pitch: freq }}) : freq }}),
);
export default song(() => {call});
"#
        )
    };
    for call in ["tone(semis(10))", "tone(semis(127))", "tone(hz(20))", "tone(440)", "tone()"] {
        build_full(&options(&[("song.ts", &song(call))])).unwrap_or_else(|e| panic!("{call}: {e:?}"));
    }
    for (call, want) in [
        ("tone(semis(200))", "tone(): freq 200 semitones is outside its range 0..127 semitones"),
        ("tone(10)", "tone(): freq 10 is outside its range 20..20000"),
        ("tone(khz(30))", "tone(): freq 30000 is outside its range 20..20000"),
    ] {
        let e = build_full(&options(&[("song.ts", &song(call))])).err().unwrap_or_else(|| panic!("{call}: expected an error"));
        assert!(e[0].message.contains(want), "{call}: {}", e[0].message);
    }
}

// PKG-10, PKG-11: the build emits the unit table generated from `@m2/core`, with exactly one
// canonical spelling per unit, and the descriptor of every component its modules declare,
// numbers in base units with their spelling.
#[test]
fn descriptors_and_the_unit_table_are_emitted() {
    let b = described_ok("return delay(kernel(\"std:saw\"));");
    let units: Vec<(&str, &str, f64, bool)> = b.units.iter().map(|u| (u.name.as_str(), u.unit.name(), u.scale, u.canonical)).collect();
    assert_eq!(units, [
        ("hz", "hz", 1.0, true),
        ("khz", "hz", 1000.0, false),
        ("db", "db", 1.0, true),
        ("linear", "gain", 1.0, true),
        ("semis", "semitones", 1.0, true),
        ("secs", "seconds", 1.0, true),
        ("ms", "seconds", 0.001, false),
        ("beats", "beats", 1.0, true),
        ("frac", "unit01", 1.0, true),
        ("bipolar", "bipolar", 1.0, true),
        ("scalar", "scalar", 1.0, true),
    ]);
    for u in m2_ir::Unit::ALL {
        let canonical: Vec<&UnitCtor> = b.units.iter().filter(|c| c.unit == u && c.canonical).collect();
        assert_eq!(canonical.len(), 1, "{u:?}");
        assert_eq!(canonical[0].scale, 1.0, "a canonical spelling writes base units");
    }
    // Every spelling is an `@m2/core` export of that name: the table is the constructors' own.
    for u in &b.units {
        assert!(SDK.contains(&format!("export const {} = unitFn(\"{}\");", u.name, u.name)), "{}", u.name);
    }

    let ids: Vec<&str> = b.components.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, ["lowpass", "delay", "follow", "mix", "noise", "sum", "when", "gain", "square"]);
    let d = |id: &str| b.components.iter().find(|d| d.id == id).unwrap();
    let delay = d("delay");
    assert_eq!((delay.module.as_str(), delay.file.as_str(), delay.label.as_str()), ("lib/fx.ts", "lib/fx.ts", "delay"));
    assert_eq!(delay.at, format!("lib/fx.ts:{}:22", LIB.lines().position(|l| l.starts_with("export const delay")).unwrap() + 1));
    let time = delay.control("time").unwrap();
    assert_eq!(time.unit, Some(m2_ir::Unit::Seconds));
    let spelled = component::Spelled { value: 250.0 * 0.001, spelled: Some("ms".into()), written: 250.0 };
    assert_eq!(time.default.as_ref(), Some(&spelled));
    assert_eq!((time.range, time.log, time.smooth, time.strict), (Some((0.001, 2.0)), true, true, false));
    assert_eq!(delay.editor.category.as_deref(), Some("Delay & Reverb"));
    let insert: Vec<(&str, &component::Spelled)> = delay.editor.insert.iter().map(|i| (i.slot.as_str(), &i.value)).collect();
    let mix = component::Spelled { value: 0.3, spelled: Some("frac".into()), written: 0.3 };
    assert_eq!(insert, [("time", &spelled), ("mix", &mix)]);
    assert!(delay.is_effect() && !d("mix").is_effect());

    let lowpass = d("lowpass");
    assert_eq!(lowpass.positional, ["cutoff", "q"]);
    assert_eq!(lowpass.inputs[0].kind, component::PortKind::Audio);
    // A value in the declared unit shows the declared range; in another unit, none of it (D4).
    assert_eq!(lowpass.range_for("cutoff", Some(m2_ir::Unit::Hz)), Some(((20.0, 20000.0), true)));
    assert_eq!(lowpass.range_for("cutoff", Some(m2_ir::Unit::Semitones)), None);
    assert!(d("follow").inputs[0].sidechain);
    assert!(d("sum").inputs[0].many);
    assert_eq!(d("when").controls[0].default, None, "a required control has no default");
    assert_eq!(d("noise").settings[0].default, Some(serde_json::json!(1)));
    // A dictionary slot's controls are found by `<slot>.<key>.<control>`.
    let pan = d("mix").control("channels.hats.pan").unwrap();
    assert!(pan.optional && pan.smooth && pan.unit == Some(m2_ir::Unit::Bipolar));
    assert_eq!(d("mix").range_for("channels.hats.db", Some(m2_ir::Unit::Db)), None, "declared without a range");
}

// PKG-9, PKG-8, D16: a descriptor is checked where it is declared, and an error there is a build
// error at its `component()` call.
#[test]
fn descriptors_are_checked_where_declared() {
    let declare = |decl: &str| -> Diagnostic {
        let lib = format!(
            "import {{ component, control, port, setting, each, kernel }} from \"@m2/core\";\nexport const c = component({decl}, (x: any) => x);\n"
        );
        // The import is used, or TypeScript would drop it.
        let song = "import { song, kernel } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(() => (c, kernel(\"std:sine\")));\n";
        let mut e = build(&options(&[("lib/c.ts", &lib), ("song.ts", song)])).expect_err(decl);
        assert_eq!(e.len(), 1, "{e:?}");
        e.remove(0)
    };
    let a = "inputs: { in: port.audio() }, output: port.audio()";
    for (decl, want) in [
        (format!("{{ id: \"Low Pass\", {a} }}"), "the component id \"Low Pass\" is not a valid key"),
        (format!("{{ id: \"lp\", ctrls: {{}}, {a} }}"), "no field \"ctrls\""),
        (format!("{{ id: \"lp\", {a}, controls: {{ cutoff: control.hz(1000).range(0, 100).log() }} }}"), ".log() needs a range above 0"),
        (format!("{{ id: \"lp\", {a}, controls: {{ cutoff: control.hz(1000).log() }} }}"), ".log() needs a range"),
        (format!("{{ id: \"lp\", {a}, controls: {{ cutoff: control.hz(1000).range(20, 500).strict() }} }}"), "outside its strict range"),
        (format!("{{ id: \"lp\", {a}, controls: {{ cutoff: control.hz(NaN) }} }}"), "must be a finite number"),
        (format!("{{ id: \"lp\", {a}, controls: {{ cutoff: control.hz(1).accepts(\"hz\", 0, 1) }} }}"), "accepts its own unit"),
        (format!("{{ id: \"lp\", {a}, controls: {{ cutoff: 1000 }} }}"), "must be declared with control."),
        (format!("{{ id: \"lp\", {a}, controls: {{ in: control.hz(1) }} }}"), "\"in\" is declared twice"),
        (format!("{{ id: \"lp\", {a}, positional: [\"cutoff\"] }}"), "positional names \"cutoff\", which isn't"),
        ("{ id: \"lp\", inputs: { a: port.audio(), b: port.audio() }, positional: [\"b\"] }".into(), "positional names some inputs but not \"a\""),
        (format!("{{ id: \"lp\", {a}, controls: {{ cutoff: control.hz(1), q: control.scalar() }}, positional: [\"cutoff\", \"q\"] }}"), "comes before one that may not"),
        ("{ id: \"lp\", inputs: { xs: port.any().many(), y: port.any() } }".into(), "\"xs\" takes many signals, so it must come last"),
        (format!("{{ id: \"lp\", {a}, settings: {{ voices: setting.int(1.5) }} }}"), "must be a whole number"),
        (format!("{{ id: \"lp\", {a}, editor: {{ category: \"Filter\", insert: {{ cutoff: 1 }} }} }}"), "editor.insert names \"cutoff\", which isn't a control"),
        ("{ id: \"lp\", inputs: { x: port.control() }, output: port.audio(), editor: { category: \"Filter\" } }".into(), "takes audio as its first argument"),
        // A category with notes first is an instrument: it makes audio, from its notes alone (PKG-7).
        ("{ id: \"lp\", inputs: { notes: port.notes() }, output: port.control(), editor: { category: \"Drums\" } }".into(), "an instrument (editor.category, notes first) outputs audio"),
        ("{ id: \"lp\", inputs: { notes: port.notes(), x: port.audio() }, output: port.audio(), editor: { category: \"Drums\" } }".into(), "has one input"),
        ("{ id: \"lp\", inputs: { notes: port.notes() }, controls: { c: control.hz() }, output: port.audio(), editor: { category: \"Drums\" } }".into(), "from its notes alone, which leaves out \"c\""),
        ("{ id: \"lp\", inputs: { notes: port.notes() }, controls: { c: control.hz(1) }, output: port.audio(), editor: { category: \"Drums\", insert: { c: 2 } } }".into(), "has no editor.insert"),
        (format!("{{ id: \"lp\", {a}, controls: {{ c: control.hz() }}, editor: {{ category: \"Filter\" }} }}"), "leaves out \"c\""),
    ] {
        let e = declare(&decl);
        assert_eq!(e.code, "code.build", "{decl}: {e}");
        assert!(e.message.contains(want), "{decl}: {e}");
        assert!(e.src.as_deref().unwrap_or("").starts_with("lib/c.ts:2:"), "{decl}: {:?}", e.src);
    }
}

// Polish A4: an instrument names what its notes play (`editor.notes`), for the piano roll's rows: a
// drum kit's say "Kick", "Snare". Whole MIDI notes only, each name one line of 1 to 24 characters.
#[test]
fn editor_notes_are_read_and_checked() {
    let lib = |notes: &str| {
        format!(
            "import {{ component, port, kernel }} from \"@m2/core\";\nexport const c = component({{ id: \"kit\", inputs: {{ notes: port.notes() }}, output: port.audio(), editor: {{ category: \"Drums\", notes: {notes} }} }}, (n: any) => kernel(\"std:sine\"));\n"
        )
    };
    let song =
        "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(({ lane }) => c(lane.notes(\"hits\")));\n";
    let b = build_full(&options(&[("lib/c.ts", &lib("{ 36: \"Kick\", 38: \"Snare\", 0: \"Low\" }")), ("song.ts", song)]))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let kit = b.components.iter().find(|d| d.id == "kit").expect("the kit's descriptor");
    let want: BTreeMap<u8, String> = [(0, "Low"), (36, "Kick"), (38, "Snare")].into_iter().map(|(k, v)| (k, v.to_string())).collect();
    assert_eq!(kit.editor.notes, want);
    // A component that names none has none.
    let plain = "import { component, port, kernel } from \"@m2/core\";\nexport const c = component({ id: \"kit\", inputs: { notes: port.notes() }, output: port.audio(), editor: { category: \"Drums\" } }, (n: any) => kernel(\"std:sine\"));\n";
    let b = build_full(&options(&[("lib/c.ts", plain), ("song.ts", song)])).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(b.components[0].editor.notes.is_empty());
    for (notes, want) in [
        ("{ 128: \"High\" }", "editor.notes names the note 128: notes are whole numbers 0 to 127"),
        ("{ \"1.5\": \"Half\" }", "editor.notes names the note 1.5"),
        ("{ \"-1\": \"Below\" }", "editor.notes names the note -1"),
        ("{ 36: \"Kick\\ndrum\" }", "editor.notes' name of 36 must be one line of 1 to 24 characters"),
        ("{ 36: \" \" }", "editor.notes' name of 36 must be one line"),
        ("{ 36: \"A very long name for a kick\" }", "editor.notes' name of 36 must be one line of 1 to 24 characters"),
        ("{ 36: 5 }", "editor.notes' name of 36 must be one line"),
        ("[\"Kick\"]", "editor.notes must be an object"),
    ] {
        let mut e = build(&options(&[("lib/c.ts", &lib(notes)), ("song.ts", song)])).expect_err(notes);
        assert_eq!(e.len(), 1, "{e:?}");
        let e = e.remove(0);
        assert_eq!(e.code, "code.build", "{notes}: {e}");
        assert!(e.message.contains(want), "{notes}: {e}");
        assert!(e.src.as_deref().unwrap_or("").starts_with("lib/c.ts:2:"), "{notes}: {:?}", e.src);
    }
}

// PKG-9, D15: one definition site with one descriptor is one component, however often it runs
// (a factory); the same id from two sites, or one site declaring two descriptors, is an error.
#[test]
fn a_factory_component_registers_once() {
    let lib = r#"import { component, control, port, kernel, type Params } from "@m2/core";
export function makeGain(n: number) {
  return component({ id: "gain", inputs: { in: port.audio() }, controls: { db: control.db(n) }, output: port.audio() },
    (x: any, { db }: Params) => kernel("std:gain", { in: x, db }));
}
"#;
    let build_with = |song_body: &str| {
        let song = format!("import {{ song, kernel }} from \"@m2/core\";\nimport {{ makeGain }} from \"./lib/g\";\n{song_body}\n");
        build_full(&options(&[("lib/g.ts", lib), ("song.ts", &song)]))
    };
    let b = build_with("const a = makeGain(0);\nconst b = makeGain(0);\nexport default song(() => b(a(kernel(\"std:sine\"))));").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(b.components.len(), 1);
    assert_eq!(b.ir.modules[0].frames.iter().filter(|f| f.component.as_deref() == Some("gain")).count(), 2);
    let e = build_with("const a = makeGain(0);\nconst b = makeGain(-6);\nexport default song(() => b(a(kernel(\"std:sine\"))));").err().expect("two descriptors from one site");
    assert!(e[0].message.contains("declared again here with another descriptor"), "{}", e[0]);
    let two = r#"import { component, port } from "@m2/core";
export const a = component({ id: "dup", inputs: { in: port.audio() } }, (x: any) => x);
export const b = component({ id: "dup", inputs: { in: port.audio() } }, (x: any) => x);
"#;
    let song = "import { song, kernel } from \"@m2/core\";\nimport { a } from \"./lib/two\";\nexport default song(() => a(kernel(\"std:sine\")));\n";
    let e = build(&options(&[("lib/two.ts", two), ("song.ts", song)])).unwrap_err();
    assert!(e[0].message.contains("another component has the id \"dup\"") && e[0].message.contains("lib/two.ts:2:"), "{}", e[0]);
}

// PKG-1: the old form keeps working: its numbers are knobs at their paths, and its call is a
// frame with no ComponentId, opened after its constants, which stay the caller's.
#[test]
fn the_old_component_form_still_works() {
    let lib = r#"import { component, kernel, type Value } from "@m2/core";
export const lp = component({ controls: ["1"] }, (x: any, cutoff: Value) => kernel("std:svf", { in: x, cutoff }));
"#;
    let song = "import { song, kernel } from \"@m2/core\";\nimport { lp } from \"./lib/lp\";\nexport default song(() => lp(kernel(\"std:sine\"), 800));\n";
    let b = build_full(&options(&[("lib/lp.ts", lib), ("song.ts", song)])).unwrap_or_else(|e| panic!("{e:?}"));
    let m = &b.ir.modules[0];
    assert_eq!(m.frames.len(), 1);
    assert_eq!((m.frames[0].component.as_ref(), m.frames[0].key.as_ref()), (None, None));
    let c = m.nodes.iter().find(|n| matches!(n.op, Op::Const { value, .. } if value == 800.0)).unwrap();
    assert_eq!(c.frame, None);
    assert_eq!(c.origin.as_ref().map(|o| (o.arg.clone(), o.control.is_none())), Some((vec!["1".to_string()], true)));
    let svf = m.nodes.iter().find(|n| matches!(n.op, Op::Kernel { .. }) && n.frame == Some(0)).unwrap();
    assert_eq!(m.chain(svf.frame).iter().map(|f| f.site).collect::<Vec<_>>(), [m.frames[0].site]);
    assert!(b.components.is_empty(), "the old form declares no descriptor");
}

/// A package folder for one test, `<temp>/<name>/src/<file>` for each file; removed when dropped.
struct TempPackage(PathBuf);

impl TempPackage {
    fn new(name: &str, files: &[(&str, &str)]) -> TempPackage {
        let dir = std::env::temp_dir().join(format!("m2-script-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, text) in files {
            let path = dir.join("src").join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        TempPackage(dir)
    }
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Build `song` with the package `pkg` installed and listed.
fn with_package(pkg: &TempPackage, song: &str) -> Result<Build, Vec<Diagnostic>> {
    let mut o = Options::new(code(&[("song.ts", song)]), "song.ts");
    o.packages.push(("pkg".into(), pkg.0.clone()));
    o.dependencies.push("pkg".into());
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

const FX: &str = r#"import { component, control, port, kernel, type Audio, type Params } from "@m2/core";
export const lowpass = component(
  { id: "lowpass", inputs: { in: port.audio() }, controls: { cutoff: control.hz(1000) }, output: port.audio(), positional: ["cutoff"],
    editor: { category: "Filter", insert: { cutoff: 1000 } } },
  (x: Audio, { cutoff }: Params) => kernel("std:svf", { in: x, cutoff }).port("lp"),
);
export const osc = {
  saw: component({ id: "saw", controls: { freq: control.hz(440) }, positional: ["freq"], output: port.audio() }, ({ freq }: Params) => kernel("std:saw", { freq })),
  sine: component({ id: "sine", controls: { freq: control.hz(440) }, positional: ["freq"], output: port.audio() }, ({ freq }: Params) => kernel("std:sine", { freq })),
};
"#;

// PKG-2, PKG-9, WRITE-20: describing the packages lists every component once, by ComponentId,
// with where each is exported (its defining module first). A module that throws, or one id at
// two places, is `pkg.invalid` naming the files; those components are left out, and the rest is
// listed. Describing never fails.
#[test]
fn describe_lists_components_and_problems() {
    let delay = |id: &str| {
        format!(
            "import {{ component, port }} from \"@m2/core\";\nexport const d = component({{ id: \"{id}\", inputs: {{ in: port.audio() }}, output: port.audio() }}, (x: any) => x);\n"
        )
    };
    let (a, b) = (delay("delay"), delay("delay"));
    let pkg = TempPackage::new("describe", &[
        ("fx.ts", FX),
        ("uses.ts", "export { lowpass as lp } from \"./fx\";\nexport * from \"./fx\";\n"),
        ("a.ts", &a),
        ("b.ts", &b),
        ("broken.ts", "throw new Error(\"boom\");\n"),
        ("broken-user.ts", "import \"./broken\";\nexport const x = 1;\n"),
    ]);
    let catalog = describe(&[("pkg".into(), pkg.0.clone())]);
    assert_eq!(catalog.components.keys().collect::<Vec<_>>(), ["pkg:lowpass", "pkg:saw", "pkg:sine"]);
    let lp = &catalog.components["pkg:lowpass"];
    assert_eq!((lp.module.as_str(), lp.file.as_str(), lp.is_effect()), ("pkg/fx", "pkg/src/fx.ts", true));
    let exports = |id: &str| catalog.exports[id].iter().map(|e| format!("{}.{}", e.module, e.name)).collect::<Vec<_>>();
    assert_eq!(exports("pkg:lowpass"), ["pkg/fx.lowpass", "pkg/uses.lowpass", "pkg/uses.lp"]);
    assert_eq!(exports("pkg:saw"), ["pkg/fx.osc.saw", "pkg/uses.osc.saw"]);
    assert_eq!(catalog.effects().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["pkg:lowpass"]);
    let problems: Vec<(&str, &str, Option<&str>)> = catalog.problems.iter().map(|d| (d.code, d.message.as_str(), d.src.as_deref())).collect();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems.iter().all(|p| p.0 == "pkg.invalid"));
    let says = |want: &[&str]| problems.iter().any(|(_, m, _)| want.iter().all(|w| m.contains(w)));
    assert!(says(&["`pkg/broken`", "boom"]), "{problems:?}");
    assert!(says(&["`pkg/broken-user`"]), "a module that imports a broken one is broken too: {problems:?}");
    assert!(says(&["two components have the id `pkg:delay`", "pkg/src/a.ts:2:", "pkg/src/b.ts:2:"]), "{problems:?}");
    // A song that doesn't import the broken modules builds, and its frames name ComponentIds.
    let song = "import { song } from \"@m2/core\";\nimport { lp, osc } from \"pkg/uses\";\nexport default song(() => lp(osc.saw(110)));\n";
    let b = with_package(&pkg, song).unwrap_or_else(|e| panic!("{e:?}"));
    let ids: Vec<&str> = b.ir.modules[0].frames.iter().filter_map(|f| f.component.as_deref()).collect();
    assert_eq!(ids, ["pkg:saw", "pkg:lowpass"]);
    // A changed file is described again; an unchanged one comes from the cache.
    std::fs::write(pkg.0.join("src/broken.ts"), "export const fine = 1;\n").unwrap();
    let again = describe(&[("pkg".into(), pkg.0.clone())]);
    assert_eq!(again.problems.len(), 1, "{:?}", again.problems);
    assert_eq!(again.components.keys().collect::<Vec<_>>(), ["pkg:lowpass", "pkg:saw", "pkg:sine"]);
}

// PKG-9: a ComponentId names the package and the descriptor's id, never the module file, so
// moving a component to another file, or importing it under another name, changes nothing.
#[test]
fn moving_a_component_keeps_its_id() {
    let song = "import { song } from \"@m2/core\";\nimport { lowpass as echo, osc } from \"pkg/fx\";\nexport default song(() => echo(osc.saw(110), 800));\n";
    let here = TempPackage::new("here", &[("fx.ts", FX)]);
    // The same lowpass, defined in lowpass.ts and exported from fx.ts.
    let (import, rest) = FX.split_once('\n').unwrap();
    let (lowpass, osc) = rest.split_at(rest.find("export const osc").unwrap());
    let lowpass_ts = format!("{import}\n{lowpass}");
    let moved_fx = format!("{import}\nexport {{ lowpass }} from \"./lowpass\";\n{osc}");
    let moved = TempPackage::new("moved", &[("fx.ts", &moved_fx), ("lowpass.ts", &lowpass_ts)]);
    let frames = |p: &TempPackage| {
        let b = with_package(p, song).unwrap_or_else(|e| panic!("{e:?}"));
        b.ir.modules[0].frames.iter().map(|f| f.component.clone()).collect::<Vec<_>>()
    };
    assert_eq!(frames(&here), frames(&moved));
    assert_eq!(frames(&here), [Some("pkg:saw".to_string()), Some("pkg:lowpass".to_string())]);
    let (a, b) = (describe(&[("pkg".into(), here.0.clone())]), describe(&[("pkg".into(), moved.0.clone())]));
    assert_eq!(a.components.keys().collect::<Vec<_>>(), b.components.keys().collect::<Vec<_>>());
    assert_eq!(b.components["pkg:lowpass"].module, "pkg/lowpass");
    assert_eq!(b.exports["pkg:lowpass"].iter().map(|e| e.module.as_str()).collect::<Vec<_>>(), ["pkg/lowpass", "pkg/fx"]);
}

// PKG-9: a component's package and module come from the file that declares it, as the host
// names them: a package file is `<package>/<module>` (an `index.ts` is its folder's), a
// project file is its path and belongs to no package.
#[test]
fn the_host_names_a_files_package_and_module() {
    let dir = std_dir().canonicalize().unwrap();
    let st = State { packages: vec![("std".into(), dir.clone())], ..Default::default() };
    let home = |f: &Path| st.home(&f.to_string_lossy());
    assert_eq!(home(&dir.join("src/fx.ts")), serde_json::json!({"package": "std", "module": "std/fx", "file": "std/src/fx.ts"}));
    assert_eq!(home(&dir.join("src/index.ts"))["module"], "std");
    assert_eq!(home(&dir.join("src/gen/index.ts"))["module"], "std/gen");
    assert_eq!(st.home("./lib/fx.ts"), serde_json::json!({"package": null, "module": "lib/fx.ts", "file": "lib/fx.ts"}));
}
