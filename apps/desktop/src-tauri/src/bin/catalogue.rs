//! `caskade-catalogue`: builds and checks the add-on catalogue (#184).
//! See `docs/plugins/publishing.md`.
//!
//! ```text
//! caskade-catalogue build <source.json> --out <dir> [--previous <catalogue.json>]
//! caskade-catalogue check <catalogue.json | URL> [--key <minisign public key or file> | --release-key]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  caskade-catalogue build <source.json> --out <dir> [--previous <catalogue.json>]
  caskade-catalogue check <catalogue.json | URL> [--key <public key or .pub file> | --release-key]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("build") => build(&args[1..]),
        Some("check") => check(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// `--name value` from `args`, and the first argument that isn't a flag.
fn options<'a>(
    args: &'a [String],
    flags: &[&str],
    switches: &[&str],
) -> Result<Parsed<'a>, String> {
    let mut parsed = Parsed::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if switches.contains(&arg.as_str()) {
            parsed.switches.push(arg);
        } else if flags.contains(&arg.as_str()) {
            let value = rest.next().ok_or(format!("{arg} needs a value\n{USAGE}"))?;
            parsed.flags.push((arg, value));
        } else if arg.starts_with("--") || parsed.target.is_some() {
            return Err(format!("unexpected {arg}\n{USAGE}"));
        } else {
            parsed.target = Some(arg);
        }
    }
    Ok(parsed)
}

#[derive(Default)]
struct Parsed<'a> {
    target: Option<&'a str>,
    flags: Vec<(&'a str, &'a str)>,
    switches: Vec<&'a str>,
}

impl Parsed<'_> {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(flag, _)| *flag == name)
            .map(|(_, value)| *value)
    }
}

fn build(args: &[String]) -> Result<(), String> {
    let parsed = options(args, &["--out", "--previous"], &[])?;
    let source = parsed.target.ok_or(USAGE)?;
    let out = PathBuf::from(parsed.flag("--out").ok_or(USAGE)?);
    let previous = parsed.flag("--previous").map(PathBuf::from);
    let built = caskade_addons::author::build(source.as_ref(), &out, previous.as_deref())?;
    for (id, version, file, sha256) in &built.packages {
        println!("{id} {version}  {file}  sha256 {sha256}");
    }
    if built.carried > 0 {
        println!("kept {} earlier releases", built.carried);
    }
    println!(
        "wrote {}; sign it before publishing",
        out.join("catalogue.json").display()
    );
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    let parsed = options(args, &["--key"], &["--release-key"])?;
    let location = parsed.target.ok_or(USAGE)?;
    let key = match (
        parsed.flag("--key"),
        parsed.switches.contains(&"--release-key"),
    ) {
        (Some(_), true) => return Err(format!("--key or --release-key, not both\n{USAGE}")),
        (Some(key), false) => Some(match std::fs::read_to_string(key) {
            Ok(text) => text,
            Err(_) => key.to_owned(),
        }),
        (None, true) => Some(
            caskade_desktop::addon_tool::release_key()
                .ok_or("tauri.conf.json has no updater key")?,
        ),
        (None, false) => None,
    };
    let work = std::env::temp_dir().join(format!("caskade-catalogue-{}", std::process::id()));
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let checked = runtime.block_on(caskade_desktop::addon_tool::check(
        location,
        key.as_deref().map(str::trim),
        &work,
    ));
    let _ = std::fs::remove_dir_all(&work);
    let checked = checked?;
    for entry in &checked.catalogue.entries {
        let versions: Vec<&str> = entry.releases.iter().map(|r| r.version.as_str()).collect();
        println!("{} ({:?}): {}", entry.id, entry.kind, versions.join(", "));
    }
    for warning in &checked.warnings {
        println!("warning: {warning}");
    }
    for problem in &checked.problems {
        eprintln!("problem: {problem}");
    }
    if checked.problems.is_empty() {
        println!(
            "{} releases of {} packages check out",
            checked.releases.len(),
            checked.catalogue.entries.len()
        );
        Ok(())
    } else {
        Err(format!("{} problems", checked.problems.len()))
    }
}
