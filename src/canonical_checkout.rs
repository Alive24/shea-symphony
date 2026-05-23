use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::artifacts::artifact_layout;
use crate::config::RuntimeConfig;
use crate::workspace::safe_identifier;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCheckoutReport {
    pub root: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub upstream_head: Option<String>,
    pub tracked_dirty: Vec<String>,
    pub untracked: Vec<CanonicalUntrackedPath>,
    pub migrated: Vec<CanonicalMigratedPath>,
    pub quarantine_root: PathBuf,
}

impl CanonicalCheckoutReport {
    pub fn is_clean(&self) -> bool {
        self.tracked_dirty.is_empty() && self.untracked.is_empty()
    }

    pub fn unclassified_untracked(&self) -> Vec<&CanonicalUntrackedPath> {
        self.untracked
            .iter()
            .filter(|entry| entry.kind.is_none())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanonicalCheckoutRefreshMode {
    Apply,
    DryRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanonicalCheckoutRefreshAction {
    AlreadyCurrent,
    FfOnly,
    WouldFfOnly,
}

impl CanonicalCheckoutRefreshAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyCurrent => "already_current",
            Self::FfOnly => "ff_only",
            Self::WouldFfOnly => "would_ff_only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalCheckoutRefreshReport {
    pub action: CanonicalCheckoutRefreshAction,
    pub root: PathBuf,
    pub upstream: String,
    pub head_before: String,
    pub upstream_head: String,
    pub head_after: String,
    pub checkout: CanonicalCheckoutReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalUntrackedPath {
    pub path: PathBuf,
    pub kind: Option<CanonicalArtifactKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalMigratedPath {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub kind: CanonicalArtifactKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanonicalArtifactKind {
    Runtime,
    Log,
    Prompt,
    Evidence,
    Draft,
    Scratch,
}

impl fmt::Display for CanonicalArtifactKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Runtime => "runtime",
            Self::Log => "log",
            Self::Prompt => "prompt",
            Self::Evidence => "evidence",
            Self::Draft => "draft",
            Self::Scratch => "scratch",
        })
    }
}

#[derive(Debug, Error)]
pub enum CanonicalCheckoutError {
    #[error("canonical checkout git status failed in {root}: status={status} stdout={stdout} stderr={stderr}")]
    GitStatusFailed {
        root: PathBuf,
        status: i32,
        stdout: String,
        stderr: String,
    },
    #[error("canonical checkout is blocked: tracked dirty files in {root}: {paths}. Move these edits into an issue worktree, commit them, or restore them before running a live write lane.")]
    TrackedDirty { root: PathBuf, paths: String },
    #[error("canonical checkout is blocked: unclassified untracked files in {root}: {paths}. Move them to an issue worktree or artifact location, or add legitimate ignored files to .gitignore before running a live write lane.")]
    UnclassifiedUntracked { root: PathBuf, paths: String },
    #[error("canonical checkout is blocked: {root} is detached at {head}. Switch the canonical checkout back to clean latest `main` before running a live write lane.")]
    Detached { root: PathBuf, head: String },
    #[error("canonical checkout is blocked: {root} is on branch `{branch}` instead of `main`. Do local inspection in an issue worktree; keep the canonical checkout on clean latest `main`.")]
    NonMain { root: PathBuf, branch: String },
    #[error("canonical checkout is blocked: local `main` at {head} does not match upstream `{upstream}` at {upstream_head}. Refresh the canonical checkout to latest `main` before running a live write lane.")]
    StaleMain {
        root: PathBuf,
        head: String,
        upstream: String,
        upstream_head: String,
    },
    #[error("canonical checkout refresh is blocked: local `main` in {root} has no upstream. Configure it to track `origin/main` before running a live write lane.")]
    MissingUpstream { root: PathBuf },
    #[error("canonical checkout refresh is blocked: {root} is under workflow workspace root {workspace_root}. Run write-mode lane/control commands from the canonical checkout, not an issue worktree.")]
    IssueWorktree {
        root: PathBuf,
        workspace_root: PathBuf,
    },
    #[error("canonical checkout refresh failed: git fetch {remote} {branch} failed in {root}: status={status} stdout={stdout} stderr={stderr}")]
    GitFetchFailed {
        root: PathBuf,
        remote: String,
        branch: String,
        status: i32,
        stdout: String,
        stderr: String,
    },
    #[error("canonical checkout refresh failed: git merge-base --is-ancestor {head} {upstream_head} failed in {root}: status={status} stdout={stdout} stderr={stderr}")]
    GitMergeBaseFailed {
        root: PathBuf,
        head: String,
        upstream_head: String,
        status: i32,
        stdout: String,
        stderr: String,
    },
    #[error("canonical checkout refresh is blocked: local `main` at {head} cannot fast-forward to upstream `{upstream}` at {upstream_head}.")]
    NonFastForward {
        root: PathBuf,
        head: String,
        upstream: String,
        upstream_head: String,
    },
    #[error("canonical checkout refresh failed: git merge --ff-only {upstream} failed in {root}: status={status} stdout={stdout} stderr={stderr}")]
    GitMergeFailed {
        root: PathBuf,
        upstream: String,
        status: i32,
        stdout: String,
        stderr: String,
    },
    #[error(
        "failed to migrate canonical checkout artifact {artifact_path} to {destination}: {error}"
    )]
    MigrationFailed {
        artifact_path: PathBuf,
        destination: PathBuf,
        error: String,
    },
    #[error("canonical checkout io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn inspect_canonical_checkout(
    root: &Path,
    config: &RuntimeConfig,
) -> Result<CanonicalCheckoutReport, CanonicalCheckoutError> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let status = git_status_entries(&root)?;
    let branch = current_branch(&root)?;
    let head = git_rev_parse(&root, "HEAD")?;
    let upstream = upstream_branch(&root)?;
    let upstream_head = upstream
        .as_deref()
        .map(|upstream| git_rev_parse(&root, upstream))
        .transpose()?
        .flatten();
    let quarantine_root = canonical_quarantine_root(config);
    let mut tracked_dirty = Vec::new();
    let mut untracked = Vec::new();

    for entry in status {
        match entry {
            GitStatusEntry::Tracked(path) => tracked_dirty.push(path),
            GitStatusEntry::Untracked(path) => {
                let kind = classify_untracked_artifact(&path);
                untracked.push(CanonicalUntrackedPath { path, kind });
            }
        }
    }

    Ok(CanonicalCheckoutReport {
        root,
        branch,
        head,
        upstream,
        upstream_head,
        tracked_dirty,
        untracked,
        migrated: Vec::new(),
        quarantine_root,
    })
}

pub fn enforce_clean_canonical_checkout_for_write(
    root: &Path,
    config: &RuntimeConfig,
) -> Result<CanonicalCheckoutReport, CanonicalCheckoutError> {
    let mut report = inspect_canonical_checkout(root, config)?;
    ensure_not_issue_worktree(&report, config)?;
    ensure_attached_main(&report)?;
    ensure_clean_enough_for_write(&report)?;
    migrate_classified_artifacts(&mut report)?;
    if !report.migrated.is_empty() {
        let migrated = report.migrated.clone();
        report = inspect_canonical_checkout(root, config)?;
        report.migrated = migrated;
        ensure_attached_main(&report)?;
        ensure_clean_enough_for_write(&report)?;
    }
    if let (Some(upstream), Some(upstream_head), Some(head)) = (
        report.upstream.as_deref(),
        report.upstream_head.as_deref(),
        report.head.as_deref(),
    ) {
        if head != upstream_head {
            return Err(CanonicalCheckoutError::StaleMain {
                root: report.root.clone(),
                head: head.to_string(),
                upstream: upstream.to_string(),
                upstream_head: upstream_head.to_string(),
            });
        }
    }

    Ok(report)
}

pub fn refresh_canonical_checkout_before_write(
    root: &Path,
    config: &RuntimeConfig,
    mode: CanonicalCheckoutRefreshMode,
) -> Result<CanonicalCheckoutRefreshReport, CanonicalCheckoutError> {
    let mut report = inspect_canonical_checkout(root, config)?;
    ensure_not_issue_worktree(&report, config)?;
    ensure_attached_main(&report)?;
    ensure_clean_enough_for_write(&report)?;
    let upstream = require_upstream(&report)?;

    if matches!(mode, CanonicalCheckoutRefreshMode::Apply) {
        migrate_classified_artifacts(&mut report)?;
        fetch_upstream(&report.root, &upstream)?;
        let migrated = report.migrated.clone();
        report = inspect_canonical_checkout(root, config)?;
        report.migrated = migrated;
        ensure_attached_main(&report)?;
        ensure_clean_enough_for_write(&report)?;
    }

    let head_before = report.head.as_deref().unwrap_or("unknown").to_string();
    let upstream_head = report
        .upstream_head
        .as_deref()
        .ok_or_else(|| CanonicalCheckoutError::MissingUpstream {
            root: report.root.clone(),
        })?
        .to_string();

    if head_before == upstream_head {
        return Ok(CanonicalCheckoutRefreshReport {
            action: CanonicalCheckoutRefreshAction::AlreadyCurrent,
            root: report.root.clone(),
            upstream,
            head_before: head_before.clone(),
            upstream_head,
            head_after: head_before,
            checkout: report,
        });
    }

    if !git_can_fast_forward(&report.root, &head_before, &upstream_head)? {
        return Err(CanonicalCheckoutError::NonFastForward {
            root: report.root.clone(),
            head: head_before,
            upstream,
            upstream_head,
        });
    }

    if matches!(mode, CanonicalCheckoutRefreshMode::DryRun) {
        return Ok(CanonicalCheckoutRefreshReport {
            action: CanonicalCheckoutRefreshAction::WouldFfOnly,
            root: report.root.clone(),
            upstream,
            head_before: head_before.clone(),
            upstream_head,
            head_after: head_before,
            checkout: report,
        });
    }

    merge_ff_only(&report.root, &upstream)?;
    let migrated = report.migrated.clone();
    let mut final_report = enforce_clean_canonical_checkout_for_write(root, config)?;
    final_report.migrated = migrated;
    let head_after = final_report
        .head
        .as_deref()
        .unwrap_or("unknown")
        .to_string();

    Ok(CanonicalCheckoutRefreshReport {
        action: CanonicalCheckoutRefreshAction::FfOnly,
        root: final_report.root.clone(),
        upstream,
        head_before,
        upstream_head,
        head_after,
        checkout: final_report,
    })
}

pub fn canonical_checkout_status_line(report: &CanonicalCheckoutReport) -> String {
    let unclassified = report.unclassified_untracked().len();
    format!(
        "canonical_checkout root={} branch={} upstream={} clean={} tracked_dirty={} untracked={} unclassified={} migrated={} quarantine={}",
        report.root.display(),
        report.branch.as_deref().unwrap_or("detached"),
        report.upstream.as_deref().unwrap_or("none"),
        report.is_clean(),
        report.tracked_dirty.len(),
        report.untracked.len(),
        unclassified,
        report.migrated.len(),
        report.quarantine_root.display()
    )
}

pub fn canonical_checkout_refresh_status_line(report: &CanonicalCheckoutRefreshReport) -> String {
    format!(
        "canonical_checkout_refresh={} upstream={} head_before={} upstream_head={} head_after={}",
        report.action.as_str(),
        report.upstream,
        report.head_before,
        report.upstream_head,
        report.head_after
    )
}

pub fn canonical_checkout_warning_lines(report: &CanonicalCheckoutReport) -> Vec<String> {
    report
        .migrated
        .iter()
        .map(|entry| {
            format!(
                "canonical_checkout_migrated kind={} source={} destination={}",
                entry.kind,
                entry.source.display(),
                entry.destination.display()
            )
        })
        .collect()
}

pub fn canonical_quarantine_root(config: &RuntimeConfig) -> PathBuf {
    artifact_layout(config)
        .scratch
        .join("canonical-checkout-quarantine")
}

fn ensure_not_issue_worktree(
    report: &CanonicalCheckoutReport,
    config: &RuntimeConfig,
) -> Result<(), CanonicalCheckoutError> {
    let workspace_root = config
        .workspace
        .root
        .canonicalize()
        .unwrap_or_else(|_| config.workspace.root.clone());
    if report.root == workspace_root || report.root.starts_with(&workspace_root) {
        return Err(CanonicalCheckoutError::IssueWorktree {
            root: report.root.clone(),
            workspace_root,
        });
    }
    Ok(())
}

fn ensure_attached_main(report: &CanonicalCheckoutReport) -> Result<(), CanonicalCheckoutError> {
    let head = report.head.as_deref().unwrap_or("unknown").to_string();
    let Some(branch) = report.branch.as_deref() else {
        return Err(CanonicalCheckoutError::Detached {
            root: report.root.clone(),
            head,
        });
    };
    if branch != "main" {
        return Err(CanonicalCheckoutError::NonMain {
            root: report.root.clone(),
            branch: branch.to_string(),
        });
    }
    Ok(())
}

fn ensure_clean_enough_for_write(
    report: &CanonicalCheckoutReport,
) -> Result<(), CanonicalCheckoutError> {
    if !report.tracked_dirty.is_empty() {
        return Err(CanonicalCheckoutError::TrackedDirty {
            root: report.root.clone(),
            paths: report.tracked_dirty.join(", "),
        });
    }

    let unclassified_paths = report
        .unclassified_untracked()
        .iter()
        .map(|entry| entry.path.display().to_string())
        .collect::<Vec<_>>();
    if !unclassified_paths.is_empty() {
        return Err(CanonicalCheckoutError::UnclassifiedUntracked {
            root: report.root.clone(),
            paths: unclassified_paths.join(", "),
        });
    }
    Ok(())
}

fn require_upstream(report: &CanonicalCheckoutReport) -> Result<String, CanonicalCheckoutError> {
    report
        .upstream
        .clone()
        .filter(|upstream| !upstream.trim().is_empty())
        .ok_or_else(|| CanonicalCheckoutError::MissingUpstream {
            root: report.root.clone(),
        })
}

fn migrate_classified_artifacts(
    report: &mut CanonicalCheckoutReport,
) -> Result<(), CanonicalCheckoutError> {
    let run_root = report
        .quarantine_root
        .join(format!("run-{}", unix_timestamp_ms()));
    for entry in report.untracked.clone() {
        let Some(kind) = entry.kind else {
            continue;
        };
        let source = report.root.join(&entry.path);
        let destination = run_root.join(kind.to_string()).join(&entry.path);
        move_path(&source, &destination).map_err(|error| {
            CanonicalCheckoutError::MigrationFailed {
                artifact_path: source.clone(),
                destination: destination.clone(),
                error: error.to_string(),
            }
        })?;
        report.migrated.push(CanonicalMigratedPath {
            source,
            destination,
            kind,
        });
    }

    if !report.migrated.is_empty() {
        write_manifest(&run_root, &report.migrated)?;
    }
    Ok(())
}

fn fetch_upstream(root: &Path, upstream: &str) -> Result<(), CanonicalCheckoutError> {
    let (remote, branch) =
        upstream
            .split_once('/')
            .ok_or_else(|| CanonicalCheckoutError::MissingUpstream {
                root: root.to_path_buf(),
            })?;
    let output = Command::new("git")
        .args(["fetch", remote, branch])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(CanonicalCheckoutError::GitFetchFailed {
            root: root.to_path_buf(),
            remote: remote.into(),
            branch: branch.into(),
            status: output.status.code().unwrap_or(-1),
            stdout: single_line(&String::from_utf8_lossy(&output.stdout)),
            stderr: single_line(&String::from_utf8_lossy(&output.stderr)),
        });
    }
    Ok(())
}

fn git_can_fast_forward(
    root: &Path,
    head: &str,
    upstream_head: &str,
) -> Result<bool, CanonicalCheckoutError> {
    let output = Command::new("git")
        .args(["merge-base", "--is-ancestor", head, upstream_head])
        .current_dir(root)
        .output()?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(1) {
        return Ok(false);
    }
    Err(CanonicalCheckoutError::GitMergeBaseFailed {
        root: root.to_path_buf(),
        head: head.into(),
        upstream_head: upstream_head.into(),
        status: output.status.code().unwrap_or(-1),
        stdout: single_line(&String::from_utf8_lossy(&output.stdout)),
        stderr: single_line(&String::from_utf8_lossy(&output.stderr)),
    })
}

fn merge_ff_only(root: &Path, upstream: &str) -> Result<(), CanonicalCheckoutError> {
    let output = Command::new("git")
        .args(["merge", "--ff-only", upstream])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(CanonicalCheckoutError::GitMergeFailed {
            root: root.to_path_buf(),
            upstream: upstream.into(),
            status: output.status.code().unwrap_or(-1),
            stdout: single_line(&String::from_utf8_lossy(&output.stdout)),
            stderr: single_line(&String::from_utf8_lossy(&output.stderr)),
        });
    }
    Ok(())
}

enum GitStatusEntry {
    Tracked(String),
    Untracked(PathBuf),
}

fn git_status_entries(root: &Path) -> Result<Vec<GitStatusEntry>, CanonicalCheckoutError> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(CanonicalCheckoutError::GitStatusFailed {
            root: root.to_path_buf(),
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    let mut entries = Vec::new();
    for raw in output.stdout.split(|byte| *byte == 0) {
        if raw.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(raw).to_string();
        if let Some(path) = text.strip_prefix("?? ") {
            entries.push(GitStatusEntry::Untracked(PathBuf::from(path)));
        } else {
            entries.push(GitStatusEntry::Tracked(text));
        }
    }
    Ok(entries)
}

fn current_branch(root: &Path) -> Result<Option<String>, CanonicalCheckoutError> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(root)
        .output()?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    } else {
        Ok(None)
    }
}

fn upstream_branch(root: &Path) -> Result<Option<String>, CanonicalCheckoutError> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .current_dir(root)
        .output()?;
    if output.status.success() {
        let upstream = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if upstream.is_empty() {
            Ok(None)
        } else {
            Ok(Some(upstream))
        }
    } else {
        Ok(None)
    }
}

fn git_rev_parse(root: &Path, rev: &str) -> Result<Option<String>, CanonicalCheckoutError> {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(root)
        .output()?;
    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    } else {
        Ok(None)
    }
}

fn classify_untracked_artifact(path: &Path) -> Option<CanonicalArtifactKind> {
    let normalized = path
        .components()
        .filter_map(component_text)
        .map(|component| safe_identifier(&component).to_ascii_lowercase())
        .collect::<Vec<_>>();
    let text = normalized.join("/");
    let file = normalized.last().map(String::as_str).unwrap_or_default();

    if text.contains("runtime") || file.ends_with(".runtime.json") {
        return Some(CanonicalArtifactKind::Runtime);
    }
    if text.contains("prompt") || file.ends_with(".prompt.md") || file == "jade_symphony_prompt_md"
    {
        return Some(CanonicalArtifactKind::Prompt);
    }
    if text.contains("evidence") || text.contains("handoff") || text.contains("ledger") {
        return Some(CanonicalArtifactKind::Evidence);
    }
    if text.contains("workpad") || text.contains("pr_body") || text.contains("draft") {
        return Some(CanonicalArtifactKind::Draft);
    }
    if file.ends_with(".log") || file.ends_with(".jsonl") || text.contains("logs") {
        return Some(CanonicalArtifactKind::Log);
    }
    if text.contains("jade_symphony") || text.contains("scratch") || text.contains("tmp") {
        return Some(CanonicalArtifactKind::Scratch);
    }

    None
}

fn component_text(component: Component<'_>) -> Option<String> {
    match component {
        Component::Normal(value) => Some(value.to_string_lossy().to_string()),
        _ => None,
    }
}

fn move_path(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let metadata = fs::symlink_metadata(source)?;
            if metadata.is_file() {
                fs::copy(source, destination)?;
                fs::remove_file(source)
            } else if metadata.is_dir() {
                copy_dir_recursive(source, destination)?;
                fs::remove_dir_all(source)
            } else {
                Err(rename_error)
            }
        }
    }
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn write_manifest(
    run_root: &Path,
    migrated: &[CanonicalMigratedPath],
) -> Result<(), CanonicalCheckoutError> {
    fs::create_dir_all(run_root)?;
    let mut lines = vec![
        "# Canonical Checkout Quarantine Manifest".to_string(),
        String::new(),
        "These files were moved out of the canonical checkout before a live write lane mutated tracker state.".to_string(),
        String::new(),
    ];
    for entry in migrated {
        lines.push(format!(
            "- kind={} source={} destination={}",
            entry.kind,
            entry.source.display(),
            entry.destination.display()
        ));
    }
    fs::write(run_root.join("MANIFEST.md"), lines.join("\n"))?;
    Ok(())
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unix_timestamp_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowDefinition;
    use std::process::Command;

    fn config(root: &Path) -> RuntimeConfig {
        let markdown = format!(
            "---\ntracker:\n  kind: memory\nartifacts:\n  root: {}\nworkspace:\n  root: {}/worktrees\nobservability:\n  logs_root: {}/logs\n---\nPrompt",
            root.display(),
            root.display(),
            root.display()
        );
        let workflow = WorkflowDefinition::parse("/tmp/WORKFLOW.md", &markdown).unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn init_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(path)
            .status()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-q", "-B", "main"])
            .current_dir(path)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .status()
            .unwrap();
        fs::write(path.join("tracked.txt"), "clean\n").unwrap();
        Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(path)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(path)
            .status()
            .unwrap();
    }

    fn git_ok(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap_or_else(|error| panic!("git {args:?} failed to start: {error}"));
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout={}\nstderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_head(path: &Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo_with_origin(root: &Path) -> (PathBuf, PathBuf) {
        init_repo_with_origin_at(root, "repo")
    }

    fn init_repo_with_origin_at(root: &Path, repo_rel: &str) -> (PathBuf, PathBuf) {
        let remote = root.join("origin.git");
        let repo = root.join(repo_rel);
        git_ok(
            root,
            &["init", "--bare", "--initial-branch=main", "origin.git"],
        );
        if let Some(parent) = repo.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        git_ok(
            root,
            &["init", "--initial-branch=main", repo.to_str().unwrap()],
        );
        git_ok(&repo, &["config", "user.email", "test@example.com"]);
        git_ok(&repo, &["config", "user.name", "Test User"]);
        fs::write(repo.join("tracked.txt"), "clean\n").unwrap();
        git_ok(&repo, &["add", "tracked.txt"]);
        git_ok(&repo, &["commit", "-qm", "init"]);
        git_ok(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_ok(&repo, &["push", "-u", "origin", "main"]);
        (repo, remote)
    }

    fn advance_remote(root: &Path, remote: &Path, file: &str, text: &str) -> String {
        let other = root.join(format!("other-{}", safe_identifier(file)));
        git_ok(
            root,
            &["clone", remote.to_str().unwrap(), other.to_str().unwrap()],
        );
        git_ok(&other, &["config", "user.email", "test@example.com"]);
        git_ok(&other, &["config", "user.name", "Test User"]);
        fs::write(other.join(file), text).unwrap();
        git_ok(&other, &["add", file]);
        git_ok(&other, &["commit", "-qm", "advance main"]);
        git_ok(&other, &["push", "origin", "main"]);
        git_head(&other)
    }

    #[test]
    fn refresh_reports_already_current() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, _remote) = init_repo_with_origin(temp.path());

        let report = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap();

        assert_eq!(
            report.action,
            CanonicalCheckoutRefreshAction::AlreadyCurrent
        );
        assert!(canonical_checkout_refresh_status_line(&report)
            .contains("canonical_checkout_refresh=already_current"));
        assert_eq!(report.head_before, report.head_after);
    }

    #[test]
    fn refresh_fast_forwards_clean_main_behind_origin() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, remote) = init_repo_with_origin(temp.path());
        let remote_head = advance_remote(temp.path(), &remote, "CHANGELOG.md", "change\n");

        let report = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap();

        assert_eq!(report.action, CanonicalCheckoutRefreshAction::FfOnly);
        assert_eq!(report.head_after, remote_head);
        assert_eq!(git_head(&repo), remote_head);
    }

    #[test]
    fn refresh_dry_run_reports_would_ff_only_without_changing_head() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, remote) = init_repo_with_origin(temp.path());
        advance_remote(temp.path(), &remote, "CHANGELOG.md", "change\n");
        git_ok(&repo, &["fetch", "origin", "main"]);
        let before = git_head(&repo);

        let report = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::DryRun,
        )
        .unwrap();

        assert_eq!(report.action, CanonicalCheckoutRefreshAction::WouldFfOnly);
        assert_eq!(git_head(&repo), before);
        assert_eq!(report.head_after, before);
    }

    #[test]
    fn refresh_blocks_dirty_canonical_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, _remote) = init_repo_with_origin(temp.path());
        fs::write(repo.join("tracked.txt"), "dirty\n").unwrap();

        let error = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap_err();

        assert!(error.to_string().contains("tracked dirty files"));
    }

    #[test]
    fn refresh_blocks_non_main_canonical_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, _remote) = init_repo_with_origin(temp.path());
        git_ok(&repo, &["checkout", "-q", "-b", "feature/test"]);

        let error = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap_err();

        assert!(error.to_string().contains("instead of `main`"));
    }

    #[test]
    fn refresh_blocks_detached_canonical_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, _remote) = init_repo_with_origin(temp.path());
        let head = git_head(&repo);
        git_ok(&repo, &["checkout", "-q", "--detach", &head]);

        let error = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap_err();

        assert!(error.to_string().contains("detached"));
    }

    #[test]
    fn refresh_blocks_missing_upstream() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);

        let error = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap_err();

        assert!(error.to_string().contains("has no upstream"));
    }

    #[test]
    fn refresh_blocks_issue_worktree_under_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, _remote) = init_repo_with_origin_at(temp.path(), "worktrees/issue-344");

        let error = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap_err();

        assert!(error.to_string().contains("workflow workspace root"));
    }

    #[test]
    fn refresh_blocks_non_fast_forward_update() {
        let temp = tempfile::tempdir().unwrap();
        let (repo, remote) = init_repo_with_origin(temp.path());
        fs::write(repo.join("local.txt"), "local\n").unwrap();
        git_ok(&repo, &["add", "local.txt"]);
        git_ok(&repo, &["commit", "-qm", "local change"]);
        advance_remote(temp.path(), &remote, "remote.txt", "remote\n");

        let error = refresh_canonical_checkout_before_write(
            &repo,
            &config(temp.path()),
            CanonicalCheckoutRefreshMode::Apply,
        )
        .unwrap_err();

        assert!(error.to_string().contains("cannot fast-forward"));
    }

    #[test]
    fn tracked_dirty_blocks_live_write() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "dirty\n").unwrap();

        let error =
            enforce_clean_canonical_checkout_for_write(&repo, &config(temp.path())).unwrap_err();

        assert!(error.to_string().contains("tracked dirty files"));
        assert!(error.to_string().contains("tracked.txt"));
    }

    #[test]
    fn unclassified_untracked_blocks_live_write() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("notes.txt"), "operator notes\n").unwrap();

        let error =
            enforce_clean_canonical_checkout_for_write(&repo, &config(temp.path())).unwrap_err();

        assert!(error.to_string().contains("unclassified untracked files"));
        assert!(repo.join("notes.txt").exists());
    }

    #[test]
    fn classified_untracked_artifact_is_migrated() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("handoff-evidence.md"), "evidence\n").unwrap();

        let report =
            enforce_clean_canonical_checkout_for_write(&repo, &config(temp.path())).unwrap();

        assert_eq!(report.migrated.len(), 1);
        assert!(!repo.join("handoff-evidence.md").exists());
        assert!(report.migrated[0].destination.exists());
        assert_eq!(report.migrated[0].kind, CanonicalArtifactKind::Evidence);
    }

    #[test]
    fn non_main_branch_blocks_live_write() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        Command::new("git")
            .args(["checkout", "-q", "-b", "feature/review-local-inspection"])
            .current_dir(&repo)
            .status()
            .unwrap();

        let error =
            enforce_clean_canonical_checkout_for_write(&repo, &config(temp.path())).unwrap_err();

        assert!(error.to_string().contains("instead of `main`"));
    }

    #[test]
    fn detached_checkout_blocks_live_write() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        Command::new("git")
            .args(["checkout", "-q", "--detach", &head])
            .current_dir(&repo)
            .status()
            .unwrap();

        let error =
            enforce_clean_canonical_checkout_for_write(&repo, &config(temp.path())).unwrap_err();

        assert!(error.to_string().contains("detached"));
    }
}
