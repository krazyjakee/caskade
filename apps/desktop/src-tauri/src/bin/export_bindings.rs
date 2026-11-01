//! Writes the TypeScript bindings for every command and event to
//! `apps/desktop/src/api/bindings.ts` (`just bindings`).

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(caskade_desktop::bindings::default_path);
    caskade_desktop::bindings::export(&out).expect("export bindings");
    println!("{}", out.display());
}
