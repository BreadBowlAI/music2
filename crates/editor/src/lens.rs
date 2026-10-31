//! The editable source subset. A literal lens can read a numeric token and propose
//! its replacement; an insertion can write a declared control that a call leaves out. Neither
//! can choose a graph target or approve its own result: revision resolution and independent
//! graph verification belong to the session.

use m2_ir::{Origin, Unit};
use m2_script::component::EachDesc;
use m2_script::sites::{Arg, Site};
use m2_script::{Descriptor, UnitCtor};

use super::EditError;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LiteralLens {
    pub start: u32,
    pub end: u32,
    pub literal: String,
    pub scale: f64,
}

pub(crate) struct LiteralEdit {
    pub text: String,
    /// What the written token evaluates to, in canonical units. Both preview and
    /// commit use this value, including rounding through the source unit conversion.
    pub value: f64,
}

/// How `value` is written as a number that is `scale` times what it says, and what that reads
/// back as: the one rounding every preview and commit share.
pub(crate) fn propose(value: f64, scale: f64) -> Result<LiteralEdit, EditError> {
    let written = value / scale;
    if !value.is_finite() || !written.is_finite() {
        return Err(EditError::Rejected("the control needs a finite number".into()));
    }
    let text = if written == 0.0 { "0".into() } else { written.to_string() };
    let value = text.parse::<f64>().expect("formatted number") * scale;
    if !value.is_finite() || !(value as f32).is_finite() {
        return Err(EditError::Rejected("the value is outside the audio engine's numeric range".into()));
    }
    Ok(LiteralEdit { text, value })
}

impl LiteralLens {
    pub fn read(site: &Site, origin: &Origin, source: &str, value: f64) -> Option<Self> {
        let Arg::Number { start, end, value: written } = site.arg(&origin.arg)? else { return None };
        let scale = origin.scale.unwrap_or(1.0);
        if !scale.is_finite() || scale <= 0.0 || !value.is_finite() || written * scale != value {
            return None;
        }
        Some(Self { start: *start, end: *end, literal: source.get(*start as usize..*end as usize)?.into(), scale })
    }

    pub fn propose(&self, value: f64) -> Result<LiteralEdit, EditError> {
        propose(value, self.scale)
    }

    /// A choice's name or a switch's `true` or `false` written at `origin` (CARD-10, CARD-11): its
    /// literal, when it says `value`, the place (or 1 or 0) the build made of it. A name is a
    /// string written without escapes.
    pub fn named(site: &Site, origin: &Origin, source: &str, unit: Unit, value: f64) -> Option<Self> {
        let (start, end) = site.arg(&origin.arg)?.span();
        let literal = source.get(start as usize..end as usize)?;
        let says = match unit {
            Unit::Switch => f64::from(u8::from(match literal {
                "true" => true,
                "false" => false,
                _ => return None,
            })),
            Unit::Choice(o) => {
                let quote =
                    literal.chars().next().filter(|q| matches!(q, '"' | '\'' | '`') && literal.len() >= 2 && literal.ends_with(*q))?;
                let name = &literal[quote.len_utf8()..literal.len() - quote.len_utf8()];
                if name.contains(['\\', quote]) || (quote == '`' && name.contains("${")) {
                    return None;
                }
                o.place(name)? as f64
            }
            _ => return None,
        };
        (says == value).then(|| Self { start, end, literal: literal.into(), scale: 1.0 })
    }
}

/// Where a declared control that a call leaves out (inherited, PKG-3) is written when it is first
/// edited (the step 5 design's D5): positionally when that is the next argument, else in the
/// call's options object, else in a new one. A dictionary entry's control (`channels.kick.db`) is
/// written into its entry: an object gets a property, and a bare signal is wrapped as
/// `{ in: <signal>, db: … }` (design-8b §5.2). The value is a plain number in base units, or, in
/// a dictionary whose other entries write the control with a unit call (`db(-6)`), that call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Insertion {
    /// Bytes `start..end` of the file are replaced: an empty range after the last argument (or
    /// inside empty parentheses, or just inside an options object with no properties, or after a
    /// dictionary entry or its last property), or the blank inside of `{}` or `{ }`.
    pub start: u32,
    pub end: u32,
    /// What goes around the number: `, ` and ``, `, q: ` and ``, `, { q: ` and ` }`, ` q: ` and
    /// ` ` (or `,` before a comment or a line break that stays), `, db: ` and ` }` for a wrap.
    /// A new options object in a call written over lines goes on a line of its own:
    /// `\n    { q: ` and ` },` after the last argument's comma, or `,\n    { q: ` and ` }`.
    pub prefix: String,
    pub suffix: String,
    /// A bare dictionary entry is wrapped: this text goes in where the entry's signal starts
    /// (`{ in: `, or `saws: { in: ` for the shorthand `saws`), which the suffix closes. The
    /// signal's own text stays as it is, so every call in it keeps its place (ID-13).
    pub open: Option<(u32, String)>,
    /// The dictionary entry it goes into, if it does: what history shows.
    pub entry: Option<Entry>,
    /// The unit call a number is written in (`db` for `db(-3)`), to follow the dictionary's style.
    pub spelling: Option<String>,
    /// The argument path the value is written at: its origin's `arg` once written.
    pub path: Vec<String>,
}

/// A dictionary entry as written: its key, its value's bytes, and whether it is a shorthand
/// (`saws` for `saws: saws`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub key: String,
    pub value: (u32, u32),
    pub shorthand: bool,
}

impl Insertion {
    /// Plan writing control `slot` of `desc` into `site`, the call of one of its instances, whose
    /// argument `options` held its options when it ran (`Frame.options`): only the run can tell
    /// `lowpass(x, opts)`'s options from a cutoff. Why not, when the call holds an argument at
    /// that place already (one that was `undefined`), its options aren't a plain object written
    /// in the call, it spreads its arguments, or, for a dictionary entry's, the entry isn't written
    /// in the call as a plain property. `units` are the build's unit constructors (PKG-11).
    pub fn plan(
        site: &Site,
        desc: &Descriptor,
        slot: &str,
        source: &str,
        options: Option<usize>,
        units: &[UnitCtor],
    ) -> Result<Insertion, &'static str> {
        let text = |a: &Arg| {
            let (s, e) = a.span();
            source.get(s as usize..e as usize).unwrap_or("")
        };
        let args = &site.args;
        if args.iter().any(|a| text(a).trim_start().starts_with("...")) {
            return Err("the call spreads its arguments, so where it would go isn't known");
        }
        // The call's parameters in order: the inputs `positional` doesn't name, then `positional`.
        let mut params: Vec<&str> = desc.inputs.iter().filter(|i| !desc.positional.contains(&i.name)).map(|i| i.name.as_str()).collect();
        params.extend(desc.positional.iter().map(String::as_str));
        let each = |name: &str| desc.controls.iter().any(|c| c.name == name && c.each.is_some());
        // The options always come last, so the arguments before them are positional.
        let n = args.len();
        if options.is_some_and(|i| i + 1 != n) {
            return Err("its arguments aren't where its run read them");
        }
        let given = options.unwrap_or(n);
        if let Some((dict, rest)) = slot.split_once('.') {
            // The dictionary: at its place among the arguments, else in the options object.
            let (written, mut path) = match params.iter().position(|x| *x == dict) {
                Some(p) if p < given => (&args[p], vec![p.to_string()]),
                _ => match options.map(|i| (i, &args[i])) {
                    Some((i, Arg::Object { props, .. })) => match props.iter().find(|(k, _)| k == dict) {
                        Some((_, a)) => (a, vec![i.to_string(), dict.to_string()]),
                        None => return Err("its dictionary isn't written in the call"),
                    },
                    _ => return Err("its dictionary isn't written in the call"),
                },
            };
            // `kick.db`, or a nested dictionary's entry: `vox.sends.verb.db` (PKG-6).
            let parts: Vec<&str> = rest.split('.').collect();
            if !parts.len().is_multiple_of(2) {
                return Err("it isn't a control of a dictionary entry");
            }
            let mut each =
                desc.controls.iter().find(|c| c.name == dict).and_then(|c| c.each.as_deref()).ok_or("its dictionary isn't declared")?;
            let mut written = written;
            let mut at = 0;
            while parts.len() - at > 2 {
                let (key, sub) = (parts[at], parts[at + 1]);
                let Arg::Object { props, .. } = written else { return Err("its dictionary isn't an object written in the call") };
                let (_, value) = props.iter().find(|(k, _)| k == key).ok_or("its entry isn't written in the call")?;
                let Arg::Object { props: fields, .. } = value else { return Err("its entry doesn't write its own dictionary") };
                written =
                    fields.iter().find(|(k, _)| k == sub).map(|(_, a)| a).ok_or("its entry's dictionary isn't written in the call")?;
                each =
                    each.controls.iter().find(|c| c.name == sub).and_then(|c| c.each.as_deref()).ok_or("its dictionary isn't declared")?;
                path.extend([key.to_string(), sub.to_string()]);
                at += 2;
            }
            let (key, control) = (parts[at], parts[at + 1]);
            path.extend([key.to_string(), control.to_string()]);
            return entry(each, key, control, written, source, units, path);
        }
        let after = |a: &Arg| a.span().1;
        if let Some(p) = params.iter().position(|x| *x == slot) {
            if p < given {
                return Err("an argument is written at its place in the call already");
            }
            if p == given && options.is_none() {
                return match args.last() {
                    Some(last) => Ok(Insertion {
                        start: after(last),
                        end: after(last),
                        prefix: ", ".into(),
                        suffix: String::new(),
                        path: vec![p.to_string()],
                        ..Insertion::bare()
                    }),
                    None => {
                        let close = site.end.checked_sub(1).filter(|&c| source.as_bytes().get(c as usize) == Some(&b')'));
                        let close = close.ok_or("the call's parentheses aren't where it ends")?;
                        Ok(Insertion { start: close, end: close, path: vec![p.to_string()], ..Insertion::bare() })
                    }
                };
            }
        }
        match options {
            Some(i) => match &args[i] {
                Arg::Object { start, end, props } if props.is_empty() => {
                    let path = vec![i.to_string(), slot.into()];
                    let inside = source.get(*start as usize..*end as usize).and_then(|t| t.strip_prefix('{')?.strip_suffix('}'));
                    let inside = inside.ok_or("the call's options aren't an object written in the call")?;
                    // Inside the braces: only blanks on one line are replaced, and anything else in
                    // them (a comment, a line break) stays after the new property.
                    Ok(if inside.trim().is_empty() && !inside.contains('\n') {
                        Insertion {
                            start: start + 1,
                            end: end - 1,
                            prefix: format!(" {slot}: "),
                            suffix: " ".into(),
                            path,
                            ..Insertion::bare()
                        }
                    } else {
                        Insertion {
                            start: start + 1,
                            end: start + 1,
                            prefix: format!(" {slot}: "),
                            suffix: ",".into(),
                            path,
                            ..Insertion::bare()
                        }
                    })
                }
                Arg::Object { props, .. } => {
                    if props.iter().any(|(k, _)| k == slot) {
                        return Err("its options object names it already");
                    }
                    let last = props.last().expect("not empty").1.span().1;
                    Ok(Insertion {
                        start: last,
                        end: last,
                        prefix: format!(", {slot}: "),
                        suffix: String::new(),
                        path: vec![i.to_string(), slot.into()],
                        ..Insertion::bare()
                    })
                }
                _ => Err("the call's options aren't an object written in the call"),
            },
            None => {
                // A plain object there must be read as the options, not as a dictionary.
                if params.get(given).is_some_and(|p| each(p)) {
                    return Err("an object written there would be read as its dictionary");
                }
                let path = vec![given.to_string(), slot.into()];
                // A call written over lines gets its new options on a line of their own, at its
                // arguments' indent, with a comma after them when its last argument has one.
                let first = args.first().map(|a| a.span().0 as usize);
                let lines =
                    first.is_some_and(|f| site.open >= site.start && source.get(site.open as usize..f).is_some_and(|t| t.contains('\n')));
                match args.last() {
                    Some(last) if lines => {
                        let at = after(last);
                        let indent = indent_at(source, last.span().0);
                        let rest = source.get(at as usize..).unwrap_or("");
                        let comma = rest.find(|c: char| !c.is_whitespace()).filter(|&i| rest[i..].starts_with(','));
                        let (at, prefix, suffix) = match comma {
                            Some(i) => (at + i as u32 + 1, format!("\n{indent}{{ {slot}: "), " },"),
                            None => (at, format!(",\n{indent}{{ {slot}: "), " }"),
                        };
                        Ok(Insertion { start: at, end: at, prefix, suffix: suffix.into(), path, ..Insertion::bare() })
                    }
                    Some(last) => Ok(Insertion {
                        start: after(last),
                        end: after(last),
                        prefix: format!(", {{ {slot}: "),
                        suffix: " }".into(),
                        path,
                        ..Insertion::bare()
                    }),
                    None => {
                        let close = site.end.checked_sub(1).filter(|&c| source.as_bytes().get(c as usize) == Some(&b')'));
                        let close = close.ok_or("the call's parentheses aren't where it ends")?;
                        Ok(Insertion {
                            start: close,
                            end: close,
                            prefix: format!("{{ {slot}: "),
                            suffix: " }".into(),
                            path,
                            ..Insertion::bare()
                        })
                    }
                }
            }
        }
    }

    /// An insertion with nothing in it yet, to fill in.
    fn bare() -> Insertion {
        Insertion { start: 0, end: 0, prefix: String::new(), suffix: String::new(), open: None, entry: None, spelling: None, path: vec![] }
    }

    /// How a number in base units (as [`propose`] writes it) is written here: in the unit call the
    /// dictionary's other entries use (`db(-3)`), else as it is.
    pub fn number(&self, number: &str) -> String {
        match &self.spelling {
            Some(call) => format!("{call}({number})"),
            None => number.to_string(),
        }
    }

    /// The edits that write `written` (a number as [`Insertion::number`] spells it, or a signal's
    /// name), in the file's order: each `(start, end, text)`.
    pub fn edits(&self, written: &str) -> Vec<(u32, u32, String)> {
        let open = self.open.iter().map(|(at, text)| (*at, *at, text.clone()));
        open.chain([(self.start, self.end, format!("{}{written}{}", self.prefix, self.suffix))]).collect()
    }
}

/// The edits writing several declared controls a call leaves out into it at once, each
/// `(insertion, written)` as [`Insertion::edits`] takes it, in the file's order: where several
/// share a place (a preset's load, FMT-22), one insertion writes them all, so two never collide.
/// A positional one goes first, and the others join the first one's options as more properties.
/// None when they can't be joined: a dictionary entry's, or options planned inside empty
/// parentheses after a positional one.
pub(crate) fn together(items: &[(Insertion, String)]) -> Option<Vec<(u32, u32, String)>> {
    let mut places: std::collections::BTreeMap<(u32, u32), Vec<&(Insertion, String)>> = Default::default();
    for item in items {
        places.entry((item.0.start, item.0.end)).or_default().push(item);
    }
    let mut out = vec![];
    for ((start, end), group) in places {
        if let [(one, written)] = group[..] {
            out.extend(one.edits(written));
            continue;
        }
        if group.iter().any(|(i, _)| i.open.is_some() || i.entry.is_some()) {
            return None;
        }
        let positional = |i: &Insertion| i.path.len() == 1;
        let mut text: String = group.iter().filter(|(i, _)| positional(i)).map(|(i, w)| format!("{}{w}{}", i.prefix, i.suffix)).collect();
        let mut rest = group.iter().filter(|(i, _)| !positional(i));
        if let Some((first, written)) = rest.next() {
            if !text.is_empty() && !first.prefix.starts_with([',', '\n']) {
                return None;
            }
            text += &first.prefix;
            text += written;
            for (i, w) in rest {
                text += &format!(", {}: {w}", i.path.last()?);
            }
            text += &first.suffix;
        }
        out.push((start, end, text));
    }
    Some(out)
}

/// The blanks that start the line byte `at` of `source` is on: its indent.
fn indent_at(source: &str, at: u32) -> &str {
    let line = source.get(..at as usize).and_then(|t| t.rfind('\n')).map_or(0, |i| i + 1);
    let text = source.get(line..).unwrap_or("");
    &text[..text.len() - text.trim_start_matches([' ', '\t']).len()]
}

/// Plan writing control `control` into entry `key` of a dictionary whose entries are `each`,
/// written in the call as `written` at argument path `path` (design-8b §5.2): into an object entry
/// as one more property, or around a bare signal as `{ in: <signal>, <control>: … }`. Why not, when
/// the dictionary isn't an object written in the call, the entry isn't in it, or names the control.
fn entry(
    each: &EachDesc,
    key: &str,
    control: &str,
    written: &Arg,
    source: &str,
    units: &[UnitCtor],
    path: Vec<String>,
) -> Result<Insertion, &'static str> {
    let declared = each.controls.iter().find(|c| c.name == control).ok_or("its entry declares no such control")?;
    let Arg::Object { props, .. } = written else { return Err("its dictionary isn't an object written in the call") };
    let (_, value) = props.iter().find(|(k, _)| k == key).ok_or("its entry isn't written in the call")?;
    let (start, end) = value.span();
    let text = source.get(start as usize..end as usize).unwrap_or("");
    let shorthand = matches!(value, Arg::Other { .. }) && text == key;
    let spelling = style(props, key, control, declared.unit, source, units);
    let entry = Some(Entry { key: key.to_string(), value: (start, end), shorthand });
    match value {
        Arg::Object { props: fields, .. } => {
            if fields.iter().any(|(k, _)| k == control) {
                return Err("its entry names it already");
            }
            let (_, last) = fields.last().ok_or("its entry is an empty object")?;
            let at = last.span().1;
            Ok(Insertion { start: at, end: at, prefix: format!(", {control}: "), suffix: String::new(), open: None, entry, spelling, path })
        }
        _ => {
            // A bare entry is its one input's signal (`kick: kick.out` is `{ in: kick.out }`).
            let [input] = each.inputs.as_slice() else { return Err("its entry is a bare signal, which this dictionary doesn't take") };
            let open = match shorthand {
                true => format!("{key}: {{ {}: ", input.name),
                false => format!("{{ {}: ", input.name),
            };
            Ok(Insertion {
                start: end,
                end,
                prefix: format!(", {control}: "),
                suffix: " }".into(),
                open: Some((start, open)),
                entry,
                spelling,
                path,
            })
        }
    }
}

/// The unit call another entry of the dictionary writes `control` with (`db` in
/// `hats: { in: hats.out, db: db(-6) }`), when it is a spelling in base units of the control's
/// unit and the file imports it from `@m2/core` under its own name: so an insertion never mixes
/// two spellings in one object (design-8b §5.2).
fn style(props: &[(String, Arg)], key: &str, control: &str, unit: Option<m2_ir::Unit>, source: &str, units: &[UnitCtor]) -> Option<String> {
    let unit = unit?;
    let called = props.iter().filter(|(k, _)| k != key).find_map(|(_, v)| {
        let Arg::Object { props: fields, .. } = v else { return None };
        let (start, end) = fields.iter().find(|(k, _)| k == control)?.1.span();
        let (name, _) = source.get(start as usize..end as usize)?.split_once('(')?;
        units.iter().find(|u| u.name == name.trim() && u.unit == unit && u.scale == 1.0)
    })?;
    crate::sites::imported_names(source, "@m2/core").contains(&called.name).then(|| called.name.clone())
}
