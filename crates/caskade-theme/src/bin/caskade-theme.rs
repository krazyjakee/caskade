//! `caskade-theme lint <dir>…` checks theme folders against the theming
//! contract, and their translations (`locales/*.po`, #219);
//! `caskade-theme build <dir> [<dir>…]` prints the first theme's
//! compiled stylesheet, resolving `extends` among the folders given.
use caskade_theme::{DirFiles, Locales, Theme, chain, compile};
use std::process::ExitCode;

/// [`load`], and the theme's catalogs: a catalog problem fails the lint,
/// though the app would still apply the theme in English.
fn lint(dir: &str) -> Result<(), ()> {
    let theme = load(dir);
    let (_, problems) = Locales::load(&DirFiles::new(dir));
    for problem in &problems {
        eprintln!("{dir}/{problem}");
    }
    match theme {
        Ok(_) if problems.is_empty() => Ok(()),
        _ => Err(()),
    }
}

fn load(dir: &str) -> Result<Theme, ()> {
    Theme::load(&DirFiles::new(dir)).map_err(|problems| {
        for problem in problems {
            eprintln!("{dir}/{problem}");
        }
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((command, dirs)) if command == "lint" && !dirs.is_empty() => {
            let failed = dirs.iter().filter(|dir| lint(dir).is_err()).count();
            if failed == 0 {
                println!("{} theme(s) ok", dirs.len());
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Some((command, dirs)) if command == "build" && !dirs.is_empty() => {
            let Ok(themes) = dirs
                .iter()
                .map(|dir| load(dir))
                .collect::<Result<Vec<_>, _>>()
            else {
                return ExitCode::FAILURE;
            };
            let lookup = |id: &str| themes.iter().find(|theme| theme.id() == id);
            match chain(themes[0].id(), lookup) {
                Ok(chain) => {
                    print!("{}", compile(&chain));
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!(
                "usage: caskade-theme lint <dir>…\n       caskade-theme build <dir> [<extended theme dir>…]"
            );
            ExitCode::from(2)
        }
    }
}
