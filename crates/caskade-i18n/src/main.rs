//! `caskade-i18n extract [--check] [--root <repository>]`: writes the core
//! domain's template (`locales/core.pot`) and merges it into each
//! `locales/<language>/core.po`. With `--check` it writes nothing and fails
//! when a file is out of date, naming the entries that changed. `just i18n`
//! runs it, and `just i18n-check` with `--check`.

use caskade_i18n::extract::CORE;
use caskade_i18n::po::{Message, Po};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: caskade-i18n extract [--check] [--root <repository>]";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("extract") {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut check = false;
    let mut root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => check = true,
            "--root" => match args.next() {
                Some(path) => root = PathBuf::from(path),
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let root = root.canonicalize().unwrap_or(root);
    let outputs = match CORE.outputs(&root) {
        Ok(outputs) => outputs,
        Err(error) => {
            eprintln!("caskade-i18n: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut stale = false;
    for output in &outputs {
        let shown = output.path.strip_prefix(&root).unwrap_or(&output.path);
        if check {
            if !output.is_current() {
                eprintln!("out of date: {}", shown.display());
                for change in changes(&output.path, &output.contents) {
                    eprintln!("  {change}");
                }
                stale = true;
            }
            continue;
        }
        match output.write() {
            Ok(true) => println!("wrote {}", shown.display()),
            Ok(false) => {}
            Err(error) => {
                eprintln!("caskade-i18n: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    if stale {
        eprintln!("run `just i18n`");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// What `expected` changes in the file at `path`, by entry: messages it
/// adds and drops, else that only comments, references or flags differ.
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
        changes.push("comments, references or flags changed".to_owned());
    }
    changes
}

/// `msgctxt "…" msgid "…"`, to name an entry.
fn describe(message: &Message) -> String {
    match &message.context {
        Some(context) => format!("msgctxt {context:?} msgid {:?}", message.id),
        None => format!("msgid {:?}", message.id),
    }
}
