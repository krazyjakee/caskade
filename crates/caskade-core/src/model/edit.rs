//! Manual game edits (#171): a patch of chosen fields, what it saved or why
//! it didn't, and which fields the user controls.
//!
//! A field the user set is theirs until they reset it: automatic sources
//! (discovery, installs, IGDB, RomM, ROM scanners) keep writing the value
//! they would give it beneath, and Reset brings that value back. Setting a
//! field to nothing (an empty description, no genres) is a value like any
//! other. Rating and notes are the user's alone, so they are set or
//! cleared, never reset.

use super::Game;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

/// A game field the editor changes.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    Type,
)]
#[serde(rename_all = "camelCase")]
pub enum GameField {
    Title,
    SortTitle,
    Description,
    Aliases,
    Genres,
    Tags,
    ReleaseDate,
    Platforms,
    Rating,
    Notes,
}

impl GameField {
    pub const ALL: [Self; 10] = [
        Self::Title,
        Self::SortTitle,
        Self::Description,
        Self::Aliases,
        Self::Genres,
        Self::Tags,
        Self::ReleaseDate,
        Self::Platforms,
        Self::Rating,
        Self::Notes,
    ];

    /// The field's key in a serialized [`Game`] and in `overrides_json`.
    pub fn key(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::SortTitle => "sortTitle",
            Self::Description => "description",
            Self::Aliases => "aliases",
            Self::Genres => "genres",
            Self::Tags => "tags",
            Self::ReleaseDate => "releaseDate",
            Self::Platforms => "platforms",
            Self::Rating => "rating",
            Self::Notes => "notes",
        }
    }

    /// Whether automatic sources fill the field, so that a value the user
    /// sets overrides theirs. Rating and notes are only ever the user's.
    pub fn automatic(self) -> bool {
        !matches!(self, Self::Rating | Self::Notes)
    }
}

/// A text field's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TextEdit {
    /// The user's value; empty clears it (not for a title).
    Set { value: String },
    /// Back to the automatic value.
    Reset,
}

/// A list field's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ListEdit {
    /// Exactly these values; none clears the list.
    Set { values: Vec<String> },
    /// These values join the list (bulk edits): those already there stay
    /// where they are.
    Add { values: Vec<String> },
    /// These values leave the list.
    Remove { values: Vec<String> },
    /// Back to the automatic values.
    Reset,
}

/// The release date's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DateEdit {
    /// A `YYYY-MM-DD` date, or none.
    Set {
        value: Option<String>,
    },
    Reset,
}

/// The personal rating's change: 0 through 100, or none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RatingEdit {
    /// Wider than the rating so an out-of-range number is reported on the
    /// field rather than failing the whole patch.
    pub value: Option<i32>,
}

/// The notes' change; empty clears them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotesEdit {
    pub value: String,
}

/// The fields to change; those left out stay as they are. Unknown keys
/// fail the whole patch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct GameEdit {
    pub title: Option<TextEdit>,
    pub sort_title: Option<TextEdit>,
    pub description: Option<TextEdit>,
    pub aliases: Option<ListEdit>,
    pub genres: Option<ListEdit>,
    pub tags: Option<ListEdit>,
    pub release_date: Option<DateEdit>,
    pub platforms: Option<ListEdit>,
    pub rating: Option<RatingEdit>,
    pub notes: Option<NotesEdit>,
}

impl GameEdit {
    /// The fields the patch changes.
    pub fn fields(&self) -> Vec<GameField> {
        GameField::ALL
            .into_iter()
            .filter(|field| match field {
                GameField::Title => self.title.is_some(),
                GameField::SortTitle => self.sort_title.is_some(),
                GameField::Description => self.description.is_some(),
                GameField::Aliases => self.aliases.is_some(),
                GameField::Genres => self.genres.is_some(),
                GameField::Tags => self.tags.is_some(),
                GameField::ReleaseDate => self.release_date.is_some(),
                GameField::Platforms => self.platforms.is_some(),
                GameField::Rating => self.rating.is_some(),
                GameField::Notes => self.notes.is_some(),
            })
            .collect()
    }
}

/// A game as the editor shows it: what the library shows, the automatic
/// values beneath, and which fields are the user's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameEditState {
    pub game: Game,
    /// The game as automatic sources have it: Reset brings a field's value
    /// here back.
    pub automatic: Game,
    /// The fields the user set, in [`GameField::ALL`] order.
    pub manual: Vec<GameField>,
}

/// Why a field's change wasn't saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum FieldProblem {
    /// The value isn't allowed.
    Invalid,
    /// The field changed since the editor loaded it.
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    pub field: GameField,
    pub problem: FieldProblem,
    pub message: String,
}

/// What became of a patch. A rejected patch saved nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GameEditOutcome {
    Saved {
        state: GameEditState,
        /// The fields whose value or control changed.
        changed: Vec<GameField>,
    },
    Rejected {
        /// The game as it is now, for a reload.
        state: GameEditState,
        errors: Vec<FieldError>,
    },
}

/// The source of artwork the user chose (#175): an image they imported, or
/// their copy of an automatic one (the cache bytes are shared). Automatic
/// sources never write or delete it.
pub const USER_ARTWORK_SOURCE: &str = "user";

/// The kinds of artwork the editor changes: those the app shows one of.
/// Screenshots are a strip of several, and logos show nowhere yet.
pub const EDITABLE_ARTWORK: [super::ArtworkKind; 3] = [
    super::ArtworkKind::Cover,
    super::ArtworkKind::Hero,
    super::ArtworkKind::Icon,
];

/// Who decides a kind of a game's artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum ArtworkChoice {
    /// Automatic sources, in their usual precedence.
    Automatic,
    /// The user: an image they chose or imported.
    Chosen,
    /// The user: none at all.
    Removed,
}

/// One kind of a game's artwork as the editor shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkSlot {
    pub kind: super::ArtworkKind,
    pub choice: ArtworkChoice,
    /// What the game shows now.
    pub shown: Option<super::Artwork>,
    /// The images automatic sources supplied, to choose from; the first
    /// is what Reset shows.
    pub automatic: Vec<super::Artwork>,
}

/// What a bulk edit (#173) does, or did, to one game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub enum BulkStatus {
    /// Its fields change (preview) or changed (applied).
    Changed,
    /// It already has what the edit sets.
    Unchanged,
    /// The edit isn't valid for it; nothing of it was saved.
    Rejected,
    /// It is no longer in the library.
    Missing,
}

/// One field of one game in a bulk edit, as text to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkChange {
    pub field: GameField,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkGameResult {
    pub game_id: String,
    /// Empty when the game is missing.
    pub title: String,
    pub status: BulkStatus,
    pub changes: Vec<BulkChange>,
    pub errors: Vec<FieldError>,
}
