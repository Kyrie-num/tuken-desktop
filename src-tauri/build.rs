fn main() {
    // Link the VC++ runtime into the exe. A fresh Windows has no
    // VCRUNTIME140(_1).dll unless another program installed the
    // redistributable, and the app then fails to start.
    std::env::set_var("STATIC_VCRUNTIME", "true");
    tauri_build::build();

    // Test binaries link Tauri but get no application manifest, so they would
    // fail to start without Common Controls v6. The app binary gets its
    // manifest from Tauri, hence /MANIFEST:NO for bins.
    #[cfg(target_os = "windows")]
    {
        let manifest = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        println!("cargo:rerun-if-changed={}", manifest.display());
    }
}
