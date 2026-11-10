//! Installed themes (#139): the built-in ones (`caskade.default`
//! first), folders under `<data>/themes/`, and folders plugins bundle. Each
//! one is checked with `caskade-theme`; a broken theme is listed with its
//! problems and never hides the others.
//!
//! The active theme (#140) is a theme id in `core.appearance`; [`ThemeService::resolve`]
//! falls back to `caskade.default`, with a notice, when it went missing or
//! broke. In dev mode a theme folder is loaded in place and recompiled on
//! every change ([`ThemeService::watch_dev`]), like a dev plugin (#131).
//!
//! A theme's name and description show in the language in use, from its
//! own `locales/<lang>.po` (#219); a catalog that can't be read is one of
//! its `warnings`, and its English shows.
use crate::{Error, Result, read, write};
use caskade_theme::{DEFAULT_THEME, DirFiles, Files, Locales, MemFiles, Problem, Theme};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use specta::Type;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

/// Where a theme was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSource {
    Builtin,
    User,
    Plugin,
    /// A folder loaded in place in dev mode, watched for changes.
    Dev,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeProblem {
    /// Relative to the theme folder.
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}

impl From<Problem> for ThemeProblem {
    fn from(problem: Problem) -> Self {
        Self {
            file: problem.file,
            line: problem.line,
            column: problem.column,
            message: problem.message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub extends: Option<String>,
    /// Colour schemes it provides, its default first.
    pub modes: Vec<String>,
    /// Preview images, relative to the theme folder (the app's
    /// `themes_list` turns them into URLs).
    pub previews: Vec<String>,
    pub source: ThemeSource,
    /// The folder on disk; none for built-in themes.
    pub folder: Option<String>,
    /// Why it can't be applied; empty when it can.
    pub problems: Vec<ThemeProblem>,
    /// Problems that don't stop it applying (#219): a translation catalog
    /// in `locales/` that can't be read, whose English shows instead.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[specta(optional)]
    pub warnings: Vec<ThemeProblem>,
}

/// The theme to apply for the one the settings name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTheme {
    /// The requested theme, or `caskade.default` when it can't be applied.
    pub id: String,
    pub name: String,
    /// The id the settings name.
    pub requested: String,
    /// Colour schemes it provides, its default first.
    pub modes: Vec<String>,
    /// Why the requested theme isn't the one applied.
    pub notice: Option<String>,
}

struct Entry {
    /// As `theme.json` has it, in English.
    info: ThemeInfo,
    files: Arc<dyn Files>,
    theme: Option<Theme>,
    /// Its name and description in other languages (#219).
    locales: Locales,
}

impl Entry {
    /// [`Self::info`] with the name and description in the language in use.
    fn translated(&self) -> ThemeInfo {
        let locale = crate::i18n::locale();
        let mut info = self.info.clone();
        info.name = self.locales.text(&locale, &info.name);
        info.description = info
            .description
            .map(|description| self.locales.text(&locale, &description));
        info
    }
}

struct State {
    user_folder: PathBuf,
    plugin_folders: Vec<PathBuf>,
    dev_folders: Vec<PathBuf>,
    entries: Vec<Entry>,
    compiled: HashMap<String, Arc<str>>,
}

/// The themes the app can apply.
#[derive(Clone)]
pub struct ThemeService {
    state: Arc<RwLock<State>>,
    dev: Arc<Mutex<Vec<DevThemeWatch>>>,
}

/// What a dev theme's watch task holds, so it never keeps the service alive.
type WeakService = (Weak<RwLock<State>>, Weak<Mutex<Vec<DevThemeWatch>>>);

/// A built-in theme: the files of `themes/<folder>` in the repository,
/// embedded in the binary.
macro_rules! builtin_theme {
    ($folder:literal: $($file:literal),+ $(,)?) => {
        MemFiles::new([$((
            $file,
            &include_bytes!(concat!("../../../themes/", $folder, "/", $file))[..],
        )),+])
    };
}

/// The built-in themes, `caskade.default` first. A new one is a line here
/// listing its folder and every file in it (previews and assets too).
fn builtins() -> Vec<MemFiles> {
    vec![
        builtin_theme!(
            "default": "theme.json",
            "tokens.json",
            "theme.css",
            "locales/sv.po",
        ),
        // Deep cobalt and white, in the spirit of a console home screen (#125).
        builtin_theme!(
            "cobalt-five": "theme.json",
            "tokens.json",
            "theme.css",
            "assets/fonts/OFL.txt",
            "assets/fonts/manrope-latin-ext-wght-normal.woff2",
            "assets/fonts/manrope-latin-wght-normal.woff2",
            "locales/sv.po",
            "previews/home.webp",
        ),
        // Near-black and emerald, in the spirit of a console dashboard (#125).
        builtin_theme!(
            "emerald-one": "theme.json",
            "tokens.json",
            "theme.css",
            "assets/fonts/OFL.txt",
            "assets/fonts/barlow-semi-condensed-latin-500-normal.woff2",
            "assets/fonts/barlow-semi-condensed-latin-600-normal.woff2",
            "assets/fonts/barlow-semi-condensed-latin-700-normal.woff2",
            "assets/fonts/barlow-semi-condensed-latin-ext-500-normal.woff2",
            "assets/fonts/barlow-semi-condensed-latin-ext-600-normal.woff2",
            "assets/fonts/barlow-semi-condensed-latin-ext-700-normal.woff2",
            "locales/sv.po",
            "previews/home.webp",
        ),
        // Bright white with red and teal, in the spirit of a playful console home screen (#125).
        builtin_theme!(
            "toggle-two": "theme.json",
            "tokens.json",
            "theme.css",
            "assets/fonts/OFL.txt",
            "assets/fonts/nunito-latin-ext-wght-normal.woff2",
            "assets/fonts/nunito-latin-wght-normal.woff2",
            "locales/sv.po",
            "previews/home.webp",
        ),
    ]
}

/// Themes that were built in and left the built-in set: Paper went to the
/// catalogue (#125, as `dev.caskade.paper`). Someone who had one chosen
/// gets the default theme without a notice, and [`retire`] moves their
/// setting there.
pub const RETIRED: &[&str] = &["caskade.paper"];

/// Moves a `core.appearance` theme choice that's [`RETIRED`] (and not
/// installed some other way) to the default theme, as the app starts;
/// whether it did.
pub async fn retire(
    settings: &crate::settings::SettingsService,
    themes: &ThemeService,
) -> std::result::Result<bool, crate::settings::SettingsError> {
    let current = settings.get("core.appearance").await?;
    let Some(theme) = current.value.get("theme").and_then(|theme| theme.as_str()) else {
        return Ok(false);
    };
    if !RETIRED.contains(&theme) || themes.info(theme).is_some() {
        return Ok(false);
    }
    tracing::info!(
        theme,
        "the chosen theme isn't built in any more: using the default"
    );
    let mut value = current.value.clone();
    value["theme"] = DEFAULT_THEME.into();
    settings
        .set("core.appearance", value, Some(current.revision))
        .await?;
    Ok(true)
}

/// How long a dev theme's folder must be quiet before it is recompiled:
/// editors write several files per save.
pub const DEV_DEBOUNCE: Duration = Duration::from_millis(200);

/// A dev theme folder being watched; dropping it stops watching.
struct DevThemeWatch {
    folder: PathBuf,
    _watcher: notify::RecommendedWatcher,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for DevThemeWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ThemeService {
    /// Themes from the built-in set and the subfolders of `user_folder`.
    pub fn new(user_folder: impl Into<PathBuf>) -> Self {
        let service = Self {
            state: Arc::new(RwLock::new(State {
                user_folder: user_folder.into(),
                plugin_folders: Vec::new(),
                dev_folders: Vec::new(),
                entries: Vec::new(),
                compiled: HashMap::new(),
            })),
            dev: Arc::new(Mutex::new(Vec::new())),
        };
        service.reload();
        service
    }

    /// Theme folders bundled by enabled plugins (`contributes.themes`);
    /// reloads when they changed.
    pub fn set_plugin_folders(&self, folders: Vec<PathBuf>) {
        if read(&self.state).plugin_folders == folders {
            return;
        }
        write(&self.state).plugin_folders = folders;
        self.reload();
    }

    /// Reads every theme again, e.g. after one was added or edited.
    pub fn reload(&self) {
        let (user_folder, plugin_folders, dev_folders) = {
            let state = read(&self.state);
            (
                state.user_folder.clone(),
                state.plugin_folders.clone(),
                state.dev_folders.clone(),
            )
        };
        // Dev folders come first, so a theme being worked on wins its id
        // over an installed copy (or the built-in it is a checkout of).
        let mut entries: Vec<Entry> = dev_folders
            .iter()
            .map(|folder| {
                entry(
                    Arc::new(DirFiles::new(folder)),
                    ThemeSource::Dev,
                    Some(folder.as_path()),
                )
            })
            .collect();
        entries.extend(
            builtins()
                .into_iter()
                .map(|files| entry(Arc::new(files), ThemeSource::Builtin, None)),
        );
        for folder in subfolders(&user_folder) {
            entries.push(entry(
                Arc::new(DirFiles::new(&folder)),
                ThemeSource::User,
                Some(&folder),
            ));
        }
        for folder in plugin_folders {
            entries.push(entry(
                Arc::new(DirFiles::new(&folder)),
                ThemeSource::Plugin,
                Some(&folder),
            ));
        }
        // The first theme with an id keeps it.
        for at in 1..entries.len() {
            let (before, rest) = entries.split_at_mut(at);
            let entry = &mut rest[0];
            if let Some(first) = before.iter().find(|other| other.info.id == entry.info.id) {
                entry.theme = None;
                entry.info.problems.push(ThemeProblem {
                    file: "theme.json".into(),
                    line: None,
                    column: None,
                    message: format!(
                        "id {} is already used by the {} theme {}",
                        entry.info.id,
                        source_name(first.info.source),
                        first.info.name
                    ),
                });
            }
        }
        // A theme that extends a missing or broken one can't be applied.
        let broken: Vec<(usize, String)> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.theme.is_some())
            .filter_map(|(at, entry)| {
                caskade_theme::chain(&entry.info.id, |id| lookup(&entries, id))
                    .err()
                    .map(|error| (at, error))
            })
            .collect();
        for (at, error) in broken {
            entries[at].theme = None;
            entries[at].info.problems.push(ThemeProblem {
                file: "theme.json".into(),
                line: None,
                column: None,
                message: error,
            });
        }
        for entry in &entries {
            if !entry.info.problems.is_empty() {
                tracing::warn!(theme = %entry.info.id, problems = entry.info.problems.len(), "theme can't be applied");
            }
        }
        let mut state = write(&self.state);
        state.entries = entries;
        state.compiled.clear();
    }

    /// The theme to apply for `requested` (`core.appearance.theme`): it, when
    /// it is installed and compiles, otherwise `caskade.default` with a
    /// notice saying why. The setting itself is left alone, so the theme
    /// comes back once it is reinstalled or fixed.
    pub fn resolve(&self, requested: &str) -> ActiveTheme {
        let applies = self.stylesheet(requested);
        let falls_back = applies.is_err();
        let notice = match applies {
            Ok(_) => None,
            Err(_) if requested == DEFAULT_THEME => None,
            // A theme that left the built-in set: the default, quietly.
            Err(_) if RETIRED.contains(&requested) && self.info(requested).is_none() => None,
            Err(error) => Some(match self.info(requested) {
                None => "The chosen theme isn't installed any more. Caskade is using the default theme instead.".to_owned(),
                Some(info) => format!(
                    "The theme {} can't be applied: {}. Caskade is using the default theme instead.",
                    info.name,
                    info.problems
                        .first()
                        .map_or_else(|| error.to_string(), |problem| problem.message.clone())
                ),
            }),
        };
        let id = if falls_back {
            if notice.is_some() {
                tracing::warn!(theme = %requested, "selected theme can't be applied");
            }
            DEFAULT_THEME
        } else {
            requested
        };
        let info = self.info(id);
        ActiveTheme {
            id: id.to_owned(),
            name: info
                .as_ref()
                .map_or_else(|| id.to_owned(), |info| info.name.clone()),
            requested: requested.to_owned(),
            modes: info
                .map(|info| info.modes)
                .filter(|modes| !modes.is_empty())
                .unwrap_or_else(|| vec!["dark".to_owned()]),
            notice,
        }
    }

    /// The theme that owns `id` (the first one listed with it).
    fn info(&self, id: &str) -> Option<ThemeInfo> {
        read(&self.state)
            .entries
            .iter()
            .find(|entry| entry.info.id == id)
            .map(Entry::translated)
    }

    /// Loads `folder` in place as a dev theme and recompiles it on every
    /// change: after a quiet period of `debounce`, the themes are read again
    /// and `changed` gets the theme's id and, when it no longer compiles,
    /// why. A folder already watched is not watched twice. Returns the
    /// theme's id. Must be called inside a tokio runtime.
    pub fn watch_dev(
        &self,
        folder: &Path,
        debounce: Duration,
        changed: impl Fn(&str, Option<String>) + Send + Sync + 'static,
    ) -> Result<String> {
        let folder = folder.canonicalize()?;
        let mut watches = crate::lock(&self.dev);
        if !watches.iter().any(|watch| watch.folder == folder) {
            let (sender, mut changes) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
            let mut watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    if let Ok(event) = event
                        && !matches!(event.kind, notify::EventKind::Access(_))
                    {
                        for path in event.paths {
                            let _ = sender.send(path);
                        }
                    }
                })
                .map_err(|error| Error::Invalid(error.to_string()))?;
            watcher
                .watch(&folder, RecursiveMode::Recursive)
                .map_err(|error| Error::Invalid(error.to_string()))?;
            let weak = self.downgrade();
            let watched = folder.clone();
            let task = tokio::spawn(async move {
                while let Some(first) = changes.recv().await {
                    let mut paths = BTreeSet::from([first]);
                    // Gather the rest of this save.
                    while let Ok(Some(path)) = tokio::time::timeout(debounce, changes.recv()).await
                    {
                        paths.insert(path);
                    }
                    let Some(service) = Self::upgrade(&weak) else {
                        break;
                    };
                    service.reload();
                    let id = service.dev_id(&watched);
                    let error = service.stylesheet(&id).err().map(|error| error.to_string());
                    if let Some(error) = &error {
                        tracing::warn!(theme = %id, %error, "dev theme is broken");
                    } else {
                        tracing::info!(theme = %id, files = paths.len(), "dev theme recompiled");
                    }
                    changed(&id, error);
                }
            });
            watches.push(DevThemeWatch {
                folder: folder.clone(),
                _watcher: watcher,
                task,
            });
            tracing::info!(folder = %folder.display(), "watching dev theme");
        }
        drop(watches);
        {
            let mut state = write(&self.state);
            if !state.dev_folders.contains(&folder) {
                state.dev_folders.push(folder.clone());
            }
        }
        self.reload();
        Ok(self.dev_id(&folder))
    }

    /// The id of the dev theme in `folder`.
    fn dev_id(&self, folder: &Path) -> String {
        let shown = folder.display().to_string();
        read(&self.state)
            .entries
            .iter()
            .find(|entry| {
                entry.info.source == ThemeSource::Dev
                    && entry.info.folder.as_deref() == Some(shown.as_str())
            })
            .map_or_else(|| format!("folder:{shown}"), |entry| entry.info.id.clone())
    }

    fn downgrade(&self) -> WeakService {
        (Arc::downgrade(&self.state), Arc::downgrade(&self.dev))
    }

    fn upgrade((state, dev): &WeakService) -> Option<Self> {
        Some(Self {
            state: state.upgrade()?,
            dev: dev.upgrade()?,
        })
    }

    /// Why the theme folder at `folder` can't be applied; empty when it can.
    /// Reads it without adding it (a catalogue package before it installs,
    /// #182).
    pub fn check_folder(folder: &Path) -> Vec<ThemeProblem> {
        match Theme::load(&DirFiles::new(folder)) {
            Ok(_) => Vec::new(),
            Err(problems) => problems.into_iter().map(ThemeProblem::from).collect(),
        }
    }

    /// Every theme, names and descriptions in the language in use.
    pub fn list(&self) -> Vec<ThemeInfo> {
        read(&self.state)
            .entries
            .iter()
            .map(Entry::translated)
            .collect()
    }

    /// The compiled stylesheet of a theme and everything it extends.
    pub fn stylesheet(&self, id: &str) -> Result<Arc<str>> {
        if let Some(css) = read(&self.state).compiled.get(id) {
            return Ok(css.clone());
        }
        let css: Arc<str> = {
            let state = read(&self.state);
            let entry = state
                .entries
                .iter()
                .find(|entry| entry.info.id == id)
                .ok_or_else(|| Error::Invalid(crate::tr!("This theme isn't installed.")))?;
            if entry.theme.is_none() {
                let first = entry
                    .info
                    .problems
                    .first()
                    .map(|problem| problem.message.as_str());
                return Err(Error::Invalid(match first {
                    Some(problem) => crate::tr!(
                        "The theme {name} can't be applied: {problem}",
                        name = entry.info.name,
                        problem = problem
                    ),
                    None => crate::tr!(
                        "The theme {name} can't be applied: it is invalid",
                        name = entry.info.name
                    ),
                }));
            }
            let chain = caskade_theme::chain(id, |id| lookup(&state.entries, id))
                .map_err(Error::Invalid)?;
            caskade_theme::compile(&chain).into()
        };
        write(&self.state)
            .compiled
            .insert(id.to_owned(), css.clone());
        Ok(css)
    }

    /// An asset or preview of a valid theme.
    pub fn asset(&self, id: &str, path: &str) -> Option<Vec<u8>> {
        let state = read(&self.state);
        let entry = state.entries.iter().find(|entry| entry.info.id == id)?;
        let theme = entry.theme.as_ref()?;
        caskade_theme::servable(theme, path)
            .then(|| entry.files.read(path))
            .flatten()
    }
}

fn lookup<'a>(entries: &'a [Entry], id: &str) -> Option<&'a Theme> {
    entries
        .iter()
        .find(|entry| entry.info.id == id)
        .and_then(|entry| entry.theme.as_ref())
}

fn source_name(source: ThemeSource) -> &'static str {
    match source {
        ThemeSource::Builtin => "built-in",
        ThemeSource::User => "installed",
        ThemeSource::Plugin => "plugin",
        ThemeSource::Dev => "dev",
    }
}

fn subfolders(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

fn entry(files: Arc<dyn Files>, source: ThemeSource, folder: Option<&Path>) -> Entry {
    let folder_name = folder
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| DEFAULT_THEME.to_owned());
    let (theme, problems) = match Theme::load(files.as_ref()) {
        Ok(theme) => (Some(theme), Vec::new()),
        Err(problems) => (None, problems.into_iter().map(ThemeProblem::from).collect()),
    };
    let (locales, warnings) = Locales::load(files.as_ref());
    let warnings: Vec<ThemeProblem> = warnings.into_iter().map(ThemeProblem::from).collect();
    // A theme with a broken theme.json is still listed, by its folder name.
    let manifest = theme
        .as_ref()
        .map(|theme| theme.manifest.clone())
        .or_else(|| {
            files.read("theme.json").and_then(|bytes| {
                serde_json::from_slice::<caskade_theme::ThemeManifest>(&bytes).ok()
            })
        });
    let info = match manifest {
        Some(manifest) => ThemeInfo {
            id: manifest.id,
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            author: manifest.author,
            extends: manifest.extends,
            modes: manifest.modes,
            previews: manifest.previews,
            source,
            folder: folder.map(|folder| folder.display().to_string()),
            problems,
            warnings,
        },
        None => ThemeInfo {
            id: format!("folder:{folder_name}"),
            name: folder_name,
            version: String::new(),
            description: None,
            author: None,
            extends: None,
            modes: Vec::new(),
            previews: Vec::new(),
            source,
            folder: folder.map(|folder| folder.display().to_string()),
            problems,
            warnings,
        },
    };
    Entry {
        info,
        files,
        theme,
        locales,
    }
}

#[cfg(test)]
#[path = "themes_tests.rs"]
mod tests;
