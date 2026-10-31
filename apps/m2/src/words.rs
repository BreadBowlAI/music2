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
//! | `[…]` | can be left out; of `[--from S \| --from-bar N]`, one at most |
//!
//! A form writes its place words first, then its flags. A word that starts with `-` is a flag,
//! unless it is a number (`-6`) or `-` alone. A flag's value is the next word, unless that word
//! is a flag of the command. Flags can go anywhere after the command: it is handed its place words
//! first, in order, then its flags as they were given.

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
    let c = commands::find(&args[0]).expect("main refuses a command m2 doesn't have");
    let g = Grammar::of(c).unwrap_or_else(|e| panic!("m2 {}'s usage: {e}", c.name));
    g.read(&args[0], &args[1..])
}

/// Whether `word` is a flag: it starts with `-`, and isn't `-` alone or a number (`-6`, `-.5`).
pub fn is_flag(word: &str) -> bool {
    word.len() > 1 && word.starts_with('-') && !word[1..].starts_with(|c: char| c.is_ascii_digit() || c == '.')
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
        match &self.literal[..] {
            [] => self.text.to_string(),
            [one] => one.to_string(),
            [first @ .., last] => format!("{} or {last}", first.join(", ")),
        }
    }
}

/// A flag of a form.
struct Flag {
    name: &'static str,
    /// Its value as the form writes it (`S`, `<place>`), or None: it takes none.
    value: Option<&'static str>,
    /// Written outside `[…]`: it must be given.
    required: bool,
    /// Of the flags written in one `[…]`, one at most is given.
    group: usize,
}

/// One form of a command's usage, read as a grammar.
struct Form {
    /// The form as the usage shows it: one line.
    line: &'static str,
    places: Vec<Place>,
    flags: Vec<Flag>,
}

/// Why a form doesn't take the words, and how far reading them got: a flag it hasn't got (0), a
/// flag's value, or one given twice (1), the place words, or a flag it must have (2).
struct Miss {
    stage: u8,
    why: String,
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
                if alternatives.iter().all(|a| is_flag(a)) {
                    for a in alternatives {
                        let [name, value @ ..] = &split(a, ' ')[..] else { unreachable!("not empty") };
                        if value.len() > 1 {
                            return Err(format!("`{a}` in `{line}` is a flag and more than one value"));
                        }
                        form.flags.push(Flag { name, value: value.first().copied(), required: false, group });
                    }
                    continue;
                }
                if alternatives.iter().any(|a| is_flag(a)) {
                    return Err(format!("`{t}` in `{line}` has a flag and a place word as one or the other"));
                }
                let many = alternatives.iter().any(|a| a.contains('…'));
                form.place(Place { text: t, literal: vec![], optional: true, many, stdin: alternatives.contains(&"-") })?;
            } else if is_flag(t) {
                // Its value, when the next word is one: no place word comes after a flag.
                let value = tokens.get(i).filter(|v| !is_flag(v) && !v.starts_with('[')).copied();
                i += usize::from(value.is_some());
                form.flags.push(Flag { name: t, value, required: true, group });
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

    /// How the form is named in what `m2` says: `m2 render`; to its last word, as given,
    /// `m2 clips <project> delete`.
    fn name(&self, c: &Command, places: &[&str]) -> String {
        let last = self.head().iter().rposition(|p| !p.literal.is_empty()).map_or(0, |i| i + 1);
        let shown = self.head()[..last].iter().enumerate().map(|(i, p)| match p.literal.is_empty() {
            true => p.text,
            false => places.get(i).copied().unwrap_or(p.text),
        });
        std::iter::once(format!("m2 {}", c.name)).chain(shown.map(String::from)).collect::<Vec<_>>().join(" ")
    }

    /// Read `words` against this form, `name` as [`Form::name`] says it: the words to run the
    /// command with (`cmd`, the place words, then the flags as given), or why not.
    fn fit(&self, g: &Grammar, cmd: &str, words: &[String], name: &str) -> Result<Vec<String>, Miss> {
        let miss = |stage, why: String| Miss { stage, why };
        let mut places: Vec<&str> = vec![];
        let mut given: Vec<(&Flag, Option<&str>)> = vec![];
        let mut i = 0;
        while i < words.len() {
            let w = words[i].as_str();
            i += 1;
            if !is_flag(w) {
                places.push(w);
                continue;
            }
            let f = self.flag(w).ok_or_else(|| miss(0, format!("{w} isn't something {name} takes")))?;
            let value = match f.value {
                None => None,
                Some(_) => match words.get(i).filter(|v| !g.knows(v)) {
                    Some(v) => Some(v.as_str()),
                    None => return Err(miss(1, format!("{w} needs a value"))),
                },
            };
            i += usize::from(value.is_some());
            if given.iter().any(|(g, _)| g.name == w) {
                return Err(miss(1, format!("{w} is named twice")));
            }
            if let Some((other, _)) = given.iter().find(|(o, _)| o.group == f.group) {
                return Err(miss(1, format!("{} and {w} are one or the other: give one", other.name)));
            }
            given.push((f, value));
        }
        self.fill(&places, name).map_err(|why| miss(2, why))?;
        if let Some(f) = self.flags.iter().find(|f| f.required && !given.iter().any(|(g, _)| g.name == f.name)) {
            return Err(miss(2, format!("{name} needs {}{}", f.name, f.value.map_or(String::new(), |v| format!(" {v}")))));
        }
        let mut run = vec![cmd.to_string()];
        run.extend(places.iter().map(|p| p.to_string()));
        for (f, value) in given {
            run.push(f.name.to_string());
            run.extend(value.map(String::from));
        }
        Ok(run)
    }

    /// Whether the place words `words` fill this form's: as many as it takes, each one it can be.
    fn fill(&self, words: &[&str], name: &str) -> Result<(), String> {
        let var = self.places.iter().position(|p| p.optional || p.many);
        let (head, tail) = match var {
            Some(v) => (&self.places[..v], &self.places[v + 1..]),
            None => (&self.places[..], &[][..]),
        };
        // Those that must be given, filled from the start: the first left over is what is missing.
        let needed: Vec<&Place> = head.iter().chain(var.map(|v| &self.places[v]).filter(|p| !p.optional)).chain(tail).collect();
        let n = words.len();
        if let Some(p) = needed.get(n) {
            return Err(format!("{name} needs {}", p.said()));
        }
        let most = match var.map(|v| &self.places[v]) {
            Some(p) if p.many => usize::MAX,
            Some(_) => head.len() + 1 + tail.len(),
            None => head.len(),
        };
        if n > most {
            return Err(format!("`{}` is one word too many for {name}", words[most]));
        }
        // The head from the start, the tail from the end, and what is between in the one between.
        let between = var.map(|v| &self.places[v]).into_iter().cycle().zip(&words[head.len()..n - tail.len()]);
        let filled = head.iter().zip(words).chain(between).chain(tail.iter().zip(&words[n - tail.len()..]));
        for (p, w) in filled {
            if !p.literal.contains(w) && !p.literal.is_empty() {
                return Err(format!("{name} ends with {}, not `{w}`", p.said()));
            }
            if *w == "-" && !p.stdin {
                return Err(format!("`-` can't be {}: {name} reads no standard input there", p.text));
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
    /// `c`'s usage as a grammar. Each flag it knows takes a value in every form or in none, and
    /// those gone ([`commands::GONE`]) are known to each form, so that the command says why.
    fn of(c: &'static Command) -> Result<Grammar, String> {
        let mut forms = c.usage.lines().map(|l| Form::of(c, l)).collect::<Result<Vec<_>, _>>()?;
        for (n, &(_, gone)) in commands::GONE.iter().filter(|(name, _)| *name == c.name).enumerate() {
            let [name, value @ ..] = &split(gone, ' ')[..] else { unreachable!("not empty") };
            for f in &mut forms {
                f.flags.push(Flag { name, value: value.first().copied(), required: false, group: usize::MAX - n });
            }
        }
        let flags: Vec<&Flag> = forms.iter().flat_map(|f| &f.flags).collect();
        if let Some(f) = flags.iter().find(|f| flags.iter().any(|g| g.name == f.name && g.value.is_some() != f.value.is_some())) {
            return Err(format!("{} takes a value in one form of m2 {} and none in another", f.name, c.name));
        }
        Ok(Grammar { c, forms })
    }

    /// Whether `word` is a flag of the command, or asks for its usage.
    fn knows(&self, word: &str) -> bool {
        HELP.contains(&word) || self.forms.iter().any(|f| f.flag(word).is_some())
    }

    fn takes_value(&self, word: &str) -> bool {
        self.forms.iter().filter_map(|f| f.flag(word)).any(|f| f.value.is_some())
    }

    /// Read `words`, those after `cmd`: as the first form meant takes them, else why the one that
    /// got furthest doesn't.
    fn read(&self, cmd: &str, words: &[String]) -> Read {
        // The place words, read with every flag the command knows, tell which forms are meant.
        let (mut places, mut flags) = (vec![], vec![]);
        let mut i = 0;
        while i < words.len() {
            let w = words[i].as_str();
            i += 1;
            if !is_flag(w) {
                places.push(w);
            } else if HELP.contains(&w) {
                return Read::Help(self.c);
            } else {
                flags.push(w);
                i += usize::from(self.takes_value(w) && words.get(i).is_some_and(|v| !self.knows(v)));
            }
        }
        let meant = |m: Meant| self.forms.iter().filter(|f| f.meant(&places) == m).collect::<Vec<_>>();
        let yes = meant(Meant::Yes);
        let short = yes.is_empty();
        let forms = if short { meant(Meant::Short) } else { yes };
        if forms.is_empty() {
            // No form has these words where they are: a flag the command hasn't got is said
            // first, else the word.
            let all: Vec<&Form> = self.forms.iter().collect();
            if let Some(f) = flags.iter().find(|f| !self.knows(f)) {
                return refuse(format!("{f} isn't something m2 {} takes", self.c.name), &all);
            }
            return refuse(self.not_one_of(&places), &all);
        }
        let names: Vec<String> = forms.iter().map(|f| f.name(self.c, &places)).collect();
        let name = if names.iter().all(|n| *n == names[0]) { names[0].clone() } else { format!("m2 {}", self.c.name) };
        if let Some(f) = flags.iter().find(|f| !forms.iter().any(|form| form.flag(f).is_some())) {
            return refuse(format!("{f} isn't something {name} takes"), &forms);
        }
        if short {
            return refuse(self.needs_more(&forms, &places), &forms);
        }
        let mut furthest: Option<Miss> = None;
        for (f, name) in forms.iter().zip(&names) {
            match f.fit(self, cmd, words, name) {
                Ok(run) => return Read::Run(run),
                Err(m) if furthest.as_ref().is_none_or(|b| m.stage > b.stage) => furthest = Some(m),
                Err(_) => {}
            }
        }
        refuse(furthest.expect("a form was meant").why, &forms)
    }

    /// What the forms meant need next, when the words stop before the one that tells them apart:
    /// `m2 pack needs new|import-sfz|…`.
    fn needs_more(&self, forms: &[&Form], places: &[&str]) -> String {
        let first = forms[0];
        let n = places.len();
        let at = (n..first.places.len()).find(|&i| !first.places[i].literal.is_empty()).expect("short of a word");
        let given = first.places[..n].iter().map(|p| p.text);
        let missing = first.places[n..=at].iter().enumerate().map(|(i, p)| match i + n == at {
            true => one_of(forms, at),
            false => p.text.to_string(),
        });
        let name = std::iter::once(format!("m2 {}", self.c.name)).chain(given.map(String::from)).collect::<Vec<_>>().join(" ");
        format!("{name} needs {}", missing.collect::<Vec<_>>().join(" "))
    }

    /// Why no form is meant: the first place where a word isn't one a form has there.
    fn not_one_of(&self, places: &[&str]) -> String {
        let all: Vec<&Form> = self.forms.iter().collect();
        let at = all.iter().filter_map(|f| if let Meant::No(i) = f.meant(places) { Some(i) } else { None }).min().expect("not meant");
        let before = all[0].places[..at].iter().enumerate().map(|(i, p)| if p.literal.is_empty() { p.text } else { places[i] });
        let name = std::iter::once(format!("m2 {}", self.c.name)).chain(before.map(String::from)).collect::<Vec<_>>().join(" ");
        format!("{name} takes {} next, not `{}`", one_of(&all, at), places[at])
    }
}

/// The words `forms` have at place `at`, each once: `new|import-sfz|check`.
fn one_of(forms: &[&Form], at: usize) -> String {
    let mut words: Vec<&str> = vec![];
    for w in forms.iter().filter_map(|f| f.places.get(at)).flat_map(|p| &p.literal) {
        if !words.contains(w) {
            words.push(w);
        }
    }
    words.join("|")
}

/// Refused, saying `why` and how `forms` are called.
fn refuse(why: String, forms: &[&Form]) -> Read {
    let lines: Vec<&str> = forms.iter().map(|f| f.line).collect();
    Read::Refused(format!("{why}\nusage: {}", lines.join("\n       ")))
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
                says(line(&places, &[flags.clone(), vec!["--nope".into()]].concat()), &format!("--nope isn't something m2 {}", c.name));
                for (i, f) in flags.iter().enumerate().filter(|(_, f)| is_flag(f)) {
                    let rest = |n: usize| [&flags[..i], &flags[i + n..]].concat();
                    let flag = form.flag(f).unwrap();
                    if flag.value.is_some() {
                        says(line(&places, &[rest(2), vec![f.clone()]].concat()), &format!("{f} needs a value"));
                        says(line(&places, &[flags.clone(), vec![f.clone(), "2".into()]].concat()), &format!("{f} is named twice"));
                    }
                    // With one form, it names the flag; with more, what the first meant needs.
                    if flag.required {
                        let needs = if c.usage.lines().count() == 1 { format!("needs {f}") } else { "needs".into() };
                        says(line(&places, &rest(1 + usize::from(flag.value.is_some()))), &needs);
                    }
                }
                if !form.places.iter().any(|p| p.optional || p.many) {
                    says(line(&[places.clone(), vec!["extra".into()]].concat(), &flags), "`extra` is one word too many for m2");
                }
                if let Some((_, rest)) = places.split_first() {
                    says(line(&[vec!["-x".into()], rest.to_vec()].concat(), &flags), &format!("-x isn't something m2 {}", c.name));
                }
            }
        }
    }

    // AGENT-13: a number is a value, not a flag; `-` is standard input only where a form says so;
    // a value that reads like a flag the command hasn't got (`--why`) is that value; flags go
    // anywhere after the command, and are handed on after the place words; a flag that is gone is
    // handed on, so the command says why.
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
        assert!(refused(&words("listen song --png --json")).contains("--png needs a value"));
        assert!(refused(&words("edit song f --old --all --new x")).contains("--old needs a value"));
        assert_eq!(run("render --seconds 2 song out.wav"), words("render song out.wav --seconds 2"));
        assert_eq!(run("clips --beats song create kick/hits --at 4"), words("clips song create kick/hits --beats --at 4"));
        assert_eq!(run("build song --json"), words("build song --json"));
        assert_eq!(run("render song out.wav --sr 44100"), words("render song out.wav --sr 44100"));
        assert_eq!(run("plugin-state song kick/p clap:x --force"), words("plugin-state song kick/p clap:x --force"));
        assert_eq!(run("layout song color kick teal"), words("layout song color kick teal"));
        assert_eq!(run("search --kind sample"), words("search --kind sample"));
        assert_eq!(run("point song bars:9"), words("point song bars:9"));
        assert_eq!(run("pack get"), words("pack get"));
        assert!(matches!(read(&words("edit song f --old x --new --help")), Read::Help(_)));
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
