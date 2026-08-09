use std::process::{Command, Output};

use shea_symphony::runtime_identity::{RuntimeIdentity, RuntimeRole};

#[test]
fn both_executables_report_distinct_roles_from_one_build() {
    let temporal = runtime_identity(env!("CARGO_BIN_EXE_shea-symphony"));
    let legacy = runtime_identity(env!("CARGO_BIN_EXE_shea-symphony-legacy"));

    assert_eq!(temporal.binary_role, RuntimeRole::TemporalWorker);
    assert_eq!(legacy.binary_role, RuntimeRole::LegacyCli);
    assert_eq!(temporal.source_revision, legacy.source_revision);
    assert_eq!(temporal.cli_version, legacy.cli_version);
    assert_eq!(temporal.target, legacy.target);
}

#[test]
fn legacy_help_does_not_require_temporal() {
    let output = Command::new(env!("CARGO_BIN_EXE_shea-symphony-legacy"))
        .arg("--help")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Lane orchestration:"));
    assert!(stdout.contains("Issue Forge:"));
}

fn runtime_identity(binary: &str) -> RuntimeIdentity {
    let output = Command::new(binary).arg("--runtime-info").output().unwrap();
    assert_success(&output);
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
