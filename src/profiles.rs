use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{ExecutionProfileConfig, ProfilesConfig};
use crate::workspace::safe_identifier;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProfile {
    pub profile_id: String,
    pub instance_name: String,
    pub source: String,
    pub backend: Option<String>,
    pub workspace_namespace: String,
    pub user_data_dir: Option<PathBuf>,
    pub working_dir: Option<PathBuf>,
    pub extra_args: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl ExecutionProfile {
    pub fn environment_for_backend(&self, backend: &str) -> BTreeMap<String, String> {
        let mut env = self.env.clone();
        if backend == "codex" {
            env.remove("CODEX_HOME");
        }
        env.insert("SHEA_SYMPHONY_PROFILE_ID".into(), self.profile_id.clone());
        env.insert(
            "SHEA_SYMPHONY_INSTANCE_NAME".into(),
            self.instance_name.clone(),
        );
        env.insert("SHEA_SYMPHONY_PROFILE_SOURCE".into(), self.source.clone());
        if let Some(path) = &self.user_data_dir {
            env.insert(
                "SHEA_SYMPHONY_PROFILE_HOME".into(),
                path.display().to_string(),
            );
        }
        if let Some(args) = &self.extra_args {
            if !args.trim().is_empty() {
                env.insert("SHEA_SYMPHONY_COCKPIT_EXTRA_ARGS".into(), args.clone());
            }
        }
        env
    }
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("profile config io error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("profile config parse error at {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("configured default profile was not discovered: {0}")]
    MissingDefault(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CockpitCodexInstanceStore {
    #[serde(default)]
    instances: Vec<CockpitCodexInstance>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CockpitCodexInstance {
    id: String,
    name: String,
    user_data_dir: String,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    extra_args: String,
}

pub fn discover_execution_profiles(
    config: &ProfilesConfig,
) -> Result<Vec<ExecutionProfile>, ProfileError> {
    let mut profiles = Vec::new();
    if let Some(path) = &config.cockpit_tools.codex_instances_path {
        if path.exists() {
            profiles.extend(load_cockpit_codex_profiles(path)?);
        }
    }

    if profiles.is_empty() {
        profiles.extend(explicit_profiles(&config.entries));
    }

    Ok(profiles)
}

pub fn selected_execution_profile(
    config: &ProfilesConfig,
) -> Result<Option<ExecutionProfile>, ProfileError> {
    let profiles = discover_execution_profiles(config)?;
    if profiles.is_empty() {
        return Ok(None);
    }

    if let Some(default) = config.default.as_deref() {
        return profiles
            .into_iter()
            .find(|profile| {
                profile.profile_id == default
                    || profile.instance_name == default
                    || profile.workspace_namespace == safe_identifier(default)
            })
            .map(Some)
            .ok_or_else(|| ProfileError::MissingDefault(default.into()));
    }

    Ok(profiles.into_iter().next())
}

pub fn load_cockpit_codex_profiles(path: &Path) -> Result<Vec<ExecutionProfile>, ProfileError> {
    let content = fs::read_to_string(path).map_err(|source| ProfileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let store: CockpitCodexInstanceStore =
        serde_json::from_str(&content).map_err(|source| ProfileError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(cockpit_instances_to_profiles(store.instances))
}

fn cockpit_instances_to_profiles(instances: Vec<CockpitCodexInstance>) -> Vec<ExecutionProfile> {
    let mut used = BTreeSet::new();
    instances
        .into_iter()
        .filter_map(|instance| {
            let name = instance.name.trim();
            if name.is_empty() {
                return None;
            }
            let mut namespace = safe_identifier(name);
            if !used.insert(namespace.clone()) {
                namespace = safe_identifier(&format!("{}-{}", name, instance.id));
                used.insert(namespace.clone());
            }

            Some(ExecutionProfile {
                profile_id: namespace.clone(),
                instance_name: name.to_string(),
                source: "cockpit-tools:codex_instances".into(),
                backend: Some("codex".into()),
                workspace_namespace: namespace,
                user_data_dir: nonempty_path(instance.user_data_dir),
                working_dir: instance.working_dir.and_then(nonempty_path),
                extra_args: nonempty_string(instance.extra_args),
                env: BTreeMap::new(),
            })
        })
        .collect()
}

fn explicit_profiles(configs: &[ExecutionProfileConfig]) -> Vec<ExecutionProfile> {
    configs
        .iter()
        .filter_map(|profile| {
            let id = profile.id.trim();
            if id.is_empty() {
                return None;
            }

            Some(ExecutionProfile {
                profile_id: safe_identifier(id),
                instance_name: profile
                    .instance_name
                    .clone()
                    .unwrap_or_else(|| id.to_string()),
                source: "workflow:profiles.entries".into(),
                backend: profile.backend.clone(),
                workspace_namespace: profile
                    .workspace_namespace
                    .as_deref()
                    .map(safe_identifier)
                    .unwrap_or_else(|| safe_identifier(id)),
                user_data_dir: profile.user_data_dir.clone(),
                working_dir: profile.working_dir.clone(),
                extra_args: profile.extra_args.clone().and_then(nonempty_string),
                env: profile.env.clone(),
            })
        })
        .collect()
}

fn nonempty_path(value: String) -> Option<PathBuf> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn nonempty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cockpit_tools_codex_instance_store() {
        let path = Path::new("examples/fixtures/cockpit-tools-codex-instances.json");
        let profiles = load_cockpit_codex_profiles(path).unwrap();

        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].profile_id, "codex-alpha");
        assert_eq!(profiles[0].instance_name, "codex-alpha");
        assert_eq!(
            profiles[0].user_data_dir.as_deref(),
            Some(Path::new("/tmp/cockpit/codex-alpha"))
        );
        assert_eq!(profiles[0].backend.as_deref(), Some("codex"));
    }

    #[test]
    fn cockpit_profile_environment_sets_backend_context_without_account_ids_or_codex_home() {
        let profile = load_cockpit_codex_profiles(Path::new(
            "examples/fixtures/cockpit-tools-codex-instances.json",
        ))
        .unwrap()
        .remove(0);

        let env = profile.environment_for_backend("codex");

        assert_eq!(
            env.get("SHEA_SYMPHONY_PROFILE_ID"),
            Some(&"codex-alpha".into())
        );
        assert_eq!(
            env.get("SHEA_SYMPHONY_INSTANCE_NAME"),
            Some(&"codex-alpha".into())
        );
        assert_eq!(
            env.get("SHEA_SYMPHONY_PROFILE_HOME"),
            Some(&"/tmp/cockpit/codex-alpha".into())
        );
        assert!(!env.contains_key("CODEX_HOME"));
        assert!(!env.contains_key("bindAccountId"));
        assert!(!env.contains_key("BIND_ACCOUNT_ID"));
    }

    #[test]
    fn codex_profile_environment_ignores_configured_codex_home() {
        let mut profile = load_cockpit_codex_profiles(Path::new(
            "examples/fixtures/cockpit-tools-codex-instances.json",
        ))
        .unwrap()
        .remove(0);
        profile
            .env
            .insert("CODEX_HOME".into(), "/tmp/global".into());

        let env = profile.environment_for_backend("codex");

        assert!(!env.contains_key("CODEX_HOME"));
    }
}
