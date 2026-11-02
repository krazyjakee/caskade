//! Integration tests, compiled as one binary: a change relinks once instead
//! of once per file. Run one file's tests with `cargo test --test it <file>::`.

mod emulation_definitions;
mod import;
mod rebuild;
mod roms;
mod storage;
mod streaming;
mod tools;
mod watch;
