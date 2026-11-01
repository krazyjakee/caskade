//! Emulation fixtures are generated and discarded, never recorded from users.
pub fn run(mut args: impl Iterator<Item = String>) -> crate::Result<()> {
    if args.next().as_deref() != Some("check") || args.next().is_some() {
        return Err("usage: cargo xtask emulation check".into());
    }
    let report = caskade_emulation_fixtures::validate()?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
