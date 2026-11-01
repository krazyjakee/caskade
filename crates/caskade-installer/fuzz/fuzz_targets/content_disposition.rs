#![no_main]
use caskade_installer::fetch::disposition_name;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(Some(name)) = disposition_name(&String::from_utf8_lossy(data)) {
        assert!(!name.is_empty());
        assert!(!name.contains(['/', '\\', ':', '\0']));
        assert!(!matches!(name.as_str(), "." | ".."));
    }
});
