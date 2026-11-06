//! An input a call may leave out (PKG-18, `@m2/core` 2.3.0): `port.audio({ sidechain: true })
//! .optional()`. The body gets `undefined` for it, the descriptor says `optional`, and an effect
//! may take one after its audio, so + Effect can add it with nothing to listen to yet. The types
//! take the same calls (`sdk/check/calls.ts`); the build refuses each mistake again, in words.

use std::time::Duration;

use m2_ir::Op;

use super::*;

fn build_of(files: &[(&str, &str)]) -> Result<Build, Vec<Diagnostic>> {
    let code: BTreeMap<String, String> = files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect();
    let mut o = Options::new(code, "song.ts");
    o.packages.push(("std".into(), std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    o.dependencies.push("std".into());
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

/// A keyed effect: it ducks its audio by its key when given one, and plays it as it is when not.
const KEYED: &str = r#"import { component, control, kernel, port, signature, type Audio, type Params } from "@m2/core";
export const keyed = component(
  {
    id: "keyed",
    label: "Keyed",
    signature: signature<(x: Audio, key?: Audio) => Audio>(),
    inputs: { in: port.audio(), key: port.audio({ sidechain: true }).optional() },
    controls: { depth: control.db(-8).range(-60, 0) },
    output: port.audio(),
    editor: { category: "Dynamics", insert: { depth: -8 } },
  },
  (x: Audio, key: Audio | undefined, { depth }: Params) =>
    key === undefined ? kernel("std:scale", { in: x, by: 1 }) : kernel("std:duck", { in: x, side: kernel("std:follow", { in: key }), depth }),
);
"#;

const SONG: &str = r#"import { song } from "@m2/core";
import { keyed } from "./lib/keyed";
import { kernel } from "@m2/core";
export default song(() => {
  const x = kernel("std:saw", { freq: 110 });
  const k = kernel("std:sine", { freq: 50 });
  return kernel("std:add", { a: keyed(x), b: keyed(x, k, { depth: -6 }) });
});
"#;

// PKG-18: a call may leave the input out, and the body gets `undefined`; given, it is the signal.
// The descriptor says the input is optional (and nothing of the others, so they keep their bytes),
// the frame of the call that leaves it out records no sidechain, and the one that gives it does.
#[test]
fn an_optional_input_may_be_left_out() {
    let b = build_of(&[("lib/keyed.ts", KEYED), ("song.ts", SONG)]).unwrap_or_else(|e| panic!("{e:?}"));
    let d = b.components.iter().find(|d| d.id == "keyed").expect("its descriptor");
    let optional: Vec<(&str, bool)> = d.inputs.iter().map(|i| (i.name.as_str(), i.optional)).collect();
    assert_eq!(optional, [("in", false), ("key", true)]);
    assert!(d.is_effect());
    let json = serde_json::to_value(&d.inputs).unwrap();
    assert_eq!(json[1]["optional"], serde_json::json!(true));
    assert!(json[0].get("optional").is_none(), "{json}");
    let m = &b.ir.modules[0];
    let kernels: Vec<&str> = m
        .nodes
        .iter()
        .filter_map(|n| match &n.op {
            Op::Kernel { kernel, .. } => Some(kernel.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kernels.iter().filter(|k| **k == "std:duck").count(), 1, "{kernels:?}");
    assert_eq!(kernels.iter().filter(|k| **k == "std:scale").count(), 1, "{kernels:?}");
    let frames: Vec<usize> = m.frames.iter().filter(|f| f.component.as_deref() == Some("keyed")).map(|f| f.sidechain.len()).collect();
    assert_eq!(frames, [0, 1]);
}

/// That `lib` doesn't build, with an error that says `says`.
fn refused(lib: &str, says: &str) {
    let song = "import { song } from \"@m2/core\";\nimport { c } from \"./lib/c\";\nimport { kernel } from \"@m2/core\";\nexport default song(() => c(kernel(\"std:saw\", { freq: 110 })));\n";
    let e = build_of(&[("lib/c.ts", lib), ("song.ts", song)]).err().unwrap_or_else(|| panic!("{lib} builds"));
    assert!(e.iter().any(|d| d.message.contains(says)), "{says}\n{e:?}");
}

fn lib(desc: &str) -> String {
    format!("import {{ component, control, each, kernel, port }} from \"@m2/core\";\nexport const c = component({desc}, (x: any) => x);\n")
}

// PKG-18: an output is one signal, always made; an input that takes many signals may be none of
// them already; a dictionary entry's inputs are its entry's; and a slot a call must give can't
// come after one it may leave out. Each refused in words, past the types (`as any`).
#[test]
fn what_may_not_be_optional_is_refused_in_words() {
    refused(&lib("{ id: \"c\", inputs: { in: port.audio() }, output: (port.audio() as any).optional() }"), "its output is always made");
    refused(
        &lib("{ id: \"c\", inputs: { in: port.audio(), xs: (port.audio().many() as any).optional() }, output: port.audio() }"),
        "takes many signals, so a call may give none already",
    );
    refused(
        &lib(
            "{ id: \"c\", inputs: { in: port.audio() }, controls: { ch: each({ side: (port.audio() as any).optional() }) }, output: port.audio() }",
        ),
        "can't be left out",
    );
    refused(
        &lib("{ id: \"c\", inputs: { in: port.audio(), key: port.audio().optional(), other: port.audio() }, output: port.audio() }"),
        "(\"key\") comes before one that may not",
    );
    // An effect takes one input a call must give: its audio.
    refused(
        &lib("{ id: \"c\", inputs: { in: port.audio(), key: port.audio() }, output: port.audio(), editor: { category: \"Dynamics\" } }"),
        "an effect (editor.category) has one input",
    );
}
