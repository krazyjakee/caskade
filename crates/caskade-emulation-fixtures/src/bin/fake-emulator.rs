fn main() {
    match caskade_emulation_fixtures::run_fake_emulator() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("fake-emulator: {error}");
            std::process::exit(64);
        }
    }
}
