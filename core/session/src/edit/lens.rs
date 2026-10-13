//! The editable source subset. A literal lens can read a numeric token and propose
//! its replacement; an insertion can write a declared control that a call leaves out. Neither
//! can choose a graph target or approve its own result: revision resolution and independent
//! graph verification belong to the session.

use m2_ir::Origin;
use m2_script::Descriptor;
use m2_script::sites::{Arg, Site};

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
}

/// Where a declared control that a call leaves out (inherited, PKG-3) is written when it is first
/// edited (the step 5 design's D5): positionally when that is the next argument, else in the
/// call's options object, else in a new one. The value is a plain number in base units, so the
/// insertion adds no call and renumbers no site.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Insertion {
    /// Bytes `start..end` of the file are replaced: an empty range after the last argument (or
    /// inside empty parentheses, or just inside an options object with no properties), or the
    /// blank inside of `{}` or `{ }`.
    pub start: u32,
    pub end: u32,
    /// What goes around the number: `, ` and ``, `, q: ` and ``, `, { q: ` and ` }`, ` q: ` and
    /// ` ` (or `,` before a comment or a line break that stays).
    pub prefix: String,
    pub suffix: String,
    /// The argument path the value is written at: its origin's `arg` once written.
    pub path: Vec<String>,
}

impl Insertion {
    /// Plan writing control `slot` of `desc` into `site`, the call of one of its instances, whose
    /// argument `options` held its options when it ran (`Frame.options`): only the run can tell
    /// `lowpass(x, opts)`'s options from a cutoff. Why not, when the call holds an argument at
    /// that place already (one that was `undefined`), its options aren't a plain object written
    /// in the call, it spreads its arguments, or the slot is a dictionary entry's.
    pub fn plan(site: &Site, desc: &Descriptor, slot: &str, source: &str, options: Option<usize>) -> Result<Insertion, &'static str> {
        if slot.contains('.') {
            return Err("it is a dictionary entry's, which is written with its entry");
        }
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
        let after = |a: &Arg| a.span().1;
        if let Some(p) = params.iter().position(|x| *x == slot) {
            if p < given {
                return Err("an argument is written at its place in the call already");
            }
            if p == given && options.is_none() {
                return match args.last() {
                    Some(last) => Ok(Insertion { start: after(last), end: after(last), prefix: ", ".into(), suffix: String::new(), path: vec![p.to_string()] }),
                    None => {
                        let close = site.end.checked_sub(1).filter(|&c| source.as_bytes().get(c as usize) == Some(&b')'));
                        let close = close.ok_or("the call's parentheses aren't where it ends")?;
                        Ok(Insertion { start: close, end: close, prefix: String::new(), suffix: String::new(), path: vec![p.to_string()] })
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
                        Insertion { start: start + 1, end: end - 1, prefix: format!(" {slot}: "), suffix: " ".into(), path }
                    } else {
                        Insertion { start: start + 1, end: start + 1, prefix: format!(" {slot}: "), suffix: ",".into(), path }
                    })
                }
                Arg::Object { props, .. } => {
                    if props.iter().any(|(k, _)| k == slot) {
                        return Err("its options object names it already");
                    }
                    let last = props.last().expect("not empty").1.span().1;
                    Ok(Insertion { start: last, end: last, prefix: format!(", {slot}: "), suffix: String::new(), path: vec![i.to_string(), slot.into()] })
                }
                _ => Err("the call's options aren't an object written in the call"),
            },
            None => {
                // A plain object there must be read as the options, not as a dictionary.
                if params.get(given).is_some_and(|p| each(p)) {
                    return Err("an object written there would be read as its dictionary");
                }
                let path = vec![given.to_string(), slot.into()];
                match args.last() {
                    Some(last) => {
                        Ok(Insertion { start: after(last), end: after(last), prefix: format!(", {{ {slot}: "), suffix: " }".into(), path })
                    }
                    None => {
                        let close = site.end.checked_sub(1).filter(|&c| source.as_bytes().get(c as usize) == Some(&b')'));
                        let close = close.ok_or("the call's parentheses aren't where it ends")?;
                        Ok(Insertion { start: close, end: close, prefix: format!("{{ {slot}: "), suffix: " }".into(), path })
                    }
                }
            }
        }
    }

    /// The text that replaces `start..end`, with the number written as `number`.
    pub fn text(&self, number: &str) -> String {
        format!("{}{number}{}", self.prefix, self.suffix)
    }
}
