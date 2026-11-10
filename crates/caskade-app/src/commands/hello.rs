//! `hello`: proof that typed commands and events work end to end.
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::{Event, Events};

/// The core's answer to `hello`: proof that typed commands and events work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Greeting {
    pub message: String,
    pub core_version: String,
}

/// Emitted after every `hello` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Hello(pub Greeting);

/// Greets `name` and sends the `hello` event with the same greeting.
pub fn hello(events: &Events, name: &str) -> Result<Greeting, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    let greeting = Greeting {
        message: format!("Hello, {name}!"),
        core_version: caskade_core::VERSION.into(),
    };
    events.emit(Event::Hello(Hello(greeting.clone())))?;
    Ok(greeting)
}
