//! Seeds a synthetic library for the idle benchmark (`scripts/idle-bench/`):
//! `count` title-only manual games, each with a cover of its own, with
//! discovery, launchers and streaming off and onboarding done. The start
//! view stays the default (Home): "Open to" isn't wired up, and seeding it
//! would change the scenario the day it is. Nothing here touches the network.
//!
//! ```sh
//! XDG_DATA_HOME=… XDG_CONFIG_HOME=… XDG_CACHE_HOME=… \
//!   cargo run -p caskade-core --example seed_library -- 8400
//! ```
//!
//! Paths come from the same `AppPaths::standard()` the app uses, so run it
//! with the environment the app will run with, and build it from the same
//! commit as the app (the database schema must match).

use caskade_core::AppPaths;
use caskade_core::artwork::ArtworkCache;
use caskade_core::db::LibraryStore;
use caskade_core::manual::ManualGame;
use caskade_core::model::ArtworkKind;
use caskade_core::settings::SettingsService;
use image::{ImageBuffer, Rgb};
use serde_json::{Value, json};

const WIDTH: u32 = 300;
const HEIGHT: u32 = 450;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .map(|n| n.parse())
        .transpose()?
        .unwrap_or(8_400);
    let paths = AppPaths::standard()?;
    std::fs::create_dir_all(&paths.data)?;
    let store = LibraryStore::open(&paths.database()).await?;
    let settings = SettingsService::new(store.clone());
    settings.register_core(&paths)?;

    // What the app would otherwise do on its own: find games, start
    // onboarding, host streams.
    patch(&settings, "core.discovery", |_| json!({ "enabled": false })).await?;
    patch(&settings, "core.launchers", |value| {
        let enabled = value["enabled"]
            .as_object()
            .map(|launchers| {
                launchers
                    .keys()
                    .map(|name| (name.clone(), Value::Bool(false)))
                    .collect()
            })
            .unwrap_or_default();
        json!({ "enabled": Value::Object(enabled) })
    })
    .await?;
    patch(&settings, "core.streaming", |_| json!({ "hosting": false })).await?;
    patch(
        &settings,
        "core.onboarding",
        |_| json!({ "completed": true, "sounds": false }),
    )
    .await?;

    let artwork = ArtworkCache::new(paths.cache.join("artwork"))?;
    let covers = tempfile::tempdir()?;
    for index in 0..count {
        let title = format!("Benchmark Game {:04}", index + 1);
        let game = ManualGame {
            title,
            target: None,
            link_to: None,
            keep_apart: false,
        };
        let id = store.add_manual_game(&game).await?;
        let cover = covers.path().join(format!("{index}.png"));
        cover_image(index).save(&cover)?;
        store
            .import_user_artwork(&artwork, &id, ArtworkKind::Cover, &cover)
            .await?;
        std::fs::remove_file(&cover)?;
        if (index + 1) % 500 == 0 {
            eprintln!("{} of {count} games", index + 1);
        }
    }
    eprintln!("seeded {count} games into {}", paths.database().display());
    Ok(())
}

/// Merges `change(current)` into a settings section's current value.
async fn patch(
    settings: &SettingsService,
    namespace: &str,
    change: impl FnOnce(&Value) -> Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = settings.get(namespace).await?;
    let mut value = current.value.clone();
    if let (Some(target), Value::Object(changes)) = (value.as_object_mut(), change(&current.value))
    {
        target.extend(changes);
    }
    settings
        .set(namespace, value, Some(current.revision))
        .await?;
    Ok(())
}

/// A cover no other game shares: a vertical gradient between two colours
/// picked from the game's index.
fn cover_image(index: usize) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let hue = |offset: usize, step: usize| ((index * step + offset) % 256) as u8;
    let top = [hue(0, 37), hue(85, 59), hue(170, 83)];
    let bottom = [hue(40, 41), hue(125, 67), hue(210, 97)];
    ImageBuffer::from_fn(WIDTH, HEIGHT, |_, y| {
        let mix = |a: u8, b: u8| {
            let t = y as f32 / (HEIGHT - 1) as f32;
            (f32::from(a) * (1.0 - t) + f32::from(b) * t) as u8
        };
        Rgb([
            mix(top[0], bottom[0]),
            mix(top[1], bottom[1]),
            mix(top[2], bottom[2]),
        ])
    })
}
