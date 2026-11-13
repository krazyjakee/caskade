//! Plugins' drawn UI (#109) over the fixtures' fake plugin host: a view
//! and its sidebar link, a control that tells the plugin and the state it
//! keeps, a failing contribution's placeholder, the game page's actions,
//! tabs and panels, a game menu's plugin action, and Home's sections.

use crate::frame::tests::{ONE, START, Window, source};
use caskade_ui_data::plugin_ui::ui::{
    Node, Outcome, bar, bars, heading, note, segmented, stats, table, text,
};
use caskade_ui_data::plugin_ui::{DrawnPlugin, FakePluginUi, UiContribution};
use caskade_ui_input::Action;
use dioxus_native::prelude::keyboard_types::{Key, NamedKey};
use serde_json::json;

const STATS: &str = "dev.caskade.playtime-stats";

fn fake() -> &'static FakePluginUi {
    source()
        .plugin_fake
        .as_ref()
        .expect("the fixtures' fake")
        .as_ref()
}

/// Empties the fake for the next test, whatever happened.
struct Clean;

impl Drop for Clean {
    fn drop(&mut self) {
        fake().clear();
        fake().set_plugins(Vec::new());
    }
}

fn contribution(slot: &str, id: &str, title: Option<&str>) -> UiContribution {
    UiContribution {
        slot: slot.into(),
        id: id.into(),
        title: title.map(str::to_owned),
        modes: Vec::new(),
    }
}

fn plugin(id: &str, name: &str, contributes: Vec<UiContribution>) -> DrawnPlugin {
    DrawnPlugin {
        id: id.into(),
        name: name.into(),
        version: "0.2.0".into(),
        contributes,
    }
}

fn chart(view: &str) -> Vec<Node> {
    vec![
        heading("Most played"),
        segmented(
            "Show as",
            "view",
            view,
            [("chart", "Chart"), ("table", "Table")],
        ),
        if view == "chart" {
            bars(
                "Most played games, hours played",
                [bar("Celeste", 12.0, "12 h")],
            )
        } else {
            table(
                "Most played games",
                ["Game".to_owned(), "Hours played".to_owned()],
                [vec!["Celeste".to_owned(), "12.0".to_owned()]],
            )
        },
    ]
}

fn open(href: &'static str) -> Window {
    *caskade_core::lock(&START) = href;
    let window = Window::open();
    *caskade_core::lock(&START) = "/";
    window
}

fn press(window: &mut Window, node: blitz_dom::NodeId) {
    window.focus(node);
    window.key(Key::Named(NamedKey::Enter));
}

#[test]
fn a_view_draws_tells_the_plugin_and_keeps_its_state() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    fake().set_plugins(vec![plugin(
        STATS,
        "Playtime stats",
        vec![contribution("views", "stats", Some("Playtime stats"))],
    )]);
    fake().draw(STATS, "views", "stats", "null", Ok(chart("chart")));
    fake().draw(
        STATS,
        "views",
        "stats",
        &json!({"view": "table"}).to_string(),
        Ok(chart("table")),
    );
    fake().answer(
        "view",
        Ok(Outcome {
            state: Some(json!({"view": "table"})),
            notice: None,
        }),
    );
    let mut window = open("/");
    // The sidebar links to the view.
    window.settle_until("the plugin link", |window| {
        window.find("data-part", "plugin-nav").is_some()
    });
    let nav = window.get("data-part", "plugin-nav");
    assert_eq!(window.attr(nav, "aria-label").as_deref(), Some("Plugins"));
    let link = window
        .find_in(nav, "data-part", "nav-item")
        .expect("a link to the view");
    assert!(window.text(link).contains("Playtime stats"));
    window.click(link);
    window.settle_until("the view", |window| {
        window.find("data-view", "plugin").is_some()
            && window.find("data-cs", "plugin-ui").is_some()
    });
    let view = window.get("data-view", "plugin");
    assert_eq!(window.attr(view, "data-plugin").as_deref(), Some(STATS));
    let text = window.text(view);
    assert!(
        text.contains("Playtime stats") && text.contains("Most played"),
        "{text}"
    );
    let chart = window.get("data-part", "bars");
    assert_eq!(
        window.attr(chart, "aria-label").as_deref(),
        Some("Most played games, hours played")
    );
    assert!(window.text(chart).contains("12 h"));
    // The segmented control tells the plugin; the state comes back drawn.
    let table_option = window
        .find_all("data-part", "segment")
        .into_iter()
        .find(|segment| window.text(*segment).trim() == "Table")
        .unwrap();
    press(&mut window, table_option);
    window.settle_until("the table", |window| {
        window.find("data-part", "table").is_some()
    });
    assert!(
        fake()
            .calls()
            .contains(&format!("act {STATS} views stats view \"table\""))
    );
    assert!(
        window
            .text(window.get("data-part", "table"))
            .contains("Celeste")
    );
    // A pad's A presses the other option again.
    let chart_option = window
        .find_all("data-part", "segment")
        .into_iter()
        .find(|segment| window.text(*segment).trim() == "Chart")
        .unwrap();
    window.focus(chart_option);
    window.pad(Action::Confirm);
    window.settle_until("asked again", |_| {
        fake()
            .calls()
            .contains(&format!("act {STATS} views stats view \"chart\""))
    });
}

#[test]
fn a_contribution_that_fails_shows_its_plugins_placeholder() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    fake().set_plugins(vec![
        plugin(
            "dev.caskade.crashy",
            "Crashy",
            vec![contribution("views", "broken", Some("Crashy"))],
        ),
        plugin(
            STATS,
            "Playtime stats",
            vec![contribution("views", "stats", Some("Playtime stats"))],
        ),
    ]);
    fake().draw(
        "dev.caskade.crashy",
        "views",
        "broken",
        "*",
        Err("crashy failed to render".into()),
    );
    fake().draw(STATS, "views", "stats", "*", Ok(vec![note("Fine")]));
    let mut window = open("/plugins/dev.caskade.crashy/broken");
    window.settle_until("the placeholder", |window| {
        window.find("data-cs", "plugin-error").is_some()
    });
    let placeholder = window.get("data-cs", "plugin-error");
    assert_eq!(window.attr(placeholder, "role").as_deref(), Some("alert"));
    assert_eq!(
        window.text(placeholder).trim(),
        "Crashy couldn't show this part."
    );
    // The others keep drawing.
    window.go("/plugins/dev.caskade.playtime-stats/stats");
    window.settle_until("the other view", |window| {
        window
            .find("data-part", "note")
            .is_some_and(|note| window.text(note).contains("Fine"))
    });
    // A view no plugin draws says so.
    window.go("/plugins/dev.caskade.gone/view");
    window.settle_until("not available", |window| {
        window.find("role", "status").is_some_and(|status| {
            window
                .text(status)
                .contains("This plugin view isn't available.")
        })
    });
}

#[test]
fn drawn_stats_read_as_a_list_of_values() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    fake().set_plugins(vec![plugin(
        "dev.caskade.howlongtobeat",
        "HowLongToBeat",
        vec![contribution("views", "times", Some("How long to beat"))],
    )]);
    fake().draw(
        "dev.caskade.howlongtobeat",
        "views",
        "times",
        "*",
        Ok(vec![
            stats([("Main story", "12 h"), ("Completionist", "—")]),
            text("From howlongtobeat.com"),
        ]),
    );
    let mut window = open("/plugins/dev.caskade.howlongtobeat/times");
    window.settle_until("the stats", |window| {
        window.find("data-part", "stats").is_some()
    });
    let list = window.get("data-part", "stats");
    let text = window.text(list);
    assert!(
        text.contains("Main story") && text.contains("12 h"),
        "{text}"
    );
}

const PACK: &str = "dev.caskade.example.pack";
const HLTB: &str = "dev.caskade.howlongtobeat";

#[test]
fn the_game_page_shows_plugins_actions_tabs_and_panels_for_its_game() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    fake().set_plugins(vec![
        plugin(
            PACK,
            "Caskade Pack (example)",
            vec![contribution(
                "game.actions",
                "about-packs",
                Some("About game packs"),
            )],
        ),
        plugin(
            HLTB,
            "HowLongToBeat",
            vec![
                contribution("game.details.tabs", "hltb", Some("How long to beat")),
                contribution("game.details.panels", "note", None),
            ],
        ),
    ]);
    fake().draw(
        HLTB,
        "game.details.tabs",
        "hltb",
        "*",
        Ok(vec![stats([("Main story", "12 h")])]),
    );
    fake().draw(
        HLTB,
        "game.details.panels",
        "note",
        "*",
        Ok(vec![note("From howlongtobeat.com")]),
    );
    fake().answer(
        "run",
        Ok(Outcome {
            state: None,
            notice: Some("Caskade installs .cpk game packs.".into()),
        }),
    );
    let mut window = open("/games/fixture-1");
    window.settle_until("the plugins' part", |window| {
        window.find("data-part", "plugin-details").is_some()
            && window.find("data-part", "stats").is_some()
            && window.find("data-part", "note").is_some()
    });
    let details = window.get("data-part", "plugin-details");
    assert_eq!(
        window.attr(details, "aria-label").as_deref(),
        Some("From plugins")
    );
    // The tab and the panel are drawn for this game.
    let calls = fake().calls();
    assert!(
        calls.contains(&format!("render {HLTB} game.details.tabs hltb fixture-1")),
        "{calls:?}"
    );
    assert!(calls.contains(&format!("render {HLTB} game.details.panels note fixture-1")));
    let tab = window.get("role", "tab");
    assert!(window.text(tab).contains("How long to beat"));
    // The action runs for the game, and its notice shows.
    let actions = window.get("data-part", "plugin-actions");
    let action = window.find_in(actions, "data-cs", "button").unwrap();
    assert!(window.text(action).contains("About game packs"));
    press(&mut window, action);
    window.settle_until("the notice", |window| {
        window.find_all("role", "status").into_iter().any(|toast| {
            window
                .text(toast)
                .contains("Caskade Pack (example): Caskade installs .cpk game packs.")
        })
    });
    assert!(
        fake()
            .calls()
            .contains(&format!("act {PACK} game.actions about-packs run null"))
    );
}

#[test]
fn a_games_menu_runs_plugins_actions_and_says_when_one_fails() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    fake().set_plugins(vec![plugin(
        PACK,
        "Caskade Pack (example)",
        vec![contribution(
            "game.actions",
            "about-packs",
            Some("About game packs"),
        )],
    )]);
    fake().answer("run", Err("the pack service is down".into()));
    let mut window = open("/library?filter=all");
    window.settle_until("the games", |window| {
        window.find("data-index", "0").is_some()
    });
    let card = window.get("data-index", "0");
    window.right_click(card);
    window.settle_until("the menu", |window| {
        window
            .find("role", "menu")
            .is_some_and(|menu| window.text(menu).contains("About game packs"))
    });
    let item = window
        .find_all("role", "menuitem")
        .into_iter()
        .find(|&item| window.text(item).contains("About game packs"))
        .unwrap();
    window.click(item);
    window.settle_until("the failure", |window| {
        window.find_all("role", "alert").into_iter().any(|toast| {
            window
                .text(toast)
                .contains("About game packs failed: The pack service is down")
        })
    });
    assert!(
        fake()
            .calls()
            .iter()
            .any(|call| call.starts_with(&format!("act {PACK} game.actions about-packs run")))
    );
}

#[test]
fn library_cards_show_plugins_badges() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    fake().set_plugins(vec![plugin(
        HLTB,
        "HowLongToBeat",
        vec![contribution("game.card.badges", "short", None)],
    )]);
    fake().draw(
        HLTB,
        "game.card.badges",
        "short",
        "*",
        Ok(vec![caskade_ui_data::plugin_ui::ui::badge(
            "Short",
            caskade_ui_data::plugin_ui::ui::Tone::Accent,
        )]),
    );
    let mut window = open("/library?filter=all");
    window.settle_until("a badge", |window| {
        window
            .find("data-part", "badge")
            .is_some_and(|badge| window.attr(badge, "data-tone").as_deref() == Some("accent"))
    });
    let badges = window.get("data-part", "plugin-badges");
    assert!(window.text(badges).contains("Short"));
}

#[test]
fn home_ends_with_plugins_sections() {
    let _one = caskade_core::lock(&ONE);
    let _runtime = caskade_app::runtime::handle().enter();
    let _clean = Clean;
    fake().set_plugins(vec![plugin(
        STATS,
        "Playtime stats",
        vec![contribution("home.sections", "week", Some("This week"))],
    )]);
    fake().draw(
        STATS,
        "home.sections",
        "week",
        "*",
        Ok(vec![note("3 hours across 2 games")]),
    );
    let mut window = open("/");
    window.settle_until("the section", |window| {
        window
            .find("data-part", "plugin-section")
            .is_some_and(|section| window.text(section).contains("3 hours across 2 games"))
    });
    let section = window.get("data-part", "plugin-section");
    assert_eq!(window.attr(section, "data-plugin").as_deref(), Some(STATS));
    assert_eq!(
        window.attr(section, "aria-label").as_deref(),
        Some("This week")
    );
    assert!(window.text(section).contains("This week"));
}
