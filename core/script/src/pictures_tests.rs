//! Pictures on the card (CARD-60 to CARD-65): `view.picture` in a descriptor, checked by the build,
//! and drawn by the picture runner. TypeScript refuses a dot's wrong names as the code is typed
//! (`sdk/check/pictures.ts`); the build and the runner refuse each mistake again, in words, since
//! types can be bypassed (`as any`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::pictures::{Ask, Drawn, Runner};
use super::*;
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
          draw: (v, p) => {
            if (v.hang > 0) for (;;) {}
            const points: number[] = [];
            for (let i = 0; i < 180; i++) {
              const f = 20 * 1000 ** (i / 179);
              let db = 0;
              for (const b of BANDS) if (v[`on${b}`]) db += bell(f, v[`freq${b}`], v[`gain${b}`], v[`q${b}`]);
              points.push(f, db);
            }
            return [
              p.part("grid", () => (grids++, [p.mark("y", 0, "0 dB"), p.label(30, 20, `grid ${grids} at q ${v.q1}`)])),
              p.line(points, { name: "curve" }),
              ...BANDS.map((b) =>
                p.handle(v[`freq${b}`], v[`gain${b}`], { x: `freq${b}`, y: `gain${b}`, wheel: `q${b}`, toggles: `on${b}`, opens: `Band ${b}`, mark: `${b}`, off: !v[`on${b}`] }),
              ),
            ];
          },
          words: (v) => BANDS.map((b) => `Band ${b}: ${v[`gain${b}`]} dB at ${v[`freq${b}`]} Hz`).join("; "),
        }),
      },
      groups: { "Band 1": ["on1", "freq1", "gain1", "q1"], "Band 2": ["on2", "freq2", "gain2", "q2"] },
      face: ["curve"],
    },
  },
  (x, v) => kernel<Audio>("test:curve", { x, ...v }),
);
"#;

/// The values the curve plays at its defaults, as the server gives them (CARD-63).
fn at_defaults() -> Value {
    json!({"freq1": 200, "gain1": 0, "q1": 1, "on1": true, "freq2": 2000, "gain2": 0, "q2": 1, "on2": true, "hang": 0})
}

/// A folder holding a package `@test/curve` whose `src/index.ts` is `index`.
struct Pack(PathBuf);

impl Pack {
    fn new(name: &str, index: &str) -> Pack {
        let dir = std::env::temp_dir().join(format!("m2-script-{}-pictures-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/index.ts"), index).unwrap();
        Pack(dir)
    }

    fn packages(&self) -> Vec<(String, PathBuf)> {
        vec![("@test/curve".into(), self.0.clone()), ("std".into(), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std"))]
    }

    /// An ask for the curve's picture `view` of `component`, at `values`.
    fn ask(&self, component: &str, view: &str, values: Value) -> Ask {
        Ask {
            packages: self.packages(),
            song: None,
            module: "@test/curve".into(),
            component: format!("@test/curve:{component}"),
            view: view.into(),
            values: values.to_string(),
            memo: "song#1".into(),
        }
    }
}

impl Drop for Pack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What a draw gave, as JSON: it must have drawn.
fn shapes(d: Drawn) -> Value {
    match d {
        Drawn::Shapes(json) => serde_json::from_str(&json).unwrap(),
        Drawn::Cant(why) => panic!("it didn't draw: {why}"),
    }
}

/// Why a draw couldn't be drawn: it must not have.
fn cant(d: Drawn) -> String {
    match d {
        Drawn::Cant(why) => why,
        Drawn::Shapes(json) => panic!("it drew: {json}"),
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
    assert_eq!((x.scale.as_str(), x.unit, x.lo, x.hi), ("log", m2_ir::Unit::Hz, 20.0, 20000.0));
    assert_eq!((y.scale.as_str(), y.unit, y.lo, y.hi), ("lin", m2_ir::Unit::Db, -24.0, 24.0));
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

/// The build of a sound whose `curve` view is `picture`, written past the types.
fn picture(picture: &str) -> Result<Build, Vec<Diagnostic>> {
    let lib = format!(
        "import {{ axis, component, control, kernel, knob, live, port, view }} from \"@m2/core\";\n\
         export const c = component({{ id: \"c\", controls: {{ f: control.hz(100), g: control.db(0) }}, editor: {{ views: {{ curve: {picture} }} }} as any }}, (p: any) => kernel(\"test:c\", {{}}));\n"
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

// CARD-60: each mistake in a picture's declaration is refused by the build, in words, at its call.
#[test]
fn a_pictures_declaration_is_refused_in_words() {
    let ok = "view.picture({ x: axis.log(\"hz\", 20, 20000), y: axis.lin(\"db\", -24, 24), draw: () => [], words: () => \"flat\" })";
    picture(ok).unwrap_or_else(|e| panic!("{e:?}"));
    let time = "view.picture({ x: axis.time(2), y: axis.lin(\"unit01\", 0, 1), draw: () => [], words: () => \"x\" })";
    let b = picture(time).unwrap_or_else(|e| panic!("{e:?}"));
    let ViewOf::Picture { x, .. } = &b.components.iter().find(|d| d.id == "c").unwrap().editor.views[0].of else { panic!() };
    assert_eq!((x.scale.as_str(), x.unit, x.lo, x.hi), ("time", m2_ir::Unit::Seconds, 0.0, 2.0));
    for (bad, says) in [
        (
            ok.replace("axis.log(\"hz\", 20, 20000)", "axis.log(\"hz\", 0, 20000)"),
            "its x axis is on a log scale, so it starts above 0, not at 0",
        ),
        (
            ok.replace("axis.lin(\"db\", -24, 24)", "axis.lin(\"db\", 24, -24)"),
            "its y axis must go from a lower number to a higher one, not from 24 to -24",
        ),
        (
            ok.replace("axis.lin(\"db\", -24, 24)", "axis.lin(\"dbs\" as any, -24, 24)"),
            "its y axis's unit is \"dbs\", which isn't a unit (units: hz, db,",
        ),
        (ok.replace("axis.lin(\"db\", -24, 24)", "{ unit: \"db\" }"), "its y must be made with axis.lin(), axis.log() or axis.time()"),
        (
            ok.replace(", words: () => \"flat\"", ""),
            "a picture says itself in words: give it words, a function of its values that returns one sentence",
        ),
        (ok.replace("draw: () => []", "draw: []"), "its draw must be a function of its values and the builder p"),
        (
            ok.replace("draw: () => []", "draw: () => [], live: [live.spectrum(\"side\" as any)]"),
            "its live layer 1, live.spectrum, looks at \"side\": a spectrum looks at \"in\" or \"out\"",
        ),
        (
            ok.replace("draw: () => []", "draw: () => [], live: [\"spectrum\"]"),
            "its live layer 1 must be made with live.spectrum(), live.level(), live.meter(), live.history(), live.playhead() or live.scope()",
        ),
        (
            ok.replace("draw: () => []", "draw: () => [], live: [live.spectrum(\"in\", { tone: \"red\" } as any)]"),
            "its live layer 1's tone is \"red\": a tone is main, soft or faint, never a colour",
        ),
        (
            ok.replace("draw: () => []", "draw: () => [], colour: \"red\""),
            "a picture has no field \"colour\" (fields: x, y, live, draw, words)",
        ),
    ] {
        let e = picture(&bad).err().unwrap_or_else(|| panic!("{bad} builds"));
        assert!(e.iter().any(|d| d.message.contains("editor.views' \"curve\"") && d.message.contains(says)), "{bad}: {says}\n{e:?}");
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
    assert_eq!(s[0]["shapes"][0], json!({"kind": "mark", "axis": "y", "at": 0, "words": "0 dB"}));
    let line = s[1]["points"].as_array().unwrap();
    assert_eq!((line.len(), &s[1]["name"]), (360, &json!("curve")));
    // At 2000 Hz the curve dips by band 2's gain.
    let at =
        line.chunks(2).min_by(|a, b| (a[0].as_f64().unwrap() - 2000.0).abs().total_cmp(&(b[0].as_f64().unwrap() - 2000.0).abs())).unwrap();
    assert!((at[1].as_f64().unwrap() + 3.5).abs() < 0.2, "{at:?}");
    assert_eq!(
        s[3],
        json!({"kind": "handle", "x": 2000, "y": -3.5, "moves": {"x": {"name": "freq2"}, "y": {"name": "gain2"}, "wheel": {"name": "q2"}, "toggles": {"name": "on2"}}, "opens": "Band 2", "mark": "2", "off": false})
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
        ("p.line([1, 2, 3])", "gives p.line an odd count of numbers (3): points are x, y pairs"),
        ("p.line([1, NaN])", "gives p.line a point that isn't a finite number"),
        ("p.line(new Array(20002).fill(1))", "draws more than 20,000 numbers"),
        ("{ kind: \"line\", points: [] }", "returns something p didn't make: make each shape with p.line(), p.handle() and the rest"),
        ("v.nope", "reads nope, which this sound doesn't have"),
        ("(v.freq1 = 3, p.line([]))", "can't change freq1: a picture reads what plays, and changes nothing"),
        ("(() => { throw new Error(\"no curve here\") })()", "no curve here"),
    ];
    let mut index = CURVE.to_string();
    let draws: Vec<String> = checks.iter().enumerate().map(|(i, (d, _))| format!("          k{i}: view.picture({{ x: axis.log(\"hz\", 20, 20000), y: axis.lin(\"db\", -24, 24), draw: (v: any, p: any) => [{d}], words: () => \"x\" }}),\n")).collect();
    index = index.replacen("      views: {\n", &format!("      views: {{\n{}", draws.concat()), 1);
    index = index.replace("import { axis,", "import { knob, axis,");
    let pack = Pack::new("checks", &index);
    let runner = Runner::new();
    for (i, (draw, says)) in checks.iter().enumerate() {
        let why = cant(runner.draw(pack.ask("curve", &format!("k{i}"), at_defaults())));
        assert!(why.contains(says), "{draw}: {says}\n{why}");
    }
    // Words that aren't one line.
    let words = index.replace("words: (v) => BANDS", "words: (v) => \"two\\nlines\" || BANDS");
    let pack = Pack::new("words", &words);
    assert!(cant(runner.draw(pack.ask("curve", "curve", at_defaults()))).contains("its words must be one line of 1 to 200 characters"));
    // A view that isn't a picture, or a sound that isn't there.
    assert!(cant(runner.draw(pack.ask("nothing", "curve", at_defaults()))).contains("@test/curve:nothing"));
}

// CARD-64: each draw has its own limit, counted from its start. One past it is stopped in words,
// the runtime made again, and the next draws.
#[test]
fn an_endless_draw_is_stopped_and_the_next_draws() {
    let pack = Pack::new("endless", CURVE);
    let runner = Runner::new();
    shapes(runner.draw(pack.ask("curve", "curve", at_defaults())));
    let mut hang = at_defaults();
    hang["hang"] = json!(1);
    let t = Instant::now();
    let why = cant(runner.draw(pack.ask("curve", "curve", hang)));
    assert!(why.contains("took too long and was stopped"), "{why}");
    assert!(t.elapsed() < Duration::from_secs(1), "stopped soon: {:?}", t.elapsed());
    shapes(runner.draw(pack.ask("curve", "curve", at_defaults())));
    // A hundred draws take far longer than one's limit in all, and none is stopped.
    let t = Instant::now();
    for i in 0..100 {
        let mut v = at_defaults();
        v["gain1"] = json!(i as f64 / 10.0);
        shapes(runner.draw(pack.ask("curve", "curve", v)));
    }
    assert!(t.elapsed() > Duration::from_millis(20), "{:?}", t.elapsed());
}

// CARD-64: a part runs again only when a value it read changed since that picture's last draw.
#[test]
fn a_part_runs_again_only_when_what_it_read_changed() {
    let pack = Pack::new("parts", CURVE);
    let runner = Runner::new();
    let label = |v: Value| shapes(runner.draw(pack.ask("curve", "curve", v)))["shapes"][0]["shapes"][1]["words"].clone();
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
    let code: BTreeMap<String, String> = [("lib/curve.ts".to_string(), CURVE.to_string())].into();
    let ask = Ask {
        packages: vec![],
        song: Some(Arc::new(pictures::Song { key: 7, code, dependencies: vec![] })),
        module: "lib/curve.ts".into(),
        component: "curve".into(),
        view: "curve".into(),
        values: at_defaults().to_string(),
        memo: "song#1".into(),
    };
    assert_eq!(shapes(runner.draw(ask))["words"], "Band 1: 0 dB at 200 Hz; Band 2: 0 dB at 2000 Hz");
}
