use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const TARGET_RUNTIME_EXAMPLE_DIR: &str = ".shea-example";
pub const TARGET_RUNTIME_DIR: &str = ".shea";
const LOCAL_EXCLUDE_ENTRY: &str = ".shea/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetRuntimeReport {
    pub target_path: PathBuf,
    pub example_path: PathBuf,
    pub runtime_path: PathBuf,
    pub local_exclude_path: Option<PathBuf>,
    pub state: TargetRuntimeState,
    pub initialized: bool,
    pub locally_ignored: bool,
    pub conflict: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetRuntimeState {
    ReadyToInitialize,
    Initialized,
    ExistingRuntime,
    MissingExample,
}

#[derive(Debug, Error)]
pub enum TargetRuntimeError {
    #[error("target workspace is not a directory: {0}")]
    TargetNotDirectory(PathBuf),
    #[error("failed to copy {from} to {to}: {source}")]
    Copy {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to update local git exclude at {path}: {source}")]
    Exclude {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to resolve git exclude path for {target}: {message}")]
    GitExclude { target: PathBuf, message: String },
}

pub fn inspect_target_runtime(
    target_path: impl AsRef<Path>,
) -> Result<TargetRuntimeReport, TargetRuntimeError> {
    report(target_path.as_ref(), false)
}

pub fn initialize_target_runtime(
    target_path: impl AsRef<Path>,
) -> Result<TargetRuntimeReport, TargetRuntimeError> {
    report(target_path.as_ref(), true)
}

fn report(target_path: &Path, write: bool) -> Result<TargetRuntimeReport, TargetRuntimeError> {
    let target_path = target_path.to_path_buf();
    if !target_path.is_dir() {
        return Err(TargetRuntimeError::TargetNotDirectory(target_path));
    }

    let example_path = target_path.join(TARGET_RUNTIME_EXAMPLE_DIR);
    let runtime_path = target_path.join(TARGET_RUNTIME_DIR);
    let local_exclude_path = git_exclude_path(&target_path).ok();
    let locally_ignored = local_exclude_path
        .as_deref()
        .is_some_and(local_exclude_contains_runtime);

    if runtime_path.exists() {
        let locally_ignored = if write {
            ensure_local_exclude(&target_path)?
        } else {
            locally_ignored
        };
        return Ok(TargetRuntimeReport {
            target_path,
            example_path,
            runtime_path,
            local_exclude_path,
            state: TargetRuntimeState::ExistingRuntime,
            initialized: false,
            locally_ignored,
            conflict: Some("runtime directory already exists".into()),
            message: "existing .shea preserved; no files copied".into(),
        });
    }

    if !example_path.is_dir() {
        return Ok(TargetRuntimeReport {
            target_path,
            example_path,
            runtime_path,
            local_exclude_path,
            state: TargetRuntimeState::MissingExample,
            initialized: false,
            locally_ignored,
            conflict: Some("missing .shea-example baseline".into()),
            message: "add a committed .shea-example baseline before initializing .shea".into(),
        });
    }

    if !write {
        return Ok(TargetRuntimeReport {
            target_path,
            example_path,
            runtime_path,
            local_exclude_path,
            state: TargetRuntimeState::ReadyToInitialize,
            initialized: false,
            locally_ignored,
            conflict: None,
            message: "ready to initialize .shea from .shea-example".into(),
        });
    }

    copy_dir(&example_path, &runtime_path)?;
    let locally_ignored = ensure_local_exclude(&target_path)?;
    let local_exclude_path = git_exclude_path(&target_path).ok();
    Ok(TargetRuntimeReport {
        target_path,
        example_path,
        runtime_path,
        local_exclude_path,
        state: TargetRuntimeState::Initialized,
        initialized: true,
        locally_ignored,
        conflict: None,
        message: "initialized .shea from .shea-example".into(),
    })
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), TargetRuntimeError> {
    fs::create_dir(to).map_err(|source| TargetRuntimeError::Copy {
        from: from.into(),
        to: to.into(),
        source,
    })?;
    for entry in fs::read_dir(from).map_err(|source| TargetRuntimeError::Copy {
        from: from.into(),
        to: to.into(),
        source,
    })? {
        let entry = entry.map_err(|source| TargetRuntimeError::Copy {
            from: from.into(),
            to: to.into(),
            source,
        })?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            copy_dir(&source, &target)?;
        } else {
            fs::copy(&source, &target).map_err(|source_error| TargetRuntimeError::Copy {
                from: source,
                to: target,
                source: source_error,
            })?;
        }
    }
    Ok(())
}

fn ensure_local_exclude(target_path: &Path) -> Result<bool, TargetRuntimeError> {
    let exclude_path = git_exclude_path(target_path)?;
    let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.trim() == LOCAL_EXCLUDE_ENTRY)
    {
        return Ok(true);
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(LOCAL_EXCLUDE_ENTRY);
    next.push('\n');
    fs::create_dir_all(exclude_path.parent().unwrap_or(target_path)).map_err(|source| {
        TargetRuntimeError::Exclude {
            path: exclude_path.clone(),
            source,
        }
    })?;
    fs::write(&exclude_path, next).map_err(|source| TargetRuntimeError::Exclude {
        path: exclude_path,
        source,
    })?;
    Ok(true)
}

fn local_exclude_contains_runtime(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|content| {
            content
                .lines()
                .any(|line| line.trim() == LOCAL_EXCLUDE_ENTRY)
        })
        .unwrap_or(false)
}

fn git_exclude_path(target_path: &Path) -> Result<PathBuf, TargetRuntimeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(target_path)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output()
        .map_err(|error| TargetRuntimeError::GitExclude {
            target: target_path.into(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(TargetRuntimeError::GitExclude {
            target: target_path.into(),
            message,
        });
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = PathBuf::from(raw);
    Ok(if path.is_absolute() {
        path
    } else {
        target_path.join(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn target_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "--initial-branch=main"]);
        temp
    }

    fn write_example(path: &Path) {
        fs::create_dir_all(path.join(".shea-example/workflows")).unwrap();
        fs::create_dir_all(path.join(".shea-example/prompts")).unwrap();
        fs::write(path.join(".shea-example/workflows/target.md"), "workflow\n").unwrap();
        fs::write(path.join(".shea-example/prompts/main-agent.md"), "prompt\n").unwrap();
    }

    #[test]
    fn initializes_runtime_from_example_and_local_exclude() {
        let temp = target_repo();
        write_example(temp.path());

        let report = initialize_target_runtime(temp.path()).unwrap();

        assert_eq!(report.state, TargetRuntimeState::Initialized);
        assert!(report.initialized);
        assert!(report.locally_ignored);
        assert_eq!(
            fs::read_to_string(temp.path().join(".shea/workflows/target.md")).unwrap(),
            "workflow\n"
        );
        let exclude = fs::read_to_string(temp.path().join(".git/info/exclude")).unwrap();
        assert!(exclude.lines().any(|line| line == ".shea/"));
    }

    #[test]
    fn existing_runtime_is_preserved_and_reported() {
        let temp = target_repo();
        write_example(temp.path());
        fs::create_dir(temp.path().join(".shea")).unwrap();
        fs::write(temp.path().join(".shea/local.txt"), "local tweak\n").unwrap();

        let report = initialize_target_runtime(temp.path()).unwrap();

        assert_eq!(report.state, TargetRuntimeState::ExistingRuntime);
        assert!(!report.initialized);
        assert_eq!(
            fs::read_to_string(temp.path().join(".shea/local.txt")).unwrap(),
            "local tweak\n"
        );
        assert!(report.conflict.unwrap().contains("already exists"));
        assert!(report.locally_ignored);
    }

    #[test]
    fn missing_example_reports_conflict_without_creating_runtime() {
        let temp = target_repo();

        let report = initialize_target_runtime(temp.path()).unwrap();

        assert_eq!(report.state, TargetRuntimeState::MissingExample);
        assert!(!report.initialized);
        assert!(!temp.path().join(".shea").exists());
        assert!(report.conflict.unwrap().contains("missing"));
    }
}
