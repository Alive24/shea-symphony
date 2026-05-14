use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::config::{GitIdentityConfig, HooksConfig};

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
    #[error("workspace hook {hook} failed with status {status}: stdout={stdout} stderr={stderr}")]
    HookFailed {
        hook: String,
        status: i32,
        stdout: String,
        stderr: String,
    },
    #[error("workspace hook {hook} timed out after {timeout_ms}ms")]
    HookTimedOut { hook: String, timeout_ms: u64 },
    #[error("workspace path is not valid unicode for hook cwd: {0}")]
    InvalidHookCwd(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookResult {
    pub hook: String,
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIdentityApplyResult {
    pub status: GitIdentityApplyStatus,
    pub author: Option<String>,
    pub applied_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitIdentityApplyStatus {
    Applied,
    NotConfigured,
    NotGitRepository,
}

impl GitIdentityApplyResult {
    pub fn summary(&self) -> String {
        match self.status {
            GitIdentityApplyStatus::Applied => {
                let author = self.author.as_deref().unwrap_or("configured");
                format!("applied:{author}")
            }
            GitIdentityApplyStatus::NotConfigured => "skipped:not_configured".to_string(),
            GitIdentityApplyStatus::NotGitRepository => "skipped:not_git_repository".to_string(),
        }
    }
}

impl HookResult {
    fn success(hook: &str, stdout: String, stderr: String) -> Self {
        Self {
            hook: hook.into(),
            status: 0,
            stdout,
            stderr,
        }
    }
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

pub fn profile_scoped_identifier(profile_id: Option<&str>, identifier: &str) -> String {
    match profile_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(profile_id) => format!(
            "{}--{}",
            safe_identifier(profile_id),
            safe_identifier(identifier)
        ),
        None => identifier.to_string(),
    }
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
            run_hook("after_create", command, &path, hooks.timeout_ms)?;
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
        run_hook("before_run", command, path, hooks.timeout_ms)?;
    }
    Ok(())
}

pub fn apply_local_git_identity(
    path: &Path,
    identity: &GitIdentityConfig,
) -> Result<GitIdentityApplyResult, WorkspaceError> {
    if identity.is_empty() {
        return Ok(GitIdentityApplyResult {
            status: GitIdentityApplyStatus::NotConfigured,
            author: None,
            applied_keys: Vec::new(),
        });
    }

    if !path.join(".git").exists() {
        return Ok(GitIdentityApplyResult {
            status: GitIdentityApplyStatus::NotGitRepository,
            author: identity.author(),
            applied_keys: Vec::new(),
        });
    }

    let mut applied_keys = Vec::new();
    if let Some(name) = identity.name.as_deref() {
        set_local_git_config(path, "user.name", name)?;
        applied_keys.push("user.name".to_string());
    }
    if let Some(email) = identity.email.as_deref() {
        set_local_git_config(path, "user.email", email)?;
        applied_keys.push("user.email".to_string());
    }
    if let Some(signing_key) = identity.signing_key.as_deref() {
        set_local_git_config(path, "user.signingkey", signing_key)?;
        applied_keys.push("user.signingkey".to_string());
    }
    for (key, value) in &identity.extra {
        set_local_git_config(path, key, value)?;
        applied_keys.push(key.clone());
    }

    Ok(GitIdentityApplyResult {
        status: GitIdentityApplyStatus::Applied,
        author: identity.author(),
        applied_keys,
    })
}

pub fn run_after_run(path: &Path, hooks: &HooksConfig) {
    if let Some(command) = hooks.after_run.as_deref() {
        let _ = run_hook("after_run", command, path, hooks.timeout_ms);
    }
}

pub fn run_workspace_command(
    label: &str,
    command: &str,
    path: &Path,
    timeout_ms: u64,
) -> Result<HookResult, WorkspaceError> {
    run_hook(label, command, path, timeout_ms)
}

pub fn remove_issue_workspace(
    root: &Path,
    identifier: &str,
    hooks: &HooksConfig,
) -> Result<(), WorkspaceError> {
    let root = canonical_or_create(root)?;
    let workspace = root.join(safe_identifier(identifier));
    remove_workspace_path(&root, &workspace, hooks)
}

pub fn remove_workspace_path(
    root: &Path,
    workspace: &Path,
    hooks: &HooksConfig,
) -> Result<(), WorkspaceError> {
    let root = canonical_or_create(root)?;

    if !workspace.exists() {
        validate_inside_root(&root, workspace)?;
        return Ok(());
    }

    let canonical_workspace = workspace.canonicalize()?;
    validate_inside_root(&root, &canonical_workspace)?;

    if workspace == root || canonical_workspace == root {
        return Err(WorkspaceError::EqualsRoot(root));
    }

    if workspace.is_dir() {
        if let Some(command) = hooks.before_remove.as_deref() {
            let _ = run_hook("before_remove", command, workspace, hooks.timeout_ms);
        }
    }

    let metadata = fs::symlink_metadata(workspace)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(workspace)?;
    } else if metadata.is_dir() {
        fs::remove_dir_all(workspace)?;
    }

    Ok(())
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

fn run_hook(
    hook: &str,
    command: &str,
    cwd: &Path,
    timeout_ms: u64,
) -> Result<HookResult, WorkspaceError> {
    let mut child = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let started = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                return Ok(HookResult::success(hook, stdout, stderr));
            }

            return Err(WorkspaceError::HookFailed {
                hook: hook.into(),
                status: output.status.code().unwrap_or(-1),
                stdout,
                stderr,
            });
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(WorkspaceError::HookTimedOut {
                hook: hook.into(),
                timeout_ms,
            });
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn set_local_git_config(path: &Path, key: &str, value: &str) -> Result<(), WorkspaceError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("config")
        .arg("--local")
        .arg(key)
        .arg(value)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(WorkspaceError::HookFailed {
            hook: format!("git config --local {key}"),
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
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

    #[test]
    fn scopes_workspace_identifier_by_profile() {
        assert_eq!(
            profile_scoped_identifier(Some("codex alpha"), "#39"),
            "codex_alpha--_39"
        );
        assert_eq!(profile_scoped_identifier(None, "#39"), "#39");
    }

    #[test]
    fn before_run_hook_timeout_is_fatal() {
        let temp = tempfile::tempdir().unwrap();
        let result = run_before_run(
            temp.path(),
            &HooksConfig {
                before_run: Some("sleep 1".into()),
                timeout_ms: 10,
                ..Default::default()
            },
        );

        assert!(matches!(result, Err(WorkspaceError::HookTimedOut { .. })));
    }

    #[test]
    fn hook_failure_captures_stdout_and_stderr() {
        let temp = tempfile::tempdir().unwrap();
        let result = run_before_run(
            temp.path(),
            &HooksConfig {
                before_run: Some("echo out; echo err >&2; exit 2".into()),
                timeout_ms: 5_000,
                ..Default::default()
            },
        );

        match result {
            Err(WorkspaceError::HookFailed {
                status,
                stdout,
                stderr,
                ..
            }) => {
                assert_eq!(status, 2);
                assert!(stdout.contains("out"));
                assert!(stderr.contains("err"));
            }
            other => panic!("expected hook failure, got {other:?}"),
        }
    }

    #[test]
    fn before_remove_hook_runs_before_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = prepare_workspace(
            temp.path(),
            "#cleanup",
            &HooksConfig {
                timeout_ms: 5_000,
                ..Default::default()
            },
        )
        .unwrap();
        remove_issue_workspace(
            temp.path(),
            "#cleanup",
            &HooksConfig {
                before_remove: Some("printf removed > ../removed.txt".into()),
                timeout_ms: 5_000,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!workspace.path.exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("removed.txt")).unwrap(),
            "removed"
        );
    }

    #[test]
    fn refuses_to_remove_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let result = remove_workspace_path(
            temp.path(),
            temp.path(),
            &HooksConfig {
                timeout_ms: 1_000,
                ..Default::default()
            },
        );

        assert!(matches!(result, Err(WorkspaceError::EqualsRoot(_))));
        assert!(temp.path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_escape_cleanup() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = temp.path().join("escape");
        symlink(outside.path(), &link).unwrap();

        let result = remove_workspace_path(
            temp.path(),
            &link,
            &HooksConfig {
                timeout_ms: 1_000,
                ..Default::default()
            },
        );

        assert!(matches!(result, Err(WorkspaceError::OutsideRoot { .. })));
        assert!(outside.path().exists());
    }

    #[test]
    fn skips_git_identity_when_not_configured() {
        let temp = tempfile::tempdir().unwrap();
        let result = apply_local_git_identity(temp.path(), &GitIdentityConfig::default()).unwrap();

        assert_eq!(result.status, GitIdentityApplyStatus::NotConfigured);
        assert_eq!(result.summary(), "skipped:not_configured");
    }

    #[test]
    fn skips_git_identity_outside_git_repository() {
        let temp = tempfile::tempdir().unwrap();
        let identity = GitIdentityConfig {
            name: Some("Jade Symphony Agent".into()),
            email: Some("jade@example.invalid".into()),
            signing_key: None,
            extra: Default::default(),
        };

        let result = apply_local_git_identity(temp.path(), &identity).unwrap();

        assert_eq!(result.status, GitIdentityApplyStatus::NotGitRepository);
        assert_eq!(
            result.author.as_deref(),
            Some("Jade Symphony Agent <jade@example.invalid>")
        );
    }

    #[test]
    fn applies_git_identity_as_local_config_only() {
        let temp = tempfile::tempdir().unwrap();
        Command::new("git")
            .arg("init")
            .arg(temp.path())
            .output()
            .unwrap();
        let mut extra = std::collections::BTreeMap::new();
        extra.insert("jade.actorRole".into(), "implementation_agent".into());
        let identity = GitIdentityConfig {
            name: Some("Jade Symphony Agent".into()),
            email: Some("jade@example.invalid".into()),
            signing_key: None,
            extra,
        };

        let result = apply_local_git_identity(temp.path(), &identity).unwrap();

        assert_eq!(result.status, GitIdentityApplyStatus::Applied);
        assert!(result.applied_keys.contains(&"user.name".to_string()));
        assert_eq!(
            git_local_config(temp.path(), "user.name"),
            "Jade Symphony Agent"
        );
        assert_eq!(
            git_local_config(temp.path(), "user.email"),
            "jade@example.invalid"
        );
        assert_eq!(
            git_local_config(temp.path(), "jade.actorRole"),
            "implementation_agent"
        );
    }

    fn git_local_config(path: &Path, key: &str) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("config")
            .arg("--local")
            .arg(key)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
