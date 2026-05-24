use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::TrackerIssue;
use crate::tracker::TrackerError;

use super::super::cli::{gh_available, run_gh_api_json, run_gh_graphql};
use super::super::project_v2::{
    github_rest_project_path, project_field_from_metadata_with_refresh,
    project_metadata_from_response, project_rest_item_id, rest_project_item_field_update_body,
    rest_project_item_overlays_from_response, rest_project_metadata_from_response,
    ProjectFieldMetadata, ProjectFieldUpdateValue, ProjectMetadata, ProjectV2OwnerType,
    RestProjectItemOverlay,
};
use super::super::queries::{
    github_project_metadata_query, github_project_query, GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
    GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION,
};
use super::super::GithubProjectReadMode;
use super::GithubProjectV2GhClient;

impl GithubProjectV2GhClient {
    pub(super) fn project_metadata(&self) -> Result<ProjectMetadata, TrackerError> {
        self.metadata_cache
            .get_or_try_init(|| self.load_project_metadata())
    }

    fn refresh_project_metadata(&self) -> Result<ProjectMetadata, TrackerError> {
        self.metadata_cache.refresh(|| self.load_project_metadata())
    }

    fn load_project_metadata(&self) -> Result<ProjectMetadata, TrackerError> {
        let owner = self
            .config
            .tracker
            .project_owner
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_owner".into()))?;
        let number =
            self.config.tracker.project_number.ok_or_else(|| {
                TrackerError::IntegrationUnavailable("missing project_number".into())
            })?;

        match self.query_project_owner("ProjectV2 REST metadata", |owner_type| {
            self.rest_project_metadata(owner_type, owner, number)
        }) {
            Ok((_owner_type, metadata)) => Ok(metadata),
            Err(rest_error) => {
                let (_owner_type, response) = self
                    .query_project_owner("ProjectV2 metadata", |owner_type| {
                        self.graphql_project_metadata(owner_type, owner, number)
                    })
                    .map_err(|graphql_error| {
                        TrackerError::IntegrationUnavailable(format!(
                            "REST ProjectV2 metadata fallback reason: {rest_error}; GraphQL fallback failed: {graphql_error}"
                        ))
                    })?;

                project_metadata_from_response(&response, &self.config.tracker.status_field)
            }
        }
    }

    pub(super) fn project_field_with_refresh(
        &self,
        field_name: &str,
    ) -> Result<(ProjectMetadata, ProjectFieldMetadata), TrackerError> {
        project_field_from_metadata_with_refresh(&self.metadata_cache, field_name, || {
            self.load_project_metadata()
        })
    }

    pub(super) fn status_option_id_with_refresh(
        &self,
        option_name: &str,
    ) -> Result<(ProjectMetadata, String), TrackerError> {
        let (metadata, field) =
            self.project_field_with_refresh(&self.config.tracker.status_field)?;
        let Some(option_id) = field.option_id(option_name) else {
            let refreshed = self.refresh_project_metadata()?;
            let refreshed_field = refreshed
                .field(&self.config.tracker.status_field)
                .ok_or_else(|| {
                    TrackerError::IntegrationUnavailable(format!(
                        "ProjectV2 field {:?} was not found after metadata refresh",
                        self.config.tracker.status_field
                    ))
                })?;
            let option_id = refreshed_field.option_id(option_name).ok_or_else(|| {
                TrackerError::IntegrationUnavailable(format!(
                    "status option {option_name:?} was not found in ProjectV2 field {:?} after metadata refresh",
                    self.config.tracker.status_field
                ))
            })?;
            return Ok((refreshed, option_id));
        };
        Ok((metadata, option_id))
    }

    pub(super) fn project_field_option_id_with_refresh(
        &self,
        field_name: &str,
        option_name: &str,
    ) -> Result<String, TrackerError> {
        let (_metadata, field) = self.project_field_with_refresh(field_name)?;
        if let Some(option_id) = field.option_id(option_name) {
            return Ok(option_id);
        }

        let refreshed = self.refresh_project_metadata()?;
        let refreshed_field = refreshed.field(field_name).ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "ProjectV2 field {field_name:?} was not found after metadata refresh"
            ))
        })?;
        refreshed_field.option_id(option_name).ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "option {option_name:?} was not found in ProjectV2 field {field_name:?} after metadata refresh"
            ))
        })
    }

    fn rest_project_metadata(
        &self,
        owner_type: ProjectV2OwnerType,
        owner: &str,
        number: u64,
    ) -> Result<ProjectMetadata, TrackerError> {
        let base_path = github_rest_project_path(owner_type, owner, number);
        let project = run_gh_api_json(vec!["api".into(), base_path.clone()])?;
        let fields = run_gh_api_json(vec![
            "api".into(),
            format!("{base_path}/fields?per_page=100"),
            "--paginate".into(),
            "--slurp".into(),
        ])?;

        rest_project_metadata_from_response(
            &project,
            &fields,
            &self.config.tracker.status_field,
            owner_type,
        )
    }

    pub(super) fn rest_project_item_overlays(
        &self,
        metadata: &ProjectMetadata,
    ) -> Result<BTreeMap<String, RestProjectItemOverlay>, TrackerError> {
        let field_ids = metadata
            .supported_rest_field_ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let owner = self
            .config
            .tracker
            .project_owner
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_owner".into()))?;
        let number =
            self.config.tracker.project_number.ok_or_else(|| {
                TrackerError::IntegrationUnavailable("missing project_number".into())
            })?;
        let base_path = github_rest_project_path(metadata.owner_type, owner, number);
        let endpoint = if field_ids.is_empty() {
            format!("{base_path}/items?per_page=100")
        } else {
            format!("{base_path}/items?per_page=100&fields={field_ids}")
        };
        let response = run_gh_api_json(vec![
            "api".into(),
            endpoint,
            "--paginate".into(),
            "--slurp".into(),
        ])?;
        rest_project_item_overlays_from_response(&response)
    }

    pub(in crate::tracker) fn update_project_item_field_rest(
        &self,
        issue: &TrackerIssue,
        metadata: &ProjectMetadata,
        field: &ProjectFieldMetadata,
        value: ProjectFieldUpdateValue,
    ) -> Result<(), TrackerError> {
        let item_rest_id = project_rest_item_id(issue).ok_or_else(|| {
            TrackerError::NotImplemented(
                "REST Projects v2 item update fallback reason: current Project read lacks REST item id; using GraphQL where available"
                    .into(),
            )
        })?;
        self.update_project_item_field_rest_by_id(metadata, item_rest_id, field, value)
    }

    fn update_project_item_field_rest_by_id(
        &self,
        metadata: &ProjectMetadata,
        item_rest_id: u64,
        field: &ProjectFieldMetadata,
        value: ProjectFieldUpdateValue,
    ) -> Result<(), TrackerError> {
        let field_rest_id = field.rest_id.ok_or_else(|| {
            TrackerError::NotImplemented(format!(
                "REST Projects v2 item update fallback reason: ProjectV2 field {:?} lacks a REST field id; using GraphQL where available",
                field.name
            ))
        })?;
        let owner = self
            .config
            .tracker
            .project_owner
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_owner".into()))?;
        let number =
            self.config.tracker.project_number.ok_or_else(|| {
                TrackerError::IntegrationUnavailable("missing project_number".into())
            })?;
        let base_path = github_rest_project_path(metadata.owner_type, owner, number);
        let body = rest_project_item_field_update_body(field_rest_id, value)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let body_path =
            std::env::temp_dir().join(format!("shea-symphony-project-item-field-{nonce}.json"));
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|error| TrackerError::Payload(format!("invalid REST update body: {error}")))?;
        fs::write(&body_path, body_bytes)
            .map_err(|error| TrackerError::IntegrationUnavailable(error.to_string()))?;
        let result = run_gh_api_json(vec![
            "api".into(),
            "-X".into(),
            "PATCH".into(),
            format!("{base_path}/items/{item_rest_id}"),
            "--input".into(),
            body_path.to_string_lossy().to_string(),
        ]);
        let _ = fs::remove_file(&body_path);
        result.map(|_| ())
    }

    pub(super) fn graphql_update_project_single_select_field(
        &self,
        project_id: String,
        item_id: String,
        field_id: String,
        option_id: String,
    ) -> Result<(), TrackerError> {
        self.graphql(
            GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
            &[
                ("projectId", project_id),
                ("itemId", item_id),
                ("fieldId", field_id),
                ("optionId", option_id),
            ],
        )?;
        Ok(())
    }

    pub(super) fn graphql_update_project_text_field(
        &self,
        project_id: String,
        item_id: String,
        field_id: String,
        text: String,
    ) -> Result<(), TrackerError> {
        self.graphql(
            GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION,
            &[
                ("projectId", project_id),
                ("itemId", item_id),
                ("fieldId", field_id),
                ("text", text),
            ],
        )?;
        Ok(())
    }

    fn graphql_project_metadata(
        &self,
        owner_type: ProjectV2OwnerType,
        owner: &str,
        number: u64,
    ) -> Result<serde_json::Value, TrackerError> {
        let query = github_project_metadata_query(owner_type.query_field());

        self.graphql_magic(
            &query,
            &[("owner", owner.to_string()), ("number", number.to_string())],
            &["number"],
        )
    }

    pub(super) fn graphql_project_page(
        &self,
        owner_type: ProjectV2OwnerType,
        cursor: Option<&str>,
        mode: GithubProjectReadMode,
    ) -> Result<serde_json::Value, TrackerError> {
        let owner = self
            .config
            .tracker
            .project_owner
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_owner".into()))?;
        let number =
            self.config.tracker.project_number.ok_or_else(|| {
                TrackerError::IntegrationUnavailable("missing project_number".into())
            })?;

        let mut args = vec![
            "api".to_string(),
            "graphql".to_string(),
            "-f".to_string(),
            format!(
                "query={}",
                github_project_query(owner_type.query_field(), mode)
            ),
            "-F".to_string(),
            format!("owner={owner}"),
            "-F".to_string(),
            format!("number={number}"),
        ];

        if let Some(cursor) = cursor {
            args.push("-F".to_string());
            args.push(format!("cursor={cursor}"));
        }

        run_gh_graphql(args)
    }

    pub(super) fn graphql(
        &self,
        query: &str,
        variables: &[(&str, String)],
    ) -> Result<serde_json::Value, TrackerError> {
        self.graphql_magic(query, variables, &[])
    }

    pub(super) fn graphql_magic(
        &self,
        query: &str,
        variables: &[(&str, String)],
        magic_fields: &[&str],
    ) -> Result<serde_json::Value, TrackerError> {
        if !gh_available() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 live operations require the `gh` CLI on PATH".into(),
            ));
        }

        let mut args = vec![
            "api".to_string(),
            "graphql".to_string(),
            "-f".to_string(),
            format!("query={query}"),
        ];

        for (name, value) in variables {
            if magic_fields.contains(name) {
                args.push("-F".to_string());
            } else {
                args.push("-f".to_string());
            }
            args.push(format!("{name}={value}"));
        }

        run_gh_graphql(args)
    }
}
