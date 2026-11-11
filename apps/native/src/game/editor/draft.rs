//! The editor's unsaved changes to a game's details, field by field, and
//! the patch they make (the webview's `gameEditor/draft.ts`). Only changed
//! fields go in the patch, so a refresh meanwhile keeps whatever it wrote
//! to the others.

use caskade_core::model::{
    DateEdit, Game, GameEdit, GameEditState, GameField, ListEdit, NotesEdit, RatingEdit, TextEdit,
};

/// A field's value as the editor holds it. A release date is
/// `YYYY-MM-DD`, or empty for none.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
    Rating(Option<u8>),
}

impl Value {
    pub fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::List(values) => values.join(", "),
            Self::Rating(rating) => rating.map(|rating| rating.to_string()).unwrap_or_default(),
        }
    }

    pub fn list(&self) -> Vec<String> {
        match self {
            Self::List(values) => values.clone(),
            _ => Vec::new(),
        }
    }

    pub fn rating(&self) -> Option<u8> {
        match self {
            Self::Rating(rating) => *rating,
            _ => None,
        }
    }
}

/// A field's change: a value, or back to the automatic one.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Set(Value),
    Reset,
}

/// The changed fields.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Draft(Vec<(GameField, Change)>);

impl Draft {
    pub fn get(&self, field: GameField) -> Option<&Change> {
        self.0
            .iter()
            .find(|(known, _)| *known == field)
            .map(|(_, change)| change)
    }

    pub fn is_dirty(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn has(&self, field: GameField) -> bool {
        self.get(field).is_some()
    }

    /// Without `field`'s change.
    pub fn without(&self, field: GameField) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(known, _)| *known != field)
                .cloned()
                .collect(),
        )
    }
}

/// `field`'s value in `game`.
pub fn value(game: &Game, field: GameField) -> Value {
    match field {
        GameField::Title => Value::Text(game.title.clone()),
        GameField::SortTitle => Value::Text(game.sort_title.clone()),
        GameField::Description => Value::Text(game.description.clone()),
        GameField::Notes => Value::Text(game.notes.clone()),
        GameField::ReleaseDate => Value::Text(game.release_date.clone().unwrap_or_default()),
        GameField::Aliases => Value::List(game.aliases.clone()),
        GameField::Genres => Value::List(game.genres.clone()),
        GameField::Tags => Value::List(game.tags.clone()),
        GameField::Platforms => Value::List(game.platforms.clone()),
        GameField::Rating => Value::Rating(game.rating),
    }
}

/// `game` with `field` as `from` has it: a conflict's base taken again.
pub fn take(game: &mut Game, from: &Game, field: GameField) {
    match field {
        GameField::Title => game.title = from.title.clone(),
        GameField::SortTitle => game.sort_title = from.sort_title.clone(),
        GameField::Description => game.description = from.description.clone(),
        GameField::Notes => game.notes = from.notes.clone(),
        GameField::ReleaseDate => game.release_date = from.release_date.clone(),
        GameField::Aliases => game.aliases = from.aliases.clone(),
        GameField::Genres => game.genres = from.genres.clone(),
        GameField::Tags => game.tags = from.tags.clone(),
        GameField::Platforms => game.platforms = from.platforms.clone(),
        GameField::Rating => game.rating = from.rating,
    }
}

/// What the field shows: the change, or the saved value.
pub fn shown(state: &GameEditState, draft: &Draft, field: GameField) -> Value {
    match draft.get(field) {
        Some(Change::Set(value)) => value.clone(),
        Some(Change::Reset) => value(&state.automatic, field),
        None => value(&state.game, field),
    }
}

/// Whether the field will be the user's once saved.
pub fn is_manual(state: &GameEditState, draft: &Draft, field: GameField) -> bool {
    match draft.get(field) {
        Some(Change::Reset) => false,
        Some(Change::Set(_)) => field.automatic(),
        None => state.manual.contains(&field),
    }
}

/// `draft` with `field` changed; a change back to the saved value, or a
/// reset of a field that isn't the user's, drops out.
pub fn change(
    state: &GameEditState,
    draft: &Draft,
    field: GameField,
    next: Option<Change>,
) -> Draft {
    let mut out = draft.without(field);
    let noop = match &next {
        None => true,
        Some(Change::Set(value)) => *value == self::value(&state.game, field),
        Some(Change::Reset) => !state.manual.contains(&field),
    };
    if let (false, Some(next)) = (noop, next) {
        out.0.push((field, next));
        out.0.sort_by_key(|(field, _)| {
            GameField::ALL
                .iter()
                .position(|known| known == field)
                .unwrap_or(usize::MAX)
        });
    }
    out
}

/// Drops changes the saved game already has: after a save, or when a
/// refresh brought the same values.
pub fn prune(state: &GameEditState, draft: &Draft) -> Draft {
    draft.0.iter().fold(draft.clone(), |out, (field, next)| {
        change(state, &out, *field, Some(next.clone()))
    })
}

/// The patch for `draft`.
pub fn to_edit(draft: &Draft) -> GameEdit {
    let text = |field| {
        draft.get(field).map(|change| match change {
            Change::Reset => TextEdit::Reset,
            Change::Set(value) => TextEdit::Set {
                value: value.text(),
            },
        })
    };
    let list = |field| {
        draft.get(field).map(|change| match change {
            Change::Reset => ListEdit::Reset,
            Change::Set(value) => ListEdit::Set {
                values: value.list(),
            },
        })
    };
    let set = |field| match draft.get(field) {
        Some(Change::Set(value)) => Some(value.clone()),
        _ => None,
    };
    GameEdit {
        title: text(GameField::Title),
        sort_title: text(GameField::SortTitle),
        description: text(GameField::Description),
        aliases: list(GameField::Aliases),
        genres: list(GameField::Genres),
        tags: list(GameField::Tags),
        platforms: list(GameField::Platforms),
        release_date: draft
            .get(GameField::ReleaseDate)
            .map(|change| match change {
                Change::Reset => DateEdit::Reset,
                Change::Set(value) => {
                    let date = value.text().trim().to_owned();
                    DateEdit::Set {
                        value: (!date.is_empty()).then_some(date),
                    }
                }
            }),
        rating: set(GameField::Rating).map(|value| RatingEdit {
            value: value.rating().map(i32::from),
        }),
        notes: set(GameField::Notes).map(|value| NotesEdit {
            value: value.text(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> GameEditState {
        let mut game = Game::new("Mine");
        game.sort_title = "Mine".into();
        game.tags = vec!["Old".into()];
        let mut automatic = Game::new("Theirs");
        automatic.sort_title = "Theirs".into();
        GameEditState {
            game,
            automatic,
            manual: vec![GameField::Title],
        }
    }

    #[test]
    fn a_change_back_to_the_saved_value_drops_out() {
        let state = state();
        let draft = change(
            &state,
            &Draft::default(),
            GameField::Title,
            Some(Change::Set(Value::Text("New".into()))),
        );
        assert!(draft.is_dirty());
        let draft = change(
            &state,
            &draft,
            GameField::Title,
            Some(Change::Set(Value::Text("Mine".into()))),
        );
        assert!(!draft.is_dirty());
    }

    #[test]
    fn a_reset_shows_the_automatic_value_and_only_the_users_fields_reset() {
        let state = state();
        let draft = change(
            &state,
            &Draft::default(),
            GameField::Title,
            Some(Change::Reset),
        );
        assert_eq!(
            shown(&state, &draft, GameField::Title),
            Value::Text("Theirs".into())
        );
        assert!(!is_manual(&state, &draft, GameField::Title));
        // The sort title isn't the user's: nothing to reset.
        let draft = change(&state, &draft, GameField::SortTitle, Some(Change::Reset));
        assert!(!draft.has(GameField::SortTitle));
    }

    #[test]
    fn the_patch_has_only_the_changed_fields() {
        let state = state();
        let mut draft = change(
            &state,
            &Draft::default(),
            GameField::Tags,
            Some(Change::Set(Value::List(vec!["New".into()]))),
        );
        draft = change(
            &state,
            &draft,
            GameField::ReleaseDate,
            Some(Change::Set(Value::Text(" ".into()))),
        );
        draft = change(
            &state,
            &draft,
            GameField::Rating,
            Some(Change::Set(Value::Rating(Some(70)))),
        );
        let edit = to_edit(&draft);
        assert_eq!(
            edit.tags,
            Some(ListEdit::Set {
                values: vec!["New".into()]
            })
        );
        assert_eq!(edit.release_date, Some(DateEdit::Set { value: None }));
        assert_eq!(edit.rating, Some(RatingEdit { value: Some(70) }));
        assert_eq!(edit.title, None);
        assert!(is_manual(&state, &draft, GameField::Tags));
        // Rating is only ever the user's, never "manual" over automatic.
        assert!(!is_manual(&state, &draft, GameField::Rating));
    }

    #[test]
    fn pruning_drops_what_the_save_kept() {
        let mut state = state();
        let draft = change(
            &state,
            &Draft::default(),
            GameField::Notes,
            Some(Change::Set(Value::Text("Hi".into()))),
        );
        state.game.notes = "Hi".into();
        assert!(!prune(&state, &draft).is_dirty());
    }
}
