//! `m2 search <words…> [--kind <kind>] [--json]` (LIB-2): the library's one search, as ⌘F asks it.
//! With the app open it asks the app (`GET /api/library`), whose list holds the open song's own
//! components and the plugins it has listed; with the app closed it gathers the same list itself,
//! from std and your presets, and searches it with the same function.

use anyhow::{Result, bail};
use m2_editor::library::{self, Kind, Query};
use serde_json::Value;

/// Run `m2 search`.
pub fn search(args: &[String]) -> Result<()> {
    let (mut words, mut json) = (vec![], false);
    let mut rest = args.iter().skip(1);
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--json" => json = true,
            "--kind" => match rest.next() {
                Some(k) if Kind::named(k).is_some() => words.push(k.clone()),
                other => bail!("--kind is instrument, sound, effect, sample or plugin, not {}", other.map_or("nothing", String::as_str)),
            },
            w => words.push(w.to_string()),
        }
    }
    let q = words.join(" ");
    let items: Vec<Value> = match crate::app::library(&q)? {
        Some(items) => items,
        None => {
            let std = m2_server::resources::std_dir();
            let yours = m2_os::dirs::presets();
            let all = library::without_song(&[("std".to_string(), std.clone())], Some(&std), yours.as_deref(), &[]);
            library::search(&all, &Query::parse(&q)).into_iter().map(serde_json::to_value).collect::<Result<_, _>>()?
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    if items.is_empty() {
        eprintln!("Nothing in the library matches {q}.");
        return Ok(());
    }
    let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    // Each item's columns: its label, its kind, what it is and where it is from, and its id.
    let rows: Vec<[String; 4]> = items
        .iter()
        .map(|i| {
            let what = if text(i, "kind") == "sound" { i["tags"][0].as_str().unwrap_or_default().to_string() } else { text(i, "category") };
            [text(i, "label"), text(i, "kind"), format!("{what} · {}", text(i, "package")), text(i, "id")]
        })
        .collect();
    let width = |c: usize| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0);
    let (w0, w1, w2) = (width(0), width(1), width(2));
    let mut out = String::new();
    for [label, kind, what, id] in &rows {
        out += &format!("{label:w0$}  {kind:w1$}  {what:w2$}  {id}\n");
    }
    print!("{out}");
    Ok(())
}
