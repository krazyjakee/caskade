//! Plugins' UI from API 0.2 (#109): which enabled plugins draw which slots
//! and views, drawing one, and telling it what the user did, through the
//! app layer's commands. The list changes with `PluginsChanged`
//! (`Key::Plugins`).
//!
//! A [`PluginUiSource`] is the running app's plugin host ([`AppPluginUi`])
//! or, in fixtures and tests, [`FakePluginUi`]: plugins a test puts there,
//! with the trees they draw and what was asked of them.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use caskade_app::commands::plugin_ui as commands;
pub use caskade_app::commands::plugin_ui::{DrawnPlugin, UiContribution};
use caskade_app::events::{Event, Events, PluginsChanged};
use caskade_core::error::{ErrorCode, ErrorReport};
use caskade_core::store_accounts::async_trait;
pub use caskade_plugin_ui as ui;
pub use caskade_plugin_ui::{Context, Mode, Node, Outcome};
use serde_json::Value;

type Result<T> = std::result::Result<T, ErrorReport>;

/// Where plugins' UI comes from.
#[async_trait]
pub trait PluginUiSource: Send + Sync {
    async fn drawn(&self) -> Vec<DrawnPlugin>;
    async fn render(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        context: Context,
    ) -> Result<Vec<Node>>;
    async fn act(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        action: &str,
        value: Value,
        context: Context,
    ) -> Result<Outcome>;
}

/// The running app's plugin host.
pub struct AppPluginUi(pub caskade_plugin_host::PluginHost);

#[async_trait]
impl PluginUiSource for AppPluginUi {
    async fn drawn(&self) -> Vec<DrawnPlugin> {
        commands::drawn_plugins(&self.0).await
    }
    async fn render(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        context: Context,
    ) -> Result<Vec<Node>> {
        commands::plugin_ui_render(
            &self.0,
            plugin.to_owned(),
            slot.to_owned(),
            id.to_owned(),
            context,
        )
        .await
    }
    async fn act(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        action: &str,
        value: Value,
        context: Context,
    ) -> Result<Outcome> {
        commands::plugin_ui_act(
            &self.0,
            plugin.to_owned(),
            slot.to_owned(),
            id.to_owned(),
            action.to_owned(),
            value,
            context,
        )
        .await
    }
}

/// Plugins' UI over one app's services; none without a plugin host.
#[derive(Clone, Default)]
pub struct PluginUi(Option<Arc<dyn PluginUiSource>>);

impl PartialEq for PluginUi {
    /// One per app: a view's props don't change with it.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// One contribution a plugin draws, with its plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    pub plugin: String,
    pub plugin_name: String,
    pub slot: String,
    pub id: String,
    pub title: Option<String>,
    pub modes: Vec<String>,
}

impl Contribution {
    /// Whether it shows in `mode` (#152): its own `modes`, or the slot's
    /// default (views, the game page and Home in both; the rest on the
    /// desktop).
    pub fn shown_in(&self, mode: Mode) -> bool {
        let name = match mode {
            Mode::Desktop => "desktop",
            Mode::Console => "console",
        };
        if !self.modes.is_empty() {
            return self.modes.iter().any(|shown| shown == name);
        }
        mode == Mode::Desktop
            || matches!(
                self.slot.as_str(),
                "views"
                    | "game.details.tabs"
                    | "game.details.panels"
                    | "game.actions"
                    | "home.sections"
            )
    }
}

impl PluginUi {
    pub fn new(source: Option<Arc<dyn PluginUiSource>>) -> Self {
        Self(source)
    }

    /// The running app's plugins' UI.
    pub fn of(services: &caskade_app::start::Services) -> Self {
        Self::new(
            services
                .plugins
                .clone()
                .map(|host| Arc::new(AppPluginUi(host)) as Arc<dyn PluginUiSource>),
        )
    }

    fn source(&self) -> Result<&Arc<dyn PluginUiSource>> {
        self.0.as_ref().ok_or_else(|| {
            ErrorReport::new(
                ErrorCode::Unavailable,
                caskade_core::tr!("Plugins are unavailable in this session."),
            )
        })
    }

    /// Every contribution enabled plugins draw, in the plugins' order.
    pub async fn contributions(&self) -> Vec<Contribution> {
        let Some(source) = &self.0 else {
            return Vec::new();
        };
        source
            .drawn()
            .await
            .into_iter()
            .flat_map(|plugin| {
                plugin
                    .contributes
                    .into_iter()
                    .map(move |contribution| Contribution {
                        plugin: plugin.id.clone(),
                        plugin_name: plugin.name.clone(),
                        slot: contribution.slot,
                        id: contribution.id,
                        title: contribution.title,
                        modes: contribution.modes,
                    })
            })
            .collect()
    }

    pub async fn render(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        context: Context,
    ) -> Result<Vec<Node>> {
        self.source()?.render(plugin, slot, id, context).await
    }

    pub async fn act(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        action: &str,
        value: Value,
        context: Context,
    ) -> Result<Outcome> {
        self.source()?
            .act(plugin, slot, id, action, value, context)
            .await
    }
}

type Key = (String, String, String);
/// What a fake contribution draws, or why it fails.
type Drawing = std::result::Result<Vec<Node>, String>;

/// Plugins a test puts in place, the trees they draw (by plugin, slot and
/// id, with the context's state as JSON text, or any state with `*`), and
/// what was asked of them: `render <plugin> <slot> <id> <game>` and `act
/// <plugin> <slot> <id> <action> <value>`. Outcomes for `act` are given by
/// action; one not given answers nothing.
pub struct FakePluginUi {
    plugins: Mutex<Vec<DrawnPlugin>>,
    trees: Mutex<BTreeMap<(Key, String), Drawing>>,
    outcomes: Mutex<BTreeMap<String, std::result::Result<Outcome, String>>>,
    calls: Mutex<Vec<String>>,
    events: Events,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl FakePluginUi {
    pub fn new(events: Events) -> Self {
        Self {
            plugins: Mutex::default(),
            trees: Mutex::default(),
            outcomes: Mutex::default(),
            calls: Mutex::default(),
            events,
        }
    }

    /// Puts the plugins that draw, as `plugins_drawn` lists them, and says
    /// the list changed.
    pub fn set_plugins(&self, plugins: Vec<DrawnPlugin>) {
        let plugin = plugins
            .first()
            .map(|plugin| plugin.id.clone())
            .unwrap_or_default();
        *lock(&self.plugins) = plugins;
        self.events
            .send(Event::PluginsChanged(PluginsChanged { plugin }));
    }

    /// What contribution `id` of `slot` draws when its state is `state`
    /// (`*` for any); an `Err` is the plugin failing.
    pub fn draw(&self, plugin: &str, slot: &str, id: &str, state: &str, tree: Drawing) {
        lock(&self.trees).insert(
            (
                (plugin.to_owned(), slot.to_owned(), id.to_owned()),
                state.to_owned(),
            ),
            tree,
        );
    }

    /// What `act` answers for `action`.
    pub fn answer(&self, action: &str, outcome: std::result::Result<Outcome, String>) {
        lock(&self.outcomes).insert(action.to_owned(), outcome);
    }

    pub fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    pub fn clear(&self) {
        lock(&self.plugins).clear();
        lock(&self.trees).clear();
        lock(&self.outcomes).clear();
        lock(&self.calls).clear();
    }
}

fn failed(message: String) -> ErrorReport {
    ErrorReport::new(ErrorCode::Other, message)
}

#[async_trait]
impl PluginUiSource for FakePluginUi {
    async fn drawn(&self) -> Vec<DrawnPlugin> {
        lock(&self.plugins).clone()
    }
    async fn render(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        context: Context,
    ) -> Result<Vec<Node>> {
        lock(&self.calls).push(format!(
            "render {plugin} {slot} {id} {}",
            context.game_id.as_deref().unwrap_or("-")
        ));
        let key = (plugin.to_owned(), slot.to_owned(), id.to_owned());
        let trees = lock(&self.trees);
        trees
            .get(&(key.clone(), context.state.to_string()))
            .or_else(|| trees.get(&(key, "*".to_owned())))
            .cloned()
            .unwrap_or_else(|| Err(format!("plugin {plugin} draws no {slot} {id}")))
            .map_err(failed)
    }
    async fn act(
        &self,
        plugin: &str,
        slot: &str,
        id: &str,
        action: &str,
        value: Value,
        _context: Context,
    ) -> Result<Outcome> {
        lock(&self.calls).push(format!("act {plugin} {slot} {id} {action} {value}"));
        lock(&self.outcomes)
            .get(action)
            .cloned()
            .unwrap_or(Ok(Outcome::default()))
            .map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contribution(slot: &str, modes: &[&str]) -> Contribution {
        Contribution {
            plugin: "p".into(),
            plugin_name: "P".into(),
            slot: slot.into(),
            id: "x".into(),
            title: None,
            modes: modes.iter().map(|mode| (*mode).to_owned()).collect(),
        }
    }

    #[test]
    fn contributions_show_in_their_modes_or_the_slots_default() {
        assert!(contribution("views", &[]).shown_in(Mode::Console));
        assert!(contribution("home.sections", &[]).shown_in(Mode::Console));
        assert!(!contribution("topbar.items", &[]).shown_in(Mode::Console));
        assert!(contribution("topbar.items", &[]).shown_in(Mode::Desktop));
        assert!(!contribution("views", &["console"]).shown_in(Mode::Desktop));
        assert!(contribution("sidebar.items", &["console"]).shown_in(Mode::Console));
    }
}
