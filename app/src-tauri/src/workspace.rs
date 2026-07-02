use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::cli::DEFAULT_WORKFLOW_PATH;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProfile {
    pub engine_root: String,
    pub target_root: String,
    pub workflow_path: String,
    pub cli_path: Option<String>,
    pub source: String,
    pub error: Option<String>,
}

impl WorkspaceProfile {
    pub fn self_targeted(engine_root: PathBuf) -> Self {
        let root = canonicalize_or_keep(engine_root);
        Self {
            engine_root: root.display().to_string(),
            target_root: root.display().to_string(),
            workflow_path: DEFAULT_WORKFLOW_PATH.into(),
            cli_path: None,
            source: "self".into(),
            error: None,
        }
    }

    pub fn engine_path(&self) -> PathBuf {
        PathBuf::from(&self.engine_root)
    }

    pub fn target_path(&self) -> PathBuf {
        PathBuf::from(&self.target_root)
    }

    pub fn workflow_file_path(&self) -> PathBuf {
        let path = PathBuf::from(&self.workflow_path);
        if path.is_absolute() {
            path
        } else {
            self.target_path().join(path)
        }
    }

    fn with_error(mut self, error: String) -> Self {
        self.error = Some(error);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredWorkspaceProfile {
    target_root: Option<String>,
    workflow_path: Option<String>,
    cli_path: Option<String>,
}

#[derive(Clone)]
pub struct WorkspaceManager {
    inner: Arc<Mutex<WorkspaceProfile>>,
    engine_root: PathBuf,
    store_path: PathBuf,
}

impl WorkspaceManager {
    pub fn new(engine_root: PathBuf, profile: WorkspaceProfile, store_path: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(profile)),
            engine_root,
            store_path,
        }
    }

    pub fn current(&self) -> WorkspaceProfile {
        self.inner
            .lock()
            .map(|profile| profile.clone())
            .unwrap_or_else(|_| WorkspaceProfile::self_targeted(self.engine_root.clone()))
    }

    pub fn set_target(&self, target_root: Option<String>) -> Result<WorkspaceProfile, String> {
        let trimmed = target_root.unwrap_or_default().trim().to_string();
        let profile = if trimmed.is_empty() {
            clear_stored_profile(&self.store_path)?;
            WorkspaceProfile::self_targeted(self.engine_root.clone())
        } else {
            let profile = profile_from_target(&self.engine_root, Path::new(&trimmed), "saved")?;
            save_stored_profile(&self.store_path, &profile)?;
            profile
        };
        *self.inner.lock().map_err(|error| error.to_string())? = profile.clone();
        Ok(profile)
    }
}

#[tauri::command]
pub fn get_workspace_profile(
    manager: State<'_, WorkspaceManager>,
) -> Result<WorkspaceProfile, String> {
    Ok(manager.current())
}

#[tauri::command]
pub fn set_active_workspace(
    manager: State<'_, WorkspaceManager>,
    target_root: Option<String>,
) -> Result<WorkspaceProfile, String> {
    manager.set_target(target_root)
}

pub fn default_profile_path() -> PathBuf {
    if let Ok(path) = env::var("SHEA_SYMPHONY_APP_PROFILE_PATH") {
        return PathBuf::from(path);
    }
    if let Ok(root) = env::var("SHEA_SYMPHONY_ARTIFACT_ROOT") {
        return PathBuf::from(root).join("app-workspace-profile.json");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".shea-symphony")
        .join("app-workspace-profile.json")
}

pub fn initial_workspace_profile(
    engine_root: PathBuf,
    args: &[OsString],
    store_path: &Path,
) -> WorkspaceProfile {
    match parse_workdir_arg(args) {
        Ok(Some(path)) => match profile_from_target(&engine_root, &path, "launch") {
            Ok(profile) => {
                if let Err(error) = save_stored_profile(store_path, &profile) {
                    profile.with_error(format!("workspace selected but not saved: {error}"))
                } else {
                    profile
                }
            }
            Err(error) => WorkspaceProfile::self_targeted(engine_root).with_error(error),
        },
        Ok(None) => load_stored_profile(&engine_root, store_path)
            .unwrap_or_else(|| WorkspaceProfile::self_targeted(engine_root)),
        Err(error) => WorkspaceProfile::self_targeted(engine_root).with_error(error),
    }
}

pub fn parse_workdir_arg(args: &[OsString]) -> Result<Option<PathBuf>, String> {
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--workdir" {
            let Some(value) = args.get(index + 1) else {
                return Err("--workdir requires a target directory".into());
            };
            if value.to_string_lossy().starts_with("--") {
                return Err("--workdir requires a target directory".into());
            }
            return Ok(Some(PathBuf::from(value)));
        }
        if let Some(value) = arg.strip_prefix("--workdir=") {
            if value.is_empty() {
                return Err("--workdir requires a target directory".into());
            }
            return Ok(Some(PathBuf::from(value)));
        }
        index += 1;
    }
    Ok(None)
}

fn load_stored_profile(engine_root: &Path, store_path: &Path) -> Option<WorkspaceProfile> {
    let text = fs::read_to_string(store_path).ok()?;
    let stored: StoredWorkspaceProfile = serde_json::from_str(&text).ok()?;
    let target_root = stored
        .target_root
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| target_root_from_profile_path(store_path))?;
    match profile_from_target(engine_root, &target_root, "saved") {
        Ok(mut profile) => {
            if let Some(workflow_path) = stored
                .workflow_path
                .filter(|value| !value.trim().is_empty())
            {
                profile.workflow_path = workflow_path;
            }
            profile.cli_path = stored.cli_path.filter(|value| !value.trim().is_empty());
            Some(profile)
        }
        Err(error) => Some(
            WorkspaceProfile::self_targeted(engine_root.to_path_buf())
                .with_error(format!("saved workspace is unavailable: {error}")),
        ),
    }
}

fn profile_from_target(
    engine_root: &Path,
    target_root: &Path,
    source: &str,
) -> Result<WorkspaceProfile, String> {
    let target = fs::canonicalize(target_root)
        .map_err(|_| format!("target workspace does not exist: {}", target_root.display()))?;
    if !target.is_dir() {
        return Err(format!(
            "target workspace is not a directory: {}",
            target.display()
        ));
    }
    Ok(WorkspaceProfile {
        engine_root: canonicalize_or_keep(engine_root.to_path_buf())
            .display()
            .to_string(),
        target_root: target.display().to_string(),
        workflow_path: DEFAULT_WORKFLOW_PATH.into(),
        cli_path: None,
        source: source.into(),
        error: None,
    })
}

fn save_stored_profile(store_path: &Path, profile: &WorkspaceProfile) -> Result<(), String> {
    if profile.target_root == profile.engine_root {
        return clear_stored_profile(store_path);
    }
    if let Some(parent) = store_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let stored = StoredWorkspaceProfile {
        target_root: Some(profile.target_root.clone()),
        workflow_path: Some(profile.workflow_path.clone()),
        cli_path: profile.cli_path.clone(),
    };
    fs::write(
        store_path,
        serde_json::to_string_pretty(&stored).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn clear_stored_profile(store_path: &Path) -> Result<(), String> {
    match fs::remove_file(store_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn canonicalize_or_keep(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn target_root_from_profile_path(path: &Path) -> Option<PathBuf> {
    let shea_dir = path.parent()?;
    (shea_dir.file_name()? == ".shea").then(|| shea_dir.parent().map(PathBuf::from))?
}

#[cfg(test)]
mod tests {
    use super::{initial_workspace_profile, parse_workdir_arg};
    use std::{
        ffi::OsString,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parses_valid_workdir_arg() {
        let target = temp_dir("parse-valid-target");
        let args = vec![
            OsString::from("shea-symphony-app"),
            OsString::from("--workdir"),
            target.clone().into_os_string(),
        ];

        assert_eq!(parse_workdir_arg(&args).unwrap(), Some(target));
    }

    #[test]
    fn rejects_missing_workdir_value() {
        let args = vec![
            OsString::from("shea-symphony-app"),
            OsString::from("--workdir"),
        ];

        assert!(parse_workdir_arg(&args).is_err());
    }

    #[test]
    fn invalid_launch_workdir_falls_back_to_self_with_error() {
        let engine = temp_dir("invalid-engine");
        let store_path = temp_dir("invalid-store").join("profile.json");
        let missing = temp_path("missing-target");
        let args = vec![
            OsString::from("shea-symphony-app"),
            OsString::from("--workdir"),
            missing.into_os_string(),
        ];

        let profile = initial_workspace_profile(engine.clone(), &args, &store_path);

        assert_eq!(
            PathBuf::from(profile.target_root),
            fs::canonicalize(engine).unwrap()
        );
        assert!(profile.error.unwrap_or_default().contains("does not exist"));
    }

    #[test]
    fn launch_workdir_is_saved_and_restored() {
        let engine = temp_dir("restore-engine");
        let target = temp_dir("restore-target");
        let store_path = temp_dir("restore-store").join("profile.json");
        let args = vec![
            OsString::from("shea-symphony-app"),
            OsString::from("--workdir"),
            target.clone().into_os_string(),
        ];

        let selected = initial_workspace_profile(engine.clone(), &args, &store_path);
        let restored =
            initial_workspace_profile(engine, &[OsString::from("shea-symphony-app")], &store_path);

        assert_eq!(
            selected.target_root,
            fs::canonicalize(&target).unwrap().display().to_string()
        );
        assert_eq!(restored.target_root, selected.target_root);
        assert_eq!(restored.source, "saved");
    }

    #[test]
    fn profile_path_under_shea_infers_target_root_and_cli_path() {
        let engine = temp_dir("profile-engine");
        let target = temp_dir("profile-target");
        let profile_path = target.join(".shea/app-profile.json");
        fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
        fs::write(
            &profile_path,
            r#"{"workflow_path":".shea/workflows/shea-symphony.md","cli_path":".shea/bin/shea-symphony"}"#,
        )
        .unwrap();

        let profile = initial_workspace_profile(
            engine,
            &[OsString::from("shea-symphony-app")],
            &profile_path,
        );

        assert_eq!(
            profile.target_root,
            fs::canonicalize(&target).unwrap().display().to_string()
        );
        assert_eq!(profile.workflow_path, ".shea/workflows/shea-symphony.md");
        assert_eq!(profile.cli_path.as_deref(), Some(".shea/bin/shea-symphony"));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = temp_path(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "shea-symphony-app-{name}-{}-{unique}",
            std::process::id()
        ))
    }
}
