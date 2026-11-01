//! Shipped emulation definitions (EMULATION-PLAN §3).
//!
//! The catalogue is data-only: loading it never probes the host or executes
//! an emulator. Plugin/user files can be appended to [`definition_files`]
//! before calling [`Catalog::load`]; later ids override earlier ones.
use caskade_core::emulation::{Catalog, DefinitionErrors};

/// Embedded `(source name, TOML)` pairs in stable override order.
/// Names are retained in validation diagnostics.
pub fn definition_files() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        (
            "emulation/platforms.toml",
            include_str!("../../emulation/platforms.toml"),
        ),
        (
            "emulation/regions.toml",
            include_str!("../../emulation/regions.toml"),
        ),
        (
            "emulation/retroarch.toml",
            include_str!("../../emulation/retroarch.toml"),
        ),
        (
            "emulation/pcsx2.toml",
            include_str!("../../emulation/pcsx2.toml"),
        ),
        (
            "emulation/duckstation.toml",
            include_str!("../../emulation/duckstation.toml"),
        ),
        (
            "emulation/dolphin.toml",
            include_str!("../../emulation/dolphin.toml"),
        ),
        (
            "emulation/ppsspp.toml",
            include_str!("../../emulation/ppsspp.toml"),
        ),
        (
            "emulation/melonds.toml",
            include_str!("../../emulation/melonds.toml"),
        ),
        (
            "emulation/mgba.toml",
            include_str!("../../emulation/mgba.toml"),
        ),
        (
            "emulation/cemu.toml",
            include_str!("../../emulation/cemu.toml"),
        ),
        (
            "emulation/ryujinx.toml",
            include_str!("../../emulation/ryujinx.toml"),
        ),
        (
            "emulation/rpcs3.toml",
            include_str!("../../emulation/rpcs3.toml"),
        ),
        (
            "emulation/mesen.toml",
            include_str!("../../emulation/mesen.toml"),
        ),
        (
            "emulation/rmg.toml",
            include_str!("../../emulation/rmg.toml"),
        ),
        (
            "emulation/gopher64.toml",
            include_str!("../../emulation/gopher64.toml"),
        ),
        (
            "emulation/flycast.toml",
            include_str!("../../emulation/flycast.toml"),
        ),
        (
            "emulation/ymir.toml",
            include_str!("../../emulation/ymir.toml"),
        ),
        (
            "emulation/ares.toml",
            include_str!("../../emulation/ares.toml"),
        ),
        (
            "emulation/mame.toml",
            include_str!("../../emulation/mame.toml"),
        ),
        (
            "emulation/azahar.toml",
            include_str!("../../emulation/azahar.toml"),
        ),
        (
            "emulation/xemu.toml",
            include_str!("../../emulation/xemu.toml"),
        ),
        (
            "emulation/xenia.toml",
            include_str!("../../emulation/xenia.toml"),
        ),
        (
            "emulation/shadps4.toml",
            include_str!("../../emulation/shadps4.toml"),
        ),
    ]
    .into_iter()
}

/// Parse and validate the shipped catalogue on every supported OS and kind.
pub fn catalog() -> Result<Catalog, DefinitionErrors> {
    Catalog::load(definition_files())
}
