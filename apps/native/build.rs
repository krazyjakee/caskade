//! Windows: the exe's resources, as Tauri's build gives the Tauri app's:
//! the app icon, and a manifest asking for Common Controls 6 (the dialogs'
//! TaskDialogIndirect, `dialogs.rs`, isn't in older comctl32: without it
//! the exe doesn't start) and per-monitor DPI awareness.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = out.join("caskade-native.manifest");
    std::fs::write(
        &manifest,
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#,
    )
    .expect("the manifest is written");
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../desktop/src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let rc = out.join("caskade-native.rc");
    let path = |path: &std::path::Path| path.display().to_string().replace('\\', "/");
    std::fs::write(
        &rc,
        format!("1 24 \"{}\"\n1 ICON \"{}\"\n", path(&manifest), path(&icon)),
    )
    .expect("the resource script is written");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .expect("the exe's resources compile");
}
