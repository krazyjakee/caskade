//! Maintainer tooling. Read-only fixture checks never invoke external builders.
pub mod checks;
#[path = "../discovery/mod.rs"]
pub mod discovery;
#[path = "../emulation/mod.rs"]
pub mod emulation;
pub mod generate;
#[path = "../package-fixtures.rs"]
pub mod packages;

// The same loader can be included directly by installer integration tests, without
// depending on this crate or its archive-building dependencies.
#[path = "../../crates/caskade-installer/tests/fixtures/support/mod.rs"]
pub mod fixtures;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut args = args.peekable();
    let group = args.next();
    if group.as_deref() == Some("emulation") {
        return emulation::run(args);
    }
    if group.as_deref() == Some("discovery") {
        return discovery::run(args, &workspace_root()).map_err(|error| error.to_string().into());
    }
    if group.as_deref() != Some("fixtures") {
        return Err(
            "usage: cargo xtask fixtures [build|windows|linux-shell|appimage|packages|disc|flatpak|linux-mini|native-uninstall|native-windows|macos-pkg|burn|record-build|check|check-stale|check-size] [--root <workspace>]"
                .into(),
        );
    }
    let mut command = if args.peek().is_some_and(|s| !s.starts_with('-')) {
        args.next().unwrap()
    } else {
        "build".to_owned()
    };
    let mut root = workspace_root();
    let mut prepare = false;
    let mut families = None;
    let mut artifact = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" if command == "build" => command = "check".to_owned(),
            "--artifact" if command == "windows" => {
                artifact = Some(args.next().ok_or("--artifact needs a directory")?)
            }
            "--prepare"
                if matches!(
                    command.as_str(),
                    "windows"
                        | "linux-shell"
                        | "appimage"
                        | "native-uninstall"
                        | "native-windows"
                        | "macos-pkg"
                        | "burn"
                ) =>
            {
                prepare = true
            }
            "--families" if command == "windows" => {
                families = Some(args.next().ok_or("--families needs a list")?)
            }
            "--root" => root = args.next().ok_or("--root needs a workspace path")?.into(),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    match command.as_str() {
        "build" => generate::build(&root)?,
        "packages" => packages::build(&root)?,
        "windows" | "linux-shell" | "appimage" | "disc" | "flatpak" | "linux-mini"
        | "native-uninstall" | "native-windows" | "macos-pkg" | "burn" => {
            let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".into());
            let mut builder = std::process::Command::new(python);
            builder.arg(root.join(match command.as_str() {
                "windows" => "xtask/windows-fixtures.py",
                "linux-shell" => "xtask/linux-shell-fixtures.py",
                "disc" => "xtask/disc-fixtures.py",
                "flatpak" => "xtask/flatpak-fixtures.py",
                "linux-mini" => "xtask/linux-mini-fixtures.py",
                "native-uninstall" => "xtask/native-uninstall-fixtures.py",
                "native-windows" => "xtask/windows-native-fixtures.py",
                "macos-pkg" => "xtask/pkg-fixtures.py",
                "burn" => "xtask/burn-fixtures.py",
                _ => "xtask/appimage-fixtures.py",
            }));
            if prepare {
                builder.arg("--prepare");
            }
            if let Some(families) = families {
                builder.args(["--families", &families]);
            }
            if let Some(artifact) = artifact {
                builder.args(["--artifact", &artifact]);
            }
            if !builder.status()?.success() {
                return Err(format!("{command} fixture builder failed").into());
            }
            checks::write_lock(&root)?;
            checks::check_stale(&root)?;
            checks::check_size(&root)?;
        }
        "record-build" => {
            // Maintainer-only finalization after running the checked-in builders;
            // read-only check/check-stale never bless changed inputs or outputs.
            checks::write_lock(&root)?;
            checks::check_stale(&root)?;
            checks::check_size(&root)?;
        }
        "check" => {
            checks::check_stale(&root)?;
            checks::check_size(&root)?;
        }
        "check-stale" => checks::check_stale(&root)?,
        "check-size" => checks::check_size(&root)?,
        _ => return Err(format!("unknown fixtures command: {command}").into()),
    }
    println!("fixtures {command}: ok");
    Ok(())
}
