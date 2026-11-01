use super::*;

/// `(component)`, and the same with a custom section: two components.
const EMPTY: &[u8] = b"\0asm\x0d\x00\x01\x00";
fn other() -> Vec<u8> {
    let mut bytes = EMPTY.to_vec();
    bytes.extend_from_slice(&[0x00, 0x06, 0x05]);
    bytes.extend_from_slice(b"other");
    bytes
}

fn engine(opt: wasmtime::OptLevel) -> Engine {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true).cranelift_opt_level(opt);
    Engine::new(&config).unwrap()
}

fn cache(dir: &Path) -> ComponentCache {
    ComponentCache::new(dir, CacheKey::from_bytes([7; 32]))
}

#[test]
fn a_second_start_loads_the_verified_entry_without_compiling() {
    let temp = tempfile::tempdir().unwrap();
    let engine = engine(wasmtime::OptLevel::Speed);
    let (_, first) = cache(temp.path()).load_or_compile(&engine, EMPTY).unwrap();
    assert_eq!(first, Outcome::Compiled(Miss::Missing));
    // A new cache over the same folder and key: the app's next start.
    let (_, second) = cache(temp.path()).load_or_compile(&engine, EMPTY).unwrap();
    assert_eq!(second, Outcome::Hit);
}

#[test]
fn another_component_wasmtime_version_or_engine_config_uses_another_entry() {
    let temp = tempfile::tempdir().unwrap();
    let speed = engine(wasmtime::OptLevel::Speed);
    let cache = cache(temp.path());
    cache.load_or_compile(&speed, EMPTY).unwrap();
    let miss = Outcome::Compiled(Miss::Missing);
    assert_eq!(cache.load_or_compile(&speed, &other()).unwrap().1, miss);
    let none = engine(wasmtime::OptLevel::None);
    assert_ne!(
        cache.entry_path(&none, EMPTY),
        cache.entry_path(&speed, EMPTY)
    );
    assert_eq!(cache.load_or_compile(&none, EMPTY).unwrap().1, miss);
    let next = cache.clone().with_wasmtime_version("51.0.0");
    assert_ne!(
        next.entry_path(&speed, EMPTY),
        cache.entry_path(&speed, EMPTY)
    );
    assert_eq!(next.load_or_compile(&speed, EMPTY).unwrap().1, miss);
    // The original entries are still hits.
    assert_eq!(
        cache.load_or_compile(&speed, EMPTY).unwrap().1,
        Outcome::Hit
    );
}

#[test]
fn a_corrupted_entry_is_rejected_and_rebuilt() {
    let temp = tempfile::tempdir().unwrap();
    let engine = engine(wasmtime::OptLevel::Speed);
    let cache = cache(temp.path());
    cache.load_or_compile(&engine, EMPTY).unwrap();
    let path = cache.entry_path(&engine, EMPTY);
    let mut file = std::fs::read(&path).unwrap();
    let last = file.len() - 1;
    file[last] ^= 0xff;
    std::fs::write(&path, &file).unwrap();
    let rejected = Outcome::Compiled(Miss::Rejected);
    assert_eq!(cache.load_or_compile(&engine, EMPTY).unwrap().1, rejected);
    // Rewritten: the next start is a hit again.
    assert_eq!(
        cache.load_or_compile(&engine, EMPTY).unwrap().1,
        Outcome::Hit
    );

    for broken in [
        Vec::new(),
        b"CSKCCv1\nshort".to_vec(),
        b"not a cache entry".to_vec(),
    ] {
        std::fs::write(&path, broken).unwrap();
        assert_eq!(cache.load_or_compile(&engine, EMPTY).unwrap().1, rejected);
    }
}

#[test]
fn a_well_formed_entry_that_isnt_this_ones_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let engine = engine(wasmtime::OptLevel::Speed);
    let cache = cache(temp.path());
    let rejected = Outcome::Compiled(Miss::Rejected);

    // A valid entry of another component, copied over this one's: its MAC
    // binds it to its own name.
    cache.load_or_compile(&engine, &other()).unwrap();
    let path = cache.entry_path(&engine, EMPTY);
    std::fs::copy(cache.entry_path(&engine, &other()), &path).unwrap();
    assert_eq!(cache.load_or_compile(&engine, EMPTY).unwrap().1, rejected);

    // A valid artifact for this very component, signed with another key.
    let forged = ComponentCache::new(temp.path(), CacheKey::from_bytes([8; 32]));
    std::fs::remove_file(&path).unwrap();
    forged.load_or_compile(&engine, EMPTY).unwrap();
    assert_eq!(cache.load_or_compile(&engine, EMPTY).unwrap().1, rejected);
    assert_eq!(
        cache.load_or_compile(&engine, EMPTY).unwrap().1,
        Outcome::Hit
    );
}

#[test]
fn keys_are_random_and_never_printed() {
    let (a, b) = (CacheKey::generate().unwrap(), CacheKey::generate().unwrap());
    assert_ne!(a.to_bytes(), b.to_bytes());
    assert_eq!(format!("{a:?}"), "CacheKey(..)");
    assert_eq!(CacheKey::from_bytes(a.to_bytes()).to_bytes(), a.to_bytes());
}
