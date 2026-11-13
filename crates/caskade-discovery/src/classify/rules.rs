use super::{Classification, Metadata, Verdict};
use crate::{ClassifierConfig, Error, Evidence, data::Data, sources::normalized};
use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf, sync::OnceLock};

struct EngineRule {
    id: String,
    patterns: Vec<GlobMatcher>,
}
#[derive(Deserialize)]
struct DenyList {
    exclude_prefixes: Vec<String>,
    exclude_contains: Vec<String>,
    exclude_directories: Vec<String>,
}
#[derive(Deserialize)]
struct Redists {
    redist: Vec<Redist>,
}
#[derive(Deserialize)]
struct Redist {
    files: Vec<String>,
    #[serde(default)]
    product_names: Vec<String>,
}
struct RedistRule {
    files: Vec<GlobMatcher>,
    product_names: Vec<String>,
}
struct Rules {
    engines: Vec<EngineRule>,
    deny: DenyList,
    redists: Vec<RedistRule>,
}
fn matcher(pattern: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(pattern)
        .case_insensitive(true)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| e.to_string())
}
fn rules() -> crate::Result<&'static Rules> {
    static RULES: OnceLock<Result<Rules, String>> = OnceLock::new();
    RULES
        .get_or_init(|| {
            let data = Data::builtin().map_err(|e| e.to_string())?;
            let engines = data
                .engines
                .engines
                .iter()
                .map(|engine| {
                    Ok(EngineRule {
                        id: engine.id.clone(),
                        patterns: engine
                            .files
                            .iter()
                            .map(|p| matcher(p))
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<_, String>>()?;
            let deny = toml::from_str(include_str!(
                "../../../caskade-installer/data/exe-denylist.toml"
            ))
            .map_err(|e| e.to_string())?;
            let redists: Redists =
                toml::from_str(include_str!("../../../caskade-installer/data/redists.toml"))
                    .map_err(|e| e.to_string())?;
            let redists = redists
                .redist
                .into_iter()
                .map(|r| {
                    Ok(RedistRule {
                        files: r
                            .files
                            .iter()
                            .map(|p| matcher(p))
                            .collect::<Result<_, _>>()?,
                        product_names: r
                            .product_names
                            .into_iter()
                            .map(|s| s.to_lowercase())
                            .collect(),
                    })
                })
                .collect::<Result<_, String>>()?;
            Ok(Rules {
                engines,
                deny,
                redists,
            })
        })
        .as_ref()
        .map_err(|e| Error::Usage(format!("invalid classifier rules: {e}")))
}
fn location(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let path = if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    };
    let path = path.trim_end_matches('/');
    if path.as_bytes().get(1) == Some(&b':') || path.starts_with("//") {
        path.to_lowercase()
    } else {
        path.to_owned()
    }
}
fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn add(evidence: &mut Vec<Evidence>, score: &mut i32, rule: &str, delta: i32) {
    *score += delta;
    evidence.push(Evidence::Rule {
        rule: rule.to_owned(),
        score: delta,
    });
}
fn under(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|tail| tail.starts_with('/'))
}
fn category(
    title: &str,
    executable: &str,
    metadata: &Metadata,
    rules: &Rules,
    data: &Data,
) -> Option<&'static str> {
    let stem = executable.strip_suffix(".exe").unwrap_or(executable);
    let root_name = metadata
        .install_dir
        .as_ref()
        .map(|p| normalized(&p.to_string_lossy()))
        .unwrap_or_default();
    let root_leaf = leaf(&root_name);
    if rules
        .deny
        .exclude_directories
        .iter()
        .any(|d| root_leaf == d)
        || rules.redists.iter().any(|redist| {
            redist
                .product_names
                .iter()
                .any(|prefix| title.starts_with(prefix))
                || redist.files.iter().any(|m| m.is_match(executable))
        })
    {
        return Some("category.redist");
    }
    if data.launchers.launchers.iter().any(|launcher| {
        title == launcher.name.to_lowercase()
            || (!metadata.category.as_deref().is_some_and(|c| {
                c.split(|c: char| !c.is_alphanumeric())
                    .any(|p| p.eq_ignore_ascii_case("game") || p.eq_ignore_ascii_case("games"))
            }) && launcher
                .exes
                .iter()
                .any(|exe| !executable.is_empty() && executable == exe.to_lowercase()))
    }) {
        return Some("category.launcher");
    }
    if [
        "retroarch",
        "dolphin",
        "pcsx2",
        "rpcs3",
        "yuzu",
        "ryujinx",
        "citra",
        "ppsspp",
        "mame",
        "duckstation",
        "cemu",
    ]
    .iter()
    .any(|name| title == *name || stem == *name)
    {
        return Some("category.emulator");
    }
    if title.contains("redistributable")
        || title.contains(".net runtime")
        || title.contains("desktop runtime")
        || title.contains("c++ runtime")
    {
        return Some("category.redist");
    }
    if [
        "graphics driver",
        "display driver",
        "driver package",
        "driver update",
        "chipset software",
        "audio driver",
        "network driver",
    ]
    .iter()
    .any(|text| title.contains(text))
    {
        return Some("category.driver");
    }
    if metadata.category.as_deref().is_some_and(|category| {
        category.split(|c: char| !c.is_alphanumeric()).any(|part| {
            part.eq_ignore_ascii_case("development")
                || part.eq_ignore_ascii_case("ide")
                || part.eq_ignore_ascii_case("sdk")
        })
    }) {
        return Some("category.sdk");
    }
    if title
        .split(|c: char| !c.is_alphanumeric())
        .any(|s| s == "sdk")
        || title.contains("software development kit")
        || title.contains("unity editor")
        || title.contains("unreal engine")
        || title.contains("game maker studio")
        || title.contains("gamemaker studio")
        || title.contains("godot engine")
    {
        return Some("category.sdk");
    }
    if [
        "google chrome",
        "mozilla firefox",
        "microsoft edge",
        "discord",
        "slack",
        "obs studio",
        "visual studio code",
        "blender",
        "7-zip",
        "winrar",
        "notepad++",
        "python",
        "node.js",
        "docker desktop",
    ]
    .iter()
    .any(|name| {
        title == *name
            || title
                .strip_prefix(name)
                .is_some_and(|tail| tail.starts_with(' '))
    }) {
        return Some("category.tool");
    }
    if [
        "chrome",
        "firefox",
        "msedge",
        "discord",
        "slack",
        "obs64",
        "obs32",
        "code",
        "blender",
        "7z",
        "winrar",
        "notepad++",
        "python",
        "node",
    ]
    .contains(&stem)
    {
        return Some("category.tool");
    }
    if !stem.is_empty()
        && (rules
            .deny
            .exclude_prefixes
            .iter()
            .any(|s| stem.starts_with(s))
            || rules.deny.exclude_contains.iter().any(|s| stem.contains(s)))
    {
        return Some("category.tool");
    }
    None
}
pub(super) fn validate(config: &ClassifierConfig) -> crate::Result<()> {
    if config.maybe_score < 0
        || config.likely_score <= config.maybe_score
        || config.likely_score > 1000
    {
        return Err(Error::Usage(
            "classifier requires 0 <= maybe_score < likely_score <= 1000".into(),
        ));
    }
    if config.max_entries == 0 || config.max_entries > 1_000_000 || config.max_depth > 128 {
        return Err(Error::Usage(
            "classifier requires 1..=1000000 entries and depth <=128".into(),
        ));
    }
    Ok(())
}
pub(super) fn classify(
    metadata: &Metadata,
    files: &[String],
    config: &ClassifierConfig,
) -> crate::Result<Classification> {
    validate(config)?;
    if files.len() > config.max_entries as usize {
        return Err(Error::Usage(
            "classifier inventory exceeds entry budget".into(),
        ));
    }
    let mut paths = BTreeSet::new();
    for path in files {
        if path.len() > 4096 {
            return Err(Error::Usage(
                "classifier inventory path exceeds 4096 bytes".into(),
            ));
        }
        let path = normalized(path);
        if path.starts_with('/')
            || path.split('/').any(|p| p == "..")
            || path.contains(':')
            || path.contains('\0')
        {
            return Err(Error::Usage(
                "classifier inventory paths must be relative".into(),
            ));
        }
        paths.insert(path);
    }
    let rules = rules()?;
    let data = Data::builtin()?;
    let mut score = 0;
    let mut evidence = Vec::new();
    let mut matched_engine = false;
    let mut matched_store = false;
    for engine in &rules.engines {
        let matched = engine
            .patterns
            .iter()
            .map(|pattern| paths.iter().find(|path| pattern.is_match(path)))
            .collect::<Option<Vec<_>>>();
        if let Some(matched) = matched {
            let store = engine.id.starts_with("store-");
            // Multiple layouts/SDKs corroborate one family, without stacking
            // dozens of weak SDK or engine scores above the Likely threshold.
            let delta = if store {
                if matched_store {
                    0
                } else {
                    matched_store = true;
                    35
                }
            } else if matched_engine {
                0
            } else {
                matched_engine = true;
                70
            };
            add(
                &mut evidence,
                &mut score,
                &format!("fingerprint.{}", engine.id),
                delta,
            );
            for path in matched {
                evidence.push(Evidence::File {
                    path: PathBuf::from(path),
                });
            }
        }
    }
    let title = metadata.title.as_deref().unwrap_or("").to_lowercase();
    let publisher = metadata
        .publisher
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if data
        .publishers
        .games
        .iter()
        .any(|p| publisher == p.to_lowercase())
    {
        add(&mut evidence, &mut score, "publisher.game", 15);
    }
    if data
        .publishers
        .non_games
        .iter()
        .any(|p| publisher == p.to_lowercase())
    {
        add(&mut evidence, &mut score, "publisher.non_game", -25);
    }
    if metadata.category.as_deref().is_some_and(|c| {
        c.split(|c: char| !c.is_alphanumeric())
            .any(|part| part.eq_ignore_ascii_case("game") || part.eq_ignore_ascii_case("games"))
    }) {
        add(&mut evidence, &mut score, "metadata.game_category", 65);
    }
    if paths.iter().any(|path| {
        let file = leaf(path);
        file.starts_with("d3d") && file.ends_with(".dll")
            || file.starts_with("vulkan") && (file.ends_with(".dll") || file.contains(".so"))
            || file.starts_with("xinput") && file.ends_with(".dll")
            || file.starts_with("sdl") && file.ends_with(".dll")
            || file.starts_with("libsdl") && (file.contains(".so") || file.ends_with(".dylib"))
    }) {
        add(&mut evidence, &mut score, "library.graphics_or_input", 8);
    }
    if let Some(dir) = &metadata.install_dir {
        let dir = location(dir);
        if metadata
            .known_game_folders
            .iter()
            .any(|folder| under(&dir, &location(folder)))
        {
            add(&mut evidence, &mut score, "path.known_game_folder", 10);
        }
    }
    let executable = metadata
        .executable
        .as_ref()
        .map(|p| normalized(&p.to_string_lossy()))
        .unwrap_or_default();
    let executable = leaf(&executable);
    if let Some(category) = category(&title, executable, metadata, rules, data) {
        add(&mut evidence, &mut score, category, -100);
        if score >= config.maybe_score {
            let delta = config.maybe_score - 1 - score;
            add(&mut evidence, &mut score, "safety.category_cap", delta);
        }
    }
    let verdict = if score >= config.likely_score {
        Verdict::Likely
    } else if score >= config.maybe_score {
        Verdict::Maybe
    } else {
        Verdict::NotAGame
    };
    Ok(Classification {
        score,
        verdict,
        evidence,
        launch: None,
        warnings: Vec::new(),
    })
}
