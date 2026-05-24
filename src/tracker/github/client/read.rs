use std::collections::BTreeMap;

use crate::model::{BlockerRef, TrackerIssue};
use crate::tracker::TrackerError;

use super::super::cli::run_gh_api_json;
use super::super::evidence::{
    github_issue_number, github_native_blocker_refs_from_response, merge_blocker_refs,
    merge_github_issue_evidence,
};
use super::super::project_v2::issue_from_repository_issue_response;
use super::super::queries::{github_issue_evidence_query, github_issue_project_item_query};
use super::super::topology::{
    enrich_native_subissue_project_statuses_for_issue, insert_native_subissue_status_fields,
    native_subissue_refs_from_rest_response, NativeSubissueRef,
};
use super::GithubProjectV2GhClient;

impl GithubProjectV2GhClient {
    pub(in crate::tracker) fn enrich_issue_evidence(
        &self,
        issue: &mut TrackerIssue,
        project_states: &BTreeMap<String, String>,
    ) -> Result<(), TrackerError> {
        let Some(number) = github_issue_number(&issue.identifier) else {
            return Ok(());
        };
        let content = self.fetch_issue_evidence(number)?;
        merge_github_issue_evidence(issue, &content, &self.config)?;
        enrich_native_subissue_project_statuses_for_issue(issue, project_states);
        Ok(())
    }

    fn fetch_issue_evidence(&self, issue_number: u64) -> Result<serde_json::Value, TrackerError> {
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.owner is required".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.repo is required".into()))?;
        let response = self.graphql_magic(
            &github_issue_evidence_query(),
            &[
                ("owner", owner.to_string()),
                ("repo", repo.to_string()),
                ("number", issue_number.to_string()),
            ],
            &["number"],
        )?;
        response
            .pointer("/data/repository/issue")
            .cloned()
            .ok_or_else(|| {
                TrackerError::Payload(format!(
                    "GitHub issue evidence response missing issue #{issue_number}"
                ))
            })
    }

    pub(in crate::tracker) fn enrich_native_issue_blockers(
        &self,
        issues: &mut [TrackerIssue],
    ) -> Result<(), TrackerError> {
        for issue in issues {
            let Some(number) = github_issue_number(&issue.identifier) else {
                continue;
            };
            let native_blockers = self.fetch_native_issue_blockers(number)?;
            merge_blocker_refs(&mut issue.blocked_by, native_blockers);
        }

        Ok(())
    }

    pub(in crate::tracker) fn enrich_native_subissues(
        &self,
        issues: &mut [TrackerIssue],
    ) -> Result<(), TrackerError> {
        for issue in issues {
            let Some(number) = github_issue_number(&issue.identifier) else {
                continue;
            };
            let native_subissues = self.fetch_native_subissues(number)?;
            if !native_subissues.is_empty() {
                insert_native_subissue_status_fields(
                    &mut issue.project_fields,
                    native_subissues,
                    &BTreeMap::new(),
                );
            }
        }

        Ok(())
    }

    pub(in crate::tracker) fn fetch_project_issue(
        &self,
        issue_ref: &str,
    ) -> Result<Option<TrackerIssue>, TrackerError> {
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.owner is required".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.repo is required".into()))?;
        let issue_number = github_issue_number(issue_ref).ok_or_else(|| {
            TrackerError::Payload(format!(
                "issue ref {issue_ref:?} is not a GitHub issue number"
            ))
        })?;

        let response = self.graphql_magic(
            &github_issue_project_item_query(),
            &[
                ("owner", owner.to_string()),
                ("repo", repo.to_string()),
                ("number", issue_number.to_string()),
            ],
            &["number"],
        )?;

        issue_from_repository_issue_response(&response, &self.config)
    }

    fn fetch_native_issue_blockers(
        &self,
        issue_number: u64,
    ) -> Result<Vec<BlockerRef>, TrackerError> {
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.owner is required".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.repo is required".into()))?;
        let response = run_gh_api_json(vec![
            "api".into(),
            format!("repos/{owner}/{repo}/issues/{issue_number}/dependencies/blocked_by"),
        ])?;

        github_native_blocker_refs_from_response(&response, issue_number)
    }

    fn fetch_native_subissues(
        &self,
        issue_number: u64,
    ) -> Result<Vec<NativeSubissueRef>, TrackerError> {
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.owner is required".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.repo is required".into()))?;
        let response = run_gh_api_json(vec![
            "api".into(),
            format!("repos/{owner}/{repo}/issues/{issue_number}/sub_issues"),
        ])?;

        native_subissue_refs_from_rest_response(&response)
    }

    pub(in crate::tracker) fn fetch_project_states_for_issue_refs(
        &self,
        issue_refs: &[String],
    ) -> Result<BTreeMap<String, String>, TrackerError> {
        let mut states = BTreeMap::new();
        for issue_ref in issue_refs {
            if let Some(issue) = self.fetch_project_issue(issue_ref)? {
                states.insert(issue.identifier, issue.state);
            }
        }
        Ok(states)
    }
}
