fn main() {
    // One app manifest (Common Controls v6, the same file tauri-build would
    // embed), linked into EVERY target. Embedded by tauri-build it reaches
    // only the app binary; the unit-test executable imports the same comctl32
    // TaskDialogIndirect and then fails to start (STATUS_ENTRYPOINT_NOT_FOUND).
    #[cfg(windows)]
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
}
