//! A verified on-disk cache of compiled plugin components (#136).
//!
//! Compiling a component costs about 20 ms for a Rust plugin and about 1 s
//! for a jco-built one (15 s in a debug build) on every app start. This
//! crate keeps wasmtime's compiled artifact on disk and loads it again on
//! the next start, without trusting the disk:
//!
//! - An entry's name is a SHA-256 over the component bytes, the wasmtime
//!   version and the engine's compatibility hash (its compilation
//!   settings). Any change to those uses another entry.
//! - Each file is `MAGIC ‖ HMAC-SHA256(key, MAGIC ‖ name ‖ artifact) ‖
//!   artifact`. The key is random per install and kept outside the cache
//!   folder (the app stores it in its database), so a file someone else
//!   wrote, or one copied from another entry, fails verification.
//! - A missing key, a bad MAC, a short or unreadable file, or an artifact
//!   wasmtime refuses all mean: compile again and overwrite the entry.
//!   Nothing unverified reaches [`wasmtime::component::Component::deserialize`].
#![deny(unsafe_code)]

use hmac::{KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};
use wasmtime::{Engine, component::Component};

type HmacSha256 = hmac::Hmac<Sha256>;

/// Starts every entry file; a new layout gets a new magic.
const MAGIC: &[u8; 8] = b"CSKCCv1\n";
const MAC_LEN: usize = 32;

/// The per-install secret that signs entries. Keep it out of the cache
/// folder.
#[derive(Clone)]
pub struct CacheKey([u8; 32]);

impl CacheKey {
    /// A new random key.
    pub fn generate() -> std::io::Result<Self> {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(Self(key))
    }
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl std::fmt::Debug for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CacheKey(..)")
    }
}

/// Why a component was compiled instead of loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    /// No entry yet (first use, or the component, wasmtime or engine changed).
    Missing,
    /// The file failed verification: corrupted, tampered with or not ours.
    Rejected,
    /// The file verified but wasmtime refused the artifact.
    Incompatible,
    /// The entry couldn't be read.
    Unreadable,
}

/// How a component was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Loaded from a verified entry; no compilation.
    Hit,
    /// Compiled, and the entry was (re)written.
    Compiled(Miss),
}

/// Feeds a `Hash` value into SHA-256.
struct Sha(Sha256);
impl Hasher for Sha {
    fn finish(&self) -> u64 {
        unreachable!("only the digest is used")
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

/// The entry name for a component on this engine and wasmtime version.
fn entry_name(bytes: &[u8], engine: &Engine, wasmtime_version: &str) -> String {
    let mut sha = Sha(Sha256::new());
    engine.precompile_compatibility_hash().hash(&mut sha);
    let compatibility = sha.0.finalize();
    let mut name = Sha256::new();
    name.update(b"caskade-component\0");
    name.update(wasmtime_version.as_bytes());
    name.update(b"\0");
    name.update(compatibility);
    name.update(Sha256::digest(bytes));
    hex(&name.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A cache of compiled components in one folder.
#[derive(Debug, Clone)]
pub struct ComponentCache {
    dir: PathBuf,
    key: CacheKey,
    wasmtime_version: String,
}

impl ComponentCache {
    pub fn new(dir: impl Into<PathBuf>, key: CacheKey) -> Self {
        Self {
            dir: dir.into(),
            key,
            wasmtime_version: wasmtime_version().to_owned(),
        }
    }

    /// The same cache, as a build of another wasmtime version would see it.
    #[doc(hidden)]
    pub fn with_wasmtime_version(mut self, version: &str) -> Self {
        self.wasmtime_version = version.to_owned();
        self
    }

    /// Where a component's entry lives.
    pub fn entry_path(&self, engine: &Engine, bytes: &[u8]) -> PathBuf {
        self.dir
            .join(entry_name(bytes, engine, &self.wasmtime_version))
            .with_extension("cwasm")
    }

    fn mac(&self, name: &str, artifact: &[u8]) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.key.0).expect("any key length works");
        mac.update(MAGIC);
        mac.update(name.as_bytes());
        mac.update(artifact);
        mac
    }

    /// The component, from a verified entry or freshly compiled (and then
    /// cached). Compilation errors are the component's own.
    pub fn load_or_compile(
        &self,
        engine: &Engine,
        bytes: &[u8],
    ) -> wasmtime::Result<(Component, Outcome)> {
        let name = entry_name(bytes, engine, &self.wasmtime_version);
        let path = self.dir.join(&name).with_extension("cwasm");
        let miss = match std::fs::read(&path) {
            Ok(file) => match self.verified(&name, &file) {
                Some(artifact) => match load(engine, artifact) {
                    Ok(component) => return Ok((component, Outcome::Hit)),
                    Err(error) => {
                        tracing::warn!(%error, "a verified component cache entry was refused; recompiling");
                        Miss::Incompatible
                    }
                },
                None => {
                    tracing::warn!(entry = %path.display(), "component cache entry failed verification; recompiling");
                    Miss::Rejected
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Miss::Missing,
            Err(error) => {
                tracing::warn!(%error, "component cache entry unreadable; recompiling");
                Miss::Unreadable
            }
        };
        let artifact = engine.precompile_component(bytes)?;
        let tag = self.mac(&name, &artifact).finalize().into_bytes();
        if let Err(error) = self.write(&path, &tag, &artifact) {
            tracing::warn!(%error, "component cache entry not written");
        }
        // Just produced by `precompile_component` on this engine.
        let component = load(engine, Trusted(&artifact))?;
        Ok((component, Outcome::Compiled(miss)))
    }

    /// The artifact of an entry file whose MAC (constant-time) matches, or
    /// `None`.
    fn verified<'a>(&self, name: &str, file: &'a [u8]) -> Option<Trusted<'a>> {
        let rest = file.strip_prefix(MAGIC.as_slice())?;
        if rest.len() < MAC_LEN {
            return None;
        }
        let (tag, artifact) = rest.split_at(MAC_LEN);
        self.mac(name, artifact).verify_slice(tag).ok()?;
        Some(Trusted(artifact))
    }

    /// Writes an entry atomically: a temporary file, then a rename.
    fn write(&self, path: &Path, tag: &[u8], artifact: &[u8]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let temp = path.with_extension(format!("tmp{}", std::process::id()));
        let mut file = Vec::with_capacity(MAGIC.len() + tag.len() + artifact.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(tag);
        file.extend_from_slice(artifact);
        // A failed write or rename (a full disk) leaves no temporary file
        // behind to pile up (#160).
        let written = std::fs::write(&temp, &file).and_then(|()| std::fs::rename(&temp, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written
    }
}

/// An artifact this cache produced: its MAC under the install's key just
/// verified, or `precompile_component` returned it a moment ago. Only
/// `ComponentCache::verified` and `load_or_compile` construct it.
struct Trusted<'a>(&'a [u8]);

/// The single place an artifact becomes code.
fn load(engine: &Engine, artifact: Trusted<'_>) -> wasmtime::Result<Component> {
    // SAFETY: `Component::deserialize` trusts its input to be an artifact
    // wasmtime produced for a compatible engine. A `Trusted` artifact is one:
    // either `Engine::precompile_component` just returned it, or it was
    // stored with an HMAC-SHA256 under a random per-install key that never
    // lives in the cache folder, and that MAC was verified before the
    // `Trusted` was made. wasmtime still checks its own header for engine
    // and version compatibility and refuses a mismatch.
    #[allow(unsafe_code)]
    let component = unsafe { Component::deserialize(engine, artifact.0) }?;
    Ok(component)
}

/// The wasmtime version these artifacts are for.
pub fn wasmtime_version() -> &'static str {
    // wasmtime pins it exactly in Cargo.toml (`=50.0.0`).
    "50.0.0"
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
