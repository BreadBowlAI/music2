//! Call sites: where in the source each call happens, and what its arguments are.
//!
//! Every call expression in a project or package file becomes a *site*. Before the
//! file runs, each call is instrumented so that, right before it is invoked (after
//! its arguments are evaluated), it records its site id in `globalThis.__m2_site`.
//! The SDK reads that at entry, which is how a node knows exactly which call in the
//! source made it. Positions reported by the JavaScript engine are not precise enough
//! for this: nested calls on one line share positions.
//!
//! The instrumentation keeps evaluation order, `this` and short-circuiting:
//! - `f(a, b)` → `f(a, __m2s(ID, b))`: the last argument is evaluated, then the site
//!   is recorded, then `f` is called. Inside an optional chain the arguments, and so
//!   the recording, are skipped exactly when the call is.
//! - `obj.m()` → `__m2s(ID, obj).m()`, only when no part of the callee is optional.
//! - `f()` → `(__m2s(ID), f)()`, only for a plain identifier.
//!
//! Other zero-argument calls (`(obj.m)()`, `obj.m!()`, `super.m()`, `a?.b.m()`, …)
//! are left alone: they get no site, and so no editable values, but keep their
//! meaning exactly.

use std::collections::HashMap;

use oxc::allocator::Allocator;
use oxc::ast::ast::{Argument, BindingPattern, CallExpression, Expression, ObjectPropertyKind, PropertyKind, UnaryOperator, VariableDeclarator};
use oxc::ast_visit::{Visit, walk};
use oxc::parser::Parser;
use oxc::span::{GetSpan, SourceType};
use serde::{Deserialize, Serialize};

/// One call in the source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Site {
    pub id: u32,
    /// The file as shown to people (`tracks/bass.ts`, `std/src/dsp.ts`).
    pub file: String,
    /// Index of this call among the file's calls, in source order.
    pub ordinal: u32,
    /// Byte range of the whole call expression in the original source.
    pub start: u32,
    pub end: u32,
    /// 1-based line of the call's start.
    pub line: u32,
    /// The callee as written: `bass`, `osc.saw`, `lane.notes`.
    pub callee: String,
    /// The directly declared variable initialized by this call, when present.
    /// A display label only: aliases and uses do not change the call's identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    /// Whether the file is the project's own (not an installed package).
    #[serde(default)]
    pub in_project: bool,
    pub args: Vec<Arg>,
}

/// What an argument looks like in the source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Arg {
    /// A number literal, possibly negated: exactly the bytes a knob may rewrite.
    /// Surrounding parentheses and comments are outside this range.
    Number { start: u32, end: u32, value: f64 },
    /// An object literal with only unique, plain properties by key.
    Object { start: u32, end: u32, props: Vec<(String, Arg)> },
    /// Anything else (an identifier, a call, a spread …).
    Other { start: u32, end: u32 },
}

impl Arg {
    /// Follow a path of property keys into nested object literals.
    pub fn at<'a>(&'a self, path: &[String]) -> Option<&'a Arg> {
        match path.split_first() {
            None => Some(self),
            Some((k, rest)) => match self {
                Arg::Object { props, .. } => props.iter().find(|(key, _)| key == k).and_then(|(_, a)| a.at(rest)),
                _ => None,
            },
        }
    }
}

impl Site {
    /// The argument at `path`: a positional index, then property keys.
    pub fn arg(&self, path: &[String]) -> Option<&Arg> {
        let (first, rest) = path.split_first()?;
        self.args.get(first.parse::<usize>().ok()?)?.at(rest)
    }
}

/// A file after instrumentation.
pub struct Instrumented {
    pub code: String,
    pub sites: Vec<Site>,
    /// `(original offset, bytes inserted before it so far)`, for mapping positions back.
    inserted: Vec<(u32, u32)>,
}

impl Instrumented {
    /// Map a byte offset in the instrumented code back to the original source.
    pub fn original_offset(&self, instrumented: u32) -> u32 {
        // Find the last insertion point whose instrumented position is <= `instrumented`.
        let mut orig = instrumented;
        for &(at, added) in &self.inserted {
            if at + added <= instrumented {
                orig = instrumented - added;
            } else {
                break;
            }
        }
        orig
    }
}

fn arg(source: &str, e: &Expression) -> Arg {
    let s = e.span();
    match e {
        Expression::NumericLiteral(n) => Arg::Number { start: s.start, end: s.end, value: n.value },
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => match &u.argument {
            // Replacing a sign and its number must not consume a comment between
            // them. Whitespace is safe; commented negation stays code-only.
            Expression::NumericLiteral(n) if source[s.start as usize..n.span.start as usize].trim() == "-" => {
                Arg::Number { start: s.start, end: s.end, value: -n.value }
            }
            _ => Arg::Other { start: s.start, end: s.end },
        },
        Expression::ParenthesizedExpression(p) => match arg(source, &p.expression) {
            number @ Arg::Number { .. } => number,
            _ => Arg::Other { start: s.start, end: s.end },
        },
        Expression::ObjectExpression(o) => {
            let mut props = vec![];
            for p in &o.properties {
                let ObjectPropertyKind::ObjectProperty(p) = p else {
                    return Arg::Other { start: s.start, end: s.end };
                };
                // A spread, computed key, duplicate, or accessor may determine
                // the runtime value instead of the literal at this source path.
                // Do not infer ownership from the value it happens to have now.
                if p.computed || p.method || p.kind != PropertyKind::Init {
                    return Arg::Other { start: s.start, end: s.end };
                }
                let Some(k) = p.key.static_name() else {
                    return Arg::Other { start: s.start, end: s.end };
                };
                if props.iter().any(|(key, _)| key == k.as_ref()) {
                    return Arg::Other { start: s.start, end: s.end };
                }
                props.push((
                    k.to_string(),
                    if p.shorthand { Arg::Other { start: p.span.start, end: p.span.end } } else { arg(source, &p.value) },
                ));
            }
            Arg::Object { start: s.start, end: s.end, props }
        }
        _ => Arg::Other { start: s.start, end: s.end },
    }
}

/// Whether wrapping `e` as a call receiver keeps its meaning: not `super`, and no
/// optional link anywhere in its chain (which would short-circuit the whole call).
fn plain_object(e: &Expression) -> bool {
    match e {
        Expression::Super(_) => false,
        Expression::ChainExpression(_) => false,
        Expression::StaticMemberExpression(m) => !m.optional && plain_object(&m.object),
        Expression::ComputedMemberExpression(m) => !m.optional && plain_object(&m.object),
        Expression::PrivateFieldExpression(m) => !m.optional && plain_object(&m.object),
        Expression::CallExpression(c) => !c.optional && plain_object(&c.callee),
        _ => true,
    }
}

struct Collect<'s> {
    source: &'s str,
    file: String,
    next_id: u32,
    sites: Vec<Site>,
    /// Direct variable initializers, keyed by the call's original byte range.
    bindings: HashMap<(u32, u32), String>,
    /// `(offset, sort key, text)`.
    inserts: Vec<(u32, (u8, i64), String)>,
}

impl<'a> Visit<'a> for Collect<'_> {
    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(binding) = &it.id
            && let Some(mut initializer) = it.init.as_ref()
        {
            while let Expression::ParenthesizedExpression(p) = initializer {
                initializer = &p.expression;
            }
            if let Expression::CallExpression(call) = initializer {
                self.bindings.insert((call.span.start, call.span.end), binding.name.to_string());
            }
        }
        walk::walk_variable_declarator(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        let id = self.next_id;
        let target = match it.arguments.last() {
            Some(Argument::SpreadElement(s)) => Some((s.argument.span(), false)),
            Some(a) => Some((a.to_expression().span(), false)),
            None if it.optional => None,
            None => match &it.callee {
                Expression::Identifier(i) => Some((i.span, true)),
                Expression::StaticMemberExpression(m) if !m.optional && plain_object(&m.object) => Some((m.object.span(), false)),
                Expression::ComputedMemberExpression(m) if !m.optional && plain_object(&m.object) => Some((m.object.span(), false)),
                _ => None,
            },
        };
        if let Some((span, sequence)) = target {
            self.next_id += 1;
            let (open, close) = if sequence { (format!("(__m2s({id}), "), ")".to_string()) } else { (format!("__m2s({id}, "), ")".to_string()) };
            let len = (span.end - span.start) as i64;
            // At one offset: closes before opens; inner closes first; outer opens first.
            self.inserts.push((span.start, (1, -len), open));
            self.inserts.push((span.end, (0, len), close));
            let line = self.source[..it.span.start as usize].matches('\n').count() as u32 + 1;
            let args = it.arguments.iter().map(|a| match a {
                Argument::SpreadElement(s) => Arg::Other { start: s.span.start, end: s.span.end },
                a => arg(self.source, a.to_expression()),
            });
            let callee = self.source[it.callee.span().start as usize..it.callee.span().end as usize].to_string();
            let binding = self.bindings.get(&(it.span.start, it.span.end)).cloned();
            self.sites.push(Site { id, file: self.file.clone(), ordinal: 0, start: it.span.start, end: it.span.end, line, callee, binding, in_project: false, args: args.collect() });
        }
        walk::walk_call_expression(self, it);
    }
}

/// Instrument `source` (TypeScript). Site ids start at `first_id`.
pub fn instrument(file: &str, source: &str, first_id: u32) -> Result<Instrumented, String> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    if let Some(e) = ret.diagnostics.first() {
        let at = e.labels.first().map(|l| l.offset() as usize).unwrap_or(0);
        let before = &source[..at.min(source.len())];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        return Err(format!("{file}:{line}:{col}: {}", e.message));
    }
    let mut c = Collect { source, file: file.to_string(), next_id: first_id, sites: vec![], bindings: HashMap::new(), inserts: vec![] };
    c.visit_program(&ret.program);
    let mut sites = c.sites;
    sites.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    for (i, s) in sites.iter_mut().enumerate() {
        s.ordinal = i as u32;
    }
    let mut inserts = c.inserts;
    inserts.sort_by_key(|(at, key, _)| (*at, *key));
    let mut code = String::with_capacity(source.len() + inserts.len() * 12);
    let mut inserted = vec![];
    let (mut from, mut added) = (0usize, 0u32);
    for (at, _, text) in &inserts {
        code.push_str(&source[from..*at as usize]);
        code.push_str(text);
        added += text.len() as u32;
        inserted.push((*at, added));
        from = *at as usize;
    }
    code.push_str(&source[from..]);
    Ok(Instrumented { code, sites, inserted })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruments_without_changing_meaning() {
        let src = "const b = vca(svf.lowpass(osc.saw(hz(110)), hz(800)), 0.5);\nf();\nobj.m();\nobj.m(...xs);\n";
        let i = instrument("t.ts", src, 0).unwrap();
        assert_eq!(
            i.code,
            "const b = vca(svf.lowpass(osc.saw(__m2s(2, hz(__m2s(3, 110)))), __m2s(1, hz(__m2s(4, 800)))), __m2s(0, 0.5));\n\
             (__m2s(5), f)();\n__m2s(6, obj).m();\nobj.m(...__m2s(7, xs));\n"
        );
        // Sites in source order with their argument shapes.
        let s = &i.sites;
        assert_eq!(s.iter().map(|s| &src[s.start as usize..s.end as usize]).collect::<Vec<_>>()[..4], [
            "vca(svf.lowpass(osc.saw(hz(110)), hz(800)), 0.5)",
            "svf.lowpass(osc.saw(hz(110)), hz(800))",
            "osc.saw(hz(110))",
            "hz(110)",
        ]);
        assert!(matches!(s[0].args[1], Arg::Number { value, .. } if value == 0.5));
        assert!(matches!(s[3].args[0], Arg::Number { value, .. } if value == 110.0));
    }

    #[test]
    fn leaves_receiver_sensitive_zero_argument_calls_alone() {
        for src in ["(obj.get)();", "obj.get!();", "super.get();", "obj?.inner.method();", "obj?.m();", "f?.();", "a.b?.c.d();"] {
            let i = instrument("t.ts", src, 0).unwrap();
            assert_eq!(i.code, src, "{src} must not be rewritten");
            assert!(i.sites.is_empty());
        }
        // With arguments, the recording sits inside the arguments, so it is skipped
        // exactly when an optional chain skips the call.
        assert_eq!(instrument("t.ts", "obj?.m(1);", 0).unwrap().code, "obj?.m(__m2s(0, 1));");
    }

    #[test]
    fn object_arguments_and_negative_numbers() {
        let src = "mixer({ hats: { in: x, db: db(-6), pan: 0.3 } });";
        let i = instrument("t.ts", src, 10).unwrap();
        let mixer = &i.sites[0];
        let pan = mixer.arg(&["0".into(), "hats".into(), "pan".into()]).unwrap();
        assert!(matches!(pan, Arg::Number { start, end, value } if &src[*start as usize..*end as usize] == "0.3" && *value == 0.3));
        let db = i.sites.iter().find(|s| &src[s.start as usize..s.end as usize] == "db(-6)").unwrap();
        assert!(matches!(db.args[0], Arg::Number { start, end, value } if &src[start as usize..end as usize] == "-6" && value == -6.0));
    }

    #[test]
    fn numeric_edits_preserve_surrounding_parentheses_and_comments() {
        for (src, expected, value) in [
            ("hz((/* before */ 800 /* after */));", "hz((/* before */ 950 /* after */));", 800.0),
            ("hz(((/* before */ -800 /* after */)));", "hz(((/* before */ 950 /* after */)));", -800.0),
            ("hz((0x320));", "hz((950));", 800.0),
            ("hz((8_00));", "hz((950));", 800.0),
        ] {
            let i = instrument("t.ts", src, 0).unwrap();
            let Arg::Number { start, end, value: actual } = i.sites[0].args[0] else {
                panic!("expected an editable literal in {src}");
            };
            assert_eq!(actual, value);
            let mut edited = src.to_string();
            edited.replace_range(start as usize..end as usize, "950");
            assert_eq!(edited, expected);
        }
    }

    #[test]
    fn commented_negation_is_read_only() {
        for src in ["db(-/* keep me */6);", "db(- // keep me\n6);", "db((-/* keep me */6));"] {
            let i = instrument("t.ts", src, 0).unwrap();
            assert!(matches!(i.sites[0].args[0], Arg::Other { .. }), "{src}");
        }
        let i = instrument("t.ts", "db(- 6);", 0).unwrap();
        assert!(matches!(i.sites[0].args[0], Arg::Number { value: -6.0, .. }));
    }

    #[test]
    fn ambiguous_objects_have_no_editable_property_paths() {
        for src in [
            "f({ cutoff: 800, ...other });",
            "f({ ...other, cutoff: 800 });",
            "f({ cutoff: 800, cutoff: 800 });",
            "f({ cutoff: 800, 'cutoff': 800 });",
            "f({ 1: 800, '1': 800 });",
            "f({ cutoff: 800, [key]: 800 });",
            "f({ cutoff: 800, get value() { return 800; } });",
            "f({ cutoff: 800, set value(v) {} });",
            "f({ cutoff: 800, method() {} });",
        ] {
            let i = instrument("t.ts", src, 0).unwrap();
            let site = &i.sites[0];
            assert!(matches!(site.args[0], Arg::Other { .. }), "{src}");
            assert!(site.arg(&["0".into(), "cutoff".into()]).is_none(), "{src}");
        }
    }

    #[test]
    fn ordinary_nested_object_paths_remain_editable() {
        let src = "f({ left: { cutoff: (/* Hz */ 800) }, right: { cutoff: 800, ...other }, input, 'gain': -6 });";
        let i = instrument("t.ts", src, 0).unwrap();
        let site = &i.sites[0];
        assert!(
            matches!(site.arg(&["0".into(), "left".into(), "cutoff".into()]), Some(Arg::Number { start, end, value: 800.0 }) if &src[*start as usize..*end as usize] == "800")
        );
        assert!(site.arg(&["0".into(), "right".into(), "cutoff".into()]).is_none());
        assert!(matches!(site.arg(&["0".into(), "input".into()]), Some(Arg::Other { .. })));
        assert!(matches!(site.arg(&["0".into(), "gain".into()]), Some(Arg::Number { value: -6.0, .. })));
    }

    #[test]
    fn names_only_direct_call_initializers() {
        let src = r#"
            export const cutoff: number = hz(800);
            const gain = ((/* label comes from syntax */ db(-6)));
            const sound = filter(input, hz(900));
            const alias = cutoff;
            const calculated = hz(1000) * 2;
            const { value } = makeValue(1);
            const delayed = () => hz(1100);
            const pair = [hz(1200)];
            hz(1300);
        "#;
        let i = instrument("t.ts", src, 0).unwrap();
        let bindings: Vec<_> = i.sites.iter().map(|site| (&src[site.start as usize..site.end as usize], site.binding.as_deref())).collect();
        assert_eq!(bindings, vec![
            ("hz(800)", Some("cutoff")),
            ("db(-6)", Some("gain")),
            ("filter(input, hz(900))", Some("sound")),
            ("hz(900)", None),
            ("hz(1000)", None),
            ("makeValue(1)", None),
            ("hz(1100)", None),
            ("hz(1200)", None),
            ("hz(1300)", None),
        ]);
    }

    #[test]
    fn source_binding_is_optional_in_serialized_sites() {
        let i = instrument("t.ts", "const cutoff = hz(800);", 0).unwrap();
        let mut serialized = serde_json::to_value(&i.sites[0]).unwrap();
        assert_eq!(serialized["binding"], "cutoff");
        serialized.as_object_mut().unwrap().remove("binding");
        let restored: Site = serde_json::from_value(serialized).unwrap();
        assert_eq!(restored.binding, None);
    }

    #[test]
    fn maps_offsets_back() {
        let src = "f(1);";
        let i = instrument("t.ts", src, 0).unwrap();
        assert_eq!(i.code, "f(__m2s(0, 1));");
        // The `)` and `;` are at 3 and 4 in the source, 13 and 14 in the instrumented code.
        assert_eq!(i.original_offset(13), 3);
        assert_eq!(i.original_offset(14), 4);
        assert_eq!(i.original_offset(0), 0);
    }
}
