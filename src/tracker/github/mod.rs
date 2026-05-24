mod cli;
mod client;
mod evidence;
mod project_v2;
mod queries;
mod topology;

pub use queries::GithubProjectReadMode;

pub(in crate::tracker) use cli::{
    gh_available, github_auth_gap, github_auth_mode, github_graphql_auth_smoke,
};
#[cfg(test)]
pub(in crate::tracker) use cli::{
    project_state_error_is_retryable, run_command_with_timeout, GithubAuthMode,
};
pub(in crate::tracker) use client::GithubProjectV2GhClient;
#[cfg(test)]
pub(in crate::tracker) use client::{project_owner_query_error, project_owner_query_order};
#[cfg(test)]
pub(in crate::tracker) use evidence::{
    blocker_refs_from_project_fields, github_issue_description_with_workpad,
    github_native_blocker_refs_from_response, linked_pull_request_from_url,
    linked_pull_requests_from_workpads, merge_blocker_refs, merge_github_issue_evidence,
    merge_linked_pull_requests,
};
pub(in crate::tracker) use evidence::{github_issue_number, json_number_to_i64, string_nodes};
#[cfg(test)]
pub(in crate::tracker) use project_v2::{
    apply_rest_project_item_overlays, issue_from_repository_issue_response,
    issues_from_project_response, project_field_from_metadata_with_refresh,
    project_item_id_from_add_response, project_metadata_from_response,
    rest_project_item_field_update_body, rest_project_item_overlays_from_response,
    rest_project_metadata_from_response, status_option_id, ProjectFieldKind, ProjectFieldMetadata,
    ProjectFieldUpdateValue, ProjectMetadata, ProjectMetadataCache, ProjectV2OwnerType,
};
#[cfg(test)]
pub(in crate::tracker) use queries::github_project_query;
pub(in crate::tracker) use topology::{
    enrich_native_subissue_project_statuses_for_issue,
    enrich_native_subissue_project_statuses_from_project_read,
    hydrate_missing_native_subissue_project_statuses, native_subissue_refs_missing_project_state,
    project_state_map,
};
