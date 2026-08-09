use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::{json, Value};

use crate::runtime;
use crate::workspace::WorkspaceProfile;

pub const DEFAULT_WORKFLOW_PATH: &str = ".shea/workflows/shea-symphony.md";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSummary {
    pub ok: bool,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub timed_out: bool,
    pub duration_ms: u128,
    pub stderr: String,
    pub stdout_preview: String,
}

#[derive(Debug, Clone)]
pub struct CommandRun {
    pub summary: CommandSummary,
    pub stdout: String,
}

pub fn run_shea_read_for_workspace(args: &[String], workspace: &WorkspaceProfile) -> CommandRun {
    let started_at = Instant::now();
    let string_args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut command = match shea_command_for_workspace(&string_args, workspace) {
        Ok(command) => command,
        Err(error) => return command_run_from_error(args, started_at, error),
    };
    match command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => match child.wait_with_output() {
            Ok(output) => command_run_from_output(args, started_at, output, false),
            Err(error) => command_run_from_error(args, started_at, error.to_string()),
        },
        Err(error) => command_run_from_error(args, started_at, error.to_string()),
    }
}

pub fn command_summary_value(summary: &CommandSummary) -> Value {
    serde_json::to_value(summary).unwrap_or_else(|_| json!({ "ok": false }))
}

pub fn parse_json_output(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or(Value::Null)
}

pub fn pending_result(args: &[&str], reason: &str) -> Value {
    json!({
        "ok": false,
        "pending": true,
        "args": args,
        "exitCode": Value::Null,
        "signal": Value::Null,
        "timedOut": false,
        "durationMs": 0,
        "stderr": reason,
        "stdoutPreview": "",
    })
}

pub fn timestamp_iso_like() -> u128 {
    now_ms()
}

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub fn shea_command_for_workspace(
    args: &[&str],
    workspace: &WorkspaceProfile,
) -> Result<Command, String> {
    Ok(command_from_spec(shea_command_spec(args, workspace)?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheaCommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub current_dir: PathBuf,
}

pub fn shea_command_spec(
    args: &[&str],
    workspace: &WorkspaceProfile,
) -> Result<SheaCommandSpec, String> {
    shea_command_spec_with_discovery(args, workspace, &runtime::default_discovery_path())
}

fn shea_command_spec_with_discovery(
    args: &[&str],
    workspace: &WorkspaceProfile,
    discovery_path: &Path,
) -> Result<SheaCommandSpec, String> {
    let engine_root = workspace.engine_path();
    let target_root = workspace.target_path();
    if let Some(cli_path) = workspace
        .cli_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(cli_path);
        let program = if path.is_absolute() {
            path
        } else {
            target_root.join(path)
        };
        runtime::validate_explicit_runtime(&program)?;
        return Ok(SheaCommandSpec {
            program,
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            current_dir: target_root,
        });
    }
    if let Some(program) = runtime::resolve_installed_runtime(discovery_path)? {
        return Ok(SheaCommandSpec {
            program,
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            current_dir: target_root,
        });
    }
    if cfg!(debug_assertions) {
        let mut command_args = vec![
            "run".into(),
            "--quiet".into(),
            "--bin".into(),
            "shea-symphony-legacy".into(),
            "--manifest-path".into(),
            engine_root.join("Cargo.toml").display().to_string(),
            "--".into(),
        ];
        command_args.extend(args.iter().map(|arg| (*arg).to_string()));
        Ok(SheaCommandSpec {
            program: PathBuf::from("cargo"),
            args: command_args,
            current_dir: target_root,
        })
    } else {
        Err(format!(
            "no validated installed Legacy runtime was found at {}; configure explicit cli_path or reinstall the App bundle",
            discovery_path.display()
        ))
    }
}

pub fn command_preview_for_workspace(
    args: &[String],
    workspace: &WorkspaceProfile,
) -> Result<Vec<String>, String> {
    let string_args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let spec = shea_command_spec(&string_args, workspace)?;
    let mut preview = vec![spec.program.display().to_string()];
    preview.extend(spec.args);
    Ok(preview)
}

fn command_from_spec(spec: SheaCommandSpec) -> Command {
    let mut command = Command::new(spec.program);
    command.args(spec.args).current_dir(spec.current_dir);
    command
}

fn command_run_from_output(
    args: &[String],
    started_at: Instant,
    output: std::process::Output,
    timed_out: bool,
) -> CommandRun {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if timed_out {
        stderr = if stderr.is_empty() {
            "read command timed out".into()
        } else {
            format!("read command timed out\n{stderr}")
        };
    }
    CommandRun {
        summary: CommandSummary {
            ok: output.status.success() && !timed_out,
            args: args.to_vec(),
            exit_code: output.status.code(),
            signal: if timed_out {
                Some("timeout".into())
            } else {
                None
            },
            timed_out,
            duration_ms: started_at.elapsed().as_millis(),
            stderr,
            stdout_preview: stdout.trim().chars().take(6000).collect(),
        },
        stdout,
    }
}

fn command_run_from_error(args: &[String], started_at: Instant, error: String) -> CommandRun {
    CommandRun {
        summary: CommandSummary {
            ok: false,
            args: args.to_vec(),
            exit_code: None,
            signal: None,
            timed_out: false,
            duration_ms: started_at.elapsed().as_millis(),
            stderr: error,
            stdout_preview: String::new(),
        },
        stdout: String::new(),
    }
}

pub fn repo_root() -> PathBuf {
    let source_root = source_repo_root();
    canonical_main_worktree(&source_root).unwrap_or(source_root)
}

fn source_repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn canonical_main_worktree(source_root: &PathBuf) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(source_root)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_canonical_main_worktree(&text)
}

fn parse_canonical_main_worktree(text: &str) -> Option<PathBuf> {
    let mut current_worktree: Option<PathBuf> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_worktree = Some(PathBuf::from(path));
        } else if line == "branch refs/heads/main" {
            return current_worktree;
        } else if line.is_empty() {
            current_worktree = None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_canonical_main_worktree, shea_command_spec_with_discovery};
    use crate::workspace::WorkspaceProfile;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn parses_main_worktree_from_git_porcelain_output() {
        let output = "\
worktree /repo/feature
HEAD abc
branch refs/heads/feature/app

worktree /repo/main
HEAD def
branch refs/heads/main
";

        assert_eq!(
            parse_canonical_main_worktree(output),
            Some(PathBuf::from("/repo/main"))
        );
    }

    #[test]
    fn command_spec_uses_engine_manifest_and_target_cwd() {
        let engine_root = PathBuf::from("/engine/shea-symphony");
        let target_root = PathBuf::from("/target/repo");
        let profile = WorkspaceProfile {
            engine_root: engine_root.display().to_string(),
            target_root: target_root.display().to_string(),
            workflow_path: ".shea/workflows/shea-symphony.md".into(),
            cli_path: None,
            source: "test".into(),
            error: None,
        };

        let temp = TempDir::new().unwrap();
        let spec = shea_command_spec_with_discovery(
            &["autopilot", "plan", ".shea/workflows/shea-symphony.md"],
            &profile,
            &temp.path().join("missing-discovery.json"),
        )
        .unwrap();

        assert_eq!(spec.program, PathBuf::from("cargo"));
        assert_eq!(spec.current_dir, target_root);
        assert_eq!(
            spec.args,
            vec![
                "run",
                "--quiet",
                "--bin",
                "shea-symphony-legacy",
                "--manifest-path",
                "/engine/shea-symphony/Cargo.toml",
                "--",
                "autopilot",
                "plan",
                ".shea/workflows/shea-symphony.md",
            ]
        );
    }

    #[test]
    fn command_spec_uses_profile_cli_path_relative_to_target() {
        let temp = TempDir::new().unwrap();
        let invalid_discovery = temp.path().join("invalid-discovery.json");
        std::fs::write(&invalid_discovery, "not json").unwrap();
        let target_root = PathBuf::from("/target/repo");
        let profile = WorkspaceProfile {
            engine_root: "/engine/shea-symphony".into(),
            target_root: target_root.display().to_string(),
            workflow_path: ".shea/workflows/shea-symphony.md".into(),
            cli_path: Some(".shea/bin/shea-symphony".into()),
            source: "test".into(),
            error: None,
        };

        let spec = shea_command_spec_with_discovery(
            &["doctor", ".shea/workflows/shea-symphony.md"],
            &profile,
            &invalid_discovery,
        )
        .unwrap();

        assert_eq!(spec.program, target_root.join(".shea/bin/shea-symphony"));
        assert_eq!(spec.current_dir, target_root);
        assert_eq!(
            spec.args,
            vec!["doctor", ".shea/workflows/shea-symphony.md"]
        );
    }
}
