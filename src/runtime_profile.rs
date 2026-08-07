//! Machine-local repository runtime profiles and pre-claim readiness checks.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{RuntimeProfileConfig, TrackerConfig};

/// Supported machine-local runtime-profile schema version.
pub const RUNTIME_PROFILE_SCHEMA_VERSION: u32 = 1;

const MAX_ENVIRONMENT_ENTRIES: usize = 64;
const MAX_REQUIREMENT_SOURCES: usize = 64;
const MAX_TOOLS: usize = 32;
const MAX_PROBE_ARGS: usize = 16;

/// Credential-free repository identity stored in a runtime profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRepositoryIdentity {
    /// Stable repository identifier, normally `owner/repository`.
    pub id: String,
}

/// One repository-owned requirement source and its Git blob fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRequirementSource {
    /// Path relative to the target repository root.
    pub path: PathBuf,
    /// Expected `git hash-object` digest for the file.
    pub git_blob: String,
}

/// One already-installed executable and the bounded probe used to verify it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeTool {
    /// Human-readable generic tool identifier.
    pub id: String,
    /// Absolute path to the selected executable.
    pub executable: PathBuf,
    /// Version text observed when the profile was generated.
    pub observed_version: String,
    /// Direct argv passed to the executable for a cheap readiness probe.
    #[serde(default)]
    pub version_args: Vec<String>,
}

/// Versioned, machine-local repository execution profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeProfile {
    /// Profile schema version.
    pub schema_version: u32,
    /// Stable profile identity used in runtime and workpad evidence.
    pub profile_id: String,
    /// Informational generation timestamp.
    pub generated_at: String,
    /// Repository identity that prevents cross-repository reuse.
    pub repository: RuntimeRepositoryIdentity,
    /// Repository sources whose content invalidates the profile when changed.
    pub requirement_sources: Vec<RuntimeRequirementSource>,
    /// Resolved executables and their non-destructive version probes.
    pub tools: Vec<RuntimeTool>,
    /// Explicit bounded environment overlay applied only to Main execution.
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

/// Non-secret result of running readiness in an exact issue worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeReadinessReport {
    /// Whether the repository is ready for a Main claim.
    pub ready: bool,
    /// `ready` or the backwards-compatible `skipped:not_configured` state.
    pub status: String,
    /// Selected runtime-profile identity, when configured.
    pub profile_id: Option<String>,
    /// Machine-local profile path.
    pub profile_path: PathBuf,
    /// Exact worktree in which source drift and tool probes were checked.
    pub workspace: PathBuf,
    /// Credential-free evidence suitable for logs and workpads.
    pub evidence: Vec<String>,
}

/// Runtime profile paired with its successful readiness report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRuntimeProfile {
    /// Parsed profile, or `None` for the optional legacy path.
    pub profile: Option<RuntimeProfile>,
    /// Readiness evidence for the exact worktree.
    pub report: RuntimeReadinessReport,
}

/// Validation or readiness failure for a machine-local runtime profile.
#[derive(Debug, Error)]
pub enum RuntimeProfileError {
    #[error("required runtime profile is missing at {0}; run the repository onboarding skill and confirm the proposed profile")]
    Missing(PathBuf),
    #[error("runtime profile io error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("runtime profile parse error at {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid runtime profile at {path}: {message}")]
    Invalid { path: PathBuf, message: String },
    #[error("runtime profile repository mismatch: expected {expected}, found {actual}; rerun repository onboarding")]
    RepositoryMismatch { expected: String, actual: String },
    #[error("runtime profile drift for {path}: expected git blob {expected}, found {actual}; rerun repository onboarding")]
    Drift {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("runtime profile source is unavailable at {0}; rerun repository onboarding")]
    MissingSource(PathBuf),
    #[error("runtime profile tool {tool} is unavailable at {path}; select an installed compatible environment and rerun onboarding")]
    MissingTool { tool: String, path: PathBuf },
    #[error(
        "runtime profile tool probe failed for {tool}: {message}; rerun repository onboarding"
    )]
    ProbeFailed { tool: String, message: String },
    #[error("runtime profile tool probe timed out for {tool} after {timeout_ms}ms")]
    ProbeTimedOut { tool: String, timeout_ms: u64 },
    #[error("runtime readiness evidence could not be written at {path}: {source}")]
    EvidenceIo {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Load and structurally validate the configured machine-local profile.
pub fn load_runtime_profile(
    config: &RuntimeProfileConfig,
) -> Result<Option<RuntimeProfile>, RuntimeProfileError> {
    if !config.path.is_file() {
        return if config.required {
            Err(RuntimeProfileError::Missing(config.path.clone()))
        } else {
            Ok(None)
        };
    }
    let content = fs::read_to_string(&config.path).map_err(|source| RuntimeProfileError::Io {
        path: config.path.clone(),
        source,
    })?;
    let profile: RuntimeProfile =
        serde_json::from_str(&content).map_err(|source| RuntimeProfileError::Parse {
            path: config.path.clone(),
            source,
        })?;
    validate_runtime_profile(&profile, &config.path)?;
    Ok(Some(profile))
}

/// Run source-drift and executable-version probes in the exact Main worktree.
pub fn resolve_runtime_readiness(
    config: &RuntimeProfileConfig,
    tracker: &TrackerConfig,
    workspace: &Path,
) -> Result<ResolvedRuntimeProfile, RuntimeProfileError> {
    let profile = load_runtime_profile(config)?;
    let Some(profile) = profile else {
        return Ok(ResolvedRuntimeProfile {
            profile: None,
            report: RuntimeReadinessReport {
                ready: true,
                status: "skipped:not_configured".into(),
                profile_id: None,
                profile_path: config.path.clone(),
                workspace: workspace.to_path_buf(),
                evidence: vec![
                    "runtime_profile=optional_missing compatibility=workflow_profiles_unchanged"
                        .into(),
                ],
            },
        });
    };

    if let (Some(owner), Some(repo)) = (tracker.owner.as_deref(), tracker.repo.as_deref()) {
        let expected = format!("{owner}/{repo}");
        if profile.repository.id != expected {
            return Err(RuntimeProfileError::RepositoryMismatch {
                expected,
                actual: profile.repository.id.clone(),
            });
        }
    }

    let mut evidence = vec![format!(
        "runtime_profile={} schema={} repository={}",
        profile.profile_id, profile.schema_version, profile.repository.id
    )];
    for source in &profile.requirement_sources {
        let source_path = workspace.join(&source.path);
        if !source_path.is_file() {
            return Err(RuntimeProfileError::MissingSource(source.path.clone()));
        }
        let actual = git_blob_fingerprint(workspace, &source.path)?;
        if actual != source.git_blob {
            return Err(RuntimeProfileError::Drift {
                path: source.path.clone(),
                expected: source.git_blob.clone(),
                actual,
            });
        }
        evidence.push(format!(
            "source={} git_blob={} status=matched",
            source.path.display(),
            source.git_blob
        ));
    }

    for tool in &profile.tools {
        let output = run_tool_probe(tool, &profile.environment, workspace, config.timeout_ms)?;
        if !output.contains(tool.observed_version.trim()) {
            return Err(RuntimeProfileError::ProbeFailed {
                tool: tool.id.clone(),
                message: format!(
                    "observed version mismatch: expected `{}`; probe output omitted from evidence",
                    tool.observed_version
                ),
            });
        }
        evidence.push(format!(
            "tool={} executable={} version={} status=matched",
            tool.id,
            tool.executable.display(),
            compact(&tool.observed_version)
        ));
    }

    Ok(ResolvedRuntimeProfile {
        report: RuntimeReadinessReport {
            ready: true,
            status: "ready".into(),
            profile_id: Some(profile.profile_id.clone()),
            profile_path: config.path.clone(),
            workspace: workspace.to_path_buf(),
            evidence,
        },
        profile: Some(profile),
    })
}

/// Merge a validated runtime-profile overlay into a Main process environment.
pub fn apply_runtime_profile_environment(
    environment: &mut BTreeMap<String, String>,
    profile: Option<&RuntimeProfile>,
) {
    let Some(profile) = profile else {
        return;
    };
    environment.extend(profile.environment.clone());
    environment.insert(
        "SHEA_SYMPHONY_RUNTIME_PROFILE_ID".into(),
        profile.profile_id.clone(),
    );
    environment.insert(
        "SHEA_SYMPHONY_RUNTIME_PROFILE_SOURCE".into(),
        "repository:.shea/runtime-profile.json".into(),
    );
}

/// Persist a local-only readiness failure without touching tracker state.
pub fn persist_runtime_readiness_failure(
    logs_root: &Path,
    issue_ref: &str,
    config: &RuntimeProfileConfig,
    workspace: &Path,
    error: &RuntimeProfileError,
) -> Result<PathBuf, RuntimeProfileError> {
    let directory = logs_root.join("runtime-readiness");
    fs::create_dir_all(&directory).map_err(|source| RuntimeProfileError::EvidenceIo {
        path: directory.clone(),
        source,
    })?;
    let path = directory.join(format!(
        "{}-{}.json",
        safe_component(issue_ref),
        unix_timestamp_ms()
    ));
    let payload = serde_json::json!({
        "schema_version": 1,
        "issue": issue_ref,
        "ready": false,
        "profile_path": config.path,
        "workspace": workspace,
        "error": error.to_string(),
        "recorded_at_unix_ms": unix_timestamp_ms(),
    });
    let content = serde_json::to_vec_pretty(&payload).expect("readiness evidence serializes");
    fs::write(&path, content).map_err(|source| RuntimeProfileError::EvidenceIo {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

fn validate_runtime_profile(
    profile: &RuntimeProfile,
    path: &Path,
) -> Result<(), RuntimeProfileError> {
    let invalid = |message: String| RuntimeProfileError::Invalid {
        path: path.to_path_buf(),
        message,
    };
    if profile.schema_version != RUNTIME_PROFILE_SCHEMA_VERSION {
        return Err(invalid(format!(
            "unsupported schema_version {}; expected {}",
            profile.schema_version, RUNTIME_PROFILE_SCHEMA_VERSION
        )));
    }
    validate_identifier("profile_id", &profile.profile_id).map_err(&invalid)?;
    validate_identifier("repository.id", &profile.repository.id).map_err(&invalid)?;
    if profile.generated_at.trim().is_empty()
        || profile.generated_at.len() > 128
        || profile.generated_at.chars().any(char::is_control)
    {
        return Err(invalid(
            "generated_at must be a short non-empty timestamp".into(),
        ));
    }
    if profile.requirement_sources.is_empty()
        || profile.requirement_sources.len() > MAX_REQUIREMENT_SOURCES
    {
        return Err(invalid(format!(
            "requirement_sources must contain 1..={MAX_REQUIREMENT_SOURCES} entries"
        )));
    }
    if profile.tools.is_empty() || profile.tools.len() > MAX_TOOLS {
        return Err(invalid(format!(
            "tools must contain 1..={MAX_TOOLS} entries"
        )));
    }
    if profile.environment.len() > MAX_ENVIRONMENT_ENTRIES {
        return Err(invalid(format!(
            "environment may contain at most {MAX_ENVIRONMENT_ENTRIES} entries"
        )));
    }

    let mut sources = BTreeSet::new();
    for source in &profile.requirement_sources {
        validate_relative_source_path(&source.path).map_err(&invalid)?;
        if !sources.insert(source.path.clone()) {
            return Err(invalid(format!(
                "duplicate requirement source {}",
                source.path.display()
            )));
        }
        if source.git_blob.len() != 40
            || !source
                .git_blob
                .chars()
                .all(|character| character.is_ascii_digit() || matches!(character, 'a'..='f'))
        {
            return Err(invalid(format!(
                "requirement source {} must contain a 40-character Git blob digest",
                source.path.display()
            )));
        }
    }

    let mut tools = BTreeSet::new();
    for tool in &profile.tools {
        validate_identifier("tool.id", &tool.id).map_err(&invalid)?;
        if !tools.insert(tool.id.clone()) {
            return Err(invalid(format!("duplicate tool id {}", tool.id)));
        }
        if !tool.executable.is_absolute() {
            return Err(invalid(format!(
                "tool {} executable must be an absolute path",
                tool.id
            )));
        }
        if tool.observed_version.trim().is_empty()
            || tool.observed_version.len() > 256
            || tool.observed_version.chars().any(char::is_control)
        {
            return Err(invalid(format!(
                "tool {} observed_version must be short and non-empty",
                tool.id
            )));
        }
        validate_non_secret_value("tool observed_version", &tool.observed_version, 256)
            .map_err(&invalid)?;
        if tool.version_args.len() > MAX_PROBE_ARGS {
            return Err(invalid(format!(
                "tool {} version_args may contain at most {MAX_PROBE_ARGS} entries",
                tool.id
            )));
        }
        if !safe_version_probe_args(&tool.version_args) {
            return Err(invalid(format!(
                "tool {} version_args must be one recognized non-destructive version probe",
                tool.id
            )));
        }
        for argument in &tool.version_args {
            validate_non_secret_value("tool probe argument", argument, 512).map_err(&invalid)?;
        }
    }

    for (key, value) in &profile.environment {
        if !valid_environment_key(key) {
            return Err(invalid(format!("unsafe environment key `{key}`")));
        }
        if is_sensitive_name(key)
            || is_process_injection_name(key)
            || key.starts_with("SHEA_SYMPHONY_")
        {
            return Err(invalid(format!(
                "environment key `{key}` is credential-bearing or reserved"
            )));
        }
        validate_non_secret_value("environment value", value, 4096).map_err(&invalid)?;
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(format!("{label} must be 1..=128 characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(format!("{label} must not contain control characters"));
    }
    Ok(())
}

fn validate_relative_source_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("requirement source paths must be non-empty and repository-relative".into());
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!(
            "requirement source path {} escapes the repository",
            path.display()
        ));
    }
    Ok(())
}

fn validate_non_secret_value(label: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.len() > maximum || value.chars().any(char::is_control) {
        return Err(format!(
            "{label} is too long or contains control characters"
        ));
    }
    let normalized = value.to_ascii_lowercase();
    if [
        "token=",
        "password=",
        "secret=",
        "cookie=",
        "authorization=",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        return Err(format!(
            "{label} appears to contain credential-bearing data"
        ));
    }
    Ok(())
}

fn valid_environment_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    key.len() <= 64
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn is_sensitive_name(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "COOKIE",
        "CREDENTIAL",
        "AUTHORIZATION",
        "API_KEY",
        "PRIVATE_KEY",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

fn is_process_injection_name(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.starts_with("LD_")
        || upper.starts_with("DYLD_")
        || upper.starts_with("GIT_CONFIG")
        || matches!(
            upper.as_str(),
            "BASH_ENV"
                | "ENV"
                | "SHELLOPTS"
                | "PROMPT_COMMAND"
                | "PYTHONSTARTUP"
                | "NODE_OPTIONS"
                | "RUBYOPT"
                | "PERL5OPT"
                | "JAVA_TOOL_OPTIONS"
        )
}

fn safe_version_probe_args(arguments: &[String]) -> bool {
    matches!(
        arguments,
        [argument]
            if matches!(argument.as_str(), "--version" | "-V" | "-v" | "version" | "-version")
    )
}

fn git_blob_fingerprint(workspace: &Path, source: &Path) -> Result<String, RuntimeProfileError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["hash-object", "--"])
        .arg(source)
        .output()
        .map_err(|source_error| RuntimeProfileError::ProbeFailed {
            tool: "git-hash-object".into(),
            message: source_error.to_string(),
        })?;
    if !output.status.success() {
        return Err(RuntimeProfileError::ProbeFailed {
            tool: "git-hash-object".into(),
            message: compact(&String::from_utf8_lossy(&output.stderr)),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase())
}

fn run_tool_probe(
    tool: &RuntimeTool,
    environment: &BTreeMap<String, String>,
    workspace: &Path,
    timeout_ms: u64,
) -> Result<String, RuntimeProfileError> {
    if !tool.executable.is_file() {
        return Err(RuntimeProfileError::MissingTool {
            tool: tool.id.clone(),
            path: tool.executable.clone(),
        });
    }
    let mut child = Command::new(&tool.executable)
        .args(&tool.version_args)
        .envs(environment)
        .current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| RuntimeProfileError::ProbeFailed {
            tool: tool.id.clone(),
            message: source.to_string(),
        })?;
    let timeout = Duration::from_millis(timeout_ms.max(1));
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|source| RuntimeProfileError::ProbeFailed {
                tool: tool.id.clone(),
                message: source.to_string(),
            })?
            .is_some()
        {
            let output =
                child
                    .wait_with_output()
                    .map_err(|source| RuntimeProfileError::ProbeFailed {
                        tool: tool.id.clone(),
                        message: source.to_string(),
                    })?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{} {}", stdout.trim(), stderr.trim())
                .trim()
                .to_string();
            if !output.status.success() {
                return Err(RuntimeProfileError::ProbeFailed {
                    tool: tool.id.clone(),
                    message: format!(
                        "status={}; probe output omitted from evidence",
                        output.status.code().unwrap_or(-1)
                    ),
                });
            }
            return Ok(combined);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RuntimeProfileError::ProbeTimedOut {
                tool: tool.id.clone(),
                timeout_ms,
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn compact(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn config(path: PathBuf, required: bool) -> RuntimeProfileConfig {
        RuntimeProfileConfig {
            path,
            required,
            timeout_ms: 2_000,
        }
    }

    fn tracker() -> TrackerConfig {
        let workflow = crate::workflow::WorkflowDefinition::parse(
            "/tmp/.shea/workflows/test.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: example\n  project_owner: Alive24\n  project_number: 9\n---\nPrompt",
        )
        .unwrap();
        crate::config::RuntimeConfig::from_workflow(
            &workflow,
            Path::new("/tmp/.shea/workflows/test.md"),
        )
        .unwrap()
        .tracker
    }

    fn write_profile(
        root: &Path,
        observed_version: &str,
        environment: BTreeMap<String, String>,
    ) -> PathBuf {
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(root)
            .output()
            .unwrap();
        fs::write(root.join("requirements.txt"), "runtime=compatible\n").unwrap();
        let digest = git_blob_fingerprint(root, Path::new("requirements.txt")).unwrap();
        let tool = root.join("tool");
        fs::write(&tool, "#!/bin/sh\nprintf 'runtime 24.18.1\\n'\n").unwrap();
        let mut permissions = fs::metadata(&tool).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&tool, permissions).unwrap();
        let profile = RuntimeProfile {
            schema_version: 1,
            profile_id: "example-compatible".into(),
            generated_at: "2026-08-07T00:00:00Z".into(),
            repository: RuntimeRepositoryIdentity {
                id: "Alive24/example".into(),
            },
            requirement_sources: vec![RuntimeRequirementSource {
                path: "requirements.txt".into(),
                git_blob: digest,
            }],
            tools: vec![RuntimeTool {
                id: "runtime".into(),
                executable: tool,
                observed_version: observed_version.into(),
                version_args: vec!["--version".into()],
            }],
            environment,
        };
        let path = root.join("runtime-profile.json");
        fs::write(&path, serde_json::to_vec_pretty(&profile).unwrap()).unwrap();
        path
    }

    #[test]
    fn compatible_profile_passes_and_exports_only_bounded_environment() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_profile(
            temp.path(),
            "24.18.1",
            BTreeMap::from([("PATH".into(), "/opt/runtime/bin:/usr/bin".into())]),
        );

        let resolved =
            resolve_runtime_readiness(&config(path, true), &tracker(), temp.path()).unwrap();
        assert!(resolved.report.ready);
        assert_eq!(
            resolved.report.profile_id.as_deref(),
            Some("example-compatible")
        );
        let mut environment = BTreeMap::new();
        apply_runtime_profile_environment(&mut environment, resolved.profile.as_ref());
        assert_eq!(
            environment.get("PATH").unwrap(),
            "/opt/runtime/bin:/usr/bin"
        );
        assert_eq!(
            environment.get("SHEA_SYMPHONY_RUNTIME_PROFILE_ID").unwrap(),
            "example-compatible"
        );
    }

    #[test]
    fn incompatible_tool_version_blocks_readiness() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_profile(temp.path(), "99.0.0", BTreeMap::new());

        let error =
            resolve_runtime_readiness(&config(path, true), &tracker(), temp.path()).unwrap_err();

        assert!(matches!(error, RuntimeProfileError::ProbeFailed { .. }));
        assert!(error.to_string().contains("version mismatch"));
    }

    #[test]
    fn tool_probe_output_is_omitted_from_failure_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_profile(temp.path(), "99.0.0", BTreeMap::new());
        fs::write(
            temp.path().join("tool"),
            "#!/bin/sh\nprintf 'token=unexpected-private-output\\n'\n",
        )
        .unwrap();

        let error =
            resolve_runtime_readiness(&config(path, true), &tracker(), temp.path()).unwrap_err();

        assert!(error.to_string().contains("probe output omitted"));
        assert!(!error.to_string().contains("unexpected-private-output"));
    }

    #[test]
    fn requirement_source_drift_blocks_readiness() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_profile(temp.path(), "24.18.1", BTreeMap::new());
        fs::write(
            temp.path().join("requirements.txt"),
            "runtime=incompatible\n",
        )
        .unwrap();

        let error =
            resolve_runtime_readiness(&config(path, true), &tracker(), temp.path()).unwrap_err();

        assert!(matches!(error, RuntimeProfileError::Drift { .. }));
        assert!(error.to_string().contains("rerun repository onboarding"));
    }

    #[test]
    fn secret_bearing_and_reserved_environment_keys_are_rejected() {
        for key in ["API_TOKEN", "SHEA_SYMPHONY_CLAIM", "LD_PRELOAD"] {
            let temp = tempfile::tempdir().unwrap();
            let path = write_profile(
                temp.path(),
                "24.18.1",
                BTreeMap::from([(key.into(), "not-recorded".into())]),
            );

            let error = load_runtime_profile(&config(path, true)).unwrap_err();

            assert!(matches!(error, RuntimeProfileError::Invalid { .. }));
            assert!(error.to_string().contains("credential-bearing or reserved"));
        }
    }

    #[test]
    fn arbitrary_probe_arguments_are_rejected_before_execution() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_profile(temp.path(), "24.18.1", BTreeMap::new());
        let mut profile: RuntimeProfile =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        profile.tools[0].version_args = vec!["install".into(), "package".into()];
        fs::write(&path, serde_json::to_vec_pretty(&profile).unwrap()).unwrap();

        let error = load_runtime_profile(&config(path, true)).unwrap_err();

        assert!(matches!(error, RuntimeProfileError::Invalid { .. }));
        assert!(error.to_string().contains("non-destructive version probe"));
    }

    #[test]
    fn missing_optional_profile_preserves_legacy_workflow_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.json");

        let resolved =
            resolve_runtime_readiness(&config(path, false), &tracker(), temp.path()).unwrap();

        assert!(resolved.report.ready);
        assert_eq!(resolved.report.status, "skipped:not_configured");
        assert!(resolved.profile.is_none());
    }

    #[test]
    fn main_backend_and_handoff_commands_receive_identical_profile_environment() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_profile(
            temp.path(),
            "24.18.1",
            BTreeMap::from([("RUNTIME_BIN".into(), "/opt/runtime/bin".into())]),
        );
        let resolved =
            resolve_runtime_readiness(&config(path, true), &tracker(), temp.path()).unwrap();
        let mut backend_environment = BTreeMap::new();
        let mut verification_environment = BTreeMap::new();
        apply_runtime_profile_environment(&mut backend_environment, resolved.profile.as_ref());
        apply_runtime_profile_environment(&mut verification_environment, resolved.profile.as_ref());

        assert_eq!(backend_environment, verification_environment);
        crate::workspace::run_workspace_command_with_env(
            "runtime-profile-parity",
            "printf '%s|%s' \"$SHEA_SYMPHONY_RUNTIME_PROFILE_ID\" \"$RUNTIME_BIN\" > parity.txt",
            temp.path(),
            2_000,
            &verification_environment,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(temp.path().join("parity.txt")).unwrap(),
            "example-compatible|/opt/runtime/bin"
        );
    }
}
