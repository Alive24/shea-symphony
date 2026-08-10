fn main() {
    println!("cargo:rerun-if-env-changed=SHEA_SYMPHONY_REQUIRE_LEGACY_SIDECAR");
    if std::env::var_os("SHEA_SYMPHONY_REQUIRE_LEGACY_SIDECAR").is_some() {
        let target = std::env::var("TARGET").expect("Cargo TARGET is unavailable");
        let suffix = if target.contains("windows") {
            ".exe"
        } else {
            ""
        };
        let sidecar = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("binaries")
            .join(format!("shea-symphony-legacy-{target}{suffix}"));
        assert!(
            sidecar.is_file(),
            "missing required target-specific Legacy sidecar: {}",
            sidecar.display()
        );
    }
    tauri_build::build();
}
