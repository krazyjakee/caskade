//! Shipped helper-tool definitions: the streaming
//! host and client, in `tools/`. Data only: loading them probes nothing.
use caskade_core::managed::{ToolCatalog, ToolDefinition};

/// Embedded `(source name, TOML)` pairs.
pub fn definition_files() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        ("sunshine.toml", include_str!("../tools/sunshine.toml")),
        ("moonlight.toml", include_str!("../tools/moonlight.toml")),
    ]
    .into_iter()
}

/// The shipped tools, checked.
pub fn catalog() -> caskade_core::Result<ToolCatalog> {
    let tools = definition_files()
        .map(|(name, text)| {
            ToolDefinition::parse(text)
                .map_err(|error| caskade_core::Error::Invalid(format!("{name}: {error}")))
        })
        .collect::<caskade_core::Result<Vec<_>>>()?;
    ToolCatalog::new(tools)
}
