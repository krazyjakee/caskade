//! Installing a port as another release of the game: the plan's release
//! file for this system, checked against the catalogue's SHA-256, with the
//! user's game file copied in or passed along as the plan says.

use super::{Host, LibraryGame, Ports, Project, Target, this_system};
use crate::{
    Error, Result,
    db::LibraryStore,
    emulation::{GameRom, RomLocation},
    install::{AttachTo, InstallService, JobSnapshot, PlacedFile},
};
use caskade_installer::discover::DiscoveryHints;
use std::{collections::BTreeSet, path::PathBuf};

/// What installing a port downloads and does afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInstall {
    /// The release file's address, with `#sha256=` when the catalogue
    /// knows it.
    pub url: String,
    pub hints: DiscoveryHints,
    pub attach: AttachTo,
}

impl Ports {
    /// Installs `project` for library game `game_id` into `root_id` (or the
    /// default root) and returns the install job.
    pub async fn install(
        &self,
        store: &LibraryStore,
        installs: &InstallService,
        game_id: &str,
        project: &str,
        root_id: Option<String>,
    ) -> Result<JobSnapshot> {
        let plan = self.plan_install(store, game_id, project).await?;
        installs
            .install_attached(plan.url, root_id, plan.hints, plan.attach)
            .await
    }

    /// What [`install`](Self::install) would do.
    pub async fn plan_install(
        &self,
        store: &LibraryStore,
        game_id: &str,
        project: &str,
    ) -> Result<PortInstall> {
        let catalogue = self
            .catalogue()
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("the port catalogue isn't downloaded yet")))?;
        let project = catalogue
            .projects
            .iter()
            .find(|candidate| candidate.key == project)
            .ok_or_else(|| {
                Error::Invalid(crate::tr!("This port is no longer in the catalogue."))
            })?;
        let game = store
            .game(game_id)
            .await?
            .ok_or_else(|| Error::Invalid(crate::tr!("This game is no longer in the library.")))?;
        let entries = store.entries_for_game(game_id).await?;
        let library = LibraryGame::new(&game, &entries);
        let system = this_system()
            .ok_or_else(|| Error::Invalid(crate::tr!("ports don't install on this system")))?;
        let (port, target) = project
            .games
            .iter()
            .filter(|port| !port.families.is_disjoint(&library.families))
            .find_map(|port| {
                let target = port.targets.get(system)?;
                target.installable().then_some((port, target))
            })
            .ok_or_else(|| {
                Error::Invalid(crate::tr!(
                    "{port} has no release for this system",
                    port = project.repo.clone()
                ))
            })?;
        let url = release_url(project, target)?;
        let roms = rom_paths(store, &library, &port.families).await?;
        let mut attach = AttachTo {
            game_id: game.id.clone(),
            executable: target.executable.clone(),
            ..Default::default()
        };
        for file in target.files.iter().filter(|file| file.kind == "game") {
            match file.how.as_str() {
                "folder" => {
                    let into = PathBuf::from(file.drop_into.clone().unwrap_or_default());
                    // A plan that names the file wants that one name; with
                    // several discs each keeps its own.
                    let name = file
                        .expected_filename
                        .clone()
                        .filter(|_| roms.len() == 1)
                        .filter(|name| !name.contains(['/', '\\', '*', '?']));
                    attach.files.extend(roms.iter().map(|rom| PlacedFile {
                        source: rom.clone(),
                        into: into.clone(),
                        name: name.clone(),
                    }));
                }
                "command_line" => {
                    if let Some(rom) = roms.first() {
                        attach.args.push(rom.to_string_lossy().into_owned());
                    }
                }
                _ => {}
            }
        }
        Ok(PortInstall {
            url,
            hints: DiscoveryHints {
                installer_title: Some(project.repo.clone()),
                source_name: Some(project.repo.clone()),
                source_url: url::Url::parse(&project.url()).ok(),
                ..Default::default()
            },
            attach,
        })
    }
}

/// The release file's address on GitHub, with its SHA-256 as a fragment
/// the download checks.
pub(super) fn release_url(project: &Project, target: &Target) -> Result<String> {
    if project.host != Host::GitHub {
        return Err(Error::Invalid(crate::tr!(
            "only ports on GitHub install from Caskade"
        )));
    }
    let (Some(tag), Some(asset)) = (&target.tag, &target.asset) else {
        return Err(Error::Invalid(crate::tr!("the plan names no release file")));
    };
    let mut url = url::Url::parse("https://github.com/").expect("valid base");
    url.path_segments_mut()
        .map_err(|()| Error::Invalid("GitHub URL cannot be a base".into()))?
        .clear()
        .extend([
            project.owner.as_str(),
            project.repo.as_str(),
            "releases",
            "download",
            tag.as_str(),
            asset.as_str(),
        ]);
    if let Some(sha256) = &target.sha256 {
        url.set_fragment(Some(&format!("sha256={sha256}")));
    }
    Ok(url.into())
}

/// Where the library keeps the game's ROMs for the port's systems.
async fn rom_paths(
    store: &LibraryStore,
    library: &LibraryGame,
    families: &BTreeSet<String>,
) -> Result<Vec<PathBuf>> {
    let roms: Vec<&GameRom> = library
        .roms
        .iter()
        .filter(|(family, _)| families.contains(family))
        .map(|(_, rom)| rom)
        .collect();
    if roms.is_empty() {
        return Ok(Vec::new());
    }
    let scanners = store.scanners().await?;
    let mut paths = Vec::new();
    for rom in roms {
        let path = match &rom.location {
            RomLocation::Absolute { path } => Some(path.clone()),
            RomLocation::Scanner { scanner_id, path } => scanners
                .iter()
                .find(|scanner| &scanner.id == scanner_id)
                .and_then(|scanner| RomLocation::scanner_path(&scanner.folder, path)),
        };
        if let Some(path) = path
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    Ok(paths)
}
