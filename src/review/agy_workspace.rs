use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::{Builder, TempDir};

#[derive(Debug)]
pub(super) struct AgyReviewIsolation {
    source_workspace: PathBuf,
    _temporary_root: TempDir,
    workspace: PathBuf,
    scratch: PathBuf,
    cargo_target: PathBuf,
    reviewed_revision: String,
    registered: bool,
}

#[derive(Debug)]
pub(super) struct AgyIsolationVerification {
    pub completed_revision: Option<String>,
    pub integrity_error: Option<String>,
    pub discarded_untracked_paths: Vec<String>,
    pub cleanup_error: Option<String>,
}

impl AgyReviewIsolation {
    pub fn create(source_workspace: &Path, revision: &str) -> Result<Self, String> {
        let temporary_root = Builder::new()
            .prefix("shea-agy-review-")
            .tempdir()
            .map_err(|error| format!("could not create temporary agy Review root: {error}"))?;
        let workspace = temporary_root.path().join("workspace");
        let scratch = temporary_root.path().join("scratch");
        let cargo_target = temporary_root.path().join("build-cache").join("cargo");
        fs::create_dir_all(&scratch)
            .map_err(|error| format!("could not create agy Review scratch directory: {error}"))?;
        fs::create_dir_all(&cargo_target).map_err(|error| {
            format!("could not create agy Review build-cache directory: {error}")
        })?;

        let output = Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&workspace)
            .arg(revision)
            .current_dir(source_workspace)
            .output()
            .map_err(|error| format!("could not create isolated agy Review worktree: {error}"))?;
        if !output.status.success() {
            return Err(command_failure(
                "could not create isolated agy Review worktree",
                &output,
            ));
        }

        let mut isolation = Self {
            source_workspace: source_workspace.to_path_buf(),
            _temporary_root: temporary_root,
            workspace,
            scratch,
            cargo_target,
            reviewed_revision: revision.to_string(),
            registered: true,
        };
        let completed_revision = revision_at(&isolation.workspace).inspect_err(|_error| {
            let _ = isolation.cleanup();
        })?;
        if completed_revision != revision {
            let _ = isolation.cleanup();
            return Err(format!(
                "isolated agy Review worktree revision `{completed_revision}` does not match linked pull request head `{revision}`"
            ));
        }
        if !tracked_state(&isolation.workspace)?.is_empty() {
            let _ = isolation.cleanup();
            return Err(
                "isolated agy Review worktree was not clean at the linked pull request revision"
                    .into(),
            );
        }

        Ok(isolation)
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn scratch(&self) -> &Path {
        &self.scratch
    }

    pub fn cargo_target(&self) -> &Path {
        &self.cargo_target
    }

    pub fn verify_and_cleanup(mut self) -> AgyIsolationVerification {
        let completed_revision = revision_at(&self.workspace).ok();
        let mut integrity_error = match completed_revision.as_deref() {
            Some(revision) if revision != self.reviewed_revision => Some(format!(
                "agy Review changed isolated worktree HEAD from `{}` to `{revision}`",
                self.reviewed_revision
            )),
            None => Some("could not read completed isolated agy Review revision".into()),
            _ => None,
        };
        if integrity_error.is_none() {
            match tracked_state(&self.workspace) {
                Ok(state) if !state.is_empty() => {
                    integrity_error = Some(
                        "agy Review modified tracked files in its isolated Review worktree".into(),
                    );
                }
                Err(error) => integrity_error = Some(error),
                Ok(_) => {}
            }
        }
        let discarded_untracked_paths = match untracked_paths(&self.workspace) {
            Ok(paths) => paths,
            Err(error) => {
                if integrity_error.is_none() {
                    integrity_error = Some(error);
                }
                Vec::new()
            }
        };
        let cleanup_error = self.cleanup().err();

        AgyIsolationVerification {
            completed_revision,
            integrity_error,
            discarded_untracked_paths,
            cleanup_error,
        }
    }

    fn cleanup(&mut self) -> Result<(), String> {
        if !self.registered {
            return Ok(());
        }
        let output = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.workspace)
            .current_dir(&self.source_workspace)
            .output()
            .map_err(|error| format!("could not remove isolated agy Review worktree: {error}"))?;
        if !output.status.success() {
            return Err(command_failure(
                "could not remove isolated agy Review worktree",
                &output,
            ));
        }
        self.registered = false;
        Ok(())
    }
}

impl Drop for AgyReviewIsolation {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn revision_at(workspace: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()
        .map_err(|error| format!("could not read isolated agy Review revision: {error}"))?;
    if !output.status.success() {
        return Err(command_failure(
            "could not read isolated agy Review revision",
            &output,
        ));
    }
    let revision = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if revision.is_empty() {
        return Err("isolated agy Review worktree returned an empty revision".into());
    }
    Ok(revision)
}

fn tracked_state(workspace: &Path) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(["diff", "--binary", "HEAD", "--", "."])
        .current_dir(workspace)
        .output()
        .map_err(|error| format!("could not inspect isolated agy Review worktree: {error}"))?;
    if !output.status.success() {
        return Err(command_failure(
            "could not inspect isolated agy Review worktree",
            &output,
        ));
    }
    Ok(output.stdout)
}

fn untracked_paths(workspace: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(workspace)
        .output()
        .map_err(|error| {
            format!("could not inventory isolated agy Review scratch files: {error}")
        })?;
    if !output.status.success() {
        return Err(command_failure(
            "could not inventory isolated agy Review scratch files",
            &output,
        ));
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(str::to_string)
                .map_err(|error| format!("isolated agy Review contains a non-UTF-8 path: {error}"))
        })
        .collect()
}

fn command_failure(context: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{context}: git exited with {}", output.status)
    } else {
        format!("{context}: {stderr}")
    }
}
