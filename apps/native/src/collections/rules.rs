//! A smart collection's rules in words (the webview's
//! `components/collections/rules.ts`): "Games are Installed", "Platform is
//! SNES", "Title has “mario”", and its sort, for its page's header.

use crate::library::collection_dialog::filters;
use crate::library::platforms;
use caskade_core::model::{GameFilter, GameSort, ListGames};
use caskade_ui_i18n::{t, tp};

/// One rule a smart collection's games all match.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub field: String,
    pub op: String,
    pub value: String,
}

/// A library filter's name.
fn filter_label(filter: GameFilter) -> String {
    filters()
        .into_iter()
        .find(|(id, ..)| *id == filter)
        .map(|(_, _, label)| label)
        .unwrap_or_default()
}

/// A smart collection's query as rules its games all match, in the
/// language in use; "Games are All games" when it narrows nothing.
pub fn describe_rules(query: &ListGames) -> Vec<Rule> {
    let games = |filter: GameFilter| Rule {
        field: tp!(
            note = "The first word of a rule such as “Games are Installed”",
            "smart collection rule",
            "Games"
        ),
        op: tp!("smart collection rule", "are"),
        value: filter_label(filter),
    };
    let mut rules = Vec::new();
    if query.filter != GameFilter::All {
        rules.push(games(query.filter));
    }
    if let Some(platform) = query.platform.as_deref().filter(|p| !p.is_empty()) {
        rules.push(Rule {
            field: tp!("smart collection rule", "Platform"),
            op: tp!("smart collection rule", "is"),
            value: platforms::present(platform).title,
        });
    }
    if let Some(text) = query.query.as_deref().filter(|text| !text.is_empty()) {
        rules.push(Rule {
            field: tp!("smart collection rule", "Title"),
            op: tp!(
                note = "As in “Title has ‘mario’”",
                "smart collection rule",
                "has"
            ),
            value: t!(
                note = "The title text a smart collection looks for, in quotes",
                "“{text}”",
                text = text
            ),
        });
    }
    if rules.is_empty() {
        rules.push(games(GameFilter::All));
    }
    rules
}

/// A smart collection's sort, in its header.
pub fn sorted_by(sort: GameSort) -> String {
    match sort {
        GameSort::Title => t!("Sorted by title"),
        GameSort::Recent => t!("Sorted by recently played"),
        GameSort::Playtime => t!("Sorted by playtime"),
        GameSort::Added => t!("Sorted by recently added"),
        GameSort::Released => t!("Sorted by release date"),
        GameSort::Popular => t!("Sorted by popularity"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_smart_collections_query_reads_as_rules() {
        let query = ListGames {
            filter: GameFilter::Installed,
            query: Some("mario".into()),
            ..ListGames::default()
        };
        let rules = describe_rules(&query);
        assert_eq!(rules.len(), 2);
        assert_eq!(
            (
                rules[0].field.as_str(),
                rules[0].op.as_str(),
                rules[0].value.as_str()
            ),
            ("Games", "are", "Installed")
        );
        assert_eq!(
            (
                rules[1].field.as_str(),
                rules[1].op.as_str(),
                rules[1].value.as_str()
            ),
            ("Title", "has", "“mario”")
        );
    }

    #[test]
    fn a_query_narrowing_nothing_is_every_game() {
        let rules = describe_rules(&ListGames::default());
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].value, "All games");
    }
}
