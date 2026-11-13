//! `caskade-i18n extract [--check] [--add <language>]… [--root <repository>]`:
//! writes each domain's template (`locales/core.pot`, `app.pot`, `ui.pot`)
//! and merges it into every language's `locales/<language>/<domain>.po`,
//! starting the catalogs a language lacks; `--add de` starts a language.
//! It refuses to write a merge that would lose a translation.
//!
//! With `--check` it writes nothing and fails when a file is out of date,
//! naming the entries that changed, when a PO file fails `msgfmt --check`
//! (plugins' and themes' too) or when a translation drops or changes its
//! English's placeholders or tags; then it prints how much of each language
//! is translated. The checks need GNU gettext's msgfmt. `just i18n` runs
//! it, and `just i18n-check` with `--check`.

use caskade_i18n::extract::{
    DOMAINS, Output, check_catalogs, completion, describe, languages, lost_translations,
};
use caskade_i18n::locale;
use caskade_i18n::po::{Message, Po};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str =
    "usage: caskade-i18n extract [--check] [--add <language>]... [--root <repository>]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("extract") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut check = false;
    let mut add = Vec::new();
    let mut root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--check" => {
                check = true;
                continue;
            }
            "--root" | "--add" => args.next(),
            _ => None,
        };
        let Some(value) = value else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        if arg == "--root" {
            root = PathBuf::from(value);
            continue;
        }
        match locale::normalize(&value) {
            Some(tag) => add.push(tag),
            None => {
                eprintln!("caskade-i18n: {value:?} isn't a language tag (sv, pt-BR)");
                return ExitCode::from(2);
            }
        }
    }
    let root = root.canonicalize().unwrap_or(root);
    let outputs = match outputs(&root, &add) {
        Ok(outputs) => outputs,
        Err(error) => {
            eprintln!("caskade-i18n: {error}");
            return ExitCode::FAILURE;
        }
    };
    let show = |path: &Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string()
    };

    // A merge never loses a translation: one that would is a bug here, not
    // a change to commit.
    let mut losing = false;
    for output in &outputs {
        let lost = lost(output);
        if !lost.is_empty() {
            eprintln!("would lose translations: {}", show(&output.path));
            for entry in lost {
                eprintln!("  {entry}");
            }
            losing = true;
        }
    }
    if losing {
        return ExitCode::FAILURE;
    }

    if !check {
        for output in &outputs {
            match output.write() {
                Ok(true) => println!("wrote {}", show(&output.path)),
                Ok(false) => {}
                Err(error) => {
                    eprintln!("caskade-i18n: {error}");
                    return ExitCode::FAILURE;
                }
            }
        }
        return ExitCode::SUCCESS;
    }

    let mut problems = Vec::new();
    for output in &outputs {
        if !output.is_current() {
            problems.push(format!(
                "out of date: {} (run `just i18n`)",
                show(&output.path)
            ));
            for change in changes(&output.path, &output.contents) {
                problems.push(format!("  {change}"));
            }
        }
    }
    match check_catalogs(&root) {
        Ok(found) => problems.extend(found),
        Err(error) => {
            eprintln!("caskade-i18n: {error}");
            return ExitCode::from(2);
        }
    }
    match completion(&root) {
        Ok(languages) => {
            for (language, domains) in languages {
                let done: usize = domains.iter().map(|domain| domain.translated).sum();
                let total: usize = domains.iter().map(|domain| domain.total).sum();
                let parts: Vec<String> = domains
                    .iter()
                    .map(|domain| {
                        format!("{} {}/{}", domain.domain, domain.translated, domain.total)
                    })
                    .collect();
                println!(
                    "{language}: {} % translated ({})",
                    percent(done, total),
                    parts.join(", ")
                );
            }
        }
        Err(error) => problems.push(error.to_string()),
    }
    if !problems.is_empty() {
        for problem in problems {
            eprintln!("{problem}");
        }
        eprintln!("The translation catalogs need fixing (docs/translating.md).");
        return ExitCode::FAILURE;
    }
    println!("The templates are current and every catalog passes.");
    ExitCode::SUCCESS
}

/// Every domain's outputs, for the translated languages and `add`.
fn outputs(root: &Path, add: &[String]) -> Result<Vec<Output>, String> {
    let mut languages = languages(root).map_err(|error| error.to_string())?;
    languages.extend(add.iter().cloned());
    languages.sort();
    languages.dedup();
    let mut outputs = Vec::new();
    for domain in DOMAINS {
        outputs.extend(
            domain
                .outputs_for(root, &languages)
                .map_err(|error| error.to_string())?,
        );
    }
    Ok(outputs)
}

/// The translations a catalog output drops from the file on disk.
fn lost(output: &Output) -> Vec<String> {
    if output.path.extension().is_none_or(|ext| ext != "po") {
        return Vec::new();
    }
    let read = |text: &str| Po::parse(text).unwrap_or_default();
    let Ok(on_disk) = std::fs::read_to_string(&output.path) else {
        return Vec::new();
    };
    lost_translations(&read(&on_disk), &read(&output.contents))
}

fn percent(done: usize, total: usize) -> usize {
    (done * 100).checked_div(total).unwrap_or(100)
}

/// What `expected` changes in the file at `path`, by entry: messages it
/// adds and drops, else that only comments, references, flags or obsolete
/// entries differ.
fn changes(path: &Path, expected: &str) -> Vec<String> {
    let read = |text: &str| Po::parse(text).map(|po| po.messages).unwrap_or_default();
    let on_disk = read(&std::fs::read_to_string(path).unwrap_or_default());
    let expected = read(expected);
    let missing = |from: &[Message], to: &[Message]| -> Vec<String> {
        from.iter()
            .filter(|message| !to.iter().any(|other| other.key() == message.key()))
            .map(describe)
            .collect()
    };
    let mut changes: Vec<String> = missing(&expected, &on_disk)
        .into_iter()
        .map(|entry| format!("new: {entry}"))
        .chain(
            missing(&on_disk, &expected)
                .into_iter()
                .map(|entry| format!("gone: {entry}")),
        )
        .collect();
    if changes.is_empty() {
        changes.push("comments, references, flags or obsolete entries changed".to_owned());
    }
    changes
}
