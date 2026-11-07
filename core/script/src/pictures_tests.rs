//! Pictures on the card (CARD-60 to CARD-65): `view.picture` in a descriptor, checked by the build,
//! and drawn by the picture runner. TypeScript refuses a dot's wrong names as the code is typed
//! (`sdk/check/pictures.ts`); the build and the runner refuse each mistake again, in words, since
//! types can be bypassed (`as any`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::pictures::{Ask, Drawn, Runner};
use super::*;
use crate::component::Descriptor;
use crate::component::ViewOf;

/// A two-band EQ-like sound whose picture is its curve, a dot for each band (CARD-61): a test pack's
/// `src/index.ts`. Its body plays a test kernel; a picture never runs it.
pub(crate) const CURVE: &str = r#"import { axis, component, control, kernel, live, port, view, type Audio } from "@m2/core";

const BANDS = [1, 2] as const;
/** A bell's height in dB at `f`: near enough a peaking filter's, for a test. */
const bell = (f: number, freq: number, gain: number, q: number) => {
  const x = Math.log2(f / freq) * q * 2;
  return gain / (1 + x * x);
};
/** How many times the grid part was worked out: what shows a part isn't run again for nothing. */
let grids = 0;

export const curve = component(
  {
    id: "curve",
    label: "Curve EQ",
    inputs: { x: port.audio() },
    controls: {
      freq1: control.hz(200).range(20, 20000).log(), gain1: control.db(0).range(-24, 24), q1: control.scalar(1).range(0.1, 10).log(), on1: control.bool(true),
      freq2: control.hz(2000).range(20, 20000).log(), gain2: control.db(0).range(-24, 24), q2: control.scalar(1).range(0.1, 10).log(), on2: control.bool(true),
      hang: control.scalar(0),
    },
    output: port.audio(),
    editor: {
      category: "EQ",
      views: {
        curve: view.picture({
          x: axis.log("hz", 20, 20000),
          y: axis.lin("db", -24, 24),
          live: [live.spectrum("in", { tone: "faint" }), live.spectrum("out"), live.playhead("beat")],
        }),
      },
      groups: { "Band 1": ["on1", "freq1", "gain1", "q1"], "Band 2": ["on2", "freq2", "gain2", "q2"] },
      face: ["curve"],
    },
  },
  (x, v) => kernel<Audio>("test:curve", { x, ...v }),
  {
    curve: {
      draw: (v, p) => {
        if ((v.hang ?? 0) > 0) for (;;) {}
        const points: number[] = [];
        for (let i = 0; i < 180; i++) {
          const f = 20 * 1000 ** (i / 179);
          let db = 0;
          for (const b of BANDS) if (v[`on${b}`]) db += bell(f, v[`freq${b}`] ?? 1000, v[`gain${b}`] ?? 0, v[`q${b}`] ?? 1);
          points.push(f, db);
        }
        return [
          p.part("grid", () => [p.mark("y", 0, "0 dB"), p.label(30, 20, `grid ${(grids = (grids % 9) + 1)} at q ${v.q1}`)]),
          p.line(points, { name: "curve" }),
          ...BANDS.map((b) =>
            p.handle(v[`freq${b}`], v[`gain${b}`], { x: `freq${b}`, y: `gain${b}`, wheel: `q${b}`, toggles: `on${b}`, opens: `Band ${b}`, mark: `${b}`, off: !v[`on${b}`] }),
          ),
        ];
      },
      words: (v) => BANDS.map((b) => `Band ${b}: ${v[`gain${b}`]} dB at ${v[`freq${b}`]} Hz`).join("; "),
    },
  },
);
"#;

/// The values the curve plays at its defaults, as the server gives them (CARD-63).
fn at_defaults() -> Value {
    json!({"freq1": 200, "gain1": 0, "q1": 1, "on1": true, "freq2": 2000, "gain2": 0, "q2": 1, "on2": true, "hang": 0})
}

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

/// A package `name` in a folder: written from `index` (and removed after), or one of the
/// hostile fixtures in `tests/pictures/`.
struct Pack {
    name: String,
    dir: PathBuf,
    ours: bool,
}

impl Pack {
    fn new(tag: &str, index: &str) -> Pack {
        let dir = std::env::temp_dir().join(format!("m2-script-{}-pictures-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/index.ts"), index).unwrap();
        Pack { name: "@test/curve".into(), dir, ours: true }
    }

    /// The hostile fixture `tests/pictures/<which>` as `@test/<which>`.
    fn fixture(which: &str) -> Pack {
        Pack { name: format!("@test/{which}"), dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pictures").join(which), ours: false }
    }

    fn packages(&self) -> Vec<(String, PathBuf)> {
        vec![(self.name.clone(), self.dir.clone()), ("std".into(), std_dir())]
    }

    /// The descriptor of `component`, as the catalog has it.
    fn descriptor(&self, component: &str) -> Arc<Descriptor> {
        let catalog = describe(&self.packages());
        let cid = format!("{}:{component}", self.name);
        catalog.components.get(&cid).cloned().unwrap_or_else(|| panic!("no {cid}: {:?}", catalog.problems))
    }

    /// An ask for picture `view` of `component`, at `values`, of the code `key` names.
    fn ask_as(&self, key: &str, desc: Arc<Descriptor>, view: &str, values: Value) -> Ask {
        Ask {
            key: key.into(),
            packages: self.packages(),
            song: None,
            module: self.name.clone(),
            descriptor: desc,
            view: view.into(),
            values: values.to_string(),
            memo: "song#1".into(),
        }
    }

    fn ask(&self, component: &str, view: &str, values: Value) -> Ask {
        self.ask_as(&self.name, self.descriptor(component), view, values)
    }
}

impl Drop for Pack {
    fn drop(&mut self) {
        if self.ours {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// What a draw gave, as JSON: it must have drawn.
fn shapes(d: Drawn) -> Value {
    match d {
        Drawn::Shapes(json) => serde_json::from_str(&json).unwrap(),
        other => panic!("it didn't draw: {other:?}"),
    }
}

/// Why a draw couldn't be drawn: it must not have.
fn cant(d: Drawn) -> String {
    match d {
        Drawn::Cant(why) => why,
        other => panic!("it drew: {other:?}"),
    }
}

fn build_curve() -> Result<Build, Vec<Diagnostic>> {
    let code: BTreeMap<String, String> = [
        ("lib/curve.ts".to_string(), CURVE.to_string()),
        ("song.ts".into(), "import { kernel, song } from \"@m2/core\";\nimport { curve } from \"./lib/curve\";\nexport default song(() => curve(kernel(\"test:src\", {}) as any));\n".into()),
    ]
    .into();
    let mut o = Options::new(code, "song.ts");
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

// CARD-60: a picture is a view of its own name; the descriptor carries its axes and live layers,
// never its code.
#[test]
fn a_picture_is_carried_with_its_axes_and_layers_and_not_its_code() {
    let b = build_curve().unwrap_or_else(|e| panic!("{e:?}"));
    let d = b.components.iter().find(|d| d.id == "curve").expect("the curve");
    assert_eq!(d.editor.face, ["curve"]);
    let v = &d.editor.views[0];
    assert_eq!(v.name, "curve");
    let ViewOf::Picture { x, y, live } = &v.of else { panic!("a picture: {:?}", v.of) };
    assert_eq!((x.scale.as_str(), &x.unit, x.lo, x.hi), ("log", &m2_ir::Unit::Hz, 20.0, 20000.0));
    assert_eq!((y.scale.as_str(), &y.unit, y.lo, y.hi), ("lin", &m2_ir::Unit::Db, -24.0, 24.0));
    assert_eq!(
        live,
        &vec![
            json!({"kind": "spectrum", "at": "in", "tone": "faint"}),
            json!({"kind": "spectrum", "at": "out"}),
            json!({"kind": "playhead", "from": "beat"})
        ]
    );
    let text = serde_json::to_string(&d.editor).unwrap();
    assert!(!text.contains("draw") && !text.contains("words") && !text.contains("bell"), "no code in the descriptor: {text}");
}

/// The build of a sound whose `curve` view is `picture` and whose third argument is `code`, written
/// past the types.
fn picture(picture: &str, code: &str) -> Result<Build, Vec<Diagnostic>> {
    let lib = format!(
        "import {{ axis, component, control, kernel, knob, live, port, view }} from \"@m2/core\";\n\
         export const c = (component as any)({{ id: \"c\", controls: {{ f: control.hz(100), g: control.db(0) }}, editor: {{ views: {{ curve: {picture} }} }} }}, (p: any) => kernel(\"test:c\", {{}}){code});\n"
    );
    let code: BTreeMap<String, String> = [
        ("lib/c.ts".to_string(), lib),
        ("song.ts".into(), "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nexport default song(() => c());\n".into()),
    ]
    .into();
    let mut o = Options::new(code, "song.ts");
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

// CARD-60: each mistake in a picture's declaration, or in its code (component()'s third argument),
// is refused by the build, in words, at its call.
#[test]
fn a_pictures_declaration_is_refused_in_words() {
    let ok = "view.picture({ x: axis.log(\"hz\", 20, 20000), y: axis.lin(\"db\", -24, 24) })";
    let code = ", { curve: { draw: () => [], words: () => \"flat\" } }";
    picture(ok, code).unwrap_or_else(|e| panic!("{e:?}"));
    let time = "view.picture({ x: axis.time(2), y: axis.lin(\"unit01\", 0, 1) })";
    let b = picture(time, code).unwrap_or_else(|e| panic!("{e:?}"));
    let ViewOf::Picture { x, .. } = &b.components.iter().find(|d| d.id == "c").unwrap().editor.views[0].of else { panic!() };
    assert_eq!((x.scale.as_str(), &x.unit, x.lo, x.hi), ("time", &m2_ir::Unit::Seconds, 0.0, 2.0));
    let views = "editor.views' \"curve\"";
    for (bad, code_, says, at) in [
        (
            ok.replace("axis.log(\"hz\", 20, 20000)", "axis.log(\"hz\", 0, 20000)"),
            code,
            "its x axis is on a log scale, so it starts above 0, not at 0",
            views,
        ),
        (
            ok.replace("axis.lin(\"db\", -24, 24)", "axis.lin(\"db\", 24, -24)"),
            code,
            "its y axis must go from a lower number to a higher one, not from 24 to -24",
            views,
        ),
        (
            ok.replace("axis.lin(\"db\", -24, 24)", "axis.lin(\"dbs\" as any, -24, 24)"),
            code,
            "its y axis's unit is \"dbs\", which isn't a unit (units: hz, db,",
            views,
        ),
        (
            ok.replace("axis.lin(\"db\", -24, 24)", "{ unit: \"db\" }"),
            code,
            "its y must be made with axis.lin(), axis.log() or axis.time()",
            views,
        ),
        (
            ok.replace(" })", ", live: [live.spectrum(\"side\" as any)] })"),
            code,
            "its live layer 1, live.spectrum, looks at \"side\": a spectrum looks at \"in\" or \"out\"",
            views,
        ),
        (
            ok.replace(" })", ", live: [\"spectrum\"] })"),
            code,
            "its live layer 1 must be made with live.spectrum(), live.level(), live.meter(), live.history(), live.playhead() or live.scope()",
            views,
        ),
        (
            ok.replace(" })", ", live: [live.spectrum(\"in\", { tone: \"red\" } as any)] })"),
            code,
            "its live layer 1's tone is \"red\": a tone is main, soft or faint, never a colour",
            views,
        ),
        (ok.replace(" })", ", colour: \"red\" })"), code, "a picture has no field \"colour\" (fields: x, y, live)", views),
        (
            ok.into(),
            "",
            "the picture \"curve\" has no code: give its draw and words in component()'s third argument, as { curve: { draw, words } }",
            "",
        ),
        (
            ok.into(),
            ", { curve: { draw: () => [] } }",
            "the picture \"curve\" says itself in words: give it words, a function of its values that returns one sentence",
            "",
        ),
        (
            ok.into(),
            ", { curve: { draw: [], words: () => \"x\" } }",
            "the picture \"curve\"'s draw must be a function of its values and the builder p",
            "",
        ),
        (
            ok.into(),
            ", { curve: { draw: () => [], words: () => \"x\", colour: 1 } }",
            "the picture \"curve\"'s code has no field \"colour\" (fields: draw, words)",
            "",
        ),
        (
            ok.into(),
            ", { curve: { draw: () => [], words: () => \"x\" }, other: {} }",
            "its third argument names \"other\", which isn't a picture of this sound (pictures: curve)",
            "",
        ),
        (ok.into(), ", 3", "its third argument is its pictures' code, by name, as { curve: { draw, words } }, not 3", ""),
    ] {
        let e = picture(&bad, code_).err().unwrap_or_else(|| panic!("{bad}{code_} builds"));
        assert!(e.iter().any(|d| d.message.contains(at) && d.message.contains(says)), "{bad}{code_}: {says}\n{e:?}");
    }
}

// CARD-64, CARD-65: the runner draws a pack's picture in the build's sandbox: the shapes in the
// axes' units, each dot with the names it moves, and the words.
#[test]
fn the_runner_draws_a_packs_picture() {
    let pack = Pack::new("draws", CURVE);
    let runner = Runner::new();
    let mut v = at_defaults();
    v["gain2"] = json!(-3.5);
    let out = shapes(runner.draw(pack.ask("curve", "curve", v)));
    assert_eq!(out["words"], "Band 1: 0 dB at 200 Hz; Band 2: -3.5 dB at 2000 Hz");
    let s = out["shapes"].as_array().unwrap();
    let kinds: Vec<&str> = s.iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["part", "line", "handle", "handle"]);
    assert_eq!(s[0]["name"], "grid");
    assert_eq!(s[0]["shapes"][0], json!({"kind": "mark", "axis": "y", "at": 0.0, "words": "0 dB"}));
    let line = s[1]["points"].as_array().unwrap();
    assert_eq!((line.len(), &s[1]["name"]), (360, &json!("curve")));
    // At 2000 Hz the curve dips by band 2's gain.
    let at =
        line.chunks(2).min_by(|a, b| (a[0].as_f64().unwrap() - 2000.0).abs().total_cmp(&(b[0].as_f64().unwrap() - 2000.0).abs())).unwrap();
    assert!((at[1].as_f64().unwrap() + 3.5).abs() < 0.2, "{at:?}");
    assert_eq!(
        s[3],
        json!({"kind": "handle", "x": 2000.0, "y": -3.5, "moves": {"x": {"name": "freq2"}, "y": {"name": "gain2"}, "wheel": {"name": "q2"}, "toggles": {"name": "on2"}}, "opens": "Band 2", "mark": "2", "off": false})
    );
}

// CARD-62, CARD-65: a name worked out as the code runs is checked as it runs; so is what the draw
// gives back. Each mistake greys the picture in words.
#[test]
fn the_runner_checks_each_draw_in_words() {
    let checks = [
        ("p.handle(1, 1, { x: `freq${9}` })", "names freq9, which this sound doesn't have"),
        ("p.handle(1, 1, { x: \"gain1\" })", "moves gain1 across, a control in db, but its x axis is in hz"),
        ("p.handle(1, 1, { wheel: \"on1\" })", "moves on1 with the wheel, a switch, but the wheel moves a number"),
        ("p.handle(1, 1, { toggles: \"q1\" })", "flips q1, a control in scalar, but only a switch flips"),
        ("p.handle(1, 1, { x: knob(\"on1\") })", "moves on1 as its knob, a switch, but a knob's drag moves a number"),
        ("p.handle(1, 1, { mark: \"long\" })", "a dot's mark is 1 to 3 characters, not \"long\""),
        ("p.handle(1, 1, { opens: \"Band 9\" })", "a dot opens \"Band 9\", which isn't a group of this card (groups: Band 1, Band 2)"),
        ("p.line([1, 2, 3])", "gives p.line an odd count of numbers (3): points are x, y pairs"),
        ("p.line([1, NaN])", "gives p.line a point that isn't a finite number"),
        ("p.line(new Array(20002).fill(1))", "draws more than 20,000 numbers"),
        ("p.line([1, 1], { name: \"a b\" })", "gives p.line the name \"a b\": a line's name is a plain name of up to 32 characters"),
        ("p.label(1, 1, \"é\".repeat(41))", "gives p.label one line of 1 to 40 characters"),
        ("p.part(\"a\", () => [p.part(\"b\", () => [])])", "makes a part inside a part: parts don't nest"),
        ("{ kind: \"line\", points: [] }", "returns something p didn't make: make each shape with p.line(), p.handle() and the rest"),
        ("v.nope", "reads nope, which this sound doesn't have"),
        ("(v.freq1 = 3, p.line([]))", "can't change freq1: a picture reads what plays, and changes nothing"),
        ("(() => { throw new Error(\"no curve here\") })()", "no curve here"),
    ];
    let mut index = CURVE.to_string();
    let views: Vec<String> = (0..checks.len())
        .map(|i| format!("        k{i}: view.picture({{ x: axis.log(\"hz\", 20, 20000), y: axis.lin(\"db\", -24, 24) }}),\n"))
        .collect();
    let code: Vec<String> = checks
        .iter()
        .enumerate()
        .map(|(i, (d, _))| format!("    k{i}: {{ draw: (v: any, p: any) => [{d}], words: () => \"x\" }},\n"))
        .collect();
    index = index.replacen("      views: {\n", &format!("      views: {{\n{}", views.concat()), 1);
    index = index.replacen("  {\n    curve: {\n      draw:", &format!("  {{\n{}    curve: {{\n      draw:", code.concat()), 1);
    index = index
        .replace("import { axis,", "import { knob, axis,")
        .replace("export const curve = component(", "export const curve = (component as any)(");
    let pack = Pack::new("checks", &index);
    let runner = Runner::new();
    let desc = pack.descriptor("curve");
    for (i, (draw, says)) in checks.iter().enumerate() {
        let why = cant(runner.draw(pack.ask_as("checks", desc.clone(), &format!("k{i}"), at_defaults())));
        assert!(why.contains(says), "{draw}: {says}\n{why}");
    }
    // A view that isn't a picture, or a sound that isn't there.
    let mut other = (*desc).clone();
    other.id = "@test/curve:nothing".into();
    assert!(cant(runner.draw(pack.ask_as("checks", Arc::new(other), "curve", at_defaults()))).contains("@test/curve:nothing"));
}

// CARD-80, CARD-65: the layers music2 measures are given with no points, each at most once, on the
// axes it draws them on, and counted as the points they are filled with; a response's click from
// −80 to −20 dB; each mistake greys the picture in words, as the runner finds it and again in Rust.
#[test]
fn a_measured_layer_is_checked_in_words() {
    let hz = "x: axis.log(\"hz\", 20, 20000), y: axis.lin(\"db\", -24, 24)";
    let time = "x: axis.time(0.5), y: axis.lin(\"bipolar\", -1, 1)";
    let long = "x: axis.time(8), y: axis.lin(\"bipolar\", -1, 1)";
    let draws = [
        (hz, "p.response({ name: \"measured\", tone: \"soft\" })", Ok(json!([{"kind": "response", "name": "measured", "tone": "soft"}]))),
        (time, "p.hit({ note: 36 })", Ok(json!([{"kind": "hit", "note": 36, "seconds": 0.5}]))),
        (
            time,
            "p.part(\"a\", () => [p.hit({ seconds: 0.25, tone: \"faint\" })])",
            Ok(json!([{"kind": "part", "name": "a", "shapes": [{"kind": "hit", "note": 60, "seconds": 0.25, "tone": "faint"}]}])),
        ),
        (
            time,
            "p.response()",
            Err("gives p.response an x axis in seconds and a y axis in bipolar: a response is drawn across hz and up db"),
        ),
        (hz, "p.hit()", Err("gives p.hit an x axis in hz and a y axis in db: a hit is drawn across seconds and up bipolar")),
        (hz, "p.response(), p.response()", Err("gives p.response twice: a picture holds one at most")),
        (time, "p.hit({ note: 128 })", Err("gives p.hit the note 128: a note is a whole number 0 to 127")),
        (time, "p.hit({ seconds: 0 })", Err("gives p.hit 0 seconds: a hit lasts more than 0 and at most 1 second")),
        (long, "p.hit({ seconds: 2 })", Err("gives p.hit 2 seconds: a hit lasts more than 0 and at most 1 second")),
        (time, "p.hit({ seconds: 0.75 })", Err("gives p.hit 0.75 seconds, past its x axis's end at 0.5 s")),
        // Left out, a hit lasts its axis, or a second at most (review M4, M10).
        (long, "p.hit()", Ok(json!([{"kind": "hit", "note": 60, "seconds": 1.0}]))),
        (hz, "p.response({ tone: \"red\" })", Err("gives p.response the tone \"red\": a tone is main, soft or faint, never a colour")),
        (
            hz,
            "p.response({ name: \"a b\" })",
            Err("gives p.response the name \"a b\": a line's name is a plain name of up to 32 characters"),
        ),
        (hz, "p.response(), p.line(new Array(19490).fill(1))", Err("draws more than 20,000 numbers")),
        // A quieter click, for a sound with thresholds to rest under (measure2 §6.2).
        (hz, "p.response({ level: -80 })", Ok(json!([{"kind": "response", "level": -80.0}]))),
        (hz, "p.response({ level: -90 })", Err("gives p.response the level -90: its click plays at −80 to −20 dB")),
        (hz, "p.response({ level: -10 })", Err("gives p.response the level -10: its click plays at −80 to −20 dB")),
        (hz, "p.response({ level: \"quiet\" })", Err("gives p.response the level \"quiet\": its click plays at −80 to −20 dB")),
    ];
    let views: Vec<String> =
        draws.iter().enumerate().map(|(i, (axes, ..))| format!("        m{i}: view.picture({{ {axes} }}),\n")).collect();
    let code: Vec<String> = draws
        .iter()
        .enumerate()
        .map(|(i, (_, d, _))| format!("    m{i}: {{ draw: (v: any, p: any) => [{d}], words: () => \"x\" }},\n"))
        .collect();
    let index = CURVE
        .replacen("      views: {\n", &format!("      views: {{\n{}", views.concat()), 1)
        .replacen("  {\n    curve: {\n      draw:", &format!("  {{\n{}    curve: {{\n      draw:", code.concat()), 1)
        .replace("export const curve = component(", "export const curve = (component as any)(");
    let pack = Pack::new("measured", &index);
    let runner = Runner::new();
    let desc = pack.descriptor("curve");
    for (i, (_, draw, says)) in draws.iter().enumerate() {
        let drawn = runner.draw(pack.ask_as("measured", desc.clone(), &format!("m{i}"), at_defaults()));
        match says {
            Ok(want) => assert_eq!(shapes(drawn)["shapes"], *want, "{draw}"),
            Err(says) => {
                let why = cant(drawn);
                assert!(why.contains(says), "{draw}: {says}\n{why}");
            }
        }
    }
    // Rust refuses the same, and points a draw forged: they are the server's to fill.
    let forged = |view: &str, shapes: &str| super::pictures::checked(&format!("{{\"shapes\":{shapes},\"words\":\"x\"}}"), &desc, view);
    assert!(forged("m0", "[{\"kind\":\"response\",\"points\":[20,0]}]").unwrap_err().contains("can't read as shapes"));
    assert_eq!(
        forged("m0", "[{\"kind\":\"response\"},{\"kind\":\"response\"}]").unwrap_err(),
        "gives p.response twice: a picture holds one at most"
    );
    assert!(forged("m1", "[{\"kind\":\"response\"}]").unwrap_err().contains("a response is drawn across hz and up db"));
    assert_eq!(
        forged("m0", "[{\"kind\":\"response\",\"level\":-90}]").unwrap_err(),
        "gives p.response the level -90: its click plays at −80 to −20 dB"
    );
    assert!(forged("m0", "[{\"kind\":\"response\",\"level\":-80}]").is_ok());
    assert!(forged("m1", "[{\"kind\":\"hit\",\"note\":60,\"seconds\":2}]").unwrap_err().contains("at most 1 second"));
    assert!(forged("m1", "[{\"kind\":\"hit\",\"note\":60,\"seconds\":0.75}]").unwrap_err().contains("past its x axis's end at 0.5 s"));
    assert!(
        forged("m0", "[{\"kind\":\"hit\",\"note\":60,\"seconds\":1}]")
            .unwrap_err()
            .contains("a hit is drawn across seconds and up bipolar")
    );
}

// CARD-61, CARD-65: words are one line of 1 to 400 characters, counted as characters, with no
// control character: 400 é is drawn, 401 isn't, nor words with an escape or U+2028 (review G2, G10).
#[test]
fn words_are_one_line_of_400_characters_at_most() {
    let pack = Pack::fixture("evil");
    let runner = Runner::new();
    let v = json!({"freq": 1000});
    assert_eq!(shapes(runner.draw(pack.ask("four00", "pic", v.clone())))["words"].as_str().unwrap().chars().count(), 400);
    assert!(cant(runner.draw(pack.ask("four01", "pic", v.clone()))).contains("its words must be one line of 1 to 400 characters"));
    assert!(cant(runner.draw(pack.ask("ansi", "pic", v.clone()))).contains("its words must be one line of 1 to 400 characters"));
    assert!(cant(runner.draw(pack.ask("sep", "pic", v.clone()))).contains("its words must be one line of 1 to 400 characters"));
    // A name of 20 MB is never sent: refused by its length, or stopped while it is made.
    let why = cant(runner.draw(pack.ask("bigname", "pic", v)));
    assert!(
        why.contains("a line's name is a plain name of up to 32 characters") || why.starts_with("took too long and was stopped"),
        "{why}"
    );
}

// CARD-65: what a draw gives is checked in Rust, whatever the pack did to its sandbox: a pack that
// replaces `__m2_picture`, `JSON.stringify` and `Array.isArray` at load still draws only what passed
// (review G2).
#[test]
fn a_draw_is_checked_whatever_its_pack_did_to_its_sandbox() {
    let hijack = Pack::fixture("hijack");
    let runner = Runner::new();
    let mut desc = (*Pack::fixture("evil").descriptor("fine")).clone();
    desc.id = "@test/hijack:hijack".into();
    let out = runner.draw(hijack.ask_as("hijack", Arc::new(desc), "pic", json!({"freq": 1000})));
    match &out {
        Drawn::Shapes(json) => {
            let v: Value = serde_json::from_str(json).unwrap();
            assert_eq!(
                v,
                json!({"shapes": [{"kind": "line", "points": [20.0, 0.0, 20000.0, 0.0]}], "words": "ok"}),
                "its own draw, not the forged one"
            );
        }
        Drawn::Cant(why) => assert!(!why.contains("forged") && !why.contains("nosuch"), "{why}"),
        Drawn::Busy => panic!("busy"),
    }
    // And Rust itself refuses what the forged shapes say, had they reached it.
    // Its own module breaks describing it (it replaces `Array.isArray`): its descriptor is `fine`'s, renamed.
    let mut desc = (*Pack::fixture("evil").descriptor("fine")).clone();
    desc.id = "@test/hijack:hijack".into();
    let desc = Arc::new(desc);
    let forged = |shapes: &str, words: &str| super::pictures::checked(&format!("{{\"shapes\":{shapes},\"words\":{words}}}"), &desc, "pic");
    let many = format!("[{{\"kind\":\"line\",\"points\":[{}]}}]", vec!["1"; 20002].join(","));
    assert_eq!(forged(&many, "\"x\"").unwrap_err(), "draws more than 20,000 numbers: draw fewer points");
    let dot = "[{\"kind\":\"handle\",\"x\":1,\"y\":1,\"moves\":{\"x\":{\"name\":\"nosuch\"}},\"off\":false}]";
    assert_eq!(forged(dot, "\"x\"").unwrap_err(), "names nosuch, which this sound doesn't have");
    let knob =
        "[{\"kind\":\"handle\",\"x\":1,\"y\":1,\"moves\":{\"x\":{\"name\":\"freq\",\"knob\":\"tracks/eq.ts#9.gain2\"}},\"off\":false}]";
    assert!(forged(knob, "\"x\"").unwrap_err().contains("can't read as shapes"), "a knob id is the server's to give, never the pack's");
    assert!(forged("[{\"kind\":\"line\",\"points\":[],\"colour\":\"red\"}]", "\"x\"").unwrap_err().contains("can't read as shapes"));
    assert!(forged("[]", "\"line one\\nforged\"").unwrap_err().contains("its words must be one line"));
    let big = format!("[{{\"kind\":\"label\",\"x\":1,\"y\":1,\"words\":\"{}\"}}]", "x".repeat(300_000));
    assert!(forged(&big, "\"x\"").unwrap_err().contains("more than 256 KB"));
}

// CARD-64: a module that hangs as it loads is stopped once, within its load's limit, and remembered:
// its pictures are refused at once after, in words; and another pack's pictures never wait for it
// (review G1).
#[test]
fn a_pack_that_hangs_at_load_holds_only_its_own_pictures_once() {
    let evil = Pack::fixture("evil");
    let hang = Pack::fixture("hang");
    let runner = Arc::new(Runner::new());
    let mut desc = (*evil.descriptor("fine")).clone();
    desc.id = "@test/hang:hang".into();
    let desc = Arc::new(desc);
    let ask = Ask {
        key: "hang".into(),
        packages: hang.packages(),
        module: "@test/hang".into(),
        ..hang.ask_as("hang", desc.clone(), "pic", json!({"freq": 1000}))
    };
    // While it loads, the other pack's picture draws.
    let r = runner.clone();
    let slow = ask.clone();
    let loading = std::thread::spawn(move || {
        let t = Instant::now();
        (r.draw(slow), t.elapsed())
    });
    std::thread::sleep(Duration::from_millis(50));
    let t = Instant::now();
    shapes(runner.draw(evil.ask("fine", "pic", json!({"freq": 1000}))));
    assert!(t.elapsed() < Duration::from_millis(500), "another pack's picture waited {:?}", t.elapsed());
    let (first, took) = loading.join().unwrap();
    assert!(cant(first).contains("its module @test/hang took too long to load and was stopped"));
    assert!(took < Duration::from_secs(5), "{took:?}");
    let t = Instant::now();
    assert!(cant(runner.draw(ask.clone())).contains("took too long to load"));
    assert!(t.elapsed() < Duration::from_millis(100), "refused at once, not loaded again: {:?}", t.elapsed());
    // Waiting for it is bounded for whoever asks.
    let fresh = Ask { key: "hang again".into(), ..ask };
    assert_eq!(runner.draw_within(fresh, Some(Duration::from_millis(100))), Drawn::Busy);
}

// CARD-64: each draw has its own limit, counted from its start. One past it is stopped in words, and
// the picture is refused at once only for a short back-off (a stop on a busy machine may be a blip),
// then tried again; at values stopped twice it is refused until its code changes. A hundred draws
// take far longer than one's limit in all, with none stopped.
#[test]
fn an_endless_draw_backs_off_and_is_refused_at_values_stopped_twice() {
    let pack = Pack::new("endless", CURVE);
    let runner = Runner::new();
    let desc = pack.descriptor("curve");
    let ask = |key: &str, v: Value| runner.draw(pack.ask_as(key, desc.clone(), "curve", v));
    let t = Instant::now();
    for i in 0..100 {
        let mut v = at_defaults();
        v["gain1"] = json!(i as f64 / 10.0);
        shapes(ask("endless", v));
    }
    assert!(t.elapsed() > Duration::from_millis(20), "{:?}", t.elapsed());
    let mut hang = at_defaults();
    hang["hang"] = json!(1);
    let t = Instant::now();
    let why = cant(ask("endless", hang.clone()));
    assert!(why.starts_with("took too long and was stopped; it is tried again in"), "{why}");
    assert!(t.elapsed() < Duration::from_secs(1), "stopped soon: {:?}", t.elapsed());
    // While it backs off, refused at once, at any values.
    let t = Instant::now();
    assert!(cant(ask("endless", at_defaults())).starts_with("took too long and was stopped; it is tried again in"));
    assert!(t.elapsed() < Duration::from_millis(50), "refused at once: {:?}", t.elapsed());
    // After it, tried again: other values draw.
    std::thread::sleep(Duration::from_millis(350));
    shapes(ask("endless", at_defaults()));
    // The same values stopped a second time: refused at them until the code changes.
    assert!(cant(ask("endless", hang.clone())).starts_with("took too long and was stopped"));
    std::thread::sleep(Duration::from_millis(350));
    let t = Instant::now();
    assert_eq!(cant(ask("endless", hang.clone())), "was stopped twice at these values, and is drawn at them again once its code changes");
    assert!(t.elapsed() < Duration::from_millis(50), "refused at once: {:?}", t.elapsed());
    shapes(ask("endless", at_defaults()));
    // Its code changed: a new runtime, which tries them again.
    assert!(cant(ask("endless, edited", hang)).starts_with("took too long and was stopped; it is tried again in"));
}

// CARD-64, CARD-65: the sandbox's limits, each said in words (review G15): too much memory, a long
// built-in, endless recursion, a huge BigInt, a catastrophic regex and an endless loop are each
// stopped; what the sandbox took away isn't there; and the pack's next picture still draws.
#[test]
fn the_sandbox_stops_each_hostile_draw_in_words() {
    let evil = Pack::fixture("evil");
    let runner = Runner::new();
    let v = json!({"freq": 1000});
    let said = |id: &str| match runner.draw(evil.ask(id, "pic", v.clone())) {
        Drawn::Cant(why) => why,
        Drawn::Shapes(json) => serde_json::from_str::<Value>(&json).unwrap()["words"].as_str().unwrap().to_string(),
        Drawn::Busy => "busy".into(),
    };
    assert!(said("memhog").starts_with("used too much memory and was stopped; it is tried again in"));
    let sorted = said("sorted");
    assert!(sorted.starts_with("took too long and was stopped") || sorted.starts_with("used too much memory and was stopped"), "{sorted}");
    assert!(said("deep").starts_with("couldn't be drawn: "), "endless recursion is said");
    assert!(said("bigint").starts_with("couldn't be drawn: "), "a huge BigInt is said");
    assert!(said("regex").starts_with("took too long and was stopped"));
    assert!(said("spin").starts_with("took too long and was stopped"));
    let reach = said("reach");
    for gone in [
        "Date:undefined",
        "perf:undefined",
        "rand:throws",
        "fetch:undefined",
        "setTimeout:undefined",
        "WebAssembly:undefined",
        "eval:throws",
        "fnctor:throws",
    ] {
        assert!(reach.contains(gone), "{gone}: {reach}");
    }
    assert_eq!(said("fine"), "at 1000 Hz");
}

// CARD-64, CARD-67: a draw is pure. One that keeps a count between draws gives something else the
// second time, and is refused in words (review G16).
#[test]
fn a_draw_that_keeps_state_is_refused() {
    let evil = Pack::fixture("evil");
    let runner = Runner::new();
    let why = cant(runner.draw(evil.ask("counter", "pic", json!({"freq": 1000}))));
    assert_eq!(why, "draws differently each time for the same values: a picture keeps nothing between its draws");
    // Remembered: refused at once at any values after, until the code changes.
    assert_eq!(cant(runner.draw(evil.ask("counter", "pic", json!({"freq": 2000})))), why);
}

// CARD-64: a picture's values at its defaults, as `m2 pack check` draws it: a choice by its name,
// a switch true or false, a setting's default.
#[test]
fn a_pictures_defaults_are_its_descriptors() {
    let b = build_curve().unwrap_or_else(|e| panic!("{e:?}"));
    let desc = b.components.into_iter().find(|d| d.id == "curve").unwrap();
    let v: Value = serde_json::from_str(&pictures::defaults(&desc)).unwrap();
    assert_eq!(
        v,
        json!({"freq1": 200.0, "gain1": 0.0, "q1": 1.0, "on1": true, "freq2": 2000.0, "gain2": 0.0, "q2": 1.0, "on2": true, "hang": 0.0})
    );
}

// CARD-64: a part runs again only when a value it read changed since that picture's last draw.
#[test]
fn a_part_runs_again_only_when_what_it_read_changed() {
    let pack = Pack::new("parts", CURVE);
    let runner = Runner::new();
    let desc = pack.descriptor("curve");
    let label = |v: Value| shapes(runner.draw(pack.ask_as("parts", desc.clone(), "curve", v)))["shapes"][0]["shapes"][1]["words"].clone();
    assert_eq!(label(at_defaults()), "grid 1 at q 1");
    let mut v = at_defaults();
    v["gain1"] = json!(6);
    assert_eq!(label(v.clone()), "grid 1 at q 1", "the grid read only q1");
    v["q1"] = json!(2);
    assert_eq!(label(v), "grid 2 at q 2");
}

// CARD-64, D16: a song's own sound draws its picture from the song's code.
#[test]
fn a_songs_own_sound_draws_from_its_code() {
    let runner = Runner::new();
    let b = build_curve().unwrap_or_else(|e| panic!("{e:?}"));
    let desc = Arc::new(b.components.into_iter().find(|d| d.id == "curve").unwrap());
    let code: BTreeMap<String, String> = [("lib/curve.ts".to_string(), CURVE.to_string())].into();
    let ask = Ask {
        key: "song 7".into(),
        packages: vec![],
        song: Some(Arc::new(pictures::Song { key: 7, code, dependencies: vec![] })),
        module: "lib/curve.ts".into(),
        descriptor: desc,
        view: "curve".into(),
        values: at_defaults().to_string(),
        memo: "song#1".into(),
    };
    assert_eq!(shapes(runner.draw(ask))["words"], "Band 1: 0 dB at 200 Hz; Band 2: 0 dB at 2000 Hz");
}

// CARD-75: an eight-band EQ's picture (a curve of 180 points, eight dots and its words), drawn twice
// as every draw is (CARD-64), checked in Rust, and the trip there, take under 8 ms at p99, the design's budget for the slowest computer music2 supports.
// It judges speed, so it runs in the nightly tier (ci/nightly-tests.txt).
#[test]
fn an_eight_band_curve_draws_inside_its_budget() {
    let eight = CURVE.replace("const BANDS = [1, 2] as const;", "const BANDS = [1, 2, 3, 4, 5, 6, 7, 8] as const;").replace(
        "      hang: control.scalar(0),",
        "      hang: control.scalar(0),\n      freq3: control.hz(400), gain3: control.db(0), q3: control.scalar(1), on3: control.bool(true),\n      freq4: control.hz(800), gain4: control.db(0), q4: control.scalar(1), on4: control.bool(true),\n      freq5: control.hz(1600), gain5: control.db(0), q5: control.scalar(1), on5: control.bool(true),\n      freq6: control.hz(3200), gain6: control.db(0), q6: control.scalar(1), on6: control.bool(true),\n      freq7: control.hz(6400), gain7: control.db(0), q7: control.scalar(1), on7: control.bool(true),\n      freq8: control.hz(12800), gain8: control.db(0), q8: control.scalar(1), on8: control.bool(true),",
    );
    let eight = eight.replace("opens: `Band ${b}`, ", "");
    let pack = Pack::new("budget", &eight);
    let runner = Runner::new();
    let mut v = at_defaults();
    for b in 3..=8 {
        v[format!("freq{b}")] = json!(100 * (1 << b));
        v[format!("gain{b}")] = json!(0);
        v[format!("q{b}")] = json!(1);
        v[format!("on{b}")] = json!(true);
    }
    let desc = pack.descriptor("curve");
    let first = shapes(runner.draw(pack.ask_as("budget", desc.clone(), "curve", v.clone())));
    assert_eq!(first["shapes"].as_array().unwrap().len(), 10, "a grid, a curve and eight dots");
    let mut took: Vec<Duration> = (0..400)
        .map(|i| {
            v["gain4"] = json!((i % 48) as f64 / 2.0 - 12.0);
            let t = Instant::now();
            shapes(runner.draw(pack.ask_as("budget", desc.clone(), "curve", v.clone())));
            t.elapsed()
        })
        .collect();
    took.sort();
    let (median, p99) = (took[took.len() / 2], took[took.len() * 99 / 100]);
    eprintln!("an eight-band curve: median {median:?}, p99 {p99:?}");
    assert!(p99 < Duration::from_millis(8), "p99 {p99:?} (median {median:?}): over the 8 ms budget");
}

/// The curve, its draw first working `hang` thousand steps of sums, then drawing as ever.
fn spinning() -> String {
    CURVE.replace(
        "if ((v.hang ?? 0) > 0) for (;;) {}",
        "let spun = 0;\n        for (let i = 0; i < (v.hang ?? 0) * 1000; i++) spun += Math.sqrt(i);\n        if (spun < 0) return [];",
    )
}

// CARD-64: a draw's 20 ms is its own thread's CPU time, so a draw that waits for a CPU is never stopped
// for the wait: the draw and sixty busy threads on one CPU, a draw of about 10 ms of work takes far past
// 200 ms by the clock (where a backstop of ten times 20 ms stopped it) and draws; one of about a second of
// work is stopped. Linux only: macOS keeps no
// thread on one CPU (m2_os::thread::one_cpu says so, and the test says it didn't run).
#[test]
fn a_draw_sharing_its_cpu_is_stopped_for_its_work_not_its_wait() {
    use std::sync::atomic::{AtomicBool, Ordering};
    // This thread and every thread it starts after, the runner's included, on one CPU.
    if let Err(e) = m2_os::thread::one_cpu() {
        eprintln!("not run here: {e}");
        return;
    }
    let pack = Pack::new("spinning", &spinning());
    let runner = Runner::new();
    let desc = pack.descriptor("curve");
    let draw = |hang: f64, gain: f64| {
        let mut v = at_defaults();
        (v["hang"], v["gain1"]) = (json!(hang), json!(gain));
        let t = Instant::now();
        let drawn = runner.draw(pack.ask_as("spinning", desc.clone(), "curve", v));
        (drawn, t.elapsed())
    };
    // Its first draw runs twice; then how long a thousand steps take, alone on the CPU: the quickest of
    // three by the clock, as other processes may share this CPU too (a slow one would make `ten` too
    // little work, and the stopped draw below not stopped).
    shapes(draw(0.0, 0.0).0);
    let mut took = Duration::MAX;
    for gain in [0.1, 0.11, 0.12] {
        let (drawn, t) = draw(20.0, gain);
        shapes(drawn);
        took = took.min(t);
    }
    let per = took.as_secs_f64() / 20.0;
    let ten = (0.01 / per).max(1.0);
    // Threads that only spin, on the same CPU, sixty more each round until the draw waits past 200 ms by
    // the clock (how much of the CPU a waking thread gets is the scheduler's: sixty once gave 196 ms).
    // Every round's draw must draw: its wait never stops it, however long.
    let busy = Arc::new(AtomicBool::new(true));
    let mut spinners = Vec::new();
    let mut took = Duration::ZERO;
    for gain in [0.2, 0.21, 0.22, 0.23] {
        spinners.extend((0..60).map(|_| {
            let busy = busy.clone();
            std::thread::spawn(move || {
                let mut x = 0u64;
                while busy.load(Ordering::Relaxed) {
                    x = std::hint::black_box(x.wrapping_add(1));
                }
            })
        }));
        let drawn;
        (drawn, took) = draw(ten, gain);
        shapes(drawn);
        if took > Duration::from_millis(200) {
            break;
        }
    }
    busy.store(false, Ordering::Relaxed);
    for s in spinners {
        s.join().unwrap();
    }
    assert!(took > Duration::from_millis(200), "the CPU was shared: {took:?} by the clock for about 10 ms of work");
    // About a second of work: stopped at its 20 ms, whatever the clock says.
    let why = cant(draw(100.0 * ten, 0.3).0);
    assert!(why.starts_with("took too long and was stopped"), "{why}");
}
