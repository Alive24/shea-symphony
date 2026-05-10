use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::config::HooksConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub path: PathBuf,
    pub workspace_key: String,
    pub created_now: bool,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace path escapes root: {workspace} not inside {root}")]
    OutsideRoot { workspace: PathBuf, root: PathBuf },
    #[error("workspace path must not equal root: {0}")]
    EqualsRoot(PathBuf),
    #[error("workspace io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace hook {hook} failed with status {status}: {output}")]
    HookFailed {
        hook: String,
        status: i32,
        output: String,
    },
}

pub fn safe_identifier(identifier: &str) -> String {
    identifier
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub fn prepare_workspace(
    root: &Path,
    identifier: &str,
    hooks: &HooksConfig,
) -> Result<Workspace, WorkspaceError> {
    let workspace_key = safe_identifier(identifier);
    let root = canonical_or_create(root)?;
    let path = root.join(&workspace_key);
    validate_inside_root(&root, &path)?;

    let created_now = if path.is_dir() {
        false
    } else {
        if path.exists() {
            fs::remove_file(&path)?;
        }
        fs::create_dir_all(&path)?;
        true
    };

    if created_now {
        if let Some(command) = hooks.after_create.as_deref() {
            run_hook("after_create", command, &path)?;
        }
    }

    Ok(Workspace {
        path,
        workspace_key,
        created_now,
    })
}

pub fn run_before_run(path: &Path, hooks: &HooksConfig) -> Result<(), WorkspaceError> {
    if let Some(command) = hooks.before_run.as_deref() {
        run_hook("before_run", command, path)?;
    }
    Ok(())
}

pub fn run_after_run(path: &Path, hooks: &HooksConfig) {
    if let Some(command) = hooks.after_run.as_deref() {
        let _ = run_hook("after_run", command, path);
    }
}

fn canonical_or_create(root: &Path) -> Result<PathBuf, WorkspaceError> {
    fs::create_dir_all(root)?;
    Ok(root.canonicalize()?)
}

fn validate_inside_root(root: &Path, workspace: &Path) -> Result<(), WorkspaceError> {
    if workspace == root {
        return Err(WorkspaceError::EqualsRoot(workspace.to_path_buf()));
    }
    if !workspace.starts_with(root) {
        return Err(WorkspaceError::OutsideRoot {
            workspace: workspace.to_path_buf(),
            root: root.to_path_buf(),
        });
    }
    Ok(())
}

fn run_hook(hook: &str, command: &str, cwd: &Path) -> Result<(), WorkspaceError> {
    let output = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(WorkspaceError::HookFailed {
            hook: hook.into(),
            status: output.status.code().unwrap_or(-1),
            output: String::from_utf8_lossy(&output.stdout).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_workspace_identifier() {
        assert_eq!(safe_identifier("#123: hello/world"), "_123__hello_world");
    }

    #[test]
    fn creates_workspace_under_root() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = prepare_workspace(
            temp.path(),
            "#1",
            &HooksConfig {
                timeout_ms: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(workspace.path.is_dir());
        assert_eq!(workspace.workspace_key, "_1");
    }
}
