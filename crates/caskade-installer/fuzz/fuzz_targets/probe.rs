#![no_main]
use caskade_installer::probe::{ProbeEngine, ProbeLimits};
use libfuzzer_sys::fuzz_target;
use std::{io::Cursor, path::Path};
fuzz_target!(|data: &[u8]| {
    let engine = ProbeEngine::with_builtin_formats(ProbeLimits {
        max_depth: 4,
        max_children: 1024,
        max_read_bytes: 4 * 1024 * 1024,
    });
    let _ = engine.probe_reader(&mut Cursor::new(data), Path::new("input.bin"));
});
