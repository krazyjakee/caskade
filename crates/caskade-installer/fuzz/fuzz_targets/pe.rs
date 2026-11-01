#![no_main]
use caskade_installer::probe::{PeProbe, PeProbeLimits, ProbeEngine, ProbeLimits};
use libfuzzer_sys::fuzz_target;
use std::{io::Cursor, path::Path};
fuzz_target!(|data: &[u8]| {
    let mut engine = ProbeEngine::new(ProbeLimits {
        max_read_bytes: 512 * 1024,
        ..ProbeLimits::default()
    });
    engine.register(Box::new(PeProbe::with_limits(PeProbeLimits {
        max_header_bytes: 64 * 1024,
        max_resource_bytes: 128 * 1024,
        max_signature_bytes: 64 * 1024,
    })));
    let _ = engine.probe_reader(&mut Cursor::new(data), Path::new("fuzz.exe"));
});
