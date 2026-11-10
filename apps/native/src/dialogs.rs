//! The OS's dialogs (#88): questions with buttons, and file and folder
//! pickers. On Linux they're GTK's, on the tray's GTK thread (GTK lives on
//! one thread, and winit doesn't run it); elsewhere rfd's (the native
//! Windows and macOS dialogs). Every call blocks until the user answers;
//! call it off the window's thread once the event loop runs.

// The pickers and the install picker are for the views (installs #99,
// Settings → Storage #104); startup's dialogs use the rest now.
#![allow(dead_code)]
use std::path::PathBuf;

/// What the dialog is about: how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Info,
    Error,
}

/// Asks `text` with `buttons` (the first is the default); the index of the
/// one pressed, `None` when the dialog was closed or can't show.
pub fn ask(title: &str, text: &str, kind: Kind, buttons: &[String]) -> Option<usize> {
    platform::ask(title, text, kind, buttons)
}

/// Picks one file, of `filter` (a name and its extensions) when given.
pub fn pick_file(title: &str, filter: Option<(&str, &[&str])>) -> Option<PathBuf> {
    platform::pick(title, filter, Pick::File).into_iter().next()
}

/// Picks any number of files.
pub fn pick_files(title: &str) -> Vec<PathBuf> {
    platform::pick(title, None, Pick::Files)
}

/// Picks one folder.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    platform::pick(title, None, Pick::Folder).into_iter().next()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pick {
    File,
    Files,
    Folder,
}

/// The install picker the views use (`caskade_app::commands::install`):
/// these dialogs, off the async runtime's workers.
pub fn install_picker() -> caskade_app::commands::install_picker::InstallPicker {
    use std::sync::Arc;
    fn utf8(path: PathBuf) -> Result<String, String> {
        path.into_os_string()
            .into_string()
            .map_err(|_| caskade_core::tr!("selected path is not UTF-8"))
    }
    caskade_app::commands::install_picker::InstallPicker {
        files: Arc::new(|| {
            Box::pin(async {
                caskade_app::runtime::spawn_blocking(|| {
                    pick_files(&caskade_core::tr!("Choose games to install"))
                        .into_iter()
                        .map(utf8)
                        .collect()
                })
                .await
                .map_err(|error| error.to_string())?
            })
        }),
        folder: Arc::new(|| {
            Box::pin(async {
                caskade_app::runtime::spawn_blocking(|| {
                    pick_folder(&caskade_core::tr!("Choose game folder to install"))
                        .map(utf8)
                        .transpose()
                })
                .await
                .map_err(|error| error.to_string())?
            })
        }),
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{Kind, Pick};
    use gtk::prelude::*;
    use std::path::PathBuf;

    pub fn ask(title: &str, text: &str, kind: Kind, buttons: &[String]) -> Option<usize> {
        let (title, text, buttons) = (title.to_owned(), text.to_owned(), buttons.to_vec());
        crate::tray::on_gtk(move || {
            let dialog = gtk::MessageDialog::new(
                None::<&gtk::Window>,
                gtk::DialogFlags::MODAL,
                match kind {
                    Kind::Info => gtk::MessageType::Info,
                    Kind::Error => gtk::MessageType::Error,
                },
                gtk::ButtonsType::None,
                &text,
            );
            dialog.set_title(&title);
            for (index, label) in buttons.iter().enumerate() {
                dialog.add_button(label, gtk::ResponseType::Other(index as u16));
            }
            dialog.set_default_response(gtk::ResponseType::Other(0));
            let response = dialog.run();
            dialog.close();
            match response {
                gtk::ResponseType::Other(index) => Some(usize::from(index)),
                _ => None,
            }
        })
        .flatten()
    }

    pub fn pick(title: &str, filter: Option<(&str, &[&str])>, pick: Pick) -> Vec<PathBuf> {
        let title = title.to_owned();
        let filter = filter.map(|(name, extensions)| {
            (
                name.to_owned(),
                extensions
                    .iter()
                    .map(|extension| format!("*.{extension}"))
                    .collect::<Vec<_>>(),
            )
        });
        crate::tray::on_gtk(move || {
            let action = match pick {
                Pick::File | Pick::Files => gtk::FileChooserAction::Open,
                Pick::Folder => gtk::FileChooserAction::SelectFolder,
            };
            let dialog =
                gtk::FileChooserNative::new(Some(&title), None::<&gtk::Window>, action, None, None);
            dialog.set_select_multiple(pick == Pick::Files);
            if let Some((name, patterns)) = &filter {
                let gtk_filter = gtk::FileFilter::new();
                gtk_filter.set_name(Some(name));
                for pattern in patterns {
                    gtk_filter.add_pattern(pattern);
                }
                dialog.add_filter(gtk_filter);
            }
            let chosen = if dialog.run() == gtk::ResponseType::Accept {
                dialog.filenames()
            } else {
                Vec::new()
            };
            dialog.destroy();
            chosen
        })
        .unwrap_or_default()
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::{Kind, Pick};
    use std::path::PathBuf;

    pub fn ask(title: &str, text: &str, kind: Kind, buttons: &[String]) -> Option<usize> {
        let buttons_rfd = match buttons {
            [only] => rfd::MessageButtons::OkCustom(only.clone()),
            [first, second] => rfd::MessageButtons::OkCancelCustom(first.clone(), second.clone()),
            [first, second, third, ..] => {
                rfd::MessageButtons::YesNoCancelCustom(first.clone(), second.clone(), third.clone())
            }
            [] => rfd::MessageButtons::Ok,
        };
        let answer = rfd::MessageDialog::new()
            .set_title(title)
            .set_description(text)
            .set_level(match kind {
                Kind::Info => rfd::MessageLevel::Info,
                Kind::Error => rfd::MessageLevel::Error,
            })
            .set_buttons(buttons_rfd)
            .show();
        match answer {
            rfd::MessageDialogResult::Custom(label) => {
                buttons.iter().position(|button| *button == label)
            }
            rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes => Some(0),
            _ => None,
        }
    }

    pub fn pick(title: &str, filter: Option<(&str, &[&str])>, pick: Pick) -> Vec<PathBuf> {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some((name, extensions)) = filter {
            dialog = dialog.add_filter(name, extensions);
        }
        match pick {
            Pick::File => dialog.pick_file().into_iter().collect(),
            Pick::Files => dialog.pick_files().unwrap_or_default(),
            Pick::Folder => dialog.pick_folder().into_iter().collect(),
        }
    }
}
