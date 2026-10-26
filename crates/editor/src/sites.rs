//! The source helpers the planners use (WRITE-10): what a file imports, where to import a name,
//! and how a track's function reaches its lanes. Read with oxc, as the build reads the source.

use std::path::Path;

use oxc::allocator::Allocator;
use oxc::ast::ast::{
    ArrowFunctionExpression, BindingIdentifier, BindingPattern, FormalParameters, Function, IdentifierReference,
    ImportDeclarationSpecifier, Statement,
};
use oxc::ast_visit::{Visit, walk};
use oxc::parser::Parser;
use oxc::span::{GetSpan, SourceType, Span};
use oxc::syntax::scope::ScopeFlags;

/// The names a file imports from `module` under their own name (`import { hz } from
/// "@m2/core"`, not `import { hz as h }`): what generated code may call by that name.
pub fn imported_names(source: &str, module: &str) -> Vec<String> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut names = vec![];
    for statement in &ret.program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if import.source.value != module || import.import_kind.is_type() {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            if let ImportDeclarationSpecifier::ImportSpecifier(s) = specifier
                && !s.import_kind.is_type()
                && s.imported.name() == s.local.name
            {
                names.push(s.local.name.to_string());
            }
        }
    }
    names
}

/// Where to insert text so that `source` imports `name` from `module`, and the text;
/// `None` if it already does. Joins an existing `import { … } from module`, or else adds
/// a line after the last import.
pub fn import_insertion(source: &str, module: &str, name: &str) -> Option<(u32, String)> {
    if imported_names(source, module).iter().any(|n| n == name) {
        return None;
    }
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut after_imports = None;
    for statement in &ret.program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        after_imports = Some(import.span.end);
        let mut named = import.specifiers.iter().flatten().filter(|s| matches!(s, ImportDeclarationSpecifier::ImportSpecifier(_)));
        if import.source.value == module
            && !import.import_kind.is_type()
            && let Some(last) = named.next_back()
        {
            return Some((last.span().end, format!(", {name}")));
        }
    }
    let line = format!("import {{ {name} }} from {};", serde_json::to_string(module).expect("a string"));
    Some(match after_imports {
        Some(end) => (end, format!("\n{line}")),
        None => (0, format!("{line}\n")),
    })
}

/// Words a binding can never be: JavaScript's and TypeScript's reserved words, and the names a
/// module's code always has in scope.
const RESERVED: &[&str] = &[
    "arguments",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "eval",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "of",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Every name `source` binds anywhere (declarations, parameters, imports): what a new binding
/// must not be.
pub fn bound_names(source: &str) -> std::collections::BTreeSet<String> {
    struct Names(std::collections::BTreeSet<String>);
    impl<'a> Visit<'a> for Names {
        fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
            self.0.insert(it.name.to_string());
        }
        fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
            // A global it reads (`Math`) is in scope too.
            self.0.insert(it.name.to_string());
        }
    }
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut names = Names(Default::default());
    names.visit_program(&ret.program);
    names.0
}

/// A key as a binding (ID-1 keys are lowercase letters, digits and hyphens): `lead-vox` →
/// `leadVox`, `808` → `t808`.
pub fn camel(key: &str) -> String {
    let mut out = String::new();
    for (i, part) in key.split('-').filter(|p| !p.is_empty()).enumerate() {
        let mut c = part.chars();
        match (i, c.next()) {
            (0, Some(f)) => out.extend(std::iter::once(f).chain(c)),
            (_, Some(f)) => out.extend(f.to_uppercase().chain(c)),
            _ => {}
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 't');
    }
    out
}

/// A name for a new binding in `source`, from `wanted`: it, unless the file binds or reads it or
/// it is a reserved word; then `<wanted>Track`, `<wanted>Track2`, and so on.
pub fn free_binding(source: &str, wanted: &str) -> String {
    let taken = bound_names(source);
    let free = |n: &str| !taken.contains(n) && !RESERVED.contains(&n);
    if free(wanted) {
        return wanted.to_string();
    }
    (1..).map(|n| if n == 1 { format!("{wanted}Track") } else { format!("{wanted}Track{n}") }).find(|n| free(n)).expect("some name is free")
}

/// Whether `key` can be written as a property name without quotes.
pub fn identifier(key: &str) -> bool {
    key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// A top-level import of a project file: its statement's bytes, and how it binds it.
#[derive(Clone, Debug, PartialEq)]
pub struct FileImport {
    pub start: u32,
    pub end: u32,
    /// `import kick from "./tracks/kick"`'s `kick`.
    pub default: Option<String>,
    /// It binds named exports too (`import k, { env } from …`).
    pub named: bool,
    /// The specifier as written.
    pub specifier: String,
}

/// The project path a relative specifier written in `from` names, `.ts` added when it has no
/// extension: `./tracks/kick` in `song.ts` is `tracks/kick.ts`. None for a package's.
pub fn resolve(from: &str, specifier: &str) -> Option<String> {
    if !(specifier.starts_with("./") || specifier.starts_with("../")) {
        return None;
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in specifier.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    let joined = parts.join("/");
    Some(match Path::new(&joined).extension() {
        Some(_) => joined,
        None => format!("{joined}.ts"),
    })
}

/// How `from` imports project file `target`: `./tracks/pluck` from `song.ts`.
pub fn specifier(from: &str, target: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').collect::<Vec<_>>().split_last().map(|(_, d)| d.to_vec()).unwrap_or_default();
    let to: Vec<&str> = target.trim_end_matches(".ts").split('/').collect();
    let common = from_dir.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let ups = from_dir.len() - common;
    let rest = to[common..].join("/");
    if ups == 0 { format!("./{rest}") } else { format!("{}{rest}", "../".repeat(ups)) }
}

/// The top-level imports in `source` (written in file `file`) of project file `target`.
pub fn imports_of(source: &str, file: &str, target: &str) -> Vec<FileImport> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut out = vec![];
    for statement in &ret.program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        // A specifier with a lone surrogate names no project file.
        let Some(specifier) = import.source.value.as_str() else { continue };
        if resolve(file, specifier).as_deref() != Some(target) {
            continue;
        }
        let mut default = None;
        let mut named = false;
        for s in import.specifiers.iter().flatten() {
            match s {
                ImportDeclarationSpecifier::ImportDefaultSpecifier(d) => default = Some(d.local.name.to_string()),
                _ => named = true,
            }
        }
        out.push(FileImport { start: import.span.start, end: import.span.end, default, named, specifier: specifier.to_string() });
    }
    out
}

/// Where a new default import of a project file goes in `source`: after the last default import
/// of a file in the same folder as `beside` (`./tracks/…`), else after the last import, else at
/// the top. The byte offset, and whether it is at the top (so the line ends rather than starts
/// with a newline).
pub fn import_place(source: &str, file: &str, beside: &str) -> (u32, bool) {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let folder = |p: &str| p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    let (mut last, mut sibling) = (None, None);
    for statement in &ret.program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        last = Some(import.span.end);
        let default = import.specifiers.iter().flatten().any(|s| matches!(s, ImportDeclarationSpecifier::ImportDefaultSpecifier(_)));
        if default && import.source.value.as_str().and_then(|s| resolve(file, s)).is_some_and(|t| folder(&t) == folder(beside)) {
            sibling = Some(import.span.end);
        }
    }
    match sibling.or(last) {
        Some(at) => (at, false),
        None => (0, true),
    }
}

/// The top-level statements of `source`, each as its bytes and whether it is an import.
pub fn statements(source: &str) -> Vec<(u32, u32, bool)> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    ret.program.body.iter().map(|s| (s.span().start, s.span().end, matches!(s, Statement::ImportDeclaration(_)))).collect()
}

/// Bytes `start..end` of `source` widened to whole lines when nothing else is on them (a trailing
/// comment on the last line goes too), so removing them leaves no blank line; else as they are.
pub fn whole_lines(source: &str, start: u32, end: u32) -> (u32, u32) {
    let (s, e) = (start as usize, end as usize);
    let line_start = source[..s].rfind('\n').map_or(0, |i| i + 1);
    let before = &source[line_start..s];
    let line_end = source[e..].find('\n').map_or(source.len(), |i| e + i);
    let after = source[e..line_end].trim();
    if !before.trim().is_empty() || !(after.is_empty() || after.starts_with("//")) {
        return (start, end);
    }
    let end = if line_end < source.len() { line_end + 1 } else { line_end };
    (line_start as u32, end as u32)
}

/// Where `source` reads the name `name`: the start of each reference to it, in order.
pub fn references(source: &str, name: &str) -> Vec<u32> {
    struct Refs<'n>(&'n str, Vec<u32>);
    impl<'a> Visit<'a> for Refs<'_> {
        fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
            if it.name == self.0 {
                self.1.push(it.span.start);
            }
        }
    }
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut refs = Refs(name, vec![]);
    refs.visit_program(&ret.program);
    refs.1
}

/// The edits that take out of `source`'s imports every name that nothing outside `removed` (byte
/// ranges about to go) reads any more: `duck` once the last duck is excised. A statement that
/// imports nothing then goes with its line.
pub fn unused_imports(source: &str, removed: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let gone = |at: u32| removed.iter().any(|(a, b)| *a <= at && at < *b);
    let mut out = vec![];
    for statement in &ret.program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        if gone(import.span.start) {
            continue;
        }
        let Some(specs) = import.specifiers.as_ref().filter(|s| !s.is_empty()) else { continue };
        let unused: Vec<bool> = specs
            .iter()
            .map(|sp| {
                let refs = references(source, &sp.local().name);
                !refs.is_empty() && refs.iter().all(|r| gone(*r))
            })
            .collect();
        if !unused.iter().any(|u| *u) {
            continue;
        }
        if unused.iter().all(|u| *u) {
            out.push(whole_lines(source, import.span.start, import.span.end));
            continue;
        }
        // One name of several: it and the comma that joins it to the next (or to the one before).
        for (i, sp) in specs.iter().enumerate().filter(|(i, _)| unused[*i]) {
            let span = sp.span();
            match specs.get(i + 1) {
                Some(next) => out.push((span.start, next.span().start)),
                None => out.push((specs[i - 1].span().end, span.end)),
            }
        }
    }
    // Two names side by side at the end overlap where they join: one range.
    out.sort();
    let mut merged: Vec<(u32, u32)> = vec![];
    for (a, b) in out {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// What reading a function's parameters gives: how it reaches `lane`, or why that isn't certain.
pub type Params = Result<FnParams, &'static str>;

/// How a module's function (a `track()`'s or the `song()`'s) reaches its context's lanes, as
/// automate writes `<lane>.control(…)` inside it (HOME-2).
#[derive(Clone, Debug, PartialEq)]
pub struct FnParams {
    /// The parameter list's bytes, parentheses included when written: what an edit of it may
    /// change.
    pub start: u32,
    pub end: u32,
    pub lane: Lane,
}

/// What `lane` is called inside the function.
#[derive(Clone, Debug, PartialEq)]
pub enum Lane {
    /// Already reachable as this expression: `lane` for `({ lane })`, `l` for `({ lane: l })`,
    /// `ctx.lane` for `(ctx)`.
    Named(String),
    /// Reachable as `lane` once bytes `start..end` of the source are replaced by `text`: `()` →
    /// `({ lane })`, `({ time })` → `({ time, lane })`.
    Added { start: u32, end: u32, text: String },
}

/// How the function written at `span` of `source` (an argument of a `track()` or `song()` call)
/// reaches `lane`. Refused, saying why, for what can't be read with certainty: a parameter with a
/// default, a rest, a nested pattern, more than one parameter, or a function that uses the name
/// `lane` for something else already.
pub fn fn_params(source: &str, span: (u32, u32)) -> Params {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut find = FindFn { within: span, found: None, bindings: vec![], lanes: vec![] };
    find.visit_program(&ret.program);
    let Some(Found { span: fn_span, list, params, rest }) = find.found else {
        return Err("the track's code isn't a function written in its call");
    };
    let inside = |s: &Span| fn_span.start <= s.start && s.end <= fn_span.end;
    // A name bound again inside the function (other than by the parameter itself) would shadow
    // the one automate writes; `lane` used at all, while no parameter binds it, would clash.
    let rebound = |name: &str, param: Span| find.bindings.iter().any(|(n, s)| n == name && inside(s) && *s != param);
    let used = find.lanes.iter().any(inside) || find.bindings.iter().any(|(n, s)| n == "lane" && inside(s));
    let clash = "the track's function uses the name `lane` for something else";
    let (start, end) = (list.start, list.end);
    let added = |at: u32, to: u32, text: &str| Lane::Added { start: at, end: to, text: text.to_string() };
    let lane = match &params[..] {
        [] if rest => return Err("the track's function takes its arguments as a rest"),
        [] if used => return Err(clash),
        // `()`, or a bare `function ()`: the context is its first parameter now.
        [] => match source.get(start as usize..end as usize) {
            Some(t) if t.starts_with('(') && t.ends_with(')') => added(start, end, "({ lane })"),
            _ => return Err("the track's function's parameters aren't where they are expected"),
        },
        [Param::Name(name, at)] if rebound(name, *at) => return Err(clash),
        [Param::Name(name, _)] => Lane::Named(format!("{name}.lane")),
        [Param::Object { props, rest: false, span: pattern }] => match props.iter().find(|p| p.key == "lane") {
            Some(Prop { value: Some((name, at)), .. }) if rebound(name, *at) => return Err(clash),
            Some(Prop { value: Some((name, _)), .. }) => Lane::Named(name.clone()),
            Some(Prop { value: None, .. }) => return Err("the track's function reads its lanes through a pattern"),
            None if used => return Err(clash),
            None => match props.last() {
                Some(last) => added(last.end, last.end, ", lane"),
                None => added(pattern.start, pattern.end, "{ lane }"),
            },
        },
        [Param::Object { rest: true, .. }] => return Err("the track's function takes its context with a rest"),
        [Param::Other] => return Err("the track's function takes its context in a form automate can't follow"),
        _ => return Err("the track's function takes more than its context"),
    };
    Ok(FnParams { start, end, lane })
}

/// A property of an object literal: its key, and the bytes of the whole property and of its value.
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub key: String,
    pub start: u32,
    pub end: u32,
    pub value: (u32, u32),
    /// Whether the value is a plain string literal.
    pub string: bool,
}

/// The properties of the object literal written exactly at `span` of `source`, in order; None
/// when there is none there, or it has a spread, a computed key or a method. A shorthand (`saws`)
/// is a property whose value is its key's bytes.
pub fn properties(source: &str, span: (u32, u32)) -> Option<Vec<Property>> {
    use oxc::ast::ast::{Expression, ObjectExpression, ObjectPropertyKind, PropertyKind};
    struct Find {
        span: (u32, u32),
        found: Option<Option<Vec<Property>>>,
    }
    impl<'a> Visit<'a> for Find {
        fn visit_object_expression(&mut self, it: &ObjectExpression<'a>) {
            if self.found.is_none() && (it.span.start, it.span.end) == self.span {
                let plain = |p: &ObjectPropertyKind<'a>| match p {
                    ObjectPropertyKind::ObjectProperty(p) if p.kind == PropertyKind::Init && !p.computed && !p.method => {
                        let v = p.value.span();
                        Some(Property {
                            key: p.key.static_name()?.into_cow_str()?.into_owned(),
                            start: p.span.start,
                            end: p.span.end,
                            value: (v.start, v.end),
                            string: matches!(p.value, Expression::StringLiteral(_)),
                        })
                    }
                    _ => None,
                };
                self.found = Some(it.properties.iter().map(plain).collect());
            }
            walk::walk_object_expression(self, it);
        }
    }
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut find = Find { span, found: None };
    find.visit_program(&ret.program);
    find.found.flatten()
}

/// A function's parameter, as far as automate reads it.
enum Param {
    /// `ctx`, with the span of its name.
    Name(String, Span),
    /// `{ a, b: c }`, with no defaults or nesting in it.
    Object {
        props: Vec<Prop>,
        rest: bool,
        span: Span,
    },
    Other,
}

/// A property of an object pattern: its key, the name it binds (None for a nested pattern or a
/// default), and where it ends.
struct Prop {
    key: String,
    value: Option<(String, Span)>,
    end: u32,
}

/// A function found: its span, its parameter list's, its parameters, and whether it has a rest.
struct Found {
    span: Span,
    list: Span,
    params: Vec<Param>,
    rest: bool,
}

/// The outermost function written inside `within`, every name the file binds, and every use of
/// `lane` in it.
struct FindFn {
    within: (u32, u32),
    found: Option<Found>,
    bindings: Vec<(String, Span)>,
    lanes: Vec<Span>,
}

impl FindFn {
    fn take(&mut self, span: Span, params: &FormalParameters) {
        if self.found.is_some() || span.start < self.within.0 || span.end > self.within.1 {
            return;
        }
        let param = |p: &oxc::ast::ast::FormalParameter| match (&p.pattern, &p.initializer) {
            (_, Some(_)) => Param::Other,
            (BindingPattern::BindingIdentifier(id), None) => Param::Name(id.name.to_string(), id.span),
            (BindingPattern::ObjectPattern(o), None) => Param::Object {
                props: o
                    .properties
                    .iter()
                    .map(|b| Prop {
                        key: b.key.static_name().and_then(|k| k.into_cow_str()).map(|k| k.into_owned()).unwrap_or_default(),
                        value: match (&b.value, b.computed) {
                            (BindingPattern::BindingIdentifier(id), false) => Some((id.name.to_string(), id.span)),
                            _ => None,
                        },
                        end: b.span.end,
                    })
                    .collect(),
                rest: o.rest.is_some(),
                span: o.span,
            },
            _ => Param::Other,
        };
        self.found = Some(Found { span, list: params.span, params: params.items.iter().map(param).collect(), rest: params.rest.is_some() });
    }
}

impl<'a> Visit<'a> for FindFn {
    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.take(it.span, &it.params);
        walk::walk_arrow_function_expression(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        self.take(it.span, &it.params);
        walk::walk_function(self, it, flags);
    }

    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if it.name == "lane" {
            self.lanes.push(it.span);
        }
    }

    fn visit_binding_identifier(&mut self, it: &BindingIdentifier<'a>) {
        self.bindings.push((it.name.to_string(), it.span));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // HOME-2: automate reaches `lane` from inside a module's function in each form it is written,
    // adding it to the parameters only where that is certain.
    #[test]
    fn a_functions_lane_is_found_or_added_to_its_parameters() {
        // The function is the call's last argument, after `track("k", `.
        let at = |src: &str| fn_params(src, (11, src.len() as u32 - 1));
        let apply = |src: &str, p: &FnParams| match &p.lane {
            Lane::Added { start, end, text } => format!("{}{text}{}", &src[..*start as usize], &src[*end as usize..]),
            Lane::Named(_) => src.to_string(),
        };
        let added = |src: &str| apply(src, &at(src).unwrap());
        let named = |src: &str| match at(src).unwrap().lane {
            Lane::Named(n) => n,
            other => panic!("{other:?}"),
        };
        assert_eq!(added("track(\"k\", () => osc.sine(440))"), "track(\"k\", ({ lane }) => osc.sine(440))");
        assert_eq!(added("track(\"k\", ({ time }) => osc.sine(440))"), "track(\"k\", ({ time, lane }) => osc.sine(440))");
        assert_eq!(added("track(\"k\", ({ time, }) => osc.sine(440))"), "track(\"k\", ({ time, lane, }) => osc.sine(440))");
        assert_eq!(added("track(\"k\", ({}) => osc.sine(440))"), "track(\"k\", ({ lane }) => osc.sine(440))");
        assert_eq!(added("track(\"k\", ({ time }: Ctx) => osc.sine(440))"), "track(\"k\", ({ time, lane }: Ctx) => osc.sine(440))");
        assert_eq!(named("track(\"k\", ({ lane, time }) => osc.sine(440))"), "lane");
        assert_eq!(named("track(\"k\", ({ lane }) => hat(lane.notes(\"hits\")))"), "lane", "its own uses are its");
        assert_eq!(named("track(\"k\", ({ lane: l }) => osc.sine(440))"), "l");
        assert_eq!(named("track(\"k\", (ctx) => osc.sine(440))"), "ctx.lane");
        assert_eq!(named("track(\"k\", (ctx: Ctx) => osc.sine(440))"), "ctx.lane");
        // A `function` reads the same.
        assert_eq!(
            added("track(\"k\", function ({ time }) { return osc.sine(440); })"),
            "track(\"k\", function ({ time, lane }) { return osc.sine(440); })"
        );
        // What can't be followed with certainty is refused.
        for src in [
            "track(\"k\", ({ lane = x }) => osc.sine(440))",
            "track(\"k\", ({ lane: { control } }) => osc.sine(440))",
            "track(\"k\", ({ ...ctx }) => osc.sine(440))",
            "track(\"k\", (ctx = {}) => osc.sine(440))",
            "track(\"k\", (a, b) => osc.sine(440))",
            "track(\"k\", () => { const lane = 1; return osc.sine(440); })",
            "track(\"k\", ({ time }) => [1].map((lane) => osc.sine(lane)))",
            "track(\"k\", ({ lane }) => [1].map((lane) => osc.sine(lane)))",
            "track(\"k\", (ctx) => [1].map((ctx) => osc.sine(ctx)))",
        ] {
            assert!(at(src).is_err(), "{src}: {:?}", at(src));
        }
        assert!(fn_params("track(\"k\", build)", (11, 16)).is_err(), "not a function written in the call");
    }

    #[test]
    fn imports_are_joined_or_added_after_the_others() {
        let insert = |src: &str, module, name| {
            import_insertion(src, module, name).map(|(at, text)| format!("{}{text}{}", &src[..at as usize], &src[at as usize..]))
        };
        let src = "import { hz } from \"@m2/core\";\nimport { drive } from \"std/fx\";\nexport default 1;\n";
        assert_eq!(insert(src, "std/fx", "drive"), None);
        assert_eq!(insert(src, "std/fx", "delay").unwrap(), src.replace("{ drive }", "{ drive, delay }"));
        assert_eq!(
            insert(src, "std/dsp", "osc").unwrap(),
            src.replace("from \"std/fx\";", "from \"std/fx\";\nimport { osc } from \"std/dsp\";")
        );
        // An alias or a type-only import doesn't bring the name in.
        let src = "import { delay as echo } from \"std/fx\";\nimport type { Audio } from \"@m2/core\";\n";
        assert_eq!(insert(src, "std/fx", "delay").unwrap(), src.replace("echo }", "echo, delay }"));
        assert_eq!(insert("export default 1;\n", "std/fx", "delay").unwrap(), "import { delay } from \"std/fx\";\nexport default 1;\n");
    }
}
