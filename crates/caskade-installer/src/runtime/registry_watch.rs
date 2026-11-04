//! Host registry keys a native installer creates. While a `Run` step executes
//! on Windows, the key names one and two levels below `Software` are listed
//! before and after; each key that appeared is journalled with its subkeys
//! and the data of every value, so rollback and uninstall remove what the
//! installer made and keep what the user changed since. Only keys that are the
//! game's are recorded: a value names its folder, or the key is named like one
//! of its Uninstall registrations. A key another program creates during the
//! install is left alone, and so is one of the installer's that says neither.
//! Shared and system trees (`Microsoft`, `Classes`, `Policies`) are not
//! claimed; Uninstall registrations are recorded from the registry after the
//! install. Values an installer adds to a key that already existed are not
//! seen. Other hosts record nothing: a Wine/Proton prefix keeps its own
//! registry.
use crate::{
    exec::{JournalHandle, ProcessObservation, ProcessObserver, Result, StepContext},
    runtime::ProcessCommand,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct RegistryWatcher;

impl ProcessObserver for RegistryWatcher {
    fn start(
        &self,
        #[cfg_attr(not(windows), allow(unused_variables))] command: &ProcessCommand,
        _: &JournalHandle,
        #[cfg_attr(not(windows), allow(unused_variables))] context: &StepContext,
    ) -> Result<Box<dyn ProcessObservation>> {
        #[cfg(windows)]
        {
            use crate::manifest::native::{VIEWS, software_keys};
            // Best effort, like the file watcher: a registry that can't be
            // listed leaves the install unrecorded there, not failed.
            let before = VIEWS
                .iter()
                .map(|(machine, bits64)| software_keys(*machine, *bits64))
                .collect::<std::io::Result<Vec<_>>>();
            match before {
                Ok(before) => {
                    return Ok(Box::new(Observation {
                        before,
                        game: context.final_destination_path(std::path::Path::new("."))?,
                        args: command.args.clone(),
                    }));
                }
                Err(error) => tracing::warn!(%error, "registry not watched"),
            }
        }
        Ok(Box::new(Unwatched))
    }
}

struct Unwatched;
impl ProcessObservation for Unwatched {
    fn finish(self: Box<Self>, _: &JournalHandle) -> Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
struct Observation {
    before: Vec<std::collections::BTreeSet<String>>,
    /// The final game folder and the installer's arguments (the folder it
    /// was given may be an alias): what ties a new key to this game.
    game: std::path::PathBuf,
    args: Vec<String>,
}

#[cfg(windows)]
impl ProcessObservation for Observation {
    fn finish(self: Box<Self>, journal: &JournalHandle) -> Result<()> {
        use crate::{
            exec::JournalEntry,
            manifest::native::{Owner, VIEWS, created_effects, software_keys},
        };
        // After the run: the game's Uninstall registrations exist now.
        let owner = Owner::new(&self.game, &self.args);
        for ((machine, bits64), before) in VIEWS.iter().zip(self.before) {
            let effects = software_keys(*machine, *bits64).and_then(|after| {
                created_effects(
                    *machine,
                    *bits64,
                    &after.difference(&before).cloned().collect(),
                    &owner,
                )
            });
            match effects {
                Ok(effects) => {
                    for effect in effects {
                        journal.record(JournalEntry::Registry { effect })?;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, machine, bits64, "registry changes not recorded")
                }
            }
        }
        Ok(())
    }
}
