//! The committed TypeScript bindings being current.

use caskade_desktop::bindings;

#[test]
fn committed_bindings_are_current() {
    let committed = std::fs::read_to_string(bindings::default_path()).expect("committed bindings");
    let fresh = bindings::render().expect("render bindings");
    assert!(
        committed == fresh,
        "apps/desktop/src/api/bindings.ts is stale: run `just bindings`"
    );
}
