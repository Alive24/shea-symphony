use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=SHEA_SOURCE_REVISION");
    track_git_head();

    let source_revision = env::var("SHEA_SOURCE_REVISION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| git_stdout(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".into());
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".into());

    println!("cargo:rustc-env=SHEA_SOURCE_REVISION={source_revision}");
    println!("cargo:rustc-env=SHEA_TARGET_TRIPLE={target}");
}

fn track_git_head() {
    if let Some(head_path) = git_stdout(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head_path}");
    }
    if let Some(head_ref) = git_stdout(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(ref_path) = git_stdout(&["rev-parse", "--git-path", &head_ref]) {
            println!("cargo:rerun-if-changed={ref_path}");
        }
    }
}

fn git_stdout(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}
