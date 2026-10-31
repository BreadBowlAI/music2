//! A command's words, read against its usage in the one table (AGENT-13). Each form of a
//! command's `usage` in `commands.rs` is its grammar, so what `m2 help` shows is what is read, and
//! nothing says it twice. `main` reads the words after the command before anything is read or
//! written, and refuses a word no form takes, naming it, with the forms meant.
//!
//! | In a form | Means |
//! |---|---|
//! | `<project>`, `@owner/name`, `clap:<id>` | a place word: any word |
//! | `new`, `on\|off` | that word, or one of them |
//! | `<clip>…` | one place word or more |
//! | `<tx.json \| ->` | a place word, or `-`: standard input |
//! | `--at <place>`, `--json` | a flag, with its value or none, given once at most |
//! | `--old <text>` | a flag whose value is the next word, whatever it is |
//! | `--with <tx.json \| ->` | a flag whose value can be `-`: standard input |
//! | `[…]` | can be left out; of `[--from S \| --from-bar N]`, one at most |
//!
//! A form writes its place words first, then its flags. A word that starts with `-` is a flag,
//! unless it is `-` alone, reads as a number (`-6`, `-.5`) or has a space in it (text). A flag's
//! value is the next word, unless that word is a flag: then it has none; a `<text>` value is the
//! next word whatever it is. Flags can go anywhere after the command: it is handed its place words
//! first, in order, then its flags as they were given; it reads them with [`value`], [`on`] and
//! [`places`], which read them the same way, and which, in a debug build, stop the command that
//! asks for a flag its form doesn't show: a mistake in the table.

use crate::commands::{self, Command};

/// What reading a command's words found.
pub enum Read {
    /// Run it with these words: the command, its place words in order, then its flags as given.
    Run(Vec<String>),
    /// `--help` (or `-h`) where a flag goes: print its usage.
    Help(&'static Command),
    /// Refused: why, naming the word, then how the command is called.
    Refused(String),
}

/// The flags that ask a command for its usage.
const HELP: [&str; 2] = ["--help", "-h"];

/// Read `args`, a command m2 has (`args[0]`, by its name or another word for it) and the words
/// after it, against the command's usage.
pub fn read(args: &[String]) -> Read {
    match Grammar::of_command(&args[0]).take(&args[0], &args[1..]) {
        Taken::Run { run, .. } => Read::Run(run),
        Taken::Help(c) => Read::Help(c),
        Taken::Refused(why) => Read::Refused(why),
    }
}

/// Whether `word` is a flag: it starts with `-`, and isn't `-` alone, a number (`-6`, `-.5`) or
/// text with a space in it (`- check the kick`).
pub fn is_flag(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-') && !word.contains(char::is_whitespace) && word.parse::<f64>().is_err()
}

/// The value of flag `name` in `args`, words [`read`] handed on; None when it wasn't given or
/// has none (a gone flag). In a debug build, a flag the form that took `args` doesn't show stops
/// m2: the code reads a flag its usage doesn't say, which the usage would refuse.
pub fn value(args: &[String], name: &str) -> Option<String> {
    flag_given(args, name).flatten()
}

/// Whether flag `name` was given in `args`, as [`value`] reads it.
pub fn on(args: &[String], name: &str) -> bool {
    flag_given(args, name).is_some()
}

/// The place words in `args`, words [`read`] handed on: those after the command, in order.
pub fn places(args: &[String]) -> Vec<String> {
    let taken = Grammar::of_command(&args[0]).take(&args[0], &args[1..]);
    let Taken::Run { given, .. } = taken else { panic!("m2 handed on words its usage doesn't take: {args:?}") };
    given.places.iter().map(|p| p.to_string()).collect()
}

/// Whether flag `name` of command `cmd` takes the next word whatever it is (`--old <text>`):
/// `--as` or `--why` there is its text, not m2's own (AGENT-10).
pub fn takes_text(cmd: &str, name: &str) -> bool {
    commands::find(cmd).is_some_and(|_| Grammar::of_command(cmd).flag(name).is_some_and(|f| f.text))
}

/// Flag `name` as given in `args`: Some(its value) when it was.
fn flag_given(args: &[String], name: &str) -> Option<Option<String>> {
    let g = Grammar::of_command(&args[0]);
    let Taken::Run { form, given, .. } = g.take(&args[0], &args[1..]) else {
        panic!("m2 handed on words its usage doesn't take: {args:?}")
    };
    let form = &g.forms[form];
    debug_assert!(
        form.flag(name).is_some(),
        "m2 {} reads {name}, which `{}` doesn't show: add it to that form in apps/m2/src/commands.rs (AGENT-13)",
        args[0],
        form.line
    );
    given.flags.iter().find(|(f, _)| *f == name).map(|(_, v)| v.map(String::from))
}

/// A place word of a form.
struct Place {
    /// As the form writes it: `<project>`, `on|off`.
    text: &'static str,
    /// The words it is one of (`on|off`), or none: any word.
    literal: Vec<&'static str>,
    /// Can be left out: `[<project>]`.
    optional: bool,
    /// Can be given again: `<clip>…`.
    many: bool,
    /// Can be `-`, standard input: `<tx.json | ->`.
    stdin: bool,
}

impl Place {
    /// As `m2` says it: `<out.wav>`; one of a few words, `on or off`.
    fn said(&self) -> String {
        match self.literal.is_empty() {
            true => plain(self.text),
            false => or(&self.literal),
        }
    }
}

/// A flag of a form.
struct Flag {
    name: &'static str,
    /// Its value as the form writes it (`S`, `<place>`), or None: it takes none.
    value: Option<&'static str>,
    /// Its value is the next word whatever it is: `<text>`.
    text: bool,
    /// Its value can be `-`, standard input: `<tx.json | ->`.
    stdin: bool,
    /// Gone ([`commands::GONE`]): known only so that the command says why. Its value, when it
    /// had one, is the next word if that isn't a flag, and it may be given more than once.
    gone: bool,
    /// Written outside `[…]`: it must be given.
    required: bool,
    /// Of the flags written in one `[…]`, one at most is given.
    group: usize,
}

impl Flag {
    /// The flag `name`, with `value` as the form writes it.
    fn new(name: &'static str, value: Option<&'static str>, required: bool, group: usize) -> Flag {
        let inner = value.and_then(|v| v.strip_prefix('<')).and_then(|v| v.strip_suffix('>'));
        let stdin = inner.is_some_and(|v| split(v, '|').contains(&"-"));
        Flag { name, value, text: value == Some("<text>"), stdin, gone: false, required, group }
    }

    /// What its value can be: none, any word, any word but a flag, or that or `-`.
    fn kind(&self) -> (bool, bool, bool) {
        (self.value.is_some(), self.text, self.stdin)
    }
}

/// One form of a command's usage, read as a grammar.
struct Form {
    /// The form as the usage shows it: one line.
    line: &'static str,
    places: Vec<Place>,
    flags: Vec<Flag>,
}

/// The words after a command, read once: its place words, each flag with its value, and the
/// first problem a value has, which no form mends.
#[derive(Default)]
struct Given<'a> {
    places: Vec<&'a str>,
    flags: Vec<(&'a str, Option<&'a str>)>,
    wrong: Option<String>,
}

/// What reading a command's words found: the form that takes them (its index), as given and as
/// handed on; its usage asked for; or why not.
enum Taken<'a> {
    Run { form: usize, given: Given<'a>, run: Vec<String> },
    Help(&'static Command),
    Refused(String),
}

/// Why a form doesn't take the words, and how far reading them got: a flag it hasn't got (0), a
/// flag given twice or with its other (1), the place words, or a flag it must have (2).
struct Miss<'a> {
    stage: u8,
    why: String,
    /// The flag it hasn't got (stage 0), or the first word too many.
    word: Option<&'a str>,
}

/// Whether a form's words (`new`, `order`) are where it has them, before any word it can have
/// many of or none: the form meant; the words stop before one; one isn't (where).
#[derive(PartialEq)]
enum Meant {
    Yes,
    Short,
    No(usize),
}

impl Form {
    /// `line`, a form of `c`'s usage, as a grammar; or why it can't be one (a test reads them all).
    fn of(c: &Command, line: &'static str) -> Result<Form, String> {
        let rest = line
            .strip_prefix("m2 ")
            .and_then(|l| l.strip_prefix(c.name))
            .filter(|r| r.is_empty() || r.starts_with(' '))
            .ok_or_else(|| format!("`{line}` doesn't start with m2 {}", c.name))?;
        let mut form = Form { line, places: vec![], flags: vec![] };
        let tokens = split(rest, ' ');
        let mut i = 0;
        while i < tokens.len() {
            let t = tokens[i];
            let group = i + 1;
            i += 1;
            if let Some(inner) = t.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
                let alternatives = split(inner, '|');
                // `--from S`: a flag, then its value.
                let flag = |a: &&str| split(a, ' ').first().is_some_and(|w| is_flag(w));
                if alternatives.iter().all(flag) {
                    for a in alternatives {
                        let [name, value @ ..] = &split(a, ' ')[..] else { unreachable!("not empty") };
                        if value.len() > 1 {
                            return Err(format!("`{a}` in `{line}` is a flag and more than one value"));
                        }
                        form.flags.push(Flag::new(name, value.first().copied(), false, group));
                    }
                    continue;
                }
                if alternatives.iter().any(flag) {
                    return Err(format!("`{t}` in `{line}` has a flag and a place word as one or the other"));
                }
                let many = alternatives.iter().any(|a| a.contains('…'));
                form.place(Place { text: t, literal: vec![], optional: true, many, stdin: alternatives.contains(&"-") })?;
            } else if is_flag(t) {
                // Its value, when the next word is one: no place word comes after a flag.
                let value = tokens.get(i).filter(|v| !is_flag(v) && !v.starts_with('[')).copied();
                i += usize::from(value.is_some());
                form.flags.push(Flag::new(t, value, true, group));
            } else {
                let word = t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-|".contains(c));
                let literal = if word { t.split('|').collect() } else { vec![] };
                let inner = t.strip_prefix('<').and_then(|t| t.strip_suffix('>')).unwrap_or(t);
                let stdin = split(inner, '|').contains(&"-");
                form.place(Place { text: t, literal, optional: false, many: t.contains('…'), stdin })?;
            }
        }
        if form.places.iter().filter(|p| p.optional || p.many).count() > 1 {
            return Err(format!("`{line}` has more than one place word that can be left out or given again"));
        }
        for (n, f) in form.flags.iter().enumerate() {
            if form.flags[..n].iter().any(|g| g.name == f.name) {
                return Err(format!("`{line}` has {} twice", f.name));
            }
        }
        Ok(form)
    }

    /// Add a place word: before any flag.
    fn place(&mut self, p: Place) -> Result<(), String> {
        if !self.flags.is_empty() {
            return Err(format!("`{}` in `{}` comes after a flag: place words go first", p.text, self.line));
        }
        self.places.push(p);
        Ok(())
    }

    fn flag(&self, name: &str) -> Option<&Flag> {
        self.flags.iter().find(|f| f.name == name)
    }

    /// The place words up to the first that can be left out or given again: those always at the
    /// same place, from the start.
    fn head(&self) -> &[Place] {
        &self.places[..self.places.iter().position(|p| p.optional || p.many).unwrap_or(self.places.len())]
    }

    /// Whether this is the form meant by `places`, the place words given.
    fn meant(&self, places: &[&str]) -> Meant {
        for (i, p) in self.head().iter().enumerate().filter(|(_, p)| !p.literal.is_empty()) {
            match places.get(i) {
                None => return Meant::Short,
                Some(w) if !p.literal.contains(w) => return Meant::No(i),
                Some(_) => {}
            }
        }
        Meant::Yes
    }

    /// How the form is named in what `m2` says, with the command as typed (`cmd`): `m2 render`;
    /// to its last word, as given, `m2 clips <project> delete`.
    fn name(&self, cmd: &str, places: &[&str]) -> String {
        let last = self.head().iter().rposition(|p| !p.literal.is_empty()).map_or(0, |i| i + 1);
        let shown = self.head()[..last].iter().enumerate().map(|(i, p)| match p.literal.is_empty() {
            true => p.text,
            false => places.get(i).copied().unwrap_or(p.text),
        });
        std::iter::once(format!("m2 {cmd}")).chain(shown.map(String::from)).collect::<Vec<_>>().join(" ")
    }

    /// Whether this form takes `given`, `name` as [`Form::name`] says it: the words to run the
    /// command with (`cmd`, the place words, then the flags as given), or why not.
    fn fit<'a>(&self, cmd: &str, given: &Given<'a>, name: &str) -> Result<Vec<String>, Miss<'a>> {
        let miss = |stage, why: String, word| Miss { stage, why, word };
        for (n, &(w, _)) in given.flags.iter().enumerate() {
            let f = self.flag(w).ok_or_else(|| miss(0, format!("`{w}` isn't a flag of {name}"), Some(w)))?;
            let before = &given.flags[..n];
            if f.gone {
                continue;
            }
            if before.iter().any(|(g, _)| *g == w) {
                return Err(miss(1, format!("`{w}` is given twice"), None));
            }
            if let Some((other, _)) = before.iter().find(|(o, _)| self.flag(o).is_some_and(|o| o.group == f.group)) {
                return Err(miss(1, format!("`{other}` and `{w}` are one or the other: give one"), None));
            }
        }
        self.fill(&given.places, name).map_err(|(why, word)| miss(2, why, word))?;
        if let Some(f) = self.flags.iter().find(|f| f.required && !given.flags.iter().any(|(g, _)| *g == f.name)) {
            return Err(miss(2, format!("{name} needs `{}`", f.name), None));
        }
        let mut run = vec![cmd.to_string()];
        run.extend(given.places.iter().map(|p| p.to_string()));
        for (f, value) in &given.flags {
            run.push(f.to_string());
            run.extend(value.map(String::from));
        }
        Ok(run)
    }

    /// Whether the place words `words` fill this form's: as many as it takes, each one it can be;
    /// or why not, with the first word too many.
    fn fill<'a>(&self, words: &[&'a str], name: &str) -> Result<(), (String, Option<&'a str>)> {
        let var = self.places.iter().position(|p| p.optional || p.many);
        let (head, tail) = match var {
            Some(v) => (&self.places[..v], &self.places[v + 1..]),
            None => (&self.places[..], &[][..]),
        };
        // Those that must be given, filled from the start: the first left over is what is missing.
        let needed: Vec<&Place> = head.iter().chain(var.map(|v| &self.places[v]).filter(|p| !p.optional)).chain(tail).collect();
        let n = words.len();
        if let Some(p) = needed.get(n) {
            return Err((format!("{name} needs {}", p.said()), None));
        }
        let most = match var.map(|v| &self.places[v]) {
            Some(p) if p.many => usize::MAX,
            Some(_) => head.len() + 1 + tail.len(),
            None => head.len(),
        };
        if n > most {
            return Err((format!("`{}` is one word too many for {name}", words[most]), Some(words[most])));
        }
        // The head from the start, the tail from the end, and what is between in the one between.
        let between = var.map(|v| &self.places[v]).into_iter().cycle().zip(&words[head.len()..n - tail.len()]);
        let filled = head.iter().zip(words).chain(between).chain(tail.iter().zip(&words[n - tail.len()..]));
        for (p, w) in filled {
            if !p.literal.contains(w) && !p.literal.is_empty() {
                return Err((format!("{name} ends with {}, not `{w}`", p.said()), None));
            }
            if *w == "-" && !p.stdin {
                return Err((format!("`-` can't be {}: {name} reads no standard input there", p.said()), None));
            }
        }
        Ok(())
    }
}

/// A command's forms, and every flag it knows.
struct Grammar {
    c: &'static Command,
    forms: Vec<Form>,
}

impl Grammar {
    /// `c`'s usage as a grammar. A flag's value is the same in every form that has it, and those
    /// gone ([`commands::GONE`]) are known to each form, so that the command says why.
    fn of(c: &'static Command) -> Result<Grammar, String> {
        let mut forms = c.usage.lines().map(|l| Form::of(c, l)).collect::<Result<Vec<_>, _>>()?;
        for (n, &(_, gone)) in commands::GONE.iter().filter(|(name, _)| *name == c.name).enumerate() {
            let [name, value @ ..] = &split(gone, ' ')[..] else { unreachable!("not empty") };
            for f in &mut forms {
                f.flags.push(Flag { gone: true, ..Flag::new(name, value.first().copied(), false, usize::MAX - n) });
            }
        }
        let flags: Vec<&Flag> = forms.iter().flat_map(|f| &f.flags).collect();
        if let Some(f) = flags.iter().find(|f| flags.iter().any(|g| g.name == f.name && g.kind() != f.kind())) {
            return Err(format!("{}'s value isn't the same in each form of m2 {}", f.name, c.name));
        }
        Ok(Grammar { c, forms })
    }

    /// The grammar of the command `cmd` names, which m2 has.
    fn of_command(cmd: &str) -> Grammar {
        let c = commands::find(cmd).expect("main refuses a command m2 doesn't have");
        Grammar::of(c).unwrap_or_else(|e| panic!("m2 {}'s usage: {e}", c.name))
    }

    /// Flag `name` of the command, in whichever form has it: the same in each.
    fn flag(&self, name: &str) -> Option<&Flag> {
        self.forms.iter().find_map(|f| f.flag(name))
    }

    /// Read `words` once: the place words, and each flag with its value; None when a flag asks
    /// for the usage (only where a flag goes, never as a value).
    fn given<'a>(&self, words: &'a [String]) -> Option<Given<'a>> {
        let mut g = Given::default();
        let mut i = 0;
        while i < words.len() {
            let w = words[i].as_str();
            i += 1;
            if !is_flag(w) {
                g.places.push(w);
                continue;
            }
            if HELP.contains(&w) {
                return None;
            }
            let next = words.get(i).map(String::as_str);
            let flag = self.flag(w).filter(|f| f.value.is_some());
            let value = match flag {
                // A flag the command hasn't got, or one without a value: said against the forms.
                None => None,
                Some(f) if f.text => next,
                Some(_) => next.filter(|v| !is_flag(v)),
            };
            i += usize::from(value.is_some());
            match (flag, value) {
                (Some(f), _) if f.gone => {}
                (Some(_), None) => {
                    g.wrong.get_or_insert(format!("`{w}` needs a value"));
                    // A --help where the value goes is no help: the refusal says what is missing.
                    i += usize::from(next.is_some_and(|v| HELP.contains(&v)));
                }
                (Some(f), Some("-")) if !f.text && !f.stdin => {
                    g.wrong.get_or_insert(format!("`-` can't be the value of `{w}`: m2 reads no standard input there"));
                }
                _ => {}
            }
            g.flags.push((w, value));
        }
        Some(g)
    }

    /// Read `words`, those after `cmd`: as the first form meant takes them, else why the one that
    /// got furthest doesn't.
    fn take<'a>(&self, cmd: &str, words: &'a [String]) -> Taken<'a> {
        let Some(given) = self.given(words) else { return Taken::Help(self.c) };
        let refuse = |why: String, forms: &[&Form]| Taken::Refused(refusal(why, forms));
        let meant = |m: Meant| self.forms.iter().filter(|f| f.meant(&given.places) == m).collect::<Vec<_>>();
        let yes = meant(Meant::Yes);
        let short = yes.is_empty();
        let forms = if short { meant(Meant::Short) } else { yes };
        let unknown = |forms: &[&Form]| given.flags.iter().map(|(f, _)| *f).find(|f| !forms.iter().any(|form| form.flag(f).is_some()));
        if forms.is_empty() {
            // No form has these words where they are: a flag the command hasn't got is said
            // first, else the word.
            let all: Vec<&Form> = self.forms.iter().collect();
            if let Some(f) = unknown(&all) {
                return refuse(format!("`{f}` isn't a flag of m2 {cmd}"), &all);
            }
            return refuse(self.not_one_of(cmd, &given.places), &all);
        }
        let names: Vec<String> = forms.iter().map(|f| f.name(cmd, &given.places)).collect();
        let name = if names.iter().all(|n| *n == names[0]) { names[0].clone() } else { format!("m2 {cmd}") };
        // A value missing (or `-` where it can't be) first: the word after it may be what was
        // meant as the value (`--png --x.png`).
        if let Some(why) = &given.wrong {
            return refuse(why.clone(), &forms);
        }
        if let Some(f) = unknown(&forms) {
            return refuse(format!("`{f}` isn't a flag of {name}"), &forms);
        }
        if short {
            return refuse(self.needs_more(cmd, &forms, &given.places), &forms);
        }
        let mut misses = vec![];
        for (f, name) in forms.iter().zip(&names) {
            match f.fit(cmd, &given, name) {
                Ok(run) => {
                    let form = self.forms.iter().position(|g| std::ptr::eq(g, *f)).expect("one of them");
                    return Taken::Run { form, given, run };
                }
                Err(m) => misses.push((*f, m)),
            }
        }
        // The one that got furthest, the first of those; but when it has a word too many, and
        // another form hasn't got a flag this one has, that flag doesn't go with the word.
        let furthest = misses.iter().map(|(_, m)| m.stage).max().expect("a form was meant");
        let (form, m) = misses.iter().find(|(_, m)| m.stage == furthest).expect("the furthest");
        let flag = misses.iter().find_map(|(_, o)| o.word.filter(|w| o.stage == 0 && form.flag(w).is_some()));
        match (m.stage, m.word, flag) {
            (2, Some(word), Some(flag)) => refuse(format!("`{flag}` doesn't go with `{word}`"), &forms),
            _ => refuse(m.why.clone(), &forms),
        }
    }

    /// What the forms meant need next, when the words stop before the one that tells them apart:
    /// `m2 pack needs new, import-sfz, … or update`.
    fn needs_more(&self, cmd: &str, forms: &[&Form], places: &[&str]) -> String {
        let first = forms[0];
        let n = places.len();
        let at = (n..first.places.len()).find(|&i| !first.places[i].literal.is_empty()).expect("short of a word");
        let missing = first.places[n..at].iter().map(Place::said).chain([one_of(forms, at)]);
        format!("{} needs {}", prefix(cmd, first, places, n), missing.collect::<Vec<_>>().join(" then "))
    }

    /// Why no form is meant: the first place where a word isn't one a form has there.
    fn not_one_of(&self, cmd: &str, places: &[&str]) -> String {
        let all: Vec<&Form> = self.forms.iter().collect();
        let at = all.iter().filter_map(|f| if let Meant::No(i) = f.meant(places) { Some(i) } else { None }).min().expect("not meant");
        format!("{} takes {} next, not `{}`", prefix(cmd, all[0], places, at), one_of(&all, at), places[at])
    }
}

/// `m2 <cmd>` and the place words of `form` before `at`, those it names by a word as given:
/// `m2 clips <project>`.
fn prefix(cmd: &str, form: &Form, places: &[&str], at: usize) -> String {
    let before = form.places[..at].iter().enumerate().map(|(i, p)| if p.literal.is_empty() { p.text } else { places[i] });
    std::iter::once(format!("m2 {cmd}")).chain(before.map(String::from)).collect::<Vec<_>>().join(" ")
}

/// The words `forms` have at place `at`, each once, as `m2` says a choice: `new, check or build`.
fn one_of(forms: &[&Form], at: usize) -> String {
    let mut words: Vec<&str> = vec![];
    for w in forms.iter().filter_map(|f| f.places.get(at)).flat_map(|p| &p.literal) {
        if !words.contains(w) {
            words.push(w);
        }
    }
    or(&words)
}

/// `a, b or c`.
fn or(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [one] => one.to_string(),
        [first @ .., last] => format!("{} or {last}", first.join(", ")),
    }
}

/// A place word as the usage writes it, plain: `<words>` for `[<words…>]`, `<tx.json>` for
/// `<tx.json | ->`, `<file.wav or file.flac>` for `<file.wav|file.flac>…`.
fn plain(text: &str) -> String {
    let text = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')).unwrap_or(text).replace('…', "");
    let words: Vec<String> = alternatives(&text)
        .into_iter()
        .map(|a| match a.strip_prefix('<').and_then(|a| a.strip_suffix('>')) {
            Some(inner) => format!("<{}>", or(&alternatives(inner))),
            None => a.to_string(),
        })
        .collect();
    or(&words.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The alternatives of `text` (`a | b`), but `-`.
fn alternatives(text: &str) -> Vec<&str> {
    split(text, '|').into_iter().filter(|a| *a != "-").collect()
}

/// Refused, saying `why` and how `forms` are called.
fn refusal(why: String, forms: &[&Form]) -> String {
    let lines: Vec<&str> = forms.iter().map(|f| f.line).collect();
    format!("{why}\nusage: {}", lines.join("\n       "))
}

/// `text` split at each `sep` outside `[…]` and `<…>`, each part trimmed, none empty.
fn split(text: &str, sep: char) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (vec![], 0i32, 0);
    for (i, ch) in text.char_indices() {
        match ch {
            '[' | '<' => depth += 1,
            ']' | '>' => depth -= 1,
            _ if ch == sep && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{COMMANDS, EXAMPLES, GONE};

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    /// What `line` is run as, which must be taken.
    fn run(line: &str) -> Vec<String> {
        match read(&words(line)) {
            Read::Run(w) => w,
            Read::Help(_) => panic!("{line}: its usage"),
            Read::Refused(why) => panic!("{line}: {why}"),
        }
    }

    /// Why `args` is refused, which it must be.
    fn refused(args: &[String]) -> String {
        match read(args) {
            Read::Refused(why) => why,
            _ => panic!("{args:?} was taken"),
        }
    }

    // AGENT-13: each form of each command reads as a grammar: place words, then flags; one place
    // word at most that can be left out or given again; a flag takes a value in every form of its
    // command or in none. Those gone are commands' flags no usage shows.
    #[test]
    fn every_form_is_a_grammar() {
        for c in COMMANDS {
            Grammar::of(c).unwrap_or_else(|e| panic!("m2 {}: {e}", c.name));
        }
        for (name, flag) in GONE {
            let c = commands::find(name).unwrap_or_else(|| panic!("GONE names `{name}`, which m2 hasn't got"));
            assert!(!c.usage.contains(flag.split(' ').next().unwrap()), "m2 {name} shows {flag}, which is gone");
        }
        assert!(Form::of(&COMMANDS[0], "m2 new --x <v> <folder>").is_err(), "a place word after a flag");
        assert!(Form::of(&COMMANDS[0], "m2 new [<a>] [<b>]").is_err(), "two that can be left out");
    }

    /// A form's own example: its place words (one of `on|off`, `x` for any word, one where it can
    /// have many, none where it can have none), then its flags (the first of each `[…]`), each with
    /// a value.
    fn example(c: &Command, form: &Form) -> (Vec<String>, Vec<String>) {
        let places = form.places.iter().filter(|p| !p.optional).map(|p| p.literal.first().copied().unwrap_or("x").to_string()).collect();
        let gone = |f: &Flag| GONE.iter().any(|(name, g)| *name == c.name && g.split(' ').next() == Some(f.name));
        let mut flags = vec![];
        for f in form.flags.iter().filter(|f| !gone(f)) {
            if form.flags.iter().take_while(|g| g.name != f.name).all(|g| g.group != f.group) {
                flags.push(f.name.to_string());
                flags.extend(f.value.map(|_| "1".to_string()));
            }
        }
        (places, flags)
    }

    // AGENT-13, the table walked: each form takes its own example as it is written. Each refuses,
    // naming the word and showing the form: an unknown flag, a flag without its value, a flag it
    // must have left out, a word too many, and a place word that starts with `-`. --help, where a
    // flag goes, asks for the usage.
    #[test]
    fn every_form_takes_its_example_and_refuses_what_it_doesnt_take() {
        for c in COMMANDS {
            for form in Grammar::of(c).unwrap().forms {
                let (places, flags) = example(c, &form);
                let line = |places: &[String], flags: &[String]| [&[c.name.to_string()][..], places, flags].concat();
                let ex = line(&places, &flags);
                let says = |args: Vec<String>, what: &str| {
                    let why = refused(&args);
                    assert!(why.contains(what) && why.contains(form.line), "{args:?} isn't refused as {what}: {why}");
                };
                match read(&ex) {
                    Read::Run(run) => assert_eq!(run, ex),
                    _ => panic!("`{}` doesn't take {ex:?}", form.line),
                }
                for help in HELP {
                    assert!(
                        matches!(read(&line(&places, &[flags.clone(), vec![help.into()]].concat())), Read::Help(h) if h.name == c.name)
                    );
                }
                says(line(&places, &[flags.clone(), vec!["--nope".into()]].concat()), &format!("`--nope` isn't a flag of m2 {}", c.name));
                for (i, f) in flags.iter().enumerate().filter(|(_, f)| is_flag(f)) {
                    let rest = |n: usize| [&flags[..i], &flags[i + n..]].concat();
                    let flag = form.flag(f).unwrap();
                    if flag.value.is_some() {
                        says(line(&places, &[rest(2), vec![f.clone()]].concat()), &format!("`{f}` needs a value"));
                        says(line(&places, &[flags.clone(), vec![f.clone(), "2".into()]].concat()), &format!("`{f}` is given twice"));
                        // `-` as its value: standard input where the form writes `<x | ->`, any
                        // word for `<text>`, else refused.
                        let dash = line(&places, &[rest(2), vec![f.clone(), "-".into()]].concat());
                        match flag.stdin || flag.text {
                            true => assert!(matches!(read(&dash), Read::Run(_)), "{dash:?} isn't taken"),
                            false => says(dash, &format!("`-` can't be the value of `{f}`")),
                        }
                    }
                    // With one form, it names the flag; with more, what the first meant needs.
                    if flag.required {
                        let needs = if c.usage.lines().count() == 1 { format!("needs `{f}`") } else { "needs".into() };
                        says(line(&places, &rest(1 + usize::from(flag.value.is_some()))), &needs);
                    }
                }
                // A word too many; with a flag another form hasn't got (point's --clear), that flag
                // doesn't go with it.
                if !form.places.iter().any(|p| p.optional || p.many) {
                    let given: Vec<&str> = places.iter().map(String::as_str).collect();
                    let meant = |f: &Form| f.line != form.line && f.meant(&given) == Meant::Yes;
                    let others = Grammar::of(c).unwrap().forms.into_iter().filter(meant).collect::<Vec<_>>();
                    let lone = flags
                        .iter()
                        .find(|f| is_flag(f) && others.iter().any(|o| o.places.len() > form.places.len() && o.flag(f).is_none()));
                    let too_many = match lone {
                        Some(f) => format!("`{f}` doesn't go with `extra`"),
                        None => "`extra` is one word too many for m2".into(),
                    };
                    says(line(&[places.clone(), vec!["extra".into()]].concat(), &flags), &too_many);
                }
                if let Some((_, rest)) = places.split_first() {
                    says(line(&[vec!["-x".into()], rest.to_vec()].concat(), &flags), &format!("`-x` isn't a flag of m2 {}", c.name));
                }
            }
        }
    }

    // AGENT-13: a number, or text with a space, is a value, not a flag; `-` is standard input only
    // where a form says so; a `<text>` value is the next word whatever it is (review F9), and any
    // other value is no flag; --help asks for the usage only where a flag goes; flags go anywhere
    // after the command, and are handed on after the place words; a flag that is gone is handed
    // on, with a value or none, given once or more, so the command says why.
    #[test]
    fn numbers_dashes_values_and_order() {
        assert_eq!(run("set song id -6"), words("set song id -6"));
        assert_eq!(run("clips song move kick/hits:a --by -32"), words("clips song move kick/hits:a --by -32"));
        assert_eq!(run("render song out.wav --from -.5"), words("render song out.wav --from -.5"));
        assert_eq!(run("tx song - --check"), words("tx song - --check"));
        assert_eq!(run("clips song json -"), words("clips song json -"));
        assert_eq!(run("write song song.ts -"), words("write song song.ts -"));
        assert!(refused(&words("render song -")).contains("`-` can't be <out.wav>"));
        assert_eq!(run("edit song f --old --why --new --as"), words("edit song f --old --why --new --as"));
        assert!(refused(&words("listen song --png --json")).contains("`--png` needs a value"));
        assert!(refused(&words("listen song --png --x.png")).contains("`--png` needs a value"));
        assert!(refused(&words("listen song --png -")).contains("`-` can't be the value of `--png`"));
        assert!(refused(&words("render song out.wav --seconds --help")).contains("`--seconds` needs a value"));
        assert_eq!(run("listen song --with -"), words("listen song --with -"));
        assert_eq!(run("edit song f --old --all --new -h"), words("edit song f --old --all --new -h"));
        assert_eq!(run("edit song f --old --help --new --old"), words("edit song f --old --help --new --old"));
        assert_eq!(run("set song id -1e3"), words("set song id -1e3"));
        let text: Vec<String> = ["point", "song", "bars:9", "- check the kick"].map(String::from).to_vec();
        assert!(matches!(read(&text), Read::Run(run) if run == text));
        assert_eq!(run("bounce song name --sr"), words("bounce song name --sr"));
        assert_eq!(run("render song out.wav --sr 1 --sr 2"), words("render song out.wav --sr 1 --sr 2"));
        assert_eq!(run("render song out.wav --sr --seconds 2"), words("render song out.wav --sr --seconds 2"));
        assert_eq!(run("render --seconds 2 song out.wav"), words("render song out.wav --seconds 2"));
        assert_eq!(run("clips --beats song create kick/hits --at 4"), words("clips song create kick/hits --beats --at 4"));
        assert_eq!(run("build song --json"), words("build song --json"));
        assert_eq!(run("render song out.wav --sr 44100"), words("render song out.wav --sr 44100"));
        assert_eq!(run("plugin-state song kick/p clap:x --force"), words("plugin-state song kick/p clap:x --force"));
        assert_eq!(run("layout song color kick teal"), words("layout song color kick teal"));
        assert_eq!(run("search --kind sample"), words("search --kind sample"));
        assert_eq!(run("point song bars:9"), words("point song bars:9"));
        assert_eq!(run("pack get"), words("pack get"));
        assert!(matches!(read(&words("edit song f --old x --new y --help")), Read::Help(_)));
        assert!(matches!(read(&words("render --help")), Read::Help(_)));
    }

    // AGENT-13, the review's W1: a command reads only flags its form shows, as `value`, `on` and
    // `places` read them; asking for another stops a debug build.
    #[test]
    fn the_words_are_read_as_their_form_takes_them() {
        let args = words("edit song f --old --new --new x --all");
        assert_eq!((value(&args, "--old"), value(&args, "--new"), on(&args, "--all")), (Some("--new".into()), Some("x".into()), true));
        assert_eq!(places(&args), ["song", "f"]);
        let args = words("render song out.wav --sr");
        assert!(on(&args, "--sr") && value(&args, "--sr").is_none() && !on(&args, "--seconds"));
    }

    #[test]
    #[should_panic(expected = "doesn't show")]
    #[cfg(debug_assertions)]
    fn a_flag_its_form_doesnt_show_stops_a_debug_build() {
        on(&words("clips song json -"), "--beats");
    }

    /// `line` as a shell splits it: its words, `'…'` and `"…"` each one, up to a `|`, `>` or `#`
    /// outside them.
    fn shell(line: &str) -> Vec<String> {
        let (mut out, mut word, mut quote, mut any) = (vec![], String::new(), None, false);
        for ch in line.chars() {
            match (quote, ch) {
                (Some(q), _) if ch == q => quote = None,
                (Some(_), _) => word.push(ch),
                (None, '\'' | '"') => (quote, any) = (Some(ch), true),
                (None, '|' | '>' | '#') => break,
                (None, ' ') => {
                    if any || !word.is_empty() {
                        out.push(std::mem::take(&mut word));
                    }
                    any = false;
                }
                (None, _) => word.push(ch),
            }
        }
        if any || !word.is_empty() {
            out.push(word);
        }
        out
    }

    // AGENT-13: every example the guide shows (each `m2` line of its code blocks, and the commands
    // page's) is taken as it is written.
    #[test]
    fn the_guides_examples_are_taken_as_written() {
        let guide = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide");
        let mut lines: Vec<String> = EXAMPLES.lines().map(String::from).collect();
        for e in std::fs::read_dir(&guide).unwrap().flatten() {
            // The commands page shows each form, and EXAMPLES.
            if e.path().extension().is_none_or(|x| x != "md") || e.file_name() == "commands.md" {
                continue;
            }
            let mut block = false;
            for l in std::fs::read_to_string(e.path()).unwrap().lines() {
                if l.trim_start().starts_with("```") {
                    block = !block;
                } else if block {
                    lines.push(l.to_string());
                }
            }
        }
        let examples: Vec<&String> = lines.iter().filter(|l| l.trim_start().starts_with("m2 ")).collect();
        assert!(examples.len() > 15, "{examples:?}");
        for l in examples {
            let mut args = shell(l.trim())[1..].to_vec();
            crate::app::take_who(&mut args).unwrap();
            assert!(matches!(read(&args), Read::Run(_)), "the guide's `{l}` isn't taken: {}", refused(&args));
        }
    }
}
