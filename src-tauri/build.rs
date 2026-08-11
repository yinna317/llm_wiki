use std::path::PathBuf;

fn main() {
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    let mut attrs = tauri_build::Attributes::new().windows_attributes(windows);

    // Headless builds omit the dialog plugin (rfd -> gtk on Linux), so the
    // `dialog:default` permission in capabilities/default.json would fail
    // tauri-build's validation. Capabilities only scope webview IPC, which
    // the headless MockRuntime never exercises, so we substitute a minimal
    // capability file (default.json minus dialog) generated into OUT_DIR and
    // point tauri-build at it via a custom glob pattern.
    if std::env::var("CARGO_FEATURE_HEADLESS").is_ok() {
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR set"));
        let cap_dir = out_dir.join("capabilities");
        std::fs::create_dir_all(&cap_dir).expect("create capabilities dir");
        std::fs::write(cap_dir.join("default.json"), HEADLESS_CAPABILITIES)
            .expect("write headless capability");
        let pattern = format!("{}/capabilities/**/*", out_dir.display());
        // Build scripts are short-lived; leaking the pattern to obtain the
        // required &'static str is fine.
        let pattern: &'static str = Box::leak(pattern.into_boxed_str());
        attrs = attrs.capabilities_path_pattern(pattern);
    }

    tauri_build::try_build(attrs).expect("failed to run tauri build script");
}

/// Mirror of capabilities/default.json with the `dialog:default` permission
/// removed (dialog plugin is absent in headless builds).
const HEADLESS_CAPABILITIES: &str = r#"{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window (headless: no dialog plugin)",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-set-background-color",
    "core:window:allow-set-theme",
    "autostart:default",
    "opener:default",
    "store:default",
    {
      "identifier": "http:default",
      "allow": [
        { "url": "http://*" },
        { "url": "http://*/*" },
        { "url": "http://*:*" },
        { "url": "http://*:*/*" },
        { "url": "http://**" },
        { "url": "https://*" },
        { "url": "https://*/*" },
        { "url": "https://*:*" },
        { "url": "https://*:*/*" },
        { "url": "https://**" }
      ]
    }
  ]
}
"#;
