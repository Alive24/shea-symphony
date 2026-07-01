use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::{json, Value};

pub use crate::target_context::DEFAULT_WORKFLOW_PATH;

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

pub fn run_shea_read(args: &[String]) -> CommandRun {
    let started_at = Instant::now();
    let string_args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut command = shea_command(&string_args);
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

pub fn shea_command(args: &[&str]) -> Command {
    let repo_root = repo_root();
    if should_use_cargo_runner() {
        let mut command = Command::new("cargo");
        command
            .args(["run", "--quiet", "--"])
            .args(args)
            .current_dir(repo_root);
        command
    } else {
        let binary = repo_root.join("target").join("debug").join("shea-symphony");
        let mut command = Command::new(binary);
        command.args(args).current_dir(repo_root);
        command
    }
}

pub fn command_preview(args: &[String]) -> Vec<String> {
    if should_use_cargo_runner() {
        let mut preview = vec!["cargo".into(), "run".into(), "--quiet".into(), "--".into()];
        preview.extend(args.iter().cloned());
        preview
    } else {
        let repo_root = repo_root();
        let binary = repo_root.join("target").join("debug").join("shea-symphony");
        let mut preview = vec![binary.display().to_string()];
        preview.extend(args.iter().cloned());
        preview
    }
}

fn should_use_cargo_runner() -> bool {
    cfg!(debug_assertions)
        || !repo_root()
            .join("target")
            .join("debug")
            .join("shea-symphony")
            .exists()
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

fn repo_root() -> PathBuf {
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
    use super::parse_canonical_main_worktree;
    use std::path::PathBuf;

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
}
