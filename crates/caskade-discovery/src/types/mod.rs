//! The data the crate returns, and the `Source` trait every source implements.
//!
//! Every public data type supports serde, JSON Schema and Specta reflection,
//! the same as `caskade-installer`, so it reaches the UI unchanged. JSON enums
//! use an adjacent `type`/`value` representation and snake_case names. Paths
//! are host paths as the source found them; times are integers (seconds,
//! milliseconds, Unix seconds) named by their unit.

mod game;
mod ids;
mod report;
mod source;

pub use game::*;
pub use ids::*;
pub use report::*;
pub use source::*;
