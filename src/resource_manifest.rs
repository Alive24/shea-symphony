use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

pub const RESOURCE_MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResourceManifest {
    pub schema_version: u32,
    pub core_group: String,
    pub groups: BTreeMap<String, ResourceGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResourceGroup {
    pub optional: bool,
    pub available: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub resources: Vec<ResourceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ResourceEntry {
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResourceClosure {
    /// Canonical repository root used to confine all resolved resources.
    pub repository_root: PathBuf,
    pub manifest_path: PathBuf,
    pub selected_groups: Vec<String>,
    pub resources: Vec<ResolvedResource>,
    pub markdown_sources: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResource {
    pub group: String,
    pub kind: String,
    pub path: PathBuf,
}

#[derive(Debug, Error)]
pub enum ResourceManifestError {
    #[error("resources must be a map/object")]
    InvalidWorkflowConfig,
    #[error("resources.manifest is required when resources is configured")]
    MissingManifestConfig,
    #[error("resource manifest is unavailable at {path}: {source}")]
    MissingManifest {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("resource manifest is invalid at {path}: {source}")]
    InvalidManifest {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("unsupported resource manifest schema {actual}; expected {expected}")]
    UnsupportedSchema { actual: u32, expected: u32 },
    #[error("resource group `{0}` is not declared")]
    UnknownGroup(String),
    #[error("resource group `{0}` is declared but unavailable in this release")]
    UnavailableGroup(String),
    #[error("resource group dependency cycle includes `{0}`")]
    DependencyCycle(String),
    #[error("core resource group `{0}` must be available and non-optional")]
    InvalidCoreGroup(String),
    #[error("resource `{path}` from group `{group}` escapes repository root {root}")]
    EscapesRepository {
        group: String,
        path: PathBuf,
        root: PathBuf,
    },
    #[error("resource `{path}` from group `{group}` is missing")]
    MissingResource { group: String, path: PathBuf },
    #[error("resource `{path}` from group `{group}` is empty")]
    EmptyResource { group: String, path: PathBuf },
    #[error("setup-shea must remain global and cannot be in installable group `{group}`: {path}")]
    SetupIncluded { group: String, path: PathBuf },
}

pub fn resolve_resource_closure(
    workflow_path: &Path,
    config: &Value,
) -> Result<Option<ResolvedResourceClosure>, ResourceManifestError> {
    let Some(resource_config) = config.get("resources") else {
        return Ok(None);
    };
    let resource_config = resource_config
        .as_object()
        .ok_or(ResourceManifestError::InvalidWorkflowConfig)?;
    let relative = resource_config
        .get("manifest")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(ResourceManifestError::MissingManifestConfig)?;
    let configured_manifest_path = resolve_relative(workflow_path, relative);
    let source = fs::read_to_string(&configured_manifest_path).map_err(|source| {
        ResourceManifestError::MissingManifest {
            path: configured_manifest_path.clone(),
            source,
        }
    })?;
    let manifest_path = configured_manifest_path
        .canonicalize()
        .unwrap_or(configured_manifest_path);
    let manifest: ResourceManifest =
        serde_json::from_str(&source).map_err(|source| ResourceManifestError::InvalidManifest {
            path: manifest_path.clone(),
            source,
        })?;
    if manifest.schema_version != RESOURCE_MANIFEST_SCHEMA_VERSION {
        return Err(ResourceManifestError::UnsupportedSchema {
            actual: manifest.schema_version,
            expected: RESOURCE_MANIFEST_SCHEMA_VERSION,
        });
    }
    let core = manifest
        .groups
        .get(&manifest.core_group)
        .ok_or_else(|| ResourceManifestError::UnknownGroup(manifest.core_group.clone()))?;
    if core.optional || !core.available {
        return Err(ResourceManifestError::InvalidCoreGroup(
            manifest.core_group.clone(),
        ));
    }

    let mut requested = vec![manifest.core_group.clone()];
    if let Some(groups) = resource_config.get("enabled_groups") {
        let groups = groups
            .as_array()
            .ok_or(ResourceManifestError::InvalidWorkflowConfig)?;
        for group in groups {
            let group = group
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or(ResourceManifestError::InvalidWorkflowConfig)?;
            requested.push(group.into());
        }
    }

    let mut selected = Vec::new();
    let mut resolved = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    for group in requested {
        resolve_group(
            &manifest,
            &group,
            &mut visiting,
            &mut resolved,
            &mut selected,
        )?;
    }

    let manifest_root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let repository_root = manifest_root
        .parent()
        .unwrap_or(manifest_root)
        .canonicalize()
        .unwrap_or_else(|_| manifest_root.to_path_buf());
    let mut resources = Vec::new();
    let mut markdown_sources = BTreeSet::new();
    for group_name in &selected {
        let group = &manifest.groups[group_name];
        for entry in &group.resources {
            let path = manifest_root.join(&entry.path);
            let canonical =
                path.canonicalize()
                    .map_err(|_| ResourceManifestError::MissingResource {
                        group: group_name.clone(),
                        path: path.clone(),
                    })?;
            if !canonical.starts_with(&repository_root) {
                return Err(ResourceManifestError::EscapesRepository {
                    group: group_name.clone(),
                    path: canonical,
                    root: repository_root.clone(),
                });
            }
            if canonical
                .components()
                .any(|component| component.as_os_str() == "setup-shea")
            {
                return Err(ResourceManifestError::SetupIncluded {
                    group: group_name.clone(),
                    path: canonical,
                });
            }
            validate_resource(group_name, &canonical, &mut markdown_sources)?;
            resources.push(ResolvedResource {
                group: group_name.clone(),
                kind: entry.kind.clone(),
                path: canonical,
            });
        }
    }

    Ok(Some(ResolvedResourceClosure {
        repository_root,
        manifest_path,
        selected_groups: selected,
        resources,
        markdown_sources: markdown_sources.into_iter().collect(),
    }))
}

fn resolve_group(
    manifest: &ResourceManifest,
    group_name: &str,
    visiting: &mut BTreeSet<String>,
    resolved: &mut BTreeSet<String>,
    selected: &mut Vec<String>,
) -> Result<(), ResourceManifestError> {
    if resolved.contains(group_name) {
        return Ok(());
    }
    if !visiting.insert(group_name.into()) {
        return Err(ResourceManifestError::DependencyCycle(group_name.into()));
    }
    let group = manifest
        .groups
        .get(group_name)
        .ok_or_else(|| ResourceManifestError::UnknownGroup(group_name.into()))?;
    if !group.available {
        return Err(ResourceManifestError::UnavailableGroup(group_name.into()));
    }
    for dependency in &group.depends_on {
        resolve_group(manifest, dependency, visiting, resolved, selected)?;
    }
    visiting.remove(group_name);
    resolved.insert(group_name.into());
    selected.push(group_name.into());
    Ok(())
}

fn validate_resource(
    group: &str,
    path: &Path,
    markdown_sources: &mut BTreeSet<PathBuf>,
) -> Result<(), ResourceManifestError> {
    if path.is_dir() {
        let mut found = false;
        for entry in fs::read_dir(path).map_err(|_| ResourceManifestError::MissingResource {
            group: group.into(),
            path: path.into(),
        })? {
            let entry = entry.map_err(|_| ResourceManifestError::MissingResource {
                group: group.into(),
                path: path.into(),
            })?;
            found = true;
            validate_resource(group, &entry.path(), markdown_sources)?;
        }
        if !found {
            return Err(ResourceManifestError::EmptyResource {
                group: group.into(),
                path: path.into(),
            });
        }
        return Ok(());
    }
    let metadata = fs::metadata(path).map_err(|_| ResourceManifestError::MissingResource {
        group: group.into(),
        path: path.into(),
    })?;
    if metadata.len() == 0 {
        return Err(ResourceManifestError::EmptyResource {
            group: group.into(),
            path: path.into(),
        });
    }
    if path.extension().is_some_and(|extension| extension == "md") {
        markdown_sources.insert(path.into());
    }
    Ok(())
}

fn resolve_relative(workflow_path: &Path, relative: &str) -> PathBuf {
    let path = PathBuf::from(relative);
    if path.is_absolute() {
        path
    } else {
        workflow_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(enabled_groups: &[&str]) -> (tempfile::TempDir, PathBuf, Value) {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join(".shea/workflows")).unwrap();
        fs::create_dir_all(temp.path().join(".shea/core")).unwrap();
        fs::create_dir_all(temp.path().join(".shea/optional")).unwrap();
        fs::write(temp.path().join(".shea/core/core.md"), "core").unwrap();
        fs::write(temp.path().join(".shea/optional/optional.md"), "optional").unwrap();
        fs::write(
            temp.path().join(".shea/resources.json"),
            r#"{"schema_version":1,"core_group":"core","groups":{"core":{"optional":false,"available":true,"depends_on":[],"resources":[{"kind":"template","path":"core/core.md"}]},"optional":{"optional":true,"available":true,"depends_on":["core"],"resources":[{"kind":"template","path":"optional/optional.md"}]}}}"#,
        )
        .unwrap();
        let workflow = temp.path().join(".shea/workflows/WORKFLOW.md");
        let config = serde_json::json!({
            "resources": {
                "manifest": "../resources.json",
                "enabled_groups": enabled_groups,
            }
        });
        (temp, workflow, config)
    }

    #[test]
    fn core_is_always_selected_and_optional_is_omittable() {
        let (temp, workflow, config) = fixture(&[]);
        let closure = resolve_resource_closure(&workflow, &config)
            .unwrap()
            .unwrap();
        assert_eq!(closure.repository_root, temp.path().canonicalize().unwrap());
        assert_eq!(closure.selected_groups, vec!["core"]);
        assert_eq!(closure.markdown_sources.len(), 1);
    }

    #[test]
    fn selected_optional_group_includes_dependency_closure() {
        let (_temp, workflow, config) = fixture(&["optional"]);
        let closure = resolve_resource_closure(&workflow, &config)
            .unwrap()
            .unwrap();
        assert_eq!(closure.selected_groups, vec!["core", "optional"]);
        assert_eq!(closure.markdown_sources.len(), 2);
    }

    #[test]
    fn missing_core_resource_fails_with_focused_path() {
        let (temp, workflow, config) = fixture(&[]);
        fs::remove_file(temp.path().join(".shea/core/core.md")).unwrap();
        let error = resolve_resource_closure(&workflow, &config)
            .unwrap_err()
            .to_string();
        assert!(error.contains("core/core.md"), "{error}");
        assert!(error.contains("group `core`"), "{error}");
    }
}
