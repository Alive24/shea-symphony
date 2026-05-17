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

    Ok(report)
}

pub fn canonical_checkout_status_line(report: &CanonicalCheckoutReport) -> String {
    let unclassified = report.unclassified_untracked().len();
    format!(
        "canonical_checkout root={} clean={} tracked_dirty={} untracked={} unclassified={} migrated={} quarantine={}",
        report.root.display(),
        report.is_clean(),
        report.tracked_dirty.len(),
        report.untracked.len(),
        unclassified,
        report.migrated.len(),
        report.quarantine_root.display()
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
}
