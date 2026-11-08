//! Play offline: the guided capture of a web game into
//! a storage root, and the bundle it leaves as the game's entry.

use super::{
    add::{WebGames, read_page},
    bundle::{Bundle, MANIFEST},
};
use crate::{
    bus::Event,
    model::{GameEntry, InstallLocation, LaunchSpec},
};
use std::path::{Path, PathBuf};

/// The game window's exit code when the capture was saved, and when it
/// closed before that (`web_player`'s `EXIT_SAVED`, `EXIT_CANCELLED`).
const EXIT_SAVED: i32 = 0;
const EXIT_CANCELLED: i32 = 3;
/// The longest folder name a game's title gives.
const NAME_LIMIT: usize = 80;

impl WebGames {
    /// Opens the guided capture of `game_id`'s web game, to keep in
    /// `root_id`, and returns once the window is open. When the player
    /// saves it, the game's entry plays from the bundle; when they close the
    /// window first, nothing is kept.
    pub async fn save_offline(&self, game_id: &str, root_id: &str) -> crate::Result<()> {
        if !cfg!(target_os = "linux") {
            return Err(crate::Error::Invalid(
                "Saving web games for offline play isn't available on this system yet.".into(),
            ));
        }
        let player = self
            .player
            .clone()
            .ok_or_else(|| crate::Error::Invalid("the game window isn't available".into()))?;
        let entry = self.web_entry(game_id).await?;
        if entry.location.is_some() {
            return Err(crate::Error::Invalid(
                "this game is already saved for offline play".into(),
            ));
        }
        let Some(LaunchSpec::Web { url }) = &entry.launch else {
            return Err(crate::Error::Invalid("that isn't a web game".into()));
        };
        let page = super::page_url(url)?;
        let root = self
            .store
            .get_root(root_id)
            .await?
            .ok_or_else(|| crate::Error::Invalid("that storage location is gone".into()))?;
        let title = self
            .store
            .game(game_id)
            .await?
            .map_or_else(|| "Web game".to_owned(), |game| game.title);
        // The game itself, without the site around it, where the page says.
        let (start, size) = match read_page(&page).await {
            Ok(super::page::PageInfo {
                unplayable: Some(why),
                ..
            }) => return Err(crate::Error::Invalid(why.message())),
            Ok(info) => match info.frame {
                Some(frame) => (frame, info.frame_size),
                None => (page, None),
            },
            Err(error) => {
                tracing::warn!(host = page.host_str(), %error, "web game page not read; capturing the page");
                (page, None)
            }
        };
        let rel_dir = free_folder(&root.path, &folder_name(&title))?;
        let dir = root.path.join(&rel_dir);
        let command = player
            .capture(&entry, &start, size, &dir)
            .map_err(|error| crate::Error::Invalid(error.to_string()))?;
        let mut child = tokio::process::Command::new(&command.program)
            .args(&command.args)
            .envs(&command.env)
            .stdin(std::process::Stdio::null())
            .spawn()?;
        tracing::info!(
            game = game_id,
            host = start.host_str(),
            "web game capture open"
        );
        let web = self.clone();
        let root_id = root_id.to_owned();
        tokio::spawn(async move {
            let status = child.wait().await;
            let code = status
                .as_ref()
                .ok()
                .and_then(std::process::ExitStatus::code);
            let kept = match code {
                Some(EXIT_SAVED) => web.keep(entry, &root_id, rel_dir, &dir).await,
                Some(EXIT_CANCELLED) => {
                    tracing::info!("web game capture closed before it was saved");
                    Ok(false)
                }
                _ => {
                    tracing::warn!(?status, "the web game capture failed");
                    Ok(false)
                }
            };
            if !matches!(kept, Ok(true)) {
                if let Err(error) = &kept {
                    tracing::error!(%error, "the web game's offline copy couldn't be kept");
                }
                if let Err(error) = tokio::fs::remove_dir_all(&dir).await
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(%error, "the unsaved web game copy wasn't removed");
                }
            }
        });
        Ok(())
    }

    /// Deletes `game_id`'s offline copy; the game plays online again.
    pub async fn remove_offline(&self, game_id: &str) -> crate::Result<()> {
        let mut entry = self.web_entry(game_id).await?;
        if !matches!(entry.location, Some(InstallLocation::Managed { .. })) {
            return Ok(());
        }
        self.delete_bundle(&entry).await?;
        entry.location = None;
        entry.size = None;
        self.store.upsert_entry(&entry).await?;
        if let Some(bus) = &self.bus {
            bus.emit(Event::LibraryGameUpdated {
                game_id: entry.game_id.clone(),
            });
        }
        Ok(())
    }

    /// Deletes the folder of `entry`'s offline copy, when it has one.
    pub(super) async fn delete_bundle(&self, entry: &GameEntry) -> crate::Result<()> {
        let Some(InstallLocation::Managed { root_id, rel_dir }) = &entry.location else {
            return Ok(());
        };
        let root =
            self.store.get_root(root_id).await?.ok_or_else(|| {
                crate::Error::Invalid("the game's storage location is gone".into())
            })?;
        // Only ever a bundle's folder, inside its root.
        if rel_dir.is_absolute()
            || rel_dir
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(crate::Error::Invalid(
                "the offline copy's folder isn't valid".into(),
            ));
        }
        let dir = root.path.join(rel_dir);
        match tokio::fs::metadata(dir.join(MANIFEST)).await {
            Ok(_) => tokio::fs::remove_dir_all(&dir).await?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// Makes the saved bundle in `dir` the entry's managed copy.
    async fn keep(
        &self,
        mut entry: GameEntry,
        root_id: &str,
        rel_dir: PathBuf,
        dir: &Path,
    ) -> crate::Result<bool> {
        let dir = dir.to_owned();
        let (files, bytes) =
            tokio::task::spawn_blocking(move || Bundle::open(dir).map(|bundle| bundle.stats()))
                .await
                .map_err(|error| crate::Error::Invalid(error.to_string()))??;
        if files == 0 {
            return Ok(false);
        }
        entry.location = Some(InstallLocation::Managed {
            root_id: root_id.to_owned(),
            rel_dir,
        });
        entry.size = Some(bytes);
        self.store.upsert_entry(&entry).await?;
        if let Some(bus) = &self.bus {
            bus.emit(Event::LibraryGameUpdated {
                game_id: entry.game_id.clone(),
            });
        }
        tracing::info!(game = %entry.game_id, files, bytes, "web game saved for offline play");
        Ok(true)
    }
}

/// A folder name from a game's title: no characters any filesystem
/// refuses, no leading or trailing dots and spaces.
fn folder_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name: String = name.chars().take(NAME_LIMIT).collect();
    let name = name.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if name.is_empty() {
        "Web game".into()
    } else {
        name.to_owned()
    }
}

/// `name`, or `name (2)`, `name (3)`, …: the first that `root` hasn't got.
fn free_folder(root: &Path, name: &str) -> crate::Result<PathBuf> {
    for n in 1..1000 {
        let candidate = if n == 1 {
            name.to_owned()
        } else {
            format!("{name} ({n})")
        };
        if !root.join(&candidate).exists() {
            return Ok(PathBuf::from(candidate));
        }
    }
    Err(crate::Error::Invalid(
        "no free folder name for the game".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{folder_name, free_folder};

    #[test]
    fn folder_names_are_safe_and_free() {
        assert_eq!(folder_name("Penalty Shooters 2"), "Penalty Shooters 2");
        assert_eq!(folder_name("What? A: Game/Two *"), "What A Game Two");
        assert_eq!(folder_name(" ...hidden. "), "hidden");
        assert_eq!(folder_name("???"), "Web game");
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Needle")).unwrap();
        std::fs::create_dir(root.path().join("Needle (2)")).unwrap();
        assert_eq!(
            free_folder(root.path(), "Needle").unwrap(),
            std::path::Path::new("Needle (3)")
        );
    }

    /// The guided capture with a stand-in game window: a shell script.
    #[cfg(target_os = "linux")]
    mod capture {
        use crate::{
            db::LibraryStore,
            launch::WebPlayer,
            model::{InstallLocation, StorageRoot},
            web::{WEB_SOURCE, WebGames},
        };
        use std::{path::Path, time::Duration};

        /// A game window that saves a one-file bundle and exits with `code`.
        fn window(dir: &Path, code: i32) -> std::path::PathBuf {
            let script = dir.join(format!("caskade-{code}"));
            std::fs::write(
                &script,
                format!(
                    r#"#!/bin/sh
while [ $# -gt 0 ]; do [ "$1" = --bundle ] && dir="$2"; shift; done
mkdir -p "$dir/files"
printf '{{"version":1,"start":"http://127.0.0.1:9/","items":{{"http://127.0.0.1:9/":{{"file":"files/a","status":200,"size":2}}}}}}' > "$dir/caskade-web.json"
printf hi > "$dir/files/a"
exit {code}
"#
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
            script
        }

        async fn web_entry(store: &LibraryStore, game: &str) -> crate::model::GameEntry {
            store
                .entries_for_game(game)
                .await
                .unwrap()
                .into_iter()
                .find(|entry| entry.source_id == WEB_SOURCE)
                .unwrap()
        }

        #[tokio::test]
        async fn a_saved_capture_becomes_the_games_offline_copy() {
            let home = tempfile::tempdir().unwrap();
            let root = home.path().join("Games");
            std::fs::create_dir(&root).unwrap();
            let store = LibraryStore::in_memory().await.unwrap();
            store
                .upsert_root(&StorageRoot {
                    id: "r1".into(),
                    path: root.clone(),
                    is_default: true,
                    label: None,
                })
                .await
                .unwrap();
            let web = |code| {
                WebGames::new(store.clone()).with_player(WebPlayer::new(
                    window(home.path(), code),
                    home.path().join("web"),
                ))
            };
            // The page can't be reached: it is named after its host, and the
            // capture starts at the page itself.
            let game = web(0).add("http://127.0.0.1:9/").await.unwrap();

            web(3).save_offline(&game, "r1").await.unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
            assert_eq!(web_entry(&store, &game).await.location, None);
            assert!(
                !root.join("127.0.0.1").exists(),
                "a cancelled capture keeps nothing"
            );

            web(0).save_offline(&game, "r1").await.unwrap();
            let mut entry = web_entry(&store, &game).await;
            for _ in 0..50 {
                if entry.location.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                entry = web_entry(&store, &game).await;
            }
            assert_eq!(
                entry.location,
                Some(InstallLocation::Managed {
                    root_id: "r1".into(),
                    rel_dir: "127.0.0.1".into(),
                })
            );
            assert_eq!(entry.size, Some(2));
            assert!(
                web(0).save_offline(&game, "r1").await.is_err(),
                "already offline"
            );

            web(0).remove_offline(&game).await.unwrap();
            assert_eq!(web_entry(&store, &game).await.location, None);
            assert!(!root.join("127.0.0.1").exists());
        }

        #[tokio::test]
        async fn removing_a_web_game_deletes_its_files_and_the_game() {
            let home = tempfile::tempdir().unwrap();
            let root = home.path().join("Games");
            std::fs::create_dir(&root).unwrap();
            let store = LibraryStore::in_memory().await.unwrap();
            store
                .upsert_root(&StorageRoot {
                    id: "r1".into(),
                    path: root.clone(),
                    is_default: true,
                    label: None,
                })
                .await
                .unwrap();
            let player = WebPlayer::new(window(home.path(), 0), home.path().join("web"));
            let web = WebGames::new(store.clone()).with_player(player.clone());
            let game = web.add("http://127.0.0.1:9/").await.unwrap();
            web.save_offline(&game, "r1").await.unwrap();
            let mut entry = web_entry(&store, &game).await;
            for _ in 0..50 {
                if entry.location.is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                entry = web_entry(&store, &game).await;
            }
            assert!(root.join("127.0.0.1").exists());
            let data = player.data_dir(&entry.id);
            std::fs::create_dir_all(data.join("localstorage")).unwrap();

            web.remove(&game).await.unwrap();
            assert!(
                !root.join("127.0.0.1").exists(),
                "the offline copy is deleted"
            );
            assert!(!data.exists(), "the game window's data is deleted");
            assert!(store.game(&game).await.unwrap().is_none());
            assert!(store.get_entry(&entry.id).await.unwrap().is_none());
            assert!(web.remove(&game).await.is_err(), "already gone");
        }
    }
}
