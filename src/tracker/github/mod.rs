mod cli;
mod evidence;
mod project_v2;
mod queries;

pub use queries::GithubProjectReadMode;

pub(in crate::tracker) use cli::{
    gh_available, github_auth_gap, github_auth_mode, github_graphql_auth_smoke, run_gh_api_json,
    run_gh_graphql, GithubCliAccess,
};
#[cfg(test)]
pub(in crate::tracker) use cli::{
    project_state_error_is_retryable, run_command_with_timeout, GithubAuthMode,
};
#[cfg(test)]
pub(in crate::tracker) use evidence::linked_pull_request_from_url;
pub(in crate::tracker) use evidence::{
    blocker_refs_from_project_fields, github_issue_comment_bodies,
    github_issue_description_with_workpad, github_issue_number,
    github_native_blocker_refs_from_response, json_number_to_i64,
    linked_pull_requests_from_workpads, merge_blocker_refs, merge_linked_pull_requests,
    project_fields, project_status, pull_requests_from_issue, string_nodes,
};
pub(in crate::tracker) use project_v2::{
    apply_rest_project_item_overlay_fallback, apply_rest_project_item_overlays,
    github_rest_project_path, issues_from_project_response,
    project_field_from_metadata_with_refresh, project_metadata_from_response, project_rest_item_id,
    rest_project_item_field_update_body, rest_project_item_overlays_from_response,
    rest_project_metadata_from_response, ProjectFieldKind, ProjectFieldMetadata,
    ProjectFieldUpdateValue, ProjectMetadata, ProjectMetadataCache, RestProjectItemOverlay,
};
pub(super) use queries::{
    github_issue_comments_query, github_issue_evidence_query, github_issue_project_item_query,
    github_project_metadata_query, github_project_query, GITHUB_ADD_COMMENT_MUTATION,
    GITHUB_ADD_PROJECT_ITEM_MUTATION, GITHUB_CLEAR_PROJECT_ITEM_FIELD_MUTATION,
    GITHUB_CLOSE_ISSUE_MUTATION, GITHUB_CREATE_ISSUE_MUTATION, GITHUB_REPOSITORY_ID_QUERY,
    GITHUB_UPDATE_ISSUE_COMMENT_MUTATION, GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
    GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION,
};
