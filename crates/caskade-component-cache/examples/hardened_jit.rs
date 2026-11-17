//! Compiles and runs one wasm function with the app's wasmtime, then prints
//! its answer (42). CI signs this with macOS's hardened runtime and the
//! release's entitlements (scripts/macos/entitlements.plist) to check that
//! plugins can still compile there: wasmtime makes its code executable with
//! mprotect, which the hardened runtime refuses unless an entitlement allows it.
//!
//!     cargo run -p caskade-component-cache --example hardened_jit

/// `(module (func (export "answer") (result i32) i32.const 42))`
const MODULE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic, version
    0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // type: () -> i32
    0x03, 0x02, 0x01, 0x00, // function 0 has type 0
    0x07, 0x0a, 0x01, 0x06, b'a', b'n', b's', b'w', b'e', b'r', 0x00, 0x00, // export
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // code: i32.const 42
];

fn main() -> wasmtime::Result<()> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, MODULE)?;
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[])?;
    let answer = instance.get_typed_func::<(), i32>(&mut store, "answer")?;
    println!("{}", answer.call(&mut store, ())?);
    Ok(())
}
