//! Stored review choices: applying them to a scan, and turning a submitted
//! review into exclusions and overrides.

use super::{ReviewChoice, ReviewGame, RomOverride, rom_path};
use crate::{
    Error, Result,
    emulation::{GameRom, RomLocation, scan::RomGame},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A scanned game; `titled` when a review chose its title, which then
/// replaces the library game's title too.
pub(super) struct Found {
    pub(super) game: RomGame,
    pub(super) titled: bool,
}

/// Regroups and renames scanned games with the stored review choices.
pub(super) fn apply_overrides(
    games: Vec<RomGame>,
    overrides: &BTreeMap<String, RomOverride>,
) -> Vec<Found> {
    if overrides.is_empty() {
        return games
            .into_iter()
            .map(|game| Found {
                game,
                titled: false,
            })
            .collect();
    }
    struct Part {
        origin: usize,
        index: usize,
        rom: GameRom,
        choice: Option<RomOverride>,
    }
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Part>> = HashMap::new();
    for (origin, game) in games.iter().enumerate() {
        for (index, rom) in game.roms.iter().enumerate() {
            let choice = rom_path(rom).and_then(|path| overrides.get(&path).cloned());
            // Paths never contain NUL, so automatic keys can't collide.
            let key = choice
                .as_ref()
                .and_then(|choice| choice.group.clone())
                .unwrap_or_else(|| format!("\0{origin}"));
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups.entry(key).or_default().push(Part {
                origin,
                index,
                rom: rom.clone(),
                choice,
            });
        }
    }
    let mut result = Vec::new();
    for key in order {
        let Some(mut parts) = groups.remove(&key) else {
            continue;
        };
        parts.sort_by_key(|part| {
            (
                part.choice.as_ref().and_then(|choice| choice.order),
                part.origin,
                part.index,
            )
        });
        let origin = &games[parts[0].origin];
        let pick = |field: fn(&RomOverride) -> Option<String>| {
            parts
                .iter()
                .find_map(|part| part.choice.as_ref().and_then(field))
        };
        let title = pick(|choice| choice.title.clone());
        let mut game = RomGame {
            title: title.clone().unwrap_or_else(|| origin.title.clone()),
            platform: pick(|choice| choice.platform.clone())
                .unwrap_or_else(|| origin.platform.clone()),
            regions: parts
                .iter()
                .find_map(|part| part.choice.as_ref().and_then(|c| c.regions.clone()))
                .unwrap_or_else(|| origin.regions.clone()),
            properties: Vec::new(),
            roms: Vec::new(),
            files: Vec::new(),
        };
        let origins: BTreeSet<usize> = parts.iter().map(|part| part.origin).collect();
        for &index in &origins {
            let source = &games[index];
            let whole = source.roms.len() == parts.iter().filter(|p| p.origin == index).count();
            game.properties.extend(source.properties.iter().cloned());
            if whole {
                game.files.extend(source.files.iter().cloned());
            }
        }
        for part in parts {
            if let RomLocation::Scanner { path, .. } = &part.rom.location
                && !game.files.contains(path)
            {
                game.files.push(path.clone());
            }
            game.roms.push(part.rom);
        }
        game.files.sort();
        game.files.dedup();
        game.properties.sort();
        game.properties.dedup();
        result.push(Found {
            game,
            titled: title.is_some(),
        });
    }
    result
}

/// What a review's choices store.
pub(super) struct Choices {
    /// Exact paths of unticked games' files.
    pub(super) exclusions: Vec<String>,
    pub(super) overrides: Vec<(String, RomOverride)>,
}

/// Turns review choices into exclusions for unticked games and overrides
/// for renamed, re-platformed, merged or split ones. Stored overrides of
/// unchanged games are kept.
pub(super) fn review_choices(
    review: &[ReviewGame],
    choices: &[ReviewChoice],
    stored: &BTreeMap<String, RomOverride>,
) -> Result<Choices> {
    let mut owner: HashMap<&str, usize> = HashMap::new();
    for (index, game) in review.iter().enumerate() {
        for rom in &game.roms {
            owner.insert(rom.path.as_str(), index);
        }
    }
    let mut seen = BTreeSet::new();
    let mut exclusions = Vec::new();
    let mut overrides = Vec::new();
    for choice in choices {
        let Some(first) = choice.roms.first() else {
            return Err(Error::Invalid(crate::tr!("a reviewed game has no ROMs")));
        };
        for path in &choice.roms {
            if !owner.contains_key(path.as_str()) {
                return Err(Error::Invalid(crate::tr!(
                    "`{path}` is not in this review",
                    path = path
                )));
            }
            if !seen.insert(path.clone()) {
                return Err(Error::Invalid(crate::tr!(
                    "`{path}` is in two reviewed games",
                    path = path
                )));
            }
        }
        let origin = &review[owner[first.as_str()]];
        if !choice.include {
            let whole =
                |game: &ReviewGame| game.roms.iter().all(|rom| choice.roms.contains(&rom.path));
            let origins: BTreeSet<usize> = choice
                .roms
                .iter()
                .map(|path| owner[path.as_str()])
                .collect();
            for index in origins {
                let game = &review[index];
                if whole(game) {
                    exclusions.extend(game.files.iter().cloned());
                } else {
                    exclusions.extend(
                        game.roms
                            .iter()
                            .filter(|rom| choice.roms.contains(&rom.path))
                            .map(|rom| rom.path.clone()),
                    );
                }
            }
            for path in &choice.roms {
                overrides.push((path.clone(), RomOverride::default()));
            }
            continue;
        }
        let title = choice.title.trim();
        if title.is_empty() {
            return Err(Error::Invalid(crate::tr!("a reviewed game needs a title")));
        }
        let regrouped = origin.roms.len() != choice.roms.len()
            || origin
                .roms
                .iter()
                .zip(&choice.roms)
                .any(|(rom, path)| rom.path != *path);
        for (order, path) in choice.roms.iter().enumerate() {
            let mut next = stored.get(path).cloned().unwrap_or_default();
            if regrouped {
                next.group = Some(first.clone());
                next.order = Some(order as u32);
            }
            if title != origin.title {
                next.title = Some(title.to_owned());
            }
            if choice.platform != origin.platform {
                next.platform = Some(choice.platform.clone());
            }
            if choice.regions != origin.regions {
                next.regions = Some(choice.regions.clone());
            }
            overrides.push((path.clone(), next));
        }
    }
    // Games the choices left out keep their state.
    exclusions.sort();
    exclusions.dedup();
    Ok(Choices {
        exclusions,
        overrides,
    })
}
