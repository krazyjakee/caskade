//! Plugins' UI from API 0.2 (#109): the backend's `ui` export draws a
//! contribution its manifest declares, as a tree of Caskade's components
//! (`caskade_plugin_ui`), and hears what the user did with it. Every tree
//! and answer is checked before Caskade draws it.

use super::{HostError, PluginHost, Result};
use crate::manifest::Manifest;
use caskade_plugin_ui::{Context, Node, Outcome};

/// Whether `manifest` declares contribution `id` of `slot`: a slot entry,
/// a view (`views`) or a command (`commands`).
pub fn declares(manifest: &Manifest, slot: &str, id: &str) -> bool {
    let contributes = &manifest.contributes;
    match slot {
        "views" => contributes.views.iter().any(|view| view.id == id),
        "commands" => contributes.commands.iter().any(|command| command.id == id),
        _ => contributes
            .slots
            .iter()
            .any(|entry| entry.slot == slot && entry.id == id),
    }
}

impl PluginHost {
    fn declared(&self, id: &str, slot: &str, contribution: &str) -> Result<()> {
        let entry = self.entry(id)?;
        let manifest = Self::valid(&entry, id)?;
        if !crate::manifest::draws_ui(&manifest.api) || manifest.backend.is_none() {
            return Err(HostError::NoUi(id.to_owned()));
        }
        if !declares(&manifest, slot, contribution) {
            return Err(HostError::Undeclared {
                id: id.to_owned(),
                slot: slot.to_owned(),
                contribution: contribution.to_owned(),
            });
        }
        Ok(())
    }

    /// `context` with its game's title, for a game slot.
    async fn with_title(&self, context: &Context) -> Context {
        let mut context = context.clone();
        if let (Some(game), None) = (&context.game_id, &context.game_title) {
            context.game_title = self
                .services()
                .store
                .game(game)
                .await
                .ok()
                .flatten()
                .map(|game| game.title);
        }
        context
    }

    fn bad(id: &str, error: impl std::fmt::Display) -> HostError {
        HostError::BadUi {
            id: id.to_owned(),
            message: error.to_string(),
        }
    }

    /// Draws contribution `contribution` of `slot`, starting the backend if
    /// it isn't running.
    pub async fn ui_render(
        &self,
        id: &str,
        slot: &str,
        contribution: &str,
        context: &Context,
    ) -> Result<Vec<Node>> {
        self.declared(id, slot, contribution)?;
        let context = self.with_title(context).await;
        let context = serde_json::to_string(&context).map_err(|error| Self::bad(id, error))?;
        let answer = self
            .call(id, async |instance| {
                instance.ui_render(slot, contribution, &context).await
            })
            .await?
            .ok_or_else(|| HostError::NoUi(id.to_owned()))?;
        let tree = Self::answered(id, answer)?;
        caskade_plugin_ui::parse(&tree).map_err(|error| Self::bad(id, error))
    }

    /// The user used `action` (with `value`) in contribution `contribution`
    /// of `slot`; `run` runs a `game.actions` or `commands` entry.
    pub async fn ui_act(
        &self,
        id: &str,
        slot: &str,
        contribution: &str,
        action: &str,
        value: &serde_json::Value,
        context: &Context,
    ) -> Result<Outcome> {
        self.declared(id, slot, contribution)?;
        let context = self.with_title(context).await;
        let context = serde_json::to_string(&context).map_err(|error| Self::bad(id, error))?;
        let value = value.to_string();
        let answer = self
            .call(id, async |instance| {
                instance
                    .ui_act(slot, contribution, action, &value, &context)
                    .await
            })
            .await?
            .ok_or_else(|| HostError::NoUi(id.to_owned()))?;
        let outcome = Self::answered(id, answer)?;
        caskade_plugin_ui::parse_outcome(&outcome).map_err(|error| Self::bad(id, error))
    }
}
